// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The producer assertions of the patient lookup in Xt-EHR D8.2 (2026-05-19)
//! §5.1.3, over the PDQm Supplier and the shared population: one match
//! answers exactly one record (TA-api-producer-patient-01), no match an
//! empty result (-02), and several matches every one of them with its match
//! grade (-03). Where -03 allows an error, PDQm 3.2.0 leads: ITI-78 answers
//! every match (§2:3.78.4.2.2.1) and ITI-119 grades each (§2:3.119.4.2.2.1).
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON"
)]

use std::collections::BTreeSet;

use ferropix_conformance::{record, verdict};
use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::profiles::Deprecated;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::FAMILY_NAMES;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::e2e::supplier::{Answered, populate, request, serve};

/// The persons of the `match` entries of a searchset, with each grade a
/// `$match` entry carries.
fn matched(answer: &Answered) -> Vec<(Uuid, Option<String>)> {
    answer.json()["entry"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["search"]["mode"] == "match")
        .map(|entry| {
            let id = entry["resource"]["id"].as_str().expect("an id");
            let grade = entry["search"]["extension"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|extension| {
                    extension["url"] == "http://hl7.org/fhir/StructureDefinition/match-grade"
                })
                .and_then(|extension| extension["valueCode"].as_str())
                .map(str::to_owned);
            (Uuid::parse_str(id).expect("a person id"), grade)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn conformance_the_patient_lookup_holds_the_xt_ehr_producer_assertions() {
    if !e2e_enabled() {
        return;
    }
    let served = serve(Deprecated::Inactive, &Matching::default()).await;
    let p = populate(&served.started.store).await;
    let token = served.consumer();
    let get = |query: String| {
        let (app, token) = (served.app.clone(), token.clone());
        async move {
            request(
                &app,
                Method::GET,
                &format!("/fhir/Patient?{query}"),
                &[("authorization", &token)],
                Vec::new(),
            )
            .await
        }
    };
    let one = get(format!(
        "family={}&given=Bram&gender=male&birthdate=1970-01-01",
        FAMILY_NAMES[1]
    ))
    .await;
    let none = get(String::from(
        "family=Nobody&given=Nemo&birthdate=1900-01-01",
    ))
    .await;
    let several = get(format!(
        "family={}&given=Ada&birthdate=1961-04-12",
        FAMILY_NAMES[0]
    ))
    .await;
    let input = json!({"resourceType": "Parameters", "parameter": [{"name": "resource", "resource": {
        "resourceType": "Patient",
        "name": [{"family": FAMILY_NAMES[0], "given": ["Ada"]}],
        "gender": "female",
        "birthDate": "1961-04-12"
    }}]});
    let graded = request(
        &served.app,
        Method::POST,
        "/fhir/Patient/$match",
        &[
            ("authorization", &token),
            ("content-type", "application/fhir+json"),
        ],
        input.to_string().into_bytes(),
    )
    .await;
    let ada = BTreeSet::from([p.ada_hospital, p.ada_laboratory]);
    let bram = p.bram;
    let cases = vec![
        verdict(String::from("TA-api-producer-patient-01"), async move {
            assert_eq!(StatusCode::OK, one.status, "{}", one.body);
            let found = matched(&one);
            assert_eq!(
                vec![bram],
                found.iter().map(|(id, _)| *id).collect::<Vec<_>>()
            );
        })
        .await,
        verdict(String::from("TA-api-producer-patient-02"), async move {
            assert_eq!(StatusCode::OK, none.status, "{}", none.body);
            let json: Value = none.json();
            assert_eq!("searchset", json["type"]);
            assert_eq!(0, json["total"], "an empty result");
            assert_eq!(Vec::<(Uuid, Option<String>)>::new(), matched(&none));
        })
        .await,
        verdict(String::from("TA-api-producer-patient-03"), async move {
            assert_eq!(StatusCode::OK, several.status, "{}", several.body);
            let searched: BTreeSet<Uuid> =
                matched(&several).into_iter().map(|(id, _)| id).collect();
            assert_eq!(ada, searched, "ITI-78 answers every match");
            assert_eq!(StatusCode::OK, graded.status, "{}", graded.body);
            let found = matched(&graded);
            let persons: BTreeSet<Uuid> = found.iter().map(|(id, _)| *id).collect();
            assert_eq!(ada, persons, "ITI-119 answers every match");
            assert!(
                found.iter().all(|(_, grade)| grade.is_some()),
                "each with its match grade"
            );
        })
        .await,
    ];
    record(
        "xtehr-patient-match",
        "The producer assertions of the Xt-EHR D8.2 (2026-05-19) patient lookup, §5.1.3, over PDQm 3.2.0",
        cases,
    );
}
