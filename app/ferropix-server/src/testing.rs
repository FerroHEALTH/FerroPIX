// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the in-crate `FERROPIX_E2E` suites share: a store over its own
//! PostgreSQL with the BSN and three synthetic local domains, the Source's
//! context, the ITI-104 messages a Source sends, and the steward's merge and
//! revert.

use base64::Engine as _;
use ferrohealth_secret::secret_url::SecretUrl;
use ferropix_testkit::containers::{Postgres, postgres};
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use ihe_iti::pixm::server::feed::{Feed, Merge, Removal, Revision};
use ihe_iti::server::format::Format;
use uuid::Uuid;

use crate::config::section::db::{DbSettings, SealingKey};
use crate::config::section::domain::{DomainKind, DomainSeed};
use crate::config::section::matching::Matching;
use crate::feed::merging::{self, MergeRequest};
use crate::feed::{Context, Feeder};
use crate::store::record::{Channel, PersonId};
use crate::store::{Job, Store};

/// The BSN naming system.
pub(crate) const BSN: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

/// The client every seeded domain names as its Source.
pub(crate) const SOURCE: &str = "synthetic-source";

/// A store over its own PostgreSQL, with a feeder.
pub(crate) struct Started {
    _postgres: Postgres,
    pub(crate) store: Store,
    pub(crate) feeder: Feeder,
}

/// Creates the application role on `postgres` and returns its URL.
pub(crate) async fn app_url(postgres: &Postgres) -> String {
    let admin = sqlx::PgPool::connect(postgres.url())
        .await
        .expect("the superuser connects");
    for statement in [
        "CREATE ROLE pix_app LOGIN PASSWORD 'pix_app'",
        "GRANT CREATE ON DATABASE ferropix TO pix_app",
    ] {
        sqlx::query(statement)
            .execute(&admin)
            .await
            .expect("the role is made");
    }
    admin.close().await;
    postgres
        .url()
        .replacen("pix_super:pix_super@", "pix_app:pix_app@", 1)
}

/// Starts PostgreSQL and the store with the BSN and three local domains.
pub(crate) async fn start() -> Started {
    let postgres = postgres().await.expect("PostgreSQL starts");
    let url = app_url(&postgres).await;
    let mut bytes = [0_u8; 32];
    aws_lc_rs::rand::fill(&mut bytes).expect("random bytes");
    let key = SealingKey::from_base64(&base64::engine::general_purpose::STANDARD.encode(bytes))
        .expect("a generated key reads");
    let settings = DbSettings {
        url: SecretUrl::new(&url),
        pool_size: 4,
        migrate: true,
        data_key: key,
    };
    let seed = |system: &str, kind: DomainKind, sensitive: bool| DomainSeed {
        system: system.to_owned(),
        name: system.to_owned(),
        kind,
        source_client_id: SOURCE.to_owned(),
        sensitive,
    };
    let seeds = vec![
        seed(BSN, DomainKind::Authoritative, true),
        seed(HOSPITAL_DOMAIN, DomainKind::Local, false),
        seed(LABORATORY_DOMAIN, DomainKind::Local, false),
        seed(PRACTICE_DOMAIN, DomainKind::Local, false),
    ];
    let store = Store::start(&settings, &seeds, Job::Migrate)
        .await
        .expect("the store starts");
    let feeder = Feeder::new(
        store.clone(),
        &Matching::default().resolve().expect("the defaults resolve"),
    );
    Started {
        _postgres: postgres,
        store,
        feeder,
    }
}

/// The Source's context.
pub(crate) fn context() -> Context {
    Context {
        actor: SOURCE.to_owned(),
        channel: Channel::Fhir,
        request_id: Some(String::from("e2e")),
    }
}

/// The conditional update query naming `value` in `system`.
pub(crate) fn condition(system: &str, value: &str) -> String {
    let token: String =
        url::form_urlencoded::byte_serialize(format!("{system}|{value}").as_bytes()).collect();
    format!("identifier={}", token.replace('+', "%20"))
}

/// The `identifier` member of a body: `value` in `system`, and `bsn` when
/// given.
pub(crate) fn identifiers(system: &str, value: &str, bsn: Option<&str>) -> String {
    let mut identifiers = vec![format!(r#"{{"system":"{system}","value":"{value}"}}"#)];
    if let Some(bsn) = bsn {
        identifiers.push(format!(r#"{{"system":"{BSN}","value":"{bsn}"}}"#));
    }
    format!(r#""identifier":[{}]"#, identifiers.join(","))
}

/// A patient a suite feeds: its identifier and the body a Source sends.
pub(crate) trait Fed {
    /// The system of the identifier the condition names.
    fn system(&self) -> &str;

    /// Its value.
    fn value(&self) -> &str;

    /// The `Patient` body, carrying `link` when it is given.
    fn body(&self, link: Option<&str>) -> String;

    /// The Add or Revise Patient message.
    fn revision(&self) -> Revision {
        let body = self.body(None);
        match Feed::from_update(
            &condition(self.system(), self.value()),
            Format::Json,
            body.as_bytes(),
        ) {
            Ok(Feed::Revise(revision)) => revision,
            other => panic!("an add or revise message: {other:?}"),
        }
    }

    /// The Resolve Duplicate Patient message subsuming this into `survivor`.
    fn subsumed_by(&self, survivor: &str) -> Merge {
        let link = format!(
            r#"{{"other":{{"identifier":{{"system":"{}","value":"{survivor}"}}}},"type":"replaced-by"}}"#,
            self.system()
        );
        let body = self.body(Some(&link));
        match Feed::from_update(
            &condition(self.system(), self.value()),
            Format::Json,
            body.as_bytes(),
        ) {
            Ok(Feed::ResolveDuplicate(merge)) => merge,
            other => panic!("a resolve duplicate message: {other:?}"),
        }
    }

    /// The Remove Patient message.
    fn removal(&self) -> Removal {
        Removal::from_delete(&condition(self.system(), self.value())).expect("a remove message")
    }
}

/// Feeds `patient` and returns the person its record is linked to.
pub(crate) async fn feed(started: &Started, patient: &impl Fed) -> PersonId {
    started
        .feeder
        .revise(&patient.revision(), &context())
        .await
        .expect("the feed applies")
        .person
        .expect("a fed record has a person")
}

/// Returns the synthetic BSN at `index`.
pub(crate) fn bsn(index: usize) -> String {
    ferropix_testkit::synthetic::bsn(index)
        .expect("a synthetic BSN")
        .digits()
        .clone()
}

/// The version of `person` as a steward reads it.
async fn version(started: &Started, person: PersonId) -> String {
    let mut connection = started.store.pool().acquire().await.expect("a connection");
    merging::version(&mut connection, person)
        .await
        .expect("the version reads")
        .expect("the person exists")
}

/// Merges `merged` into `survivor` as a steward and returns the merge.
pub(crate) async fn merge(started: &Started, survivor: PersonId, merged: PersonId) -> Uuid {
    let request = MergeRequest {
        survivor,
        survivor_version: version(started, survivor).await,
        merged,
        merged_version: version(started, merged).await,
        reason: String::from("one patient"),
    };
    let (transaction, change) = merging::merge(&started.store, &request, &context())
        .await
        .expect("merged");
    transaction.commit().await.expect("the merge commits");
    change.merge
}

/// Reverts the merge `merge` of `survivor`.
pub(crate) async fn revert(started: &Started, merge: Uuid, survivor: PersonId) {
    let read = version(started, survivor).await;
    let (transaction, _change) = merging::revert(&started.store, merge, &read, "two", &context())
        .await
        .expect("reverted");
    transaction.commit().await.expect("the revert commits");
}

/// Sets the retirement of the domain `system`.
pub(crate) async fn retire(started: &Started, system: &str, retired: bool) {
    sqlx::query(
        "UPDATE pix.identifier_domain SET retired_at = CASE WHEN $2 THEN now() END WHERE system = $1",
    )
    .bind(system)
    .bind(retired)
    .execute(started.store.pool())
    .await
    .expect("the domain is updated");
}
