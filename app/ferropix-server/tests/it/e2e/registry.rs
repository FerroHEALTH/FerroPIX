// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-93 received over PostgreSQL 18 (#103), behind `FERROPIX_E2E`: the
//! four vendored PMIR 1.6.0 message Bundles and an unmerge attempt, each in
//! FHIR JSON and XML, with the response's per-entry codes; a merged master
//! identity read inactive with its `replaced-by` link in ITI-78 and its
//! identifiers resolved to the survivor in ITI-83; the feed of a client that
//! is not the master domain's Source and of a Patient the Registry does not
//! hold refused; and one audit record per message the Registry reads.
//!
//! The vendored Bundles carry ids that are no UUID, which the Registry
//! refuses; the cases that apply them give each Patient a synthetic UUID.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the Bundles and answers are JSON values"
)]

use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use fhir_types::codec::{Object, Value as Document};
use http::{Method, StatusCode, header};
use serde_json::{Value, json};

use super::BSN_SYSTEM;
use super::pixm::{Answer, Harness, SOURCE, STRANGER, pix_uri, synthetic_seeds};
use crate::auth::bearer;

/// The master domain: the Registry's public base URL (PMIR 1.6.0
/// §2:3.93.4.1.3), as the harness serves it.
const MASTER: &str = "http://localhost:8080";

/// The FHIR JSON media type.
const FHIR_JSON: &str = "application/fhir+json";

/// The FHIR XML media type.
const FHIR_XML: &str = "application/fhir+xml";

/// A synthetic BSN that passes the eleven-proof.
const BSN: &str = "111222333";

/// The synthetic hospital number fed beside the second master identity.
const HOSPITAL: &str = "H-930001";

/// The two formats a Source sends in (§2:3.93.4.1.2).
const FORMATS: [&str; 2] = [FHIR_JSON, FHIR_XML];

/// The seeds with the master domain beside the synthetic ones, fed by
/// [`SOURCE`].
fn with_master() -> Vec<DomainSeed> {
    let mut seeds = synthetic_seeds();
    seeds.push(DomainSeed {
        system: MASTER.to_owned(),
        name: String::from("Master Patient Identification Domain"),
        kind: DomainKind::Master,
        source_client_id: SOURCE.to_owned(),
        sensitive: false,
    });
    seeds
}

/// The vendored PMIR 1.6.0 message Bundle `name` (`Create`, `Update`,
/// `Merge` or `Delete`), each id in `ids` replaced by its UUID.
fn bundle(name: &str, ids: &[(&str, &str)]) -> Value {
    let path = format!(
        "{}/../../docs/specs/ihe-pmir/package/example/Bundle-ex-PMIRBundle{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut text = std::fs::read_to_string(path).expect("the example reads");
    for (id, uuid) in ids {
        text = text.replace(id, uuid);
    }
    serde_json::from_str(&text).expect("JSON")
}

/// `bundle` as the body of `media_type`.
fn body(bundle: &Value, media_type: &str) -> String {
    let json = bundle.to_string();
    if media_type == FHIR_JSON {
        return json;
    }
    let document: Document = serde_json::from_str(&json).expect("a document");
    let object: &Object = document.as_object().expect("a resource");
    fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object).expect("an XML form")
}

/// Sends `bundle` to `POST /fhir` in `media_type` with `token`, asking for
/// JSON.
async fn feed_as(harness: &Harness, token: &str, bundle: &Value, media_type: &str) -> Answer {
    harness
        .send(
            Method::POST,
            "/fhir",
            Some(token),
            &[
                (header::CONTENT_TYPE, media_type),
                (header::ACCEPT, FHIR_JSON),
            ],
            Some(body(bundle, media_type)),
        )
        .await
}

/// Sends `bundle` as the master domain's Source.
async fn feed(harness: &Harness, bundle: &Value, media_type: &str) -> Answer {
    feed_as(harness, &harness.source(), bundle, media_type).await
}

/// The response's `MessageHeader`.
fn header_of(answer: &Answer) -> Value {
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    let response = answer.json();
    assert_eq!("message", response["type"], "{}", answer.body);
    let entries = response["entry"].as_array().expect("entries");
    assert_eq!(1, entries.len(), "one MessageHeader (§2:3.93.4.2.2)");
    let header = entries
        .first()
        .map(|entry| entry["resource"].clone())
        .expect("the header");
    assert_eq!("MessageHeader", header["resourceType"]);
    header
}

/// Each refused entry of the response as `(expression, code, diagnostics)`,
/// empty when the response code is `ok`.
fn refused(answer: &Answer) -> Vec<(String, String, String)> {
    let header = header_of(answer);
    let code = header["response"]["code"].as_str().unwrap_or_default();
    if code == "ok" {
        assert!(header["response"]["details"].is_null());
        return Vec::new();
    }
    assert_eq!("fatal-error", code, "D35");
    assert_eq!("#outcome", header["response"]["details"]["reference"]);
    header["contained"][0]["issue"]
        .as_array()
        .expect("issues")
        .iter()
        .map(|issue| {
            (
                issue["expression"][0]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                issue["code"].as_str().unwrap_or_default().to_owned(),
                issue["diagnostics"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// The history entry `index` as the response's `expression` names it.
fn entry(index: usize) -> String {
    format!("Bundle.entry[1].resource.entry[{index}]")
}

/// Reads `Patient/{id}` through ITI-78.
async fn read_patient(harness: &Harness, id: &str) -> Answer {
    harness
        .send(
            Method::GET,
            &format!("/fhir/Patient/{id}"),
            Some(&harness.consumer()),
            &[(header::ACCEPT, FHIR_JSON)],
            None,
        )
        .await
}

/// The identifiers of the ITI-83 answer for `system|value`.
async fn resolved(harness: &Harness, system: &str, value: &str) -> Answer {
    harness
        .send(
            Method::GET,
            &pix_uri(system, value, &[]),
            Some(&harness.consumer()),
            &[(header::ACCEPT, FHIR_JSON)],
            None,
        )
        .await
}

#[tokio::test]
async fn the_vendored_bundles_are_answered_entry_by_entry_and_their_ids_refused_as_no_uuid() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    for media_type in FORMATS {
        for (name, entries) in [("Create", 2), ("Update", 1), ("Merge", 1), ("Delete", 1)] {
            let before = harness.records();
            let answer = feed(&harness, &bundle(name, &[]), media_type).await;
            let label = format!("{name} in {media_type}");
            let expected: Vec<(String, String, String)> = (0..entries)
                .map(|index| {
                    (
                        entry(index),
                        String::from("invalid"),
                        String::from(
                            "400: the Patient id is no UUID, which a master identity's id is",
                        ),
                    )
                })
                .collect();
            assert_eq!(expected, refused(&answer), "{label}");
            assert_eq!(before + 1, harness.records(), "{label}: one record");
        }
    }
    let persons: i64 = sqlx::query_scalar("SELECT count(*) FROM pix.person")
        .fetch_one(harness.store.pool())
        .await
        .expect("the persons count");
    assert_eq!(0, persons, "nothing was applied");
}

/// The first master identity of the round trip, the survivor.
const FIRST: &str = "3f0c2a5e-0000-4000-8000-000000000001";

/// The second master identity of the round trip, merged into the first.
const SECOND: &str = "3f0c2a5e-0000-4000-8000-000000000002";

/// Asserts the response applied every change.
fn applied(answer: &Answer) {
    assert_eq!(Vec::<(String, String, String)>::new(), refused(answer));
}

/// The `targetId` references of an ITI-83 answer.
fn targets(answer: &Answer) -> Vec<String> {
    answer.json()["parameter"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|parameter| parameter["name"] == "targetId")
        .filter_map(|parameter| parameter["valueReference"]["reference"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Creates both master identities from the vendored create Bundle, the
/// second carrying the BSN, and feeds a hospital record with that BSN,
/// which joins the second.
async fn create_both(harness: &Harness, media_type: &str) {
    let mut created = bundle(
        "Create",
        &[
            ("ex-patient-create1", FIRST),
            ("ex-patient-create2", SECOND),
        ],
    );
    created["entry"][1]["resource"]["entry"][1]["resource"]["identifier"] =
        json!([{"system": BSN_SYSTEM, "value": BSN}]);
    let before = harness.records();
    applied(&feed(harness, &created, media_type).await);
    assert_eq!(before + 1, harness.records(), "one record per message");
    let read = read_patient(harness, FIRST).await;
    assert_eq!(StatusCode::OK, read.status, "{}", read.body);
    assert_eq!(
        FIRST,
        read.json()["id"],
        "the Source's id is the person's id"
    );

    let hospital = json!({
        "resourceType": "Patient",
        "identifier": [
            {"system": HOSPITAL_DOMAIN, "value": HOSPITAL},
            {"system": BSN_SYSTEM, "value": BSN}
        ],
        "name": [{"family": "Wooten", "given": ["Lucille"]}],
        "gender": "female",
        "birthDate": "1971-12-14"
    });
    let fed = harness
        .feed(
            HOSPITAL_DOMAIN,
            HOSPITAL,
            &serde_json::from_value(hospital).expect("a document"),
        )
        .await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    let joined = resolved(harness, HOSPITAL_DOMAIN, HOSPITAL).await;
    assert_eq!(
        vec![format!("Patient/{SECOND}")],
        targets(&joined),
        "the hospital record joins the second master identity: {}",
        joined.body
    );
}

/// Merges the second master identity into the first and reads the result
/// through ITI-78 and ITI-83.
async fn merge_second_into_first(harness: &Harness, media_type: &str) {
    let merge = bundle(
        "Merge",
        &[("ex-patient-merged", FIRST), ("ex-patient-merge", SECOND)],
    );
    applied(&feed(harness, &merge, media_type).await);
    let merged = read_patient(harness, SECOND).await;
    assert_eq!(StatusCode::OK, merged.status, "{}", merged.body);
    let merged = merged.json();
    assert_eq!(false, merged["active"], "the merged person reads inactive");
    assert_eq!("replaced-by", merged["link"][0]["type"]);
    assert_eq!(
        format!("Patient/{FIRST}"),
        merged["link"][0]["other"]["reference"]
    );
    for (system, value) in [(HOSPITAL_DOMAIN, HOSPITAL), (MASTER, SECOND)] {
        let answer = resolved(harness, system, value).await;
        assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
        assert_eq!(
            vec![format!("Patient/{FIRST}")],
            targets(&answer),
            "{system} resolves to the survivor: {}",
            answer.body
        );
    }
}

/// Runs the vendored Bundles with UUID ids in `media_type`: two creates,
/// an update, a merge, an unmerge attempt and a delete.
async fn round_trip(media_type: &str) {
    let harness = Harness::start(with_master()).await;
    create_both(&harness, media_type).await;
    let update = bundle("Update", &[("ex-patient-update", FIRST)]);
    applied(&feed(&harness, &update, media_type).await);
    merge_second_into_first(&harness, media_type).await;

    let unmerge = bundle("Update", &[("ex-patient-update", SECOND)]);
    assert_eq!(
        vec![(
            entry(0),
            String::from("not-supported"),
            String::from("405: an unmerge is not supported by this transaction"),
        )],
        refused(&feed(&harness, &unmerge, media_type).await),
        "an unmerge through the feed (§2:3.93.4.1.3)"
    );
    let still = read_patient(&harness, SECOND).await;
    assert_eq!(false, still.json()["active"], "the merge stands");

    let delete = bundle("Delete", &[("ex-patient-delete", FIRST)]);
    applied(&feed(&harness, &delete, media_type).await);
    let removed = resolved(&harness, MASTER, FIRST).await;
    assert_eq!(
        StatusCode::NOT_FOUND,
        removed.status,
        "the master record is removed: {}",
        removed.body
    );
}

#[tokio::test]
async fn the_vendored_bundles_with_uuid_ids_create_update_merge_and_delete_in_json() {
    if !e2e_enabled() {
        return;
    }
    round_trip(FHIR_JSON).await;
}

#[tokio::test]
async fn the_vendored_bundles_with_uuid_ids_create_update_merge_and_delete_in_xml() {
    if !e2e_enabled() {
        return;
    }
    round_trip(FHIR_XML).await;
}

#[tokio::test]
async fn a_feed_from_another_client_or_of_a_patient_the_registry_does_not_hold_is_refused() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    let unknown = "3f0c2a5e-0000-4000-8000-0000000000ff";
    let updated = bundle("Update", &[("ex-patient-update", unknown)]);
    let stranger = bearer(&harness.issuer.token(STRANGER, "system/Patient.cud"));
    for media_type in FORMATS {
        let answer = feed_as(&harness, &stranger, &updated, media_type).await;
        assert_eq!(StatusCode::FORBIDDEN, answer.status, "{}", answer.body);
        assert_eq!("forbidden", answer.issue().0);

        let before = harness.records();
        let answer = feed(&harness, &updated, media_type).await;
        assert_eq!(
            vec![(
                entry(0),
                String::from("not-found"),
                String::from("404: the Patient is no master identity this Registry holds"),
            )],
            refused(&answer),
            "{media_type}"
        );
        assert_eq!(before + 1, harness.records(), "one record per message");
    }
    let without = Harness::start(synthetic_seeds()).await;
    let answer = feed(&without, &updated, FHIR_JSON).await;
    assert_eq!(
        StatusCode::FORBIDDEN,
        answer.status,
        "with no master domain no client is its Source: {}",
        answer.body
    );
}
