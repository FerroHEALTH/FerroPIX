// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Supplier side of ITI-78 and ITI-119 (feature `pdqm-server`), read
//! and written over the vendored PDQm 3.2.0 examples, in JSON and XML.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the written answers and the vendored examples are compared as JSON values"
)]

use std::fmt::Debug;

use fhir_types::codec::{Json, Object, Path};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pdqm::matches::MatchGrade;
use ihe_iti::pdqm::query::{Gender, StringMatch};
use ihe_iti::pdqm::server::matching::{MatchEntry, MatchError, MatchRequest, match_set};
use ihe_iti::pdqm::server::search::{Criterion, PatientSearch, SearchError, StringParameter};
use ihe_iti::pdqm::server::searchset::{Paging, SearchEntry, searchset};
use ihe_iti::pdqm::server::{Refusal, Score};
use ihe_iti::server::format::{Format, Unacceptable};
use secrecy::ExposeSecret;
use serde_json::Value;
use url::Url;

use super::vendored;
use crate::display::displays;

/// A value that must appear in no `Debug` rendering.
const SENTINEL: &str = "SENTINEL-4711";

/// The Supplier's `[base]/Patient` in the tests.
const ENDPOINT: &str = "https://pdq.example.org/fhir/Patient";

fn url(text: &str) -> Url {
    Url::parse(text).expect("a URL")
}

/// The vendored example `file` in `format`.
fn body(file: &str, format: Format) -> Vec<u8> {
    let json = vendored(file);
    if format == Format::Xml {
        let value: fhir_types::codec::Value = serde_json::from_str(&json).expect("JSON");
        let object: &Object = value.as_object().expect("a resource");
        return fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object)
            .expect("an XML form")
            .into_bytes();
    }
    json.into_bytes()
}

/// The Bundle of the vendored example `file`.
fn bundle(file: &str) -> Bundle {
    let value: fhir_types::codec::Value = serde_json::from_str(&vendored(file)).expect("JSON");
    Bundle::from_json(
        value.as_object().expect("a resource"),
        &mut Path::root("Bundle"),
    )
    .expect("an R4 Bundle")
}

/// The `(fullUrl, Patient)` of each Patient entry of `bundle`.
fn patients(bundle: &Bundle) -> Vec<(Url, Patient)> {
    bundle
        .entry
        .iter()
        .filter_map(|entry| match &entry.resource {
            Some(Resource::Patient(patient)) => Some((
                url(entry.full_url.as_ref()?.value.as_deref()?),
                (**patient).clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn every_parameter_of_the_table_is_read() {
    for (query, kind) in [
        ("_id=ex-patient", "Id"),
        ("active=true", "Active"),
        ("family=Schmidt", "Text"),
        ("given=John", "Text"),
        (
            "identifier=http://example.org/exampleSystem%7C8675309",
            "Identifier",
        ),
        ("telecom=phone%7C555-1212", "Telecom"),
        ("birthdate=1923-07-25", "Birthdate"),
        ("address=WI", "Text"),
        ("address-city=Madison", "Text"),
        ("address-country=USA", "Text"),
        ("address-postalcode=53703", "Text"),
        ("address-state=WI", "Text"),
        ("gender=other", "Gender"),
        ("mothersMaidenName=Jones", "Text"),
    ] {
        let search =
            PatientSearch::from_query(query).unwrap_or_else(|error| panic!("{query}: {error}"));
        let [criterion] = search.criteria() else {
            panic!("{query}: one criterion");
        };
        assert!(
            format!("{criterion:?}").starts_with(kind),
            "{query}: {criterion:?}"
        );
        assert!(search.dropped().is_empty(), "{query}");
    }
    let exact = PatientSearch::from_query("family:exact=Schmidt").expect("a search");
    assert!(
        matches!(
            exact.criteria(),
            [Criterion::Text {
                parameter: StringParameter::Family,
                matching: StringMatch::Exact,
                ..
            }]
        ),
        "the :exact modifier of §2:3.78.4.1.2.2"
    );
}

#[test]
fn a_gender_token_with_more_than_one_value_is_refused() {
    let refused = PatientSearch::from_query("family=MOHR&gender=male,female")
        .expect_err("§2:3.78.4.1.2.1: a single code is the only value");
    assert_eq!(refused, SearchError::GenderValues);
    let answer = refused.refusal();
    assert_eq!(answer.status(), StatusCode::BAD_REQUEST);
    assert_eq!(answer.issue_type(), IssueType::Invalid);
    let single = PatientSearch::from_query("gender=female").expect("one code");
    assert!(matches!(
        single.criteria(),
        [Criterion::Gender(Gender::Female)]
    ));
}

#[test]
fn an_unsupported_modifier_is_ignored_and_reported() {
    let search =
        PatientSearch::from_query("family=MOHR&given:contains=AL&gender:not=male&_sort=family")
            .expect("§2:3.78.4.1.2.2: the Supplier MAY ignore other modifiers");
    assert_eq!(search.criteria().len(), 1, "family alone is respected");
    assert_eq!(search.dropped(), ["given:contains", "gender:not", "_sort"]);
    assert_eq!(
        search.self_link(&url(ENDPOINT)).url().as_str(),
        "https://pdq.example.org/fhir/Patient?family=MOHR",
        "the self link says what was respected (§2:3.78.4.1.2.1.1)"
    );
}

#[test]
fn the_vendored_match_inputs_and_the_bare_patient_are_read() {
    for format in [Format::Json, Format::Xml] {
        let certain = MatchRequest::from_request(
            "",
            format,
            &body(
                "example/Parameters-ex-match-input-onlyCertainMatches.json",
                format,
            ),
        )
        .expect("the onlyCertainMatches example");
        assert!(certain.only_certain_matches(), "{format:?}");
        let only = MatchRequest::from_request(
            "",
            format,
            &body(
                "example/Parameters-ex-match-input-patient-only.json",
                format,
            ),
        )
        .expect("the Patient-only example");
        assert!(!only.only_certain_matches(), "{format:?}");
        assert_eq!(only.count(), None, "{format:?}");
        let bare = MatchRequest::from_request(
            "_format=json",
            format,
            &body("example/Patient-ex-match-input-patient.json", format),
        )
        .expect("the bare Patient of §2:3.119.4.1.2");
        assert_eq!(
            bare.patient(),
            only.patient(),
            "the same Patient either way"
        );
        assert_eq!(bare.format(), Some("json"));
    }
}

#[test]
fn the_bare_form_with_only_certain_matches_or_count_is_refused() {
    let patient = body("example/Patient-ex-match-input-patient.json", Format::Json);
    for query in ["onlyCertainMatches=true", "count=1"] {
        let refused = MatchRequest::from_request(query, Format::Json, &patient).expect_err(query);
        assert_eq!(refused, MatchError::BareWithParameters, "{query}");
        assert_eq!(
            refused.refusal().status(),
            StatusCode::BAD_REQUEST,
            "{query}"
        );
    }
}

/// The written Bundle, with the `id` and `meta` of the vendored example it
/// is compared to, as a JSON value.
fn as_example(answer: &Bundle, example: &Bundle) -> Bundle {
    Bundle {
        id: example.id.clone(),
        meta: example.meta.clone(),
        ..answer.clone()
    }
}

/// Asserts that `written` is the vendored example `file`: equal as JSON to
/// the file's own bytes, and byte for byte equal once both are encoded by
/// the same FHIR JSON writer.
fn assert_written(written: &Bundle, file: &str) {
    let example = bundle(file);
    let written = as_example(written, &example);
    let ours: Value = serde_json::to_value(&written).expect("JSON");
    let theirs: Value = serde_json::from_str(&vendored(file)).expect("JSON");
    assert_eq!(ours, theirs, "{file}: every element and value");
    assert_eq!(
        serde_json::to_vec(&written).expect("JSON"),
        serde_json::to_vec(&example).expect("JSON"),
        "{file}: byte for byte in the codec's encoding"
    );
}

#[test]
fn the_match_writer_produces_the_vendored_outputs() {
    let only = MatchRequest::from_request(
        "",
        Format::Json,
        &body(
            "example/Parameters-ex-match-input-patient-only.json",
            Format::Json,
        ),
    )
    .expect("a request");
    let certain = MatchRequest::from_request(
        "",
        Format::Json,
        &body(
            "example/Parameters-ex-match-input-onlyCertainMatches.json",
            Format::Json,
        ),
    )
    .expect("a request");
    let score = |value| Score::new(value).expect("a score");
    let self_link = |id: &str| url(&format!("http://example.com/fhir/Bundle-{id}.html"));

    let [(full_url, patient)] = patients(&bundle("example/Bundle-ex-match-output.json"))
        .try_into()
        .expect("one Patient");
    let one = match_set(
        &only,
        vec![MatchEntry::new(
            full_url,
            patient,
            score(0.9),
            MatchGrade::Certain,
        )],
        Vec::new(),
        Some(&self_link("ex-match-output")),
    )
    .expect("Case 1");
    assert_written(one.resource(), "example/Bundle-ex-match-output.json");

    let mut found = patients(&bundle("example/Bundle-ex-match-output-multiple.json")).into_iter();
    let (first_url, first) = found.next().expect("the probable match");
    let (second_url, second) = found.next().expect("the possible match");
    let multiple = match_set(
        &only,
        vec![
            MatchEntry::new(second_url, second, score(0.5), MatchGrade::Possible),
            MatchEntry::new(first_url, first.clone(), score(0.9), MatchGrade::Probable),
        ],
        Vec::new(),
        Some(&self_link("ex-match-output-multiple")),
    )
    .expect("Case 2");
    assert_written(
        multiple.resource(),
        "example/Bundle-ex-match-output-multiple.json",
    );

    let empty = match_set(
        &certain,
        vec![MatchEntry::new(
            url("https://profiles.ihe.net/ITI/PDQm/Patient-ex-patient.html"),
            first,
            score(0.9),
            MatchGrade::Probable,
        )],
        Vec::new(),
        Some(&self_link("ex-match-output-empty")),
    )
    .expect("Case 4");
    assert_written(
        empty.resource(),
        "example/Bundle-ex-match-output-empty.json",
    );
}

#[test]
fn the_search_writer_holds_what_the_vendored_response_holds() {
    let file = "example/Bundle-ex-QueryPatientResourceResponseMessage.json";
    let example = bundle(file);
    let search = PatientSearch::from_query("family=Schmidt").expect("the example's search");
    let entries = patients(&example)
        .into_iter()
        .map(|(full_url, patient)| SearchEntry::matched(full_url, patient))
        .collect();
    let answer = searchset(&search, &url(ENDPOINT), 1, entries, &Paging::new()).expect("Case 1");
    let written = answer.resource();
    assert_eq!(written.total, example.total, "the total of Case 1");
    assert_eq!(written.r#type, example.r#type, "a searchset");
    assert_eq!(
        patients(written),
        patients(&example),
        "the same entries; the example's self link is no URL and its entries carry no search mode"
    );
    assert_eq!(
        written.link[0].url.value.as_deref(),
        Some("https://pdq.example.org/fhir/Patient?family=Schmidt"),
        "the self link"
    );
    for format in [Format::Json, Format::Xml] {
        assert!(answer.encode(format).is_ok(), "{format:?}");
    }
}

#[test]
fn the_refusals_of_the_cases_carry_their_outcomes() {
    let case_four = Refusal::DomainNotRecognized.answer();
    let json: Value =
        serde_json::from_slice(&case_four.encode(Format::Json).expect("JSON")).expect("JSON");
    assert_eq!(case_four.status(), StatusCode::NOT_FOUND, "Case 4");
    assert_eq!(json["issue"][0]["severity"], "warning", "Case 4");
    assert_eq!(json["issue"][0]["code"], "not-found", "Case 4");
    assert_eq!(
        json["issue"][0]["diagnostics"], "targetSystem not found",
        "Case 4"
    );
    let case_five = Unacceptable.refusal();
    assert_eq!(case_five.status(), StatusCode::NOT_ACCEPTABLE, "Case 5");
    assert_eq!(case_five.issue_type(), IssueType::NotSupported, "Case 5");
    assert_eq!(
        Refusal::PatientNotFound.answer().status(),
        StatusCode::NOT_FOUND,
        "§2:3.78.4.3.3 Case 2"
    );
}

#[test]
fn no_type_with_a_demographic_displays_or_shows_it() {
    let search = PatientSearch::from_query(&format!(
        "family={SENTINEL}&identifier=urn:oid:2.999.1%7C{SENTINEL}&birthdate=1923&telecom={SENTINEL}"
    ))
    .expect("a search");
    let patient = format!(r#"{{"resourceType":"Patient","name":[{{"family":"{SENTINEL}"}}]}}"#);
    let request =
        MatchRequest::from_request("", Format::Json, patient.as_bytes()).expect("a match");
    let entry = MatchEntry::new(
        url(&format!("{ENDPOINT}/{SENTINEL}")),
        request.patient().clone(),
        Score::new(0.5).expect("a score"),
        MatchGrade::Possible,
    );
    let found = SearchEntry::matched(
        url(&format!("{ENDPOINT}/{SENTINEL}")),
        request.patient().clone(),
    );
    let paging = Paging::new().next(url(&format!("{ENDPOINT}?family={SENTINEL}")));
    let link = search.self_link(&url(ENDPOINT));
    assert!(
        link.url().as_str().contains(SENTINEL),
        "the link carries the search"
    );
    let answer = match_set(&request, vec![entry.clone()], Vec::new(), None).expect("a Bundle");
    let shown: [&dyn Debug; 8] = [
        &search,
        &search.criteria()[1],
        &request,
        &entry,
        &found,
        &paging,
        &link,
        &answer,
    ];
    for rendering in shown {
        let text = format!("{rendering:?}");
        assert!(!text.contains(SENTINEL), "Debug showed a value: {text}");
    }
    assert!(!displays!(PatientSearch), "PatientSearch");
    assert!(!displays!(Criterion), "Criterion");
    assert!(!displays!(MatchRequest), "MatchRequest");
    assert!(!displays!(MatchEntry), "MatchEntry");
    assert!(!displays!(SearchEntry), "SearchEntry");
    assert!(!displays!(Paging), "Paging");
    assert!(
        !displays!(ihe_iti::pdqm::server::search::SelfLink),
        "SelfLink"
    );
    assert!(!displays!(ihe_iti::pdqm::server::search::Token), "Token");
    assert_eq!(
        search.criteria().len(),
        4,
        "every criterion read: {:?}",
        search.criteria()
    );
    let Criterion::Identifier(tokens) = &search.criteria()[1] else {
        panic!("the identifier criterion");
    };
    assert_eq!(
        tokens[0].value().expose_secret(),
        SENTINEL,
        "the value is kept"
    );
}
