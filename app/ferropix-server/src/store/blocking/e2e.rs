// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The blocking reads over the keys on the head against what they replaced
//! (#724), behind `FERROPIX_E2E`: the migration fills every head so the new
//! reads find what the reads over every version found, every writer of a
//! head sets the keys of its version, and over a long history of feeds,
//! revisions, removals, re-adds, resolved duplicates, merges, a revert and
//! a retired domain the reads find exactly the heads whose demographics give
//! the key.

use std::collections::BTreeSet;

use base64::Engine as _;
use ferrohealth_secret::secret_url::SecretUrl;
use ferropix_testkit::containers::{Postgres, e2e_enabled, postgres};
use ferropix_testkit::synthetic::{
    FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};
use ihe_iti::pixm::server::feed::{Feed, Merge, Removal, Revision};
use ihe_iti::server::format::Format;
use sqlx::PgPool;
use uuid::Uuid;

use super::oracle::{self, FeedRow, MatchRow};
use super::{FEED_BY_PHONETIC, FEED_BY_POSTAL, MATCH_BY_PHONETIC, MATCH_BY_POSTAL};
use crate::config::section::db::{DbSettings, SealingKey};
use crate::config::section::domain::{DomainKind, DomainSeed};
use crate::config::section::matching::Matching;
use crate::feed::merging::{self, MergeRequest};
use crate::feed::{Context, Feeder};
use crate::store::record::{Channel, PersonId};
use crate::store::schema::migrator;
use crate::store::{Job, Store};

/// The BSN naming system.
const BSN: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

/// The client every seeded domain names as its Source.
const SOURCE: &str = "synthetic-source";

/// The migration that moves the keys to the head.
const MOVE: &str = "head blocking keys";

/// The postal codes the corpora draw from, few so blocks are shared.
const POSTALS: [&str; 4] = ["1000AA", "1011AB", "2500CD", "9999ZZ"];

/// Creates the application role and returns its URL.
async fn app_url(postgres: &Postgres) -> String {
    let admin = PgPool::connect(postgres.url())
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

/// The `Patient` demographics of the given parts, without an identifier.
fn demographics(family: &str, given: &str, birth: Option<&str>, postal: Option<&str>) -> String {
    let mut members = vec![format!(
        r#""name":[{{"family":"{family}","given":["{given}"]}}]"#
    )];
    if let Some(birth) = birth {
        members.push(format!(r#""birthDate":"{birth}""#));
    }
    if let Some(postal) = postal {
        members.push(format!(r#""address":[{{"postalCode":"{postal}"}}]"#));
    }
    format!(r#"{{"resourceType":"Patient",{}}}"#, members.join(","))
}

/// The demographics of version `version` of the migration corpus's record
/// `index`: drawn from a small pool, so one record's old versions share
/// keys with other records' heads.
fn drawn(index: usize, version: usize) -> String {
    let at = index + version * 13;
    let birth =
        (!at.is_multiple_of(9)).then(|| format!("{}-0{}-1{}", 1950 + at % 3, 1 + at % 4, at % 5));
    let postal = (!at.is_multiple_of(6)).then(|| POSTALS[at % POSTALS.len()]);
    demographics(
        FAMILY_NAMES[at % FAMILY_NAMES.len()],
        GIVEN_NAMES[at % GIVEN_NAMES.len()],
        birth.as_deref(),
        postal,
    )
}

/// The versions the migration corpus's record `index` holds.
fn versions(index: usize) -> usize {
    if index.is_multiple_of(7) {
        60
    } else {
        1 + index % 3
    }
}

/// Seeds the migration corpus under the schema before the keys moved: two
/// local domains (the second retired), records with long histories,
/// removed and subsumed records, records with a closed link before their
/// open one, and records with no open link.
async fn seed_old(pool: &PgPool) {
    let mut transaction = pool.begin().await.expect("a transaction");
    let mut domains = Vec::new();
    for (system, retired) in [(HOSPITAL_DOMAIN, false), (LABORATORY_DOMAIN, true)] {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO pix.identifier_domain (system, name, kind, source_client_id, sensitive, retired_at)
             VALUES ($1, $1, 'local', $2, false, CASE WHEN $3 THEN now() END) RETURNING id",
        )
        .bind(system)
        .bind(SOURCE)
        .bind(retired)
        .fetch_one(&mut *transaction)
        .await
        .expect("a domain");
        domains.push(id);
    }
    let records: Vec<Uuid> = (0..600).map(|_| Uuid::new_v4()).collect();
    let persons: Vec<Uuid> = (0..300).map(|_| Uuid::new_v4()).collect();
    sqlx::query("INSERT INTO pix.person (id) SELECT unnest($1::uuid[])")
        .bind(&persons)
        .execute(&mut *transaction)
        .await
        .expect("the persons");
    for (index, record) in records.iter().enumerate() {
        let domain = domains[usize::from(index.is_multiple_of(5))];
        let head = versions(index);
        let mut hmac = [0_u8; 32];
        hmac[..8].copy_from_slice(&u64::try_from(index).unwrap().to_be_bytes());
        sqlx::query(
            "INSERT INTO pix.source_record
                 (id, domain_id, identifier_hmac, identifier_value, key_id, head_version, removed_at)
             VALUES ($1, $2, $3, $4, $5, $6, CASE WHEN $7 THEN now() END)",
        )
        .bind(record)
        .bind(domain)
        .bind(hmac.as_slice())
        .bind(format!("M-{index}"))
        .bind([0_u8; 16].as_slice())
        .bind(i32::try_from(head).unwrap())
        .bind(index % 11 == 1)
        .execute(&mut *transaction)
        .await
        .expect("a record");
        for version in 1..=head {
            let text = drawn(index, version);
            let (phonetic, postal) = oracle::keys(&text);
            sqlx::query(
                "INSERT INTO pix.source_record_version
                     (record_id, version, demographics, channel, block_phonetic, block_postal)
                 VALUES ($1, $2, $3::jsonb, 'fhir', $4, $5)",
            )
            .bind(record)
            .bind(i32::try_from(version).unwrap())
            .bind(&text)
            .bind(phonetic)
            .bind(postal)
            .execute(&mut *transaction)
            .await
            .expect("a version");
        }
        let person = persons[index.div_euclid(2)];
        if index % 17 == 3 {
            sqlx::query(
                "INSERT INTO pix.link (record_id, person_id, kind, valid, made_by)
                 VALUES ($1, $2, 'feed', tstzrange(now() - interval '2 days', now() - interval '1 day'), 'e2e')",
            )
            .bind(record)
            .bind(persons[(index.div_euclid(2) + 1) % persons.len()])
            .execute(&mut *transaction)
            .await
            .expect("a closed link");
        }
        if index % 19 != 5 {
            sqlx::query(
                "INSERT INTO pix.link (record_id, person_id, kind, valid, made_by)
                 VALUES ($1, $2, 'feed', tstzrange(now() - interval '1 day', NULL), 'e2e')",
            )
            .bind(record)
            .bind(person)
            .execute(&mut *transaction)
            .await
            .expect("an open link");
        }
    }
    for (index, record) in records.iter().enumerate() {
        let survivor = records[(index + 5) % records.len()];
        if index % 13 == 2 {
            sqlx::query(
                "UPDATE pix.source_record SET active = false, replaced_by = $2 WHERE id = $1",
            )
            .bind(record)
            .bind(survivor)
            .execute(&mut *transaction)
            .await
            .expect("a subsumed record");
        }
    }
    transaction.commit().await.expect("the corpus commits");
}

/// Every text key value the store's versions give, phonetic or not.
async fn values(pool: &PgPool) -> BTreeSet<(bool, String)> {
    let texts: Vec<String> =
        sqlx::query_scalar("SELECT demographics::text FROM pix.source_record_version")
            .fetch_all(pool)
            .await
            .expect("the versions read");
    let mut values = BTreeSet::new();
    for text in texts {
        let (phonetic, postal) = oracle::keys(&text);
        values.extend(phonetic.map(|value| (true, value)));
        values.extend(postal.map(|value| (false, value)));
    }
    values
}

/// What the four reads answer for every value, each feed read also with
/// the record and the person of its first and last rows excluded.
#[derive(Debug, PartialEq, Eq)]
struct Answers {
    feed: Vec<Vec<FeedRow>>,
    matched: Vec<Vec<MatchRow>>,
}

/// The exclusions the feed's read is asked with for `value`: none, and the
/// record and person of the first and last rows the read finds without.
fn exclusions(rows: &[FeedRow]) -> Vec<(Uuid, Uuid)> {
    let mut pairs = vec![(Uuid::nil(), Uuid::nil())];
    for row in [rows.first(), rows.last()].into_iter().flatten() {
        pairs.push((row.0, row.2));
        pairs.push((Uuid::nil(), row.2));
        pairs.push((row.0, Uuid::nil()));
    }
    pairs
}

/// Runs the feed reads `feed` and the match reads `matched` (phonetic,
/// postal) for every value of `values`.
async fn answers(
    pool: &PgPool,
    values: &BTreeSet<(bool, String)>,
    feed: [&'static str; 2],
    matched: [&'static str; 2],
) -> Answers {
    let mut found = Answers {
        feed: Vec::new(),
        matched: Vec::new(),
    };
    for (phonetic, value) in values {
        let at = usize::from(!*phonetic);
        let all = feed_rows(pool, feed[at], value, Uuid::nil(), Uuid::nil()).await;
        for (record, person) in exclusions(&all) {
            found
                .feed
                .push(feed_rows(pool, feed[at], value, record, person).await);
        }
        let mut rows: Vec<MatchRow> = sqlx::query_as(matched[at])
            .bind(value)
            .fetch_all(pool)
            .await
            .expect("the match read runs");
        rows.sort();
        found.matched.push(rows);
    }
    found
}

/// Runs the feed read `statement` for `value`, excluding `record` and
/// `person`, sorted.
async fn feed_rows(
    pool: &PgPool,
    statement: &'static str,
    value: &str,
    record: Uuid,
    person: Uuid,
) -> Vec<FeedRow> {
    let mut rows: Vec<FeedRow> = sqlx::query_as(statement)
        .bind(value)
        .bind(record)
        .bind(person)
        .fetch_all(pool)
        .await
        .expect("the feed read runs");
    rows.sort();
    rows
}

#[tokio::test]
async fn the_migration_fills_every_head_and_the_reads_find_what_the_reads_over_every_version_found()
{
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.expect("PostgreSQL starts");
    let pool = PgPool::connect(&app_url(&postgres).await)
        .await
        .expect("the role connects");
    let set = migrator();
    let before = set
        .iter()
        .take_while(|migration| migration.description != MOVE)
        .last()
        .expect("a migration precedes the move")
        .version;
    set.run_to(before, &pool)
        .await
        .expect("the set applies up to the move");
    seed_old(&pool).await;
    let values = values(&pool).await;
    let old = answers(
        &pool,
        &values,
        [oracle::OLD_FEED_BY_PHONETIC, oracle::OLD_FEED_BY_POSTAL],
        [oracle::OLD_MATCH_BY_PHONETIC, oracle::OLD_MATCH_BY_POSTAL],
    )
    .await;
    assert!(
        old.feed.iter().filter(|rows| !rows.is_empty()).count() > 100,
        "the corpus blocks"
    );
    set.run(&pool).await.expect("the rest of the set applies");
    let new = answers(
        &pool,
        &values,
        [FEED_BY_PHONETIC, FEED_BY_POSTAL],
        [MATCH_BY_PHONETIC, MATCH_BY_POSTAL],
    )
    .await;
    assert_eq!(old, new);
    oracle::assert_heads_hold_their_keys(&pool).await;
}

/// A store over its own PostgreSQL, with a feeder.
struct Started {
    _postgres: Postgres,
    store: Store,
    feeder: Feeder,
}

/// Starts PostgreSQL and the store with the BSN and three local domains.
async fn start() -> Started {
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

/// One patient as a Source sends it.
#[derive(Debug, Clone)]
struct Patient {
    system: &'static str,
    value: String,
    bsn: Option<String>,
    family: String,
    given: String,
    birth: Option<String>,
    postal: Option<String>,
}

impl Patient {
    /// The patient of the long-history corpus at `index`, as revision
    /// `turn` states it.
    fn at(index: usize, turn: usize) -> Self {
        let at = index + turn * 7;
        let bsn = index.is_multiple_of(10).then(|| {
            ferropix_testkit::synthetic::bsn(index.div_euclid(20))
                .expect("a synthetic BSN")
                .digits()
                .clone()
        });
        Self {
            system: [HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN][index % 3],
            value: format!("L-{index:04}"),
            bsn,
            family: FAMILY_NAMES[at % FAMILY_NAMES.len()].to_owned(),
            given: GIVEN_NAMES[index % GIVEN_NAMES.len()].to_owned(),
            birth: (!at.is_multiple_of(8))
                .then(|| format!("{}-0{}-1{}", 1960 + at % 3, 1 + at % 5, at % 4)),
            postal: (!at.is_multiple_of(5)).then(|| POSTALS[at % POSTALS.len()].to_owned()),
        }
    }

    /// The `Patient` body, with `link` when it is a resolved duplicate.
    fn body(&self, link: Option<&str>) -> String {
        let mut identifiers = vec![format!(
            r#"{{"system":"{}","value":"{}"}}"#,
            self.system, self.value
        )];
        if let Some(bsn) = &self.bsn {
            identifiers.push(format!(r#"{{"system":"{BSN}","value":"{bsn}"}}"#));
        }
        let text = demographics(
            &self.family,
            &self.given,
            self.birth.as_deref(),
            self.postal.as_deref(),
        );
        let rest = text
            .strip_prefix(r#"{"resourceType":"Patient","#)
            .expect("the demographics open with the type");
        let link = link.map_or_else(String::new, |link| format!(r#""link":[{link}],"#));
        format!(
            r#"{{"resourceType":"Patient","identifier":[{}],{link}{rest}"#,
            identifiers.join(",")
        )
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

/// Asserts that every head holds its version's keys and that, for every key
/// value any version ever gave, the reads find exactly what the definition
/// of a block finds; `step` names the point in the history.
async fn agree(started: &Started, step: &str) {
    let pool = started.store.pool();
    oracle::assert_heads_hold_their_keys(pool).await;
    let heads = oracle::heads(pool).await;
    let mut blocked = 0_usize;
    for (phonetic, value) in values(pool).await {
        let at = usize::from(!phonetic);
        let all = feed_rows(
            pool,
            [FEED_BY_PHONETIC, FEED_BY_POSTAL][at],
            &value,
            Uuid::nil(),
            Uuid::nil(),
        )
        .await;
        blocked += usize::from(!all.is_empty());
        for (record, person) in exclusions(&all) {
            assert_eq!(
                oracle::feed(&heads, phonetic, &value, record, person),
                feed_rows(
                    pool,
                    [FEED_BY_PHONETIC, FEED_BY_POSTAL][at],
                    &value,
                    record,
                    person
                )
                .await,
                "the feed read after {step}"
            );
        }
        let mut rows: Vec<MatchRow> = sqlx::query_as([MATCH_BY_PHONETIC, MATCH_BY_POSTAL][at])
            .bind(&value)
            .fetch_all(pool)
            .await
            .expect("the match read runs");
        rows.sort();
        assert_eq!(
            oracle::matched(&heads, phonetic, &value),
            rows,
            "the match read after {step}"
        );
    }
    assert!(blocked > 0, "something blocks after {step}");
}

#[tokio::test]
async fn over_a_long_history_the_reads_find_exactly_the_heads_that_give_the_key() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let mut persons = Vec::new();
    for index in 0..120 {
        persons.push(feed(&started, &Patient::at(index, 0)).await);
    }
    agree(&started, "the first feeds").await;

    for turn in 1..=200 {
        feed(&started, &Patient::at(1, turn)).await;
    }
    for index in (2..120).step_by(3) {
        for turn in 1..=(1 + index % 4) {
            feed(&started, &Patient::at(index, turn)).await;
        }
    }
    agree(&started, "the revisions").await;

    for index in (4..120).step_by(9) {
        started
            .feeder
            .remove(&Patient::at(index, 0).removal(), &context())
            .await
            .expect("the removal applies");
    }
    agree(&started, "the removals").await;
    for index in (4..120).step_by(18) {
        feed(&started, &Patient::at(index, 11)).await;
    }
    agree(&started, "the re-adds").await;

    let survivor = Patient::at(33, 0);
    for subsumed in [Patient::at(30, 5), Patient::at(9999, 2)] {
        started
            .feeder
            .resolve_duplicate(&subsumed.subsumed_by(&survivor.value), &context())
            .await
            .expect("the duplicate resolves");
    }
    agree(&started, "the resolved duplicates").await;

    merge(&started, persons[2], persons[5]).await;
    merge(&started, persons[7], persons[8]).await;
    let last = merge(&started, persons[2], persons[11]).await;
    agree(&started, "the merges").await;
    revert(&started, last, persons[2]).await;
    agree(&started, "the revert").await;

    retire(&started, PRACTICE_DOMAIN, true).await;
    agree(&started, "the retirement").await;
    retire(&started, PRACTICE_DOMAIN, false).await;
    agree(&started, "the restoration").await;
}

/// The keys the record of `patient` holds: phonetic, postal.
async fn held(started: &Started, patient: &Patient) -> (Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT r.block_phonetic, r.block_postal
         FROM pix.source_record r
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         WHERE d.system = $1 AND r.identifier_value = $2",
    )
    .bind(patient.system)
    .bind(&patient.value)
    .fetch_one(started.store.pool())
    .await
    .expect("the record reads")
}

/// The keys `patient`'s demographics give.
fn given(patient: &Patient) -> (Option<String>, Option<String>) {
    oracle::keys(&demographics(
        &patient.family,
        &patient.given,
        patient.birth.as_deref(),
        patient.postal.as_deref(),
    ))
}

#[tokio::test]
async fn every_writer_of_a_head_sets_the_keys_of_its_version() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let mut patient = Patient::at(1, 1);
    patient.birth = Some(String::from("1971-03-14"));
    patient.postal = Some(String::from("1000AA"));
    feed(&started, &patient).await;
    let created = held(&started, &patient).await;
    assert_eq!(given(&patient), created, "an added record");
    assert!(created.0.is_some() && created.1.is_some());

    patient.postal = Some(String::from("2500CD"));
    feed(&started, &patient).await;
    let moved = held(&started, &patient).await;
    assert_eq!(created.0, moved.0, "the phonetic key stays");
    assert_ne!(created.1, moved.1, "the postal key moves");
    assert_eq!(given(&patient), moved);

    patient.postal = None;
    feed(&started, &patient).await;
    assert_eq!(
        (moved.0.clone(), None),
        held(&started, &patient).await,
        "a key the version lacks is cleared"
    );

    started
        .feeder
        .remove(&patient.removal(), &context())
        .await
        .expect("the removal applies");
    assert_eq!(
        given(&patient),
        held(&started, &patient).await,
        "a removal keeps the head's keys"
    );

    patient.family = String::from("Verzinga");
    patient.postal = Some(String::from("9999ZZ"));
    feed(&started, &patient).await;
    assert_eq!(
        given(&patient),
        held(&started, &patient).await,
        "a re-add sets its version's keys"
    );

    let mut subsumed = Patient::at(4, 0);
    subsumed.birth = Some(String::from("1980-01-02"));
    subsumed.postal = Some(String::from("1011AB"));
    started
        .feeder
        .resolve_duplicate(&subsumed.subsumed_by(&patient.value), &context())
        .await
        .expect("the duplicate resolves");
    assert_eq!(
        given(&subsumed),
        held(&started, &subsumed).await,
        "a subsumed record inserted by its message"
    );
    oracle::assert_heads_hold_their_keys(started.store.pool()).await;
}
