// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The probabilistic tier against a table of 100,000 synthetic records: a
//! feed scores only the pairs its blocking keys produce, `auto_link =
//! false` leaves a `certain` pair a candidate, `auto_link = true` links it,
//! and `matching estimate` reads the field frequencies.
//!
//! The family names are made of invented syllables, so no name belongs to
//! anyone, and every date and postal code is made up for the case.

use std::collections::BTreeSet;

use ferropix_server::config::section::matching::{BlockingKey, Field, Matching};
use ferropix_server::feed::Feeder;
use ferropix_server::matching::blocking::text_key;
use ferropix_server::matching::subject::Subject;
use ferropix_server::store::Store;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{
    GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};
use fhir_types::codec::{Json as _, Path, Value};
use fhir_types::r4::patient::Patient as FhirPatient;
use uuid::Uuid;

use super::feeds::{Patient, context, feeder, person_of, start};

/// The records the table holds.
const RECORDS: usize = 100_000;

/// The records one transaction of the load inserts.
const BATCH: usize = 10_000;

/// The first syllable of an invented family name.
const HEADS: [&str; 16] = [
    "Bak", "Dor", "Fen", "Gul", "Kas", "Lom", "Mir", "Nes", "Pol", "Rud", "Sig", "Tav", "Vin",
    "Wes", "Zul", "Hor",
];

/// The second syllable of an invented family name.
const TAILS: [&str; 8] = ["ma", "link", "stra", "dijk", "huis", "ga", "berg", "ink"];

/// The demographics of synthetic record `index`.
fn demographics(index: usize) -> (String, String, String, String) {
    let family = format!(
        "{}{}",
        HEADS[index % HEADS.len()],
        TAILS[index.div_euclid(HEADS.len()) % TAILS.len()]
    );
    let given = GIVEN_NAMES[index.div_euclid(7) % GIVEN_NAMES.len()].to_owned();
    let day = index % 28 + 1;
    let month = index.div_euclid(28) % 12 + 1;
    let year = 1930 + index.div_euclid(336) % 80;
    let birth = format!("{year:04}-{month:02}-{day:02}");
    let postal = format!(
        "{:04}{}{}",
        1000 + (index * 7919) % 9000,
        char::from(b'A' + u8::try_from(index % 26).unwrap()),
        char::from(b'A' + u8::try_from(index.div_euclid(26) % 26).unwrap())
    );
    (family, given, birth, postal)
}

/// The `Patient` JSON of the given demographics, without an identifier.
fn json(family: &str, given: &str, birth: &str, postal: &str) -> String {
    format!(
        r#"{{"resourceType":"Patient","name":[{{"family":"{family}","given":["{given}"]}}],"gender":"female","birthDate":"{birth}","address":[{{"postalCode":"{postal}"}}]}}"#
    )
}

/// The blocking columns the feed would write for `json`.
fn blocks(json: &str) -> (Option<String>, Option<String>) {
    let value: Value = serde_json::from_str(json).unwrap();
    let patient =
        FhirPatient::from_json(value.as_object().unwrap(), &mut Path::root("Patient")).unwrap();
    let subject = Subject::from_patient(&patient, BTreeSet::new());
    (
        text_key(BlockingKey::BirthYearFamilyPhonetic, &subject),
        text_key(BlockingKey::PostalCodeBirthDate, &subject),
    )
}

/// Loads `RECORDS` synthetic records into the practice domain, each in a
/// person of its own, plus `twin`, a record equal to the probe's patient.
async fn load(store: &Store, twin: &str) {
    let domain = super::domain(store, PRACTICE_DOMAIN).await;
    let keys = store.sealer();
    let key_id = keys.key_id().as_bytes().to_vec();
    let mut documents: Vec<String> = (0..RECORDS)
        .map(|index| {
            let (family, given, birth, postal) = demographics(index);
            json(&family, &given, &birth, &postal)
        })
        .collect();
    documents.push(twin.to_owned());
    for (batch, chunk) in documents.chunks(BATCH).enumerate() {
        let mut records = Vec::new();
        let mut persons = Vec::new();
        let mut hmacs = Vec::new();
        let mut values = Vec::new();
        let mut phonetic = Vec::new();
        let mut postal = Vec::new();
        for (offset, document) in chunk.iter().enumerate() {
            let value = format!("P-{}", batch * BATCH + offset);
            let identifier =
                ferropix_server::store::seal::IdentifierValue::new(value.clone()).unwrap();
            records.push(Uuid::new_v4());
            persons.push(Uuid::new_v4());
            hmacs.push(keys.hmac(domain.id.uuid(), &identifier).to_vec());
            values.push(value);
            let (p, q) = blocks(document);
            phonetic.push(p);
            postal.push(q);
        }
        let mut transaction = store.pool().begin().await.unwrap();
        sqlx::query("INSERT INTO pix.person (id) SELECT unnest($1::uuid[])")
            .bind(&persons)
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO pix.source_record (id, domain_id, identifier_hmac, identifier_value, key_id)
             SELECT r, $2, h, v, $5 FROM unnest($1::uuid[], $3::bytea[], $4::text[]) AS t(r, h, v)",
        )
        .bind(&records)
        .bind(domain.id.uuid())
        .bind(&hmacs)
        .bind(&values)
        .bind(&key_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pix.source_record_version
                 (record_id, version, demographics, channel, block_phonetic, block_postal)
             SELECT r, 1, d::jsonb, 'fhir', p, q
             FROM unnest($1::uuid[], $2::text[], $3::text[], $4::text[]) AS t(r, d, p, q)",
        )
        .bind(&records)
        .bind(chunk)
        .bind(&phonetic)
        .bind(&postal)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pix.link (record_id, person_id, kind, valid, made_by)
             SELECT r, p, 'feed', tstzrange(now(), NULL), 'e2e'
             FROM unnest($1::uuid[], $2::uuid[]) AS t(r, p)",
        )
        .bind(&records)
        .bind(&persons)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }
}

/// The probe's patient, and the twin record equal to it.
fn probe(system: &'static str, value: &str) -> (Patient, String) {
    let mut patient = Patient::new(system, value, "Bakma", GIVEN_NAMES[0]);
    patient.birth = String::from("1961-04-30");
    patient.postal = Some(String::from("1234AB"));
    let twin = json("Bakma", GIVEN_NAMES[0], "1961-04-30", "1234AB");
    (patient, twin)
}

#[tokio::test]
async fn a_feed_scores_only_the_pairs_its_blocking_keys_produce_among_100000_records() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let store = &started.store;
    let (patient, twin) = probe(HOSPITAL_DOMAIN, "H-90");
    load(store, &twin).await;
    let (phonetic, postal) = blocks(&twin);
    let sharing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.source_record_version WHERE block_phonetic = $1 OR block_postal = $2",
    )
    .bind(&phonetic)
    .bind(&postal)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM pix.source_record")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(i64::try_from(RECORDS + 1).unwrap(), total);

    let applied = feeder(store)
        .revise(&patient.revision(), &context())
        .await
        .unwrap();
    assert_eq!(
        usize::try_from(sharing).unwrap(),
        applied.scored,
        "only the blocked pairs"
    );
    assert!(
        applied.scored < 200,
        "tens of pairs, not the table: {}",
        applied.scored
    );

    let certain: Vec<(String, String)> = sqlx::query_as(
        "SELECT grade, status FROM pix.candidate WHERE id = ANY($1) AND grade = 'certain'",
    )
    .bind(
        applied
            .candidates
            .iter()
            .map(|c| c.uuid())
            .collect::<Vec<_>>(),
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(
        vec![(String::from("certain"), String::from("open"))],
        certain
    );
    let linked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.link WHERE person_id = $1 AND upper_inf(valid)",
    )
    .bind(applied.person.unwrap().uuid())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        1, linked,
        "auto_link = false leaves the certain pair a candidate"
    );

    automatic_link_and_estimate(store, applied.record).await;
    started.store.close().await;
}

/// With `auto_link = true`, a second probe links to the certain pairs, and
/// `matching estimate` reads the table's frequencies.
async fn automatic_link_and_estimate(
    store: &Store,
    probe_record: ferropix_server::store::record::RecordId,
) {
    let automatic = Feeder::new(
        store.clone(),
        &Matching {
            auto_link: true,
            ..Matching::default()
        }
        .resolve()
        .unwrap(),
    );
    let (again, _) = probe(LABORATORY_DOMAIN, "L-90");
    let joined = automatic
        .revise(&again.revision(), &context())
        .await
        .unwrap();
    assert!(
        !joined.merged.is_empty(),
        "auto_link = true links the certain pair"
    );
    assert_eq!(
        person_of(store, probe_record).await,
        person_of(store, joined.record).await,
        "the probe and the second probe are one person"
    );
    let kind: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM pix.merge WHERE kind = 'probabilistic'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert!(!kind.is_empty(), "a probabilistic merge is recorded");

    let estimates = ferropix_server::store::estimate::estimate(store)
        .await
        .unwrap();
    let (u, records) = estimates[&Field::FamilyName];
    assert!(
        records > 100_000 && u > 0.0 && u < 0.95,
        "{u} over {records}"
    );
    let tables = ferropix_server::estimate_tables(&Matching::default(), &estimates);
    assert!(tables.contains("[matching.fields.family_name]"), "{tables}");
    assert!(
        !tables.contains("Bakma") && !tables.contains("bakma"),
        "no value in the tables"
    );
}
