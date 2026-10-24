// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-119 against PostgreSQL 18 through the guarded route: the ten cases
//! of PDQm 3.2.0 §2:3.119.4.1.3 with the testkit's and the vendored
//! examples, the score and the grade of each entry equal to the scorer's
//! for the same pair and following the configured thresholds, the bare
//! `Patient` form, the mother's maiden name read and written
//! (§2:3.119.4.1.2.1, §2:3.119.4.2.2.2), XML, the Match Input and Output
//! profiles, and one BALP record per patient returned (BALP 1.1.4
//! §1:52.4.1.6).
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers and the audit records are read as JSON"
)]

use std::collections::BTreeSet;

use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::profiles::Deprecated;
use ferropix_server::feed::demographics;
use ferropix_server::matching::scorer::{Grade, Scorer};
use ferropix_server::matching::subject::Subject;
use ferropix_server::pdqm::FEW_FIELDS_WARNING;
use ferropix_server::store::Store;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::FAMILY_NAMES;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

use super::supplier::{
    Answered, PHONE, Population, Served, example, holds_to, over, patients_of, populate, profile,
    request, serve,
};

/// Starts the store with the shared population, served with `matching`.
async fn populated(matching: &Matching) -> (Served, Population) {
    let served = serve(Deprecated::Inactive, matching).await;
    let population = populate(&served.started.store).await;
    (served, population)
}

/// Bram as a Consumer knows him: every element the scorer reads.
fn bram() -> Value {
    json!({
        "resourceType": "Patient",
        "name": [{"family": FAMILY_NAMES[1], "given": ["Bram"]}],
        "gender": "male",
        "birthDate": "1970-01-01",
        "address": [{"line": ["Proeflaan 2"], "city": "Monsterdam", "postalCode": "9999 ZZ"}],
        "telecom": [{"system": "phone", "value": PHONE}],
        "extension": [{
            "url": "http://hl7.org/fhir/StructureDefinition/patient-mothersMaidenName",
            "valueString": FAMILY_NAMES[4]
        }]
    })
}

/// Ada as a Consumer knows her, which both of her persons match.
fn ada() -> Value {
    json!({
        "resourceType": "Patient",
        "name": [{"family": FAMILY_NAMES[0], "given": ["Ada"]}],
        "gender": "female",
        "birthDate": "1961-04-12",
        "address": [{"line": ["Voorbeeldstraat 1"], "city": "Teststad", "postalCode": "0000 AA"}]
    })
}

/// Returns the `Parameters` of `patient` with the optional parameters.
fn parameters(patient: &Value, only_certain: Option<bool>, count: Option<i64>) -> Value {
    let mut parameter = vec![json!({"name": "resource", "resource": patient})];
    if let Some(flag) = only_certain {
        parameter.push(json!({"name": "onlyCertainMatches", "valueBoolean": flag}));
    }
    if let Some(count) = count {
        parameter.push(json!({"name": "count", "valueInteger": count}));
    }
    json!({"resourceType": "Parameters", "parameter": parameter})
}

/// Posts `body` of `media_type` to `$match`.
async fn post(served: &Served, query: &str, media_type: &str, body: &str) -> Answered {
    let token = served.consumer();
    request(
        &served.app,
        Method::POST,
        &format!("/fhir/Patient/$match{query}"),
        &[("authorization", &token), ("content-type", media_type)],
        body.as_bytes().to_vec(),
    )
    .await
}

/// Posts the JSON document `body` to `$match`.
async fn matched(served: &Served, body: &Value) -> Answered {
    post(served, "", "application/fhir+json", &body.to_string()).await
}

/// Posts the JSON document `body` to `$match`, asking for JSON and for XML,
/// asserts that both answer alike and returns the JSON answer.
async fn matched_both(served: &Served, body: &Value) -> Answered {
    let json = matched(served, body).await;
    let token = served.consumer();
    let xml = request(
        &served.app,
        Method::POST,
        "/fhir/Patient/$match",
        &[
            ("authorization", &token),
            ("content-type", "application/fhir+json"),
            ("accept", "application/fhir+xml"),
        ],
        body.to_string().into_bytes(),
    )
    .await;
    super::supplier::same_in_xml(&json, &xml, "$match");
    json
}

/// The persons, scores and grades of an answer's Patient entries, in order.
fn entries(answer: &Answered) -> Vec<(Uuid, f64, String)> {
    answer.json()["entry"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["resource"]["resourceType"] == "Patient")
        .map(|entry| {
            (
                Uuid::parse_str(entry["resource"]["id"].as_str().expect("an id")).expect("a uuid"),
                entry["search"]["score"].as_f64().expect("a score"),
                entry["search"]["extension"][0]["valueCode"]
                    .as_str()
                    .expect("a grade")
                    .to_owned(),
            )
        })
        .collect()
}

/// Returns the score and the grade `matching`'s scorer gives `input` against
/// the record of `person` whose demographics it serves.
async fn scorer_of(
    store: &Store,
    matching: &Matching,
    input: &Value,
    person: Uuid,
) -> (f64, Grade) {
    let stored: String = sqlx::query_scalar(
        "SELECT v.demographics::text FROM pix.link l
         JOIN pix.source_record r ON r.id = l.record_id
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         WHERE l.person_id = $1 AND upper_inf(l.valid) AND r.active
         ORDER BY r.id LIMIT 1",
    )
    .bind(person)
    .fetch_one(store.pool())
    .await
    .expect("the record reads");
    let other = demographics::patient(&stored).expect("a Patient");
    let input = demographics::patient(&input.to_string()).expect("a Patient");
    let scorer = Scorer::new(&matching.resolve().expect("the section resolves"));
    let pair = scorer.score(
        &Subject::from_patient(&input, BTreeSet::new()),
        &Subject::from_patient(&other, BTreeSet::new()),
        None,
    );
    (pair.score, pair.grade)
}

#[tokio::test]
async fn one_and_several_matches_carry_the_scorers_score_and_grade_most_likely_first() {
    if !e2e_enabled() {
        return;
    }
    let matching = Matching::default();
    let (served, p) = populated(&matching).await;
    let input = parameters(&bram(), None, None);
    holds_to(&input, &profile("MatchParametersIn"));
    let one = matched_both(&served, &input).await;
    assert_eq!(StatusCode::OK, one.status, "Case 1: {}", one.body);
    holds_to(&one.json(), &profile("MatchParametersOut"));
    let found = entries(&one);
    assert_eq!(1, found.len(), "Case 1: {found:?}");
    let (score, grade) = scorer_of(&served.started.store, &matching, &bram(), p.bram).await;
    assert_eq!(p.bram, found[0].0);
    assert!((score - found[0].1).abs() < 1e-12, "{score} {}", found[0].1);
    assert_eq!(grade.code(), found[0].2);
    assert_eq!("certain", found[0].2);
    let resource = &one.json()["entry"][0]["resource"];
    assert_eq!(
        FAMILY_NAMES[4], resource["extension"][0]["valueString"],
        "§2:3.119.4.2.2.2: the mother's maiden name, in its extension"
    );

    let several = matched_both(&served, &parameters(&ada(), None, None)).await;
    assert_eq!(StatusCode::OK, several.status, "Case 2");
    holds_to(&several.json(), &profile("MatchParametersOut"));
    let found = entries(&several);
    let persons: BTreeSet<Uuid> = found.iter().map(|(person, _, _)| *person).collect();
    assert_eq!(
        BTreeSet::from([p.ada_hospital, p.ada_laboratory]),
        persons,
        "both of Ada's persons, and never the merged one (Case 8)"
    );
    assert!(
        found.windows(2).all(|pair| pair[0].1 >= pair[1].1),
        "most likely first"
    );
    for (person, score, grade) in &found {
        let (expected, graded) = scorer_of(&served.started.store, &matching, &ada(), *person).await;
        assert!((expected - score).abs() < 1e-12, "{person}");
        assert_eq!(graded.code(), grade);
    }
}

#[tokio::test]
async fn only_certain_matches_and_count_answer_cases_three_to_six() {
    if !e2e_enabled() {
        return;
    }
    let matching = Matching::default();
    let (served, p) = populated(&matching).await;
    let certain = matched_both(&served, &parameters(&bram(), Some(true), None)).await;
    assert_eq!(StatusCode::OK, certain.status, "Case 3");
    let found = entries(&certain);
    assert_eq!(vec![p.bram], found.iter().map(|f| f.0).collect::<Vec<_>>());
    assert_eq!("certain", found[0].2);

    let probable = json!({
        "resourceType": "Patient",
        "name": [{"family": FAMILY_NAMES[1]}],
        "birthDate": "1970-01-01",
        "address": [{"postalCode": "1234 AB"}]
    });
    let (_, grade) = scorer_of(&served.started.store, &matching, &probable, p.bram).await;
    assert!(
        grade != Grade::Certain && grade != Grade::CertainlyNot,
        "the precondition: one match that is not certain ({grade:?})"
    );
    let without = matched_both(&served, &parameters(&probable, None, None)).await;
    assert_eq!(1, entries(&without).len(), "one match without the flag");
    let not_certain = matched_both(&served, &parameters(&probable, Some(true), None)).await;
    assert_eq!(StatusCode::OK, not_certain.status, "Case 4");
    assert!(entries(&not_certain).is_empty(), "Case 4: no entry");

    let both = matched_both(&served, &parameters(&ada(), None, None)).await;
    assert!(
        entries(&both).iter().filter(|f| f.2 == "certain").count() > 1,
        "the precondition: two certain persons"
    );
    let ambiguous = matched_both(&served, &parameters(&ada(), Some(true), None)).await;
    assert_eq!(StatusCode::OK, ambiguous.status, "Case 5");
    assert!(entries(&ambiguous).is_empty(), "Case 5: no entry");

    let cut = matched_both(&served, &parameters(&ada(), None, Some(1))).await;
    assert_eq!(StatusCode::OK, cut.status, "Case 6");
    let all = entries(&both);
    assert_eq!(
        vec![all[0].0],
        entries(&cut).iter().map(|f| f.0).collect::<Vec<_>>()
    );
    let testkit = matched(
        &served,
        &serde_json::from_str(&Fixture::MatchIn.text().expect("the fixture reads")).expect("JSON"),
    )
    .await;
    assert_eq!(StatusCode::OK, testkit.status, "the testkit's input");
    assert!(entries(&testkit).len() <= 5, "its count of 5");
}

#[tokio::test]
async fn no_match_errors_and_a_warning_answer_cases_seven_nine_and_ten() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(&Matching::default()).await;
    for file in [
        "Parameters-ex-match-input-patient-only.json",
        "Parameters-ex-match-input-onlyCertainMatches.json",
        "Patient-ex-match-input-patient.json",
    ] {
        let answer = post(&served, "", "application/fhir+json", &example(file)).await;
        assert_eq!(
            StatusCode::OK,
            answer.status,
            "Case 7, {file}: {}",
            answer.body
        );
        assert!(
            entries(&answer).is_empty(),
            "{file}: no synthetic patient matches"
        );
        holds_to(&answer.json(), &profile("MatchParametersOut"));
    }
    let bare = post(
        &served,
        "?count=1",
        "application/fhir+json",
        &bram().to_string(),
    )
    .await;
    assert_eq!(
        StatusCode::BAD_REQUEST,
        bare.status,
        "Case 9: a bare Patient with count"
    );
    let empty = matched(
        &served,
        &json!({"resourceType": "Parameters", "parameter": [
            {"name": "count", "valueInteger": 1}
        ]}),
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, empty.status, "Case 9: no Patient");
    assert_eq!("required", empty.issue().1);
    let text = post(&served, "", "text/plain", &bram().to_string()).await;
    assert_eq!(StatusCode::UNSUPPORTED_MEDIA_TYPE, text.status);
    let token = served.consumer();
    let unacceptable = request(
        &served.app,
        Method::POST,
        "/fhir/Patient/$match",
        &[
            ("authorization", &token),
            ("content-type", "application/fhir+json"),
            ("accept", "text/html"),
        ],
        bram().to_string().into_bytes(),
    )
    .await;
    assert_eq!(StatusCode::NOT_ACCEPTABLE, unacceptable.status);

    let few = json!({
        "resourceType": "Patient",
        "name": [{"family": FAMILY_NAMES[1]}],
        "birthDate": "1970-01-01"
    });
    let warned = matched_both(&served, &few).await;
    assert_eq!(StatusCode::OK, warned.status, "Case 10");
    assert_eq!(
        vec![p.bram],
        entries(&warned).iter().map(|f| f.0).collect::<Vec<_>>()
    );
    let json = warned.json();
    let outcome = json["entry"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["resource"]["resourceType"] == "OperationOutcome")
        .expect("Case 10: an OperationOutcome entry");
    assert_eq!("warning", outcome["resource"]["issue"][0]["severity"]);
    assert_eq!(
        FEW_FIELDS_WARNING,
        outcome["resource"]["issue"][0]["diagnostics"]
    );
    holds_to(&json, &profile("MatchParametersOut"));
}

#[tokio::test]
async fn a_bare_patient_and_xml_answer_as_the_parameters_do() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(&Matching::default()).await;
    let wrapped = matched_both(&served, &parameters(&bram(), None, None)).await;
    let bare = matched_both(&served, &bram()).await;
    assert_eq!(StatusCode::OK, bare.status, "§2:3.119.4.1.2: the bare form");
    assert_eq!(entries(&wrapped), entries(&bare));
    let xml = format!(
        r#"<Patient xmlns="http://hl7.org/fhir"><name><family value="{}"/><given value="Bram"/></name><gender value="male"/><birthDate value="1970-01-01"/></Patient>"#,
        FAMILY_NAMES[1]
    );
    let token = served.consumer();
    let answer = request(
        &served.app,
        Method::POST,
        "/fhir/Patient/$match",
        &[
            ("authorization", &token),
            ("content-type", "application/fhir+xml"),
            ("accept", "application/fhir+xml"),
        ],
        xml.into_bytes(),
    )
    .await;
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    assert!(answer.media_type.starts_with("application/fhir+xml"));
    assert!(answer.body.contains("<Bundle") && answer.body.contains(&p.bram.to_string()));
    assert!(answer.body.contains("match-grade"), "{}", answer.body);
}

#[tokio::test]
async fn the_grade_follows_the_configured_thresholds() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(&Matching::default()).await;
    let input = json!({
        "resourceType": "Patient",
        "name": [{"family": FAMILY_NAMES[1], "given": ["Bram"]}],
        "birthDate": "1970-01-01"
    });
    let strict = Matching {
        link_at: 0.999_999_9,
        review_at: 0.999_99,
        possible_at: 0.5,
        ..Matching::default()
    };
    let default = matched_both(&served, &input).await;
    let (app, issuer, _spool, _secrets) =
        over(&served.started.store, Deprecated::Inactive, &strict).await;
    let token = crate::auth::bearer(&issuer.token("east-lab", "ITI-119"));
    let graded = request(
        &app,
        Method::POST,
        "/fhir/Patient/$match",
        &[
            ("authorization", &token),
            ("content-type", "application/fhir+json"),
        ],
        input.to_string().into_bytes(),
    )
    .await;
    let (_, by_default) =
        scorer_of(&served.started.store, &Matching::default(), &input, p.bram).await;
    let (_, by_strict) = scorer_of(&served.started.store, &strict, &input, p.bram).await;
    assert_ne!(
        by_default, by_strict,
        "the precondition: the thresholds decide"
    );
    assert_eq!(by_default.code(), entries(&default)[0].2);
    assert_eq!(by_strict.code(), entries(&graded)[0].2);
}

#[tokio::test]
async fn a_match_is_one_balp_record_per_patient_returned() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(&Matching::default()).await;
    let before = served.records().len();
    let answer = matched(&served, &parameters(&ada(), None, None)).await;
    assert_eq!(StatusCode::OK, answer.status);
    let records = served.records();
    let fresh = &records[before..];
    let named: BTreeSet<Option<String>> = patients_of(fresh).into_iter().collect();
    assert_eq!(
        BTreeSet::from([
            Some(format!("Patient/{}", p.ada_hospital)),
            Some(format!("Patient/{}", p.ada_laboratory)),
        ]),
        named
    );
    assert_eq!(2, fresh.len(), "one record per patient");
    for record in fresh {
        assert_eq!(
            "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Match.Audit.Supplier",
            record["meta"]["profile"][0]
        );
        let raw = super::supplier::query_of(record);
        assert!(
            raw.starts_with("POST ") && raw.contains("\"Parameters\""),
            "{raw}"
        );
    }
    let before = served.records().len();
    let cut = matched(&served, &parameters(&ada(), None, Some(1))).await;
    assert_eq!(1, entries(&cut).len());
    assert_eq!(
        1,
        served.records().len() - before,
        "the patients returned, not found"
    );
    let before = served.records().len();
    let none = post(
        &served,
        "",
        "application/fhir+json",
        &example("Patient-ex-match-input-patient.json"),
    )
    .await;
    assert_eq!(StatusCode::OK, none.status);
    let records = served.records();
    assert_eq!(
        vec![None],
        patients_of(&records[before..]),
        "one record, no patient"
    );
}
