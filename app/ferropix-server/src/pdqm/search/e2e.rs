// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The search over the keys against the search it replaced (#722), behind
//! `FERROPIX_E2E`: over a seeded corpus of about a thousand persons, every
//! search of a matrix answers the same total and the same pages from the
//! keys as from the oracle, and after each change that moves what a person
//! serves, a search right after the commit sees it.

use base64::Engine as _;
use ferrohealth_secret::secret_url::SecretUrl;
use ferropix_testkit::containers::{Postgres, e2e_enabled, postgres};
use ferropix_testkit::synthetic::{
    FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};
use ihe_iti::pdqm::server::search::PatientSearch;
use ihe_iti::pixm::server::feed::{Feed, Merge, Removal, Revision};
use ihe_iti::server::format::Format;
use uuid::Uuid;

use super::{Page, Window, oracle, run};
use crate::config::section::db::{DbSettings, SealingKey};
use crate::config::section::domain::{DomainKind, DomainSeed};
use crate::config::section::matching::Matching;
use crate::feed::merging::{self, MergeRequest};
use crate::feed::{Context, Feeder};
use crate::store::record::{Channel, PersonId};
use crate::store::{Job, Store};

/// The BSN naming system.
const BSN: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

/// The master domain's system, which no seeded domain is.
const MASTER: &str = "urn:oid:2.999.9.9";

/// The client every seeded domain names as its Source.
const SOURCE: &str = "synthetic-source";

/// The persons the corpus feeds.
const CORPUS: usize = 1000;

/// The accented and multi-part family names beside the testkit's.
const MARKED: [&str; 4] = [
    "Proëfsma",
    "Tèsterink",
    "Van der Voorbeeldt",
    "ÉÉN-Verzinga",
];

/// A store over its own PostgreSQL, with a feeder.
struct Started {
    _postgres: Postgres,
    store: Store,
    feeder: Feeder,
}

/// Starts PostgreSQL and the store with the BSN and three local domains.
async fn start() -> Started {
    let postgres = postgres().await.expect("PostgreSQL starts");
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
    let url = postgres
        .url()
        .replacen("pix_super:pix_super@", "pix_app:pix_app@", 1);
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
fn context() -> Context {
    Context {
        actor: SOURCE.to_owned(),
        channel: Channel::Fhir,
        request_id: Some(String::from("e2e")),
    }
}

/// The conditional update query naming `value` in `system`.
fn condition(system: &str, value: &str) -> String {
    let token: String =
        url::form_urlencoded::byte_serialize(format!("{system}|{value}").as_bytes()).collect();
    format!("identifier={}", token.replace('+', "%20"))
}

/// One patient of the corpus.
#[derive(Debug, Clone)]
struct Patient {
    system: &'static str,
    value: String,
    bsn: Option<String>,
    family: String,
    given: Vec<String>,
    birth: Option<String>,
    gender: Option<&'static str>,
    address: Option<String>,
    telecom: Option<String>,
    maiden: Option<String>,
}

impl Patient {
    /// The patient of the corpus at `index`.
    fn at(index: usize) -> Self {
        let system = [HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN][index % 3];
        let family = if index % 9 == 4 {
            MARKED[index % MARKED.len()].to_owned()
        } else {
            FAMILY_NAMES[index % FAMILY_NAMES.len()].to_owned()
        };
        let mut given = vec![GIVEN_NAMES[(index * 3) % GIVEN_NAMES.len()].to_owned()];
        if index % 5 == 0 {
            given.push(GIVEN_NAMES[(index * 7 + 1) % GIVEN_NAMES.len()].to_owned());
        }
        let year = 1930 + index % 70;
        let month = 1 + index % 12;
        let day = 1 + index % 28;
        let birth = match index % 7 {
            0 => Some(format!("{year}")),
            1 => Some(format!("{year}-{month:02}")),
            2 => None,
            _ => Some(format!("{year}-{month:02}-{day:02}")),
        };
        let gender = [
            Some("female"),
            Some("male"),
            Some("other"),
            Some("unknown"),
            None,
        ][index % 5];
        let address = (index % 4 != 3).then(|| {
            let city = ["Teststad", "Proefdorp", "Voorbeeldwijk"][index % 3];
            let state = ["Noord", "Zuid"][index % 2];
            format!(
                r#"{{"use":"home","line":["Voorbeeldstraat {index}","Bus {}"],"city":"{city}","district":"Wijk {}","state":"{state}","postalCode":"{:04} AA","country":"NL","text":"Voorbeeldstraat {index}, {city}"}}"#,
                index % 3,
                index % 6,
                1000 + index % 50
            )
        });
        let telecom = match index % 6 {
            0 => Some(format!(r#"{{"system":"phone","value":"0600{index:06}"}}"#)),
            1 => Some(format!(r#"{{"value":"0600{index:06}"}}"#)),
            2 => Some(format!(
                r#"{{"system":"email","value":"p{index}@example.org"}}"#
            )),
            _ => None,
        };
        let maiden =
            (index % 11 == 0).then(|| FAMILY_NAMES[(index + 3) % FAMILY_NAMES.len()].to_owned());
        Self {
            system,
            value: format!("C-{index:05}"),
            bsn: None,
            family,
            given,
            birth,
            gender,
            address,
            telecom,
            maiden,
        }
    }

    /// The `Patient` body.
    fn body(&self, link: Option<&str>) -> String {
        let mut identifiers = vec![format!(
            r#"{{"system":"{}","value":"{}"}}"#,
            self.system, self.value
        )];
        if let Some(bsn) = &self.bsn {
            identifiers.push(format!(r#"{{"system":"{BSN}","value":"{bsn}"}}"#));
        }
        let given: Vec<String> = self.given.iter().map(|g| format!(r#""{g}""#)).collect();
        let mut members = vec![
            format!(r#""identifier":[{}]"#, identifiers.join(",")),
            String::from(r#""active":true"#),
            format!(
                r#""name":[{{"family":"{}","given":[{}]}}]"#,
                self.family,
                given.join(",")
            ),
        ];
        if let Some(gender) = self.gender {
            members.push(format!(r#""gender":"{gender}""#));
        }
        if let Some(birth) = &self.birth {
            members.push(format!(r#""birthDate":"{birth}""#));
        }
        if let Some(address) = &self.address {
            members.push(format!(r#""address":[{address}]"#));
        }
        if let Some(telecom) = &self.telecom {
            members.push(format!(r#""telecom":[{telecom}]"#));
        }
        if let Some(maiden) = &self.maiden {
            members.push(format!(
                r#""extension":[{{"url":"{}","valueString":"{maiden}"}}]"#,
                crate::matching::subject::MOTHERS_MAIDEN_NAME
            ));
        }
        if let Some(link) = link {
            members.push(format!(r#""link":[{link}]"#));
        }
        format!(r#"{{"resourceType":"Patient",{}}}"#, members.join(","))
    }

    /// The Add or Revise Patient message.
    fn revision(&self) -> Revision {
        match Feed::from_update(
            &condition(self.system, &self.value),
            Format::Json,
            self.body(None).as_bytes(),
        ) {
            Ok(Feed::Revise(revision)) => revision,
            other => panic!("an add or revise message: {other:?}"),
        }
    }

    /// The Resolve Duplicate Patient message subsuming this into `survivor`.
    fn subsumed_by(&self, survivor: &str) -> Merge {
        let link = format!(
            r#"{{"other":{{"identifier":{{"system":"{}","value":"{survivor}"}}}},"type":"replaced-by"}}"#,
            self.system
        );
        match Feed::from_update(
            &condition(self.system, &self.value),
            Format::Json,
            self.body(Some(&link)).as_bytes(),
        ) {
            Ok(Feed::ResolveDuplicate(merge)) => merge,
            other => panic!("a resolve duplicate message: {other:?}"),
        }
    }

    /// The Remove Patient message.
    fn removal(&self) -> Removal {
        Removal::from_delete(&condition(self.system, &self.value)).expect("a remove message")
    }
}

/// Feeds `patient` and returns the person it is linked to.
async fn feed(started: &Started, patient: &Patient) -> PersonId {
    started
        .feeder
        .revise(&patient.revision(), &context())
        .await
        .expect("the feed applies")
        .person
        .expect("a fed record has a person")
}

/// Returns the synthetic BSN at `index`.
fn bsn(index: usize) -> String {
    ferropix_testkit::synthetic::bsn(index)
        .expect("a synthetic BSN")
        .digits()
        .to_owned()
}

/// Merges `merged` into `survivor` as a steward and returns the merge.
async fn merge(started: &Started, survivor: PersonId, merged: PersonId) -> Uuid {
    let mut connection = started.store.pool().acquire().await.expect("a connection");
    let survivor_version = merging::version(&mut connection, survivor)
        .await
        .expect("the version reads")
        .expect("the survivor exists");
    let merged_version = merging::version(&mut connection, merged)
        .await
        .expect("the version reads")
        .expect("the merged person exists");
    drop(connection);
    let request = MergeRequest {
        survivor,
        survivor_version,
        merged,
        merged_version,
        reason: String::from("one patient"),
    };
    let (transaction, change) = merging::merge(&started.store, &request, &context())
        .await
        .expect("merged");
    transaction.commit().await.expect("the merge commits");
    change.merge
}

/// Whether every one of `persons` is active.
async fn active(started: &Started, persons: &[PersonId]) -> bool {
    let ids: Vec<Uuid> = persons.iter().map(|person| person.uuid()).collect();
    let inactive: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.person WHERE id = ANY($1) AND status <> 'active'",
    )
    .bind(&ids)
    .fetch_one(started.store.pool())
    .await
    .expect("the persons read");
    inactive == 0
}

/// Reverts the merge `merge` of `survivor`.
async fn revert(started: &Started, merge: Uuid, survivor: PersonId) {
    let mut connection = started.store.pool().acquire().await.expect("a connection");
    let read = merging::version(&mut connection, survivor)
        .await
        .expect("the version reads")
        .expect("the survivor exists");
    drop(connection);
    let (transaction, _change) = merging::revert(&started.store, merge, &read, "two", &context())
        .await
        .expect("reverted");
    transaction.commit().await.expect("the revert commits");
}

/// Sets the retirement of the domain `system`.
async fn retire(started: &Started, system: &str, retired: bool) {
    sqlx::query(
        "UPDATE pix.identifier_domain SET retired_at = CASE WHEN $2 THEN now() END WHERE system = $1",
    )
    .bind(system)
    .bind(retired)
    .execute(started.store.pool())
    .await
    .expect("the domain is updated");
}

/// Runs `query` through the keys and through the oracle, page by page, and
/// asserts they agree; returns the persons of every page.
async fn agree(started: &Started, query: &str, omit_merged: bool) -> Vec<Uuid> {
    let search = PatientSearch::from_query(query).expect("the search reads");
    let mut after = None;
    let mut found = Vec::new();
    loop {
        let window = Window {
            size: 7,
            after,
            omit_merged,
        };
        let mut connection = started.store.pool().acquire().await.expect("a connection");
        let keyed: Page = run(
            &mut connection,
            started.store.sealer(),
            MASTER,
            &search,
            window,
        )
        .await
        .expect("the search over the keys runs");
        let read: Page = oracle::run(
            &mut connection,
            started.store.sealer(),
            MASTER,
            &search,
            window,
        )
        .await
        .expect("the oracle runs");
        assert_eq!(
            read, keyed,
            "{query} (omit merged {omit_merged}) after {after:?}"
        );
        found.extend(keyed.persons.iter().copied());
        if !keyed.more {
            return found;
        }
        after = keyed.persons.last().copied();
    }
}

/// The searches of the matrix.
fn matrix(identifier: &str, person: Uuid) -> Vec<String> {
    let mut queries: Vec<String> = [
        "family=Proefsma",
        "family=proef",
        "family=PROËF",
        "family=van%20der",
        "family=een",
        "family:exact=Proefsma",
        "family:exact=proefsma",
        "family:exact=Proëfsma",
        "family=Testerink,Verzinga",
        "given=ada",
        "given:exact=Bram",
        "given=c,d",
        "family=Testerink&given=Bram",
        "birthdate=1970",
        "birthdate=1970-05",
        "birthdate=1970-05-05",
        "birthdate=ge1980-01-01",
        "birthdate=gt1990",
        "birthdate=lt1950",
        "birthdate=le1960-06",
        "birthdate=ne1971",
        "birthdate=sa1985",
        "birthdate=eb1940",
        "birthdate=ap1975-03-03",
        "birthdate=1970,1980-02",
        "gender=female",
        "gender=male",
        "gender=other",
        "gender=unknown",
        "gender=female&birthdate=ge1960",
        "address=teststad",
        "address=voorbeeldstraat%2012",
        "address=bus%201",
        "address=wijk",
        "address=1010",
        "address=nl",
        "address:exact=Teststad",
        "address-city=proef",
        "address-city:exact=Voorbeeldwijk",
        "address-state=noord",
        "address-postalcode=10",
        "address-postalcode:exact=1012%20AA",
        "address-country=nl",
        "mothersMaidenName=test",
        "mothersMaidenName:exact=Verzinga",
        "telecom=0600000006",
        "telecom=phone|0600000012",
        "telecom=|0600000001",
        "telecom=email|p2@example.org",
        "telecom=phone|0600000001",
        "active=true",
        "active=false",
        "family=proef&address-city=teststad&gender=female",
    ]
    .iter()
    .map(|query| (*query).to_owned())
    .collect();
    queries.push(condition(HOSPITAL_DOMAIN, identifier));
    queries.push(format!("identifier={identifier}"));
    queries.push(format!("_id={person}"));
    queries.push(format!("_id={person}&family=proef"));
    queries
}

#[tokio::test]
async fn every_search_of_the_matrix_answers_as_the_oracle_over_the_corpus() {
    if !e2e_enabled() {
        return;
    }
    let started = Box::pin(start()).await;
    let mut persons = Vec::with_capacity(CORPUS);
    for index in 0..CORPUS {
        let mut patient = Patient::at(index);
        if index % 13 < 2 {
            patient.bsn = Some(bsn(index - index % 13));
        }
        persons.push(feed(&started, &patient).await);
    }
    for index in (0..CORPUS).step_by(26) {
        let mut authoritative = Patient::at(index + 500);
        authoritative.system = BSN;
        authoritative.value = bsn(index);
        authoritative.family = String::from("Gezaghebber");
        feed(&started, &authoritative).await;
    }
    for index in (0..CORPUS).step_by(10) {
        let mut revised = Patient::at(index);
        revised.family = format!("Herzien{}", index % 4);
        feed(&started, &revised).await;
    }
    for index in (3..CORPUS).step_by(17) {
        started
            .feeder
            .remove(&Patient::at(index).removal(), &context())
            .await
            .expect("removed");
    }
    for index in (7..CORPUS - 3).step_by(97) {
        let subsumed = Patient::at(index);
        let survivor = Patient::at(index + 3);
        started
            .feeder
            .resolve_duplicate(&subsumed.subsumed_by(&survivor.value), &context())
            .await
            .expect("resolved");
    }
    let mut merges = Vec::new();
    for index in (13..CORPUS - 40).step_by(89) {
        let (survivor, merged) = (persons[index], persons[index + 31]);
        if survivor != merged && active(&started, &[survivor, merged]).await {
            merges.push((merge(&started, survivor, merged).await, survivor));
        }
    }
    assert!(
        merges.len() >= 5,
        "the corpus holds merges: {}",
        merges.len()
    );
    if let Some((merge_id, survivor)) = merges.first().copied() {
        revert(&started, merge_id, survivor).await;
    }
    retire(&started, PRACTICE_DOMAIN, true).await;

    let anchor = Patient::at(1);
    for query in matrix(&anchor.value, persons[1].uuid()) {
        for omit_merged in [false, true] {
            agree(&started, &query, omit_merged).await;
        }
    }
}

#[tokio::test]
async fn a_search_right_after_each_change_sees_it() {
    if !e2e_enabled() {
        return;
    }
    let started = Box::pin(start()).await;
    let mut ada = Patient::at(3);
    ada.family = String::from("Eersteling");
    ada.bsn = Some(bsn(1));
    let person = feed(&started, &ada).await;
    assert_eq!(
        vec![person.uuid()],
        agree(&started, "family=eerste", false).await,
        "the feed"
    );

    ada.family = String::from("Tweedeling");
    feed(&started, &ada).await;
    assert!(
        agree(&started, "family=eerste", false).await.is_empty(),
        "the revision"
    );
    assert_eq!(
        vec![person.uuid()],
        agree(&started, "family=tweede", false).await,
        "the revision"
    );

    let mut lab = Patient::at(4);
    lab.family = String::from("Labbeling");
    lab.bsn = Some(bsn(1));
    let joined = feed(&started, &lab).await;
    assert_eq!(person, joined, "the carried BSN joins the person");
    agree(&started, "family=labbe", false).await;

    let mut authority = Patient::at(5);
    authority.system = BSN;
    authority.value = bsn(1);
    authority.family = String::from("Gezaghebber");
    feed(&started, &authority).await;
    assert_eq!(
        vec![person.uuid()],
        agree(&started, "family=gezag", false).await,
        "the authoritative record is served"
    );

    let other = feed(&started, &{
        let mut other = Patient::at(6);
        other.family = String::from("Anderling");
        other
    })
    .await;
    let merge_id = merge(&started, person, other).await;
    assert_eq!(
        vec![person.uuid()],
        agree(&started, "family=gezag", false).await,
        "the merge"
    );
    assert!(
        agree(&started, "family=anderling&active=false", false)
            .await
            .is_empty(),
        "the merged person serves nothing"
    );
    revert(&started, merge_id, person).await;
    assert_eq!(
        vec![other.uuid()],
        agree(&started, "family=anderling", false).await,
        "the unmerge"
    );

    started
        .feeder
        .remove(&authority.removal(), &context())
        .await
        .expect("removed");
    assert!(
        agree(&started, "family=gezag", false).await.is_empty(),
        "the removal"
    );
    agree(&started, "family=tweede", false).await;

    let mut duplicate = Patient::at(9);
    duplicate.family = String::from("Dubbeling");
    let duplicated = feed(&started, &duplicate).await;
    started
        .feeder
        .resolve_duplicate(&duplicate.subsumed_by(&ada.value), &context())
        .await
        .expect("resolved");
    assert!(
        agree(&started, "family=dubbel", false).await.is_empty(),
        "the duplicate resolution"
    );
    agree(&started, &format!("_id={}", duplicated.uuid()), false).await;

    retire(&started, LABORATORY_DOMAIN, true).await;
    agree(&started, "family=labbe", false).await;
    agree(&started, "family=tweede", false).await;
    retire(&started, LABORATORY_DOMAIN, false).await;
    agree(&started, "family=labbe", false).await;
    agree(&started, "family=tweede", false).await;
}
