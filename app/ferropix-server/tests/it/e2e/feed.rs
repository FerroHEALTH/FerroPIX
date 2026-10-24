// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The feed transaction against PostgreSQL 18: concurrent joins, the hard
//! conflict, the Source's duplicate resolution and removal (PIXm 3.1.0
//! §2:3.104.4.2.3, §2:3.104.4.3.3), a local domain's shared value, the
//! steward action of every system action, the normalised identifier, the
//! demographics without an identifier, the reseal of carried identifiers
//! and the BALP record written after the commit.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the candidate's evidence and the audit record are read as JSON"
)]

use ferropix_server::feed::{Action, FeedError};
use ferropix_server::store::reseal;
use ferropix_server::store::seal::Sealer;
use ferropix_server::store::xref::{self, Lookup};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{
    FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};
use uuid::Uuid;

use super::feeds::{Patient, context, feeder, person_of, start};
use super::{BSN_SYSTEM, bsn};

/// Returns the identifier values of `lookup`, by system.
fn values(lookup: &Lookup) -> Vec<(String, String)> {
    match lookup {
        Lookup::Found(found) => found
            .identifiers
            .iter()
            .map(|(system, value)| (system.clone(), value.expose().to_owned()))
            .collect(),
        other => panic!("found: {other:?}"),
    }
}

#[tokio::test]
async fn two_feeds_that_join_the_same_persons_concurrently_both_commit_with_one_person() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    for round in 0..12 {
        let shared = bsn(round).expose().to_owned();
        let hospital = Patient::new(
            HOSPITAL_DOMAIN,
            &format!("H-{round}"),
            FAMILY_NAMES[0],
            GIVEN_NAMES[0],
        )
        .carrying(BSN_SYSTEM, &shared);
        let laboratory = Patient::new(
            LABORATORY_DOMAIN,
            &format!("L-{round}"),
            FAMILY_NAMES[0],
            GIVEN_NAMES[0],
        )
        .carrying(BSN_SYSTEM, &shared);
        let (first, second, ctx) = (hospital.revision(), laboratory.revision(), context());
        let (a, b) = tokio::join!(feeder.revise(&first, &ctx), feeder.revise(&second, &ctx));
        let (a, b) = (
            a.expect("the first commits"),
            b.expect("the second commits"),
        );
        assert_eq!(a.person, b.person, "round {round}: one person");
        assert_eq!(
            person_of(&started.store, a.record).await,
            person_of(&started.store, b.record).await
        );
    }
    for round in 12..20 {
        let shared = bsn(round).expose().to_owned();
        let alone =
            |system, value: String| Patient::new(system, &value, FAMILY_NAMES[1], GIVEN_NAMES[1]);
        let hospital = alone(HOSPITAL_DOMAIN, format!("H-{round}"));
        let laboratory = alone(LABORATORY_DOMAIN, format!("L-{round}"));
        let first = feeder
            .revise(&hospital.revision(), &context())
            .await
            .unwrap();
        let second = feeder
            .revise(&laboratory.revision(), &context())
            .await
            .unwrap();
        assert_ne!(first.person, second.person, "two persons before the BSN");
        let joined_hospital = hospital.clone().carrying(BSN_SYSTEM, &shared).revision();
        let joined_laboratory = laboratory.clone().carrying(BSN_SYSTEM, &shared).revision();
        let ctx = context();
        let (a, b) = tokio::join!(
            feeder.revise(&joined_hospital, &ctx),
            feeder.revise(&joined_laboratory, &ctx),
        );
        let (a, b) = (
            a.expect("the first commits"),
            b.expect("the second commits"),
        );
        assert_eq!(
            person_of(&started.store, a.record).await,
            person_of(&started.store, b.record).await,
            "round {round}: joined into one person"
        );
        let merged: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pix.person WHERE id = ANY($1) AND status = 'merged'",
        )
        .bind(vec![
            first.person.unwrap().uuid(),
            second.person.unwrap().uuid(),
        ])
        .fetch_one(started.store.pool())
        .await
        .unwrap();
        assert_eq!(1, merged, "the younger person is merged into the older");
    }
    started.store.close().await;
}

#[tokio::test]
async fn a_feed_with_a_hard_conflict_produces_a_candidate_and_no_link() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let shared = bsn(30).expose().to_owned();
    let hospital = Patient::new(HOSPITAL_DOMAIN, "H-30", FAMILY_NAMES[2], GIVEN_NAMES[2])
        .carrying(BSN_SYSTEM, &shared);
    let mut laboratory = Patient::new(LABORATORY_DOMAIN, "L-30", FAMILY_NAMES[2], GIVEN_NAMES[2])
        .carrying(BSN_SYSTEM, &shared);
    laboratory.birth = String::from("1975-04-12");
    let first = feeder
        .revise(&hospital.revision(), &context())
        .await
        .unwrap();
    let second = feeder
        .revise(&laboratory.revision(), &context())
        .await
        .unwrap();
    assert_ne!(first.person, second.person, "no link across the conflict");
    assert_eq!(1, second.candidates.len(), "one candidate");
    let (status, grade, evidence): (String, String, serde_json::Value) =
        sqlx::query_as("SELECT status, grade, evidence FROM pix.candidate WHERE id = $1")
            .bind(second.candidates[0].uuid())
            .fetch_one(started.store.pool())
            .await
            .unwrap();
    assert_eq!("open", status);
    assert_ne!("certainly-not", grade);
    assert_eq!(serde_json::json!(["birth_year"]), evidence["conflicts"]);
    started.store.close().await;
}

#[tokio::test]
async fn a_replaced_by_feed_moves_every_reference_to_the_survivor() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let (b1, b2) = (bsn(40).expose().to_owned(), bsn(41).expose().to_owned());
    let subsumed = Patient::new(HOSPITAL_DOMAIN, "H-40", FAMILY_NAMES[3], GIVEN_NAMES[3])
        .carrying(BSN_SYSTEM, &b1);
    let laboratory = Patient::new(LABORATORY_DOMAIN, "L-40", FAMILY_NAMES[3], GIVEN_NAMES[3])
        .carrying(BSN_SYSTEM, &b1);
    let survivor = Patient::new(HOSPITAL_DOMAIN, "H-41", FAMILY_NAMES[3], GIVEN_NAMES[3])
        .carrying(BSN_SYSTEM, &b2);
    let s = feeder
        .revise(&subsumed.revision(), &context())
        .await
        .unwrap();
    let l = feeder
        .revise(&laboratory.revision(), &context())
        .await
        .unwrap();
    let v = feeder
        .revise(&survivor.revision(), &context())
        .await
        .unwrap();
    assert_eq!(s.person, l.person);
    assert_ne!(s.person, v.person);

    let resolved = feeder
        .resolve_duplicate(&subsumed.subsumed_by("H-41"), &context())
        .await
        .unwrap();
    assert_eq!(Action::ResolveDuplicate, resolved.action);
    assert_eq!(v.person, resolved.person, "the survivor's person survives");
    assert_eq!(
        person_of(&started.store, l.record).await,
        v.person.map(ferropix_server::store::record::PersonId::uuid)
    );
    assert_eq!(
        None,
        person_of(&started.store, s.record).await,
        "the subsumed link closed"
    );
    let (active, replaced_by): (bool, Option<Uuid>) =
        sqlx::query_as("SELECT active, replaced_by FROM pix.source_record WHERE id = $1")
            .bind(s.record.uuid())
            .fetch_one(started.store.pool())
            .await
            .unwrap();
    assert_eq!((false, Some(v.record.uuid())), (active, replaced_by));
    let kind: String = sqlx::query_scalar("SELECT kind FROM pix.merge WHERE merged_id = $1")
        .bind(s.person.unwrap().uuid())
        .fetch_one(started.store.pool())
        .await
        .unwrap();
    assert_eq!("source", kind);

    let mut connection = started.store.pool().acquire().await.unwrap();
    let keys = started.store.sealer();
    let by_subsumed = xref::lookup(&mut connection, keys, HOSPITAL_DOMAIN, "H-40")
        .await
        .unwrap();
    let by_survivor = xref::lookup(&mut connection, keys, HOSPITAL_DOMAIN, "H-41")
        .await
        .unwrap();
    assert_eq!(
        values(&by_survivor),
        values(&by_subsumed),
        "the survivor's set"
    );
    let set = values(&by_subsumed);
    assert!(set.contains(&(HOSPITAL_DOMAIN.to_owned(), String::from("H-41"))));
    assert!(set.contains(&(LABORATORY_DOMAIN.to_owned(), String::from("L-40"))));
    assert!(!set.contains(&(HOSPITAL_DOMAIN.to_owned(), String::from("H-40"))));

    drop(connection);
    started.store.close().await;
}

#[tokio::test]
async fn a_removal_makes_the_identifier_not_found_and_leaves_the_versions_readable() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let keys = started.store.sealer();
    let b2 = bsn(41).expose().to_owned();
    let subsumed = Patient::new(HOSPITAL_DOMAIN, "H-40", FAMILY_NAMES[3], GIVEN_NAMES[3]);
    let laboratory = Patient::new(LABORATORY_DOMAIN, "L-40", FAMILY_NAMES[3], GIVEN_NAMES[3]);
    let survivor = Patient::new(HOSPITAL_DOMAIN, "H-41", FAMILY_NAMES[3], GIVEN_NAMES[3])
        .carrying(BSN_SYSTEM, &b2);
    feeder
        .revise(&subsumed.revision(), &context())
        .await
        .unwrap();
    let v = feeder
        .revise(&survivor.revision(), &context())
        .await
        .unwrap();
    feeder
        .resolve_duplicate(&subsumed.subsumed_by("H-41"), &context())
        .await
        .unwrap();
    let mut connection = started.store.pool().acquire().await.unwrap();
    let removed = feeder
        .remove(&survivor.removal(), &context())
        .await
        .unwrap();
    assert_eq!(Action::Remove, removed.action);
    for value in ["H-41", "H-40"] {
        assert_eq!(
            Lookup::NotFound,
            xref::lookup(&mut connection, keys, HOSPITAL_DOMAIN, value)
                .await
                .unwrap(),
            "{value} is not found after the removal"
        );
    }
    let versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.source_record_version WHERE record_id = $1")
            .bind(v.record.uuid())
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(1, versions, "the versions stay readable");
    assert!(matches!(
        feeder.remove(&survivor.removal(), &context()).await,
        Err(FeedError::NotFound)
    ));
    assert!(matches!(
        feeder
            .resolve_duplicate(&laboratory.subsumed_by("L-unknown"), &context())
            .await,
        Err(FeedError::SurvivorNotFound)
    ));
    assert!(matches!(
        feeder.revise(&subsumed.revision(), &context()).await,
        Err(FeedError::Subsumed)
    ));
    drop(connection);
    started.store.close().await;
}

#[tokio::test]
async fn a_local_domain_never_links_across_domains_on_a_shared_value() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let hospital = Patient::new(HOSPITAL_DOMAIN, "12345", FAMILY_NAMES[4], GIVEN_NAMES[4]);
    let laboratory = Patient::new(LABORATORY_DOMAIN, "12345", FAMILY_NAMES[5], GIVEN_NAMES[5])
        .carrying(PRACTICE_DOMAIN, "P-1");
    let practice_too = Patient::new(HOSPITAL_DOMAIN, "67890", FAMILY_NAMES[6], GIVEN_NAMES[6])
        .carrying(PRACTICE_DOMAIN, "P-1");
    let a = feeder
        .revise(&hospital.revision(), &context())
        .await
        .unwrap();
    let b = feeder
        .revise(&laboratory.revision(), &context())
        .await
        .unwrap();
    let c = feeder
        .revise(&practice_too.revision(), &context())
        .await
        .unwrap();
    assert_ne!(
        a.person, b.person,
        "one value in two local domains says nothing"
    );
    assert_ne!(
        b.person, c.person,
        "a shared local identifier is evidence, never a link"
    );
    let deterministic: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.link WHERE kind = 'deterministic'")
            .fetch_one(started.store.pool())
            .await
            .unwrap();
    assert_eq!(0, deterministic);
    started.store.close().await;
}

#[tokio::test]
async fn every_system_action_is_a_steward_action_row() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let shared = bsn(50).expose().to_owned();
    let hospital = Patient::new(HOSPITAL_DOMAIN, "H-50", FAMILY_NAMES[7], GIVEN_NAMES[7]);
    let laboratory = Patient::new(LABORATORY_DOMAIN, "L-50", FAMILY_NAMES[7], GIVEN_NAMES[7]);
    feeder
        .revise(&hospital.revision(), &context())
        .await
        .unwrap();
    feeder
        .revise(&laboratory.revision(), &context())
        .await
        .unwrap();
    feeder
        .revise(
            &hospital.clone().carrying(BSN_SYSTEM, &shared).revision(),
            &context(),
        )
        .await
        .unwrap();
    feeder
        .revise(
            &laboratory.clone().carrying(BSN_SYSTEM, &shared).revision(),
            &context(),
        )
        .await
        .unwrap();
    feeder
        .remove(&laboratory.removal(), &context())
        .await
        .unwrap();
    let pool = started.store.pool();
    let count = |sql: &'static str| async move {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(pool)
            .await
            .unwrap()
    };
    assert_eq!(
        4,
        count("SELECT count(*) FROM pix.steward_action WHERE kind = 'feed'").await
    );
    assert_eq!(
        1,
        count("SELECT count(*) FROM pix.steward_action WHERE kind = 'remove'").await
    );
    assert_eq!(
        count("SELECT count(*) FROM pix.link WHERE kind IN ('feed', 'deterministic')").await,
        count("SELECT count(*) FROM pix.steward_action WHERE kind = 'link'").await,
        "every link the feed opened is an action"
    );
    assert_eq!(
        0,
        count(
            "SELECT count(*) FROM pix.merge m WHERE NOT EXISTS
             (SELECT 1 FROM pix.steward_action a WHERE a.merge_id = m.id AND a.kind = 'merge')"
        )
        .await,
        "every merge is an action"
    );
    assert_eq!(
        0,
        count(
            "SELECT count(*) FROM pix.link l WHERE NOT upper_inf(l.valid) AND l.ended_by_action IS NULL"
        )
        .await,
        "every closed link names the action that closed it"
    );
    assert_eq!(
        0,
        count("SELECT count(*) FROM pix.steward_action WHERE actor <> 'synthetic-source'").await
    );
    started.store.close().await;
}

#[tokio::test]
async fn an_identifier_is_normalised_before_it_is_sealed_and_kept_out_of_the_demographics() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let digits = bsn(60).expose().to_owned();
    let spaced: String = digits
        .chars()
        .enumerate()
        .flat_map(|(i, c)| (i == 4).then_some(' ').into_iter().chain([c]))
        .collect();
    let first = feeder
        .revise(
            &Patient::new(BSN_SYSTEM, &spaced, FAMILY_NAMES[0], GIVEN_NAMES[1]).revision(),
            &context(),
        )
        .await
        .unwrap();
    let again = feeder
        .revise(
            &Patient::new(BSN_SYSTEM, &digits, FAMILY_NAMES[0], GIVEN_NAMES[1])
                .carrying(HOSPITAL_DOMAIN, "  H-60 ")
                .revision(),
            &context(),
        )
        .await
        .unwrap();
    assert_eq!(Action::Create, first.action);
    assert_eq!((Action::Update, first.record), (again.action, again.record));
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT demographics::text FROM pix.source_record_version WHERE record_id = $1",
    )
    .bind(first.record.uuid())
    .fetch_all(started.store.pool())
    .await
    .unwrap();
    for text in &stored {
        assert!(
            !text.contains(&digits) && !text.contains("H-60"),
            "no identifier stored"
        );
        assert!(!text.contains("identifier"), "{text}");
    }
    let carried: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT identifier_value FROM pix.carried_identifier WHERE record_id = $1 ORDER BY id",
    )
    .bind(first.record.uuid())
    .fetch_all(started.store.pool())
    .await
    .unwrap();
    assert!(
        carried.contains(&Some(String::from("H-60"))),
        "trimmed: {carried:?}"
    );
    assert!(carried.contains(&None), "the BSN is sealed");
    let mut broken = digits.clone();
    broken.pop();
    broken.push(if digits.ends_with('0') { '1' } else { '0' });
    let refused = feeder
        .revise(
            &Patient::new(BSN_SYSTEM, &broken, FAMILY_NAMES[0], GIVEN_NAMES[1]).revision(),
            &context(),
        )
        .await;
    assert!(
        matches!(refused, Err(FeedError::Identifier { .. })),
        "{refused:?}"
    );
    started.store.close().await;
}

#[tokio::test]
async fn db_reseal_rewrites_the_carried_identifiers_so_a_bsn_still_links() {
    if !e2e_enabled() {
        return;
    }
    let postgres = ferropix_testkit::containers::postgres().await.unwrap();
    let url = super::app_role(&postgres).await;
    let (old, new) = (super::fresh_key(), super::fresh_key());
    let store = ferropix_server::store::Store::start(
        &super::settings(&url, &old, true),
        &super::feeds::seeds(),
        ferropix_server::store::Job::Migrate,
    )
    .await
    .unwrap();
    let shared = bsn(70).expose().to_owned();
    let hospital = Patient::new(HOSPITAL_DOMAIN, "H-70", FAMILY_NAMES[1], GIVEN_NAMES[2])
        .carrying(BSN_SYSTEM, &shared);
    let first = feeder(&store)
        .revise(&hospital.revision(), &context())
        .await
        .unwrap();
    store.close().await;
    let resealed = ferropix_server::store::Store::connect(&super::settings(&url, &new, true))
        .await
        .unwrap();
    reseal::run(&resealed, &Sealer::new(&old).unwrap(), reseal::BATCH)
        .await
        .unwrap();
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.carried_identifier WHERE key_id <> $1")
            .bind(resealed.sealer().key_id().as_bytes().as_slice())
            .fetch_one(resealed.pool())
            .await
            .unwrap();
    assert_eq!(0, left, "every carried identifier is resealed");
    let laboratory = Patient::new(LABORATORY_DOMAIN, "L-70", FAMILY_NAMES[1], GIVEN_NAMES[2])
        .carrying(BSN_SYSTEM, &shared);
    let second = feeder(&resealed)
        .revise(&laboratory.revision(), &context())
        .await
        .unwrap();
    assert_eq!(
        first.person, second.person,
        "the BSN still links under the new key"
    );
    resealed.close().await;
}

#[tokio::test]
async fn the_feed_writes_its_balp_record_after_the_commit() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let feeder = feeder(&started.store);
    let spool = tempfile::tempdir().unwrap();
    let directory = spool.path().join("spool");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::set_permissions(
        &directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let base = url::Url::parse("https://pix.example.org/fhir").unwrap();
    let audit = ferropix_server::audit::Audit::open(
        &ferropix_server::config::section::audit::AuditSettings {
            url: None,
            spool_dir: directory.clone(),
            spool_max_bytes: 1 << 20,
            spool_max_events: 10,
            spool_write_timeout: std::time::Duration::from_secs(2),
            timeout: std::time::Duration::from_secs(2),
            retry_max: std::time::Duration::from_millis(200),
        },
        &base,
    )
    .unwrap();
    let access = ferrohealth_smart::resource::Access::Caller(std::sync::Arc::new(
        ferrohealth_smart::resource::Caller::new(
            super::feeds::SOURCE.to_owned(),
            Some(super::feeds::SOURCE.to_owned()),
            "system/Patient.cud",
            None,
            false,
        ),
    ));
    let call = ferropix_server::audit::records::Call::of(&access, None, &base, Some("e2e"));
    let patient = Patient::new(HOSPITAL_DOMAIN, "H-80", FAMILY_NAMES[2], GIVEN_NAMES[3]);
    let revision = patient.revision();
    let applied = feeder.revise(&revision, &context()).await;
    ferropix_server::feed::record(
        &audit,
        None,
        &call,
        revision.identifier(),
        false,
        applied.as_ref(),
    )
    .await
    .unwrap();
    let records: Vec<serde_json::Value> = std::fs::read_dir(&directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "msg"))
        .map(|entry| serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap())
        .collect();
    assert_eq!(1, records.len(), "one record in the spool");
    assert_eq!(
        "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Create.Audit.Manager",
        records[0]["meta"]["profile"][0]
    );
    let person = format!("Patient/{}", applied.unwrap().person.unwrap().uuid());
    assert!(
        records[0].to_string().contains(&person),
        "the person is the data"
    );
    started.store.close().await;
}
