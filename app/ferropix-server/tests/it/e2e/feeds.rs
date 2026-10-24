// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-104 messages the feed cases send, read by `ihe-iti` as the route
//! will read them, and the store they run against.

use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::config::section::matching::Matching;
use ferropix_server::feed::{Context, Feeder};
use ferropix_server::store::record::{Channel, RecordId};
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{Postgres, postgres};
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use ihe_iti::pixm::server::feed::{Feed, Merge, Removal, Revision};
use ihe_iti::server::format::Format;
use uuid::Uuid;

use super::{BSN_SYSTEM, app_role, fresh_key, settings};

/// The Source every synthetic domain names.
pub(crate) const SOURCE: &str = "synthetic-source";

/// The domains the feed cases seed: the BSN, authoritative and sealed, and
/// the three synthetic local domains, plain.
pub(crate) fn seeds() -> Vec<DomainSeed> {
    let local = |system: &str, name: &str| DomainSeed {
        system: system.to_owned(),
        name: name.to_owned(),
        kind: DomainKind::Local,
        source_client_id: SOURCE.to_owned(),
        sensitive: false,
    };
    vec![
        DomainSeed {
            system: BSN_SYSTEM.to_owned(),
            name: String::from("BSN"),
            kind: DomainKind::Authoritative,
            source_client_id: SOURCE.to_owned(),
            sensitive: true,
        },
        local(HOSPITAL_DOMAIN, "Synthetic hospital"),
        local(LABORATORY_DOMAIN, "Synthetic laboratory"),
        local(PRACTICE_DOMAIN, "Synthetic practice"),
    ]
}

/// A started store with the feed seeds, and the container it runs in.
pub(crate) struct Started {
    /// The container, kept for the case's length.
    pub _postgres: Postgres,
    /// The store.
    pub store: Store,
}

/// Starts PostgreSQL and the store with the feed seeds.
pub(crate) async fn start() -> Started {
    let postgres = postgres().await.expect("PostgreSQL starts");
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds(), Job::Migrate)
        .await
        .expect("the store starts");
    Started {
        _postgres: postgres,
        store,
    }
}

/// The feed transaction over `store` with the default `[matching]`.
pub(crate) fn feeder(store: &Store) -> Feeder {
    Feeder::new(
        store.clone(),
        &Matching::default().resolve().expect("the defaults resolve"),
    )
}

/// The context of a message from the synthetic Source.
pub(crate) fn context() -> Context {
    Context {
        actor: SOURCE.to_owned(),
        channel: Channel::Fhir,
        request_id: Some(String::from("e2e")),
    }
}

/// A synthetic patient as a Source sends it.
#[derive(Debug, Clone)]
pub(crate) struct Patient {
    /// The identifier the condition names.
    pub system: &'static str,
    /// Its value.
    pub value: String,
    /// The other identifiers the body carries.
    pub carried: Vec<(&'static str, String)>,
    /// The family name.
    pub family: String,
    /// The given name.
    pub given: String,
    /// The birth date.
    pub birth: String,
    /// The administrative gender.
    pub gender: &'static str,
    /// The postal code.
    pub postal: Option<String>,
}

impl Patient {
    /// Returns the patient `value` in `system` with the given demographics.
    pub(crate) fn new(system: &'static str, value: &str, family: &str, given: &str) -> Self {
        Self {
            system,
            value: value.to_owned(),
            carried: Vec::new(),
            family: family.to_owned(),
            given: given.to_owned(),
            birth: String::from("1961-04-12"),
            gender: "female",
            postal: None,
        }
    }

    /// Returns this patient carrying `value` in `system` too.
    pub(crate) fn carrying(mut self, system: &'static str, value: &str) -> Self {
        self.carried.push((system, value.to_owned()));
        self
    }

    /// Returns the `Patient` body, with `link` as its `link` member when
    /// given.
    fn body(&self, link: Option<&str>) -> String {
        let identifiers: Vec<String> = std::iter::once((self.system, self.value.as_str()))
            .chain(self.carried.iter().map(|(s, v)| (*s, v.as_str())))
            .map(|(system, value)| format!(r#"{{"system":"{system}","value":"{value}"}}"#))
            .collect();
        let address = self
            .postal
            .as_ref()
            .map(|postal| format!(r#","address":[{{"postalCode":"{postal}"}}]"#))
            .unwrap_or_default();
        let link = link
            .map(|l| format!(r#","link":[{l}]"#))
            .unwrap_or_default();
        format!(
            r#"{{"resourceType":"Patient","identifier":[{}],"active":true,
            "name":[{{"family":"{}","given":["{}"]}}],"gender":"{}","birthDate":"{}"{address}{link}}}"#,
            identifiers.join(","),
            self.family,
            self.given,
            self.gender,
            self.birth
        )
    }

    /// Returns the conditional update's query naming this patient.
    pub(crate) fn query(&self) -> String {
        condition(self.system, &self.value)
    }

    /// Returns the Add or Revise Patient message of this patient.
    pub(crate) fn revision(&self) -> Revision {
        match Feed::from_update(&self.query(), Format::Json, self.body(None).as_bytes()) {
            Ok(Feed::Revise(revision)) => revision,
            other => panic!("an add or revise message: {other:?}"),
        }
    }

    /// Returns the Resolve Duplicate Patient message subsuming this patient
    /// into `survivor` of the same domain.
    pub(crate) fn subsumed_by(&self, survivor: &str) -> Merge {
        let link = format!(
            r#"{{"other":{{"identifier":{{"system":"{}","value":"{survivor}"}}}},"type":"replaced-by"}}"#,
            self.system
        );
        match Feed::from_update(
            &self.query(),
            Format::Json,
            self.body(Some(&link)).as_bytes(),
        ) {
            Ok(Feed::ResolveDuplicate(merge)) => merge,
            other => panic!("a resolve duplicate message: {other:?}"),
        }
    }

    /// Returns the Remove Patient message of this patient.
    pub(crate) fn removal(&self) -> Removal {
        Removal::from_delete(&self.query()).expect("a remove message")
    }
}

/// Returns the `identifier=system|value` query of a conditional
/// interaction.
pub(crate) fn condition(system: &str, value: &str) -> String {
    let token: String =
        url::form_urlencoded::byte_serialize(format!("{system}|{value}").as_bytes()).collect();
    format!("identifier={}", token.replace('+', "%20"))
}

/// Returns the person `record`'s open link names.
pub(crate) async fn person_of(store: &Store, record: RecordId) -> Option<Uuid> {
    sqlx::query_scalar("SELECT person_id FROM pix.link WHERE record_id = $1 AND upper_inf(valid)")
        .bind(record.uuid())
        .fetch_optional(store.pool())
        .await
        .expect("the link reads")
}
