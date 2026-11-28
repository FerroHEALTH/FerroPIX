// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-119 inputs that name the patient by demographics (§2:3.119.4.1.2):
//! the name, birth date, gender, birth place and nationality written into the
//! input Patient as the PDQm Patient Profile for `$match` Input admits them,
//! each kept out of everything but the request body, and an input with no
//! demographic refused.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the request body and the vendored artefacts are read as JSON values"
)]

use ihe_atna::user::OnBehalfOf;
use ihe_iti::pdqm::error::InvalidInput;
use ihe_iti::pdqm::input::{BirthPlace, Gender, MatchInput};
use secrecy::SecretString;
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, PROMPT, client, vendored};

/// The match path under the stub's FHIR base.
const MATCH: &str = "/fhir/Patient/$match";

/// The core FHIR R4 birth place extension.
const BIRTH_PLACE: &str = "http://hl7.org/fhir/StructureDefinition/patient-birthPlace";

/// The core FHIR R4 nationality extension.
const NATIONALITY: &str = "http://hl7.org/fhir/StructureDefinition/patient-nationality";

fn secret(value: &str) -> SecretString {
    SecretString::from(value)
}

/// The input Patient a stub Supplier received for `input`.
async fn sent_patient(input: &MatchInput) -> Value {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(MATCH))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            vendored("example/Bundle-ex-match-output.json").into_bytes(),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    client(&server)
        .match_patient(input, &OnBehalfOf::System, PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    serde_json::from_slice::<fhir_types::r4::parameters::Parameters>(&request.body)
        .expect("the body reads back as an R4 Parameters");
    let body: Value = serde_json::from_slice(&request.body).expect("a JSON body");
    assert_eq!(body["parameter"][0]["name"], "resource");
    body["parameter"][0]["resource"].clone()
}

#[tokio::test]
async fn a_name_alone_is_sent_as_the_patients_name_with_no_identifier() {
    let input = MatchInput::named(
        Some(&secret("Proefsma")),
        &[secret("Bea"), secret(""), secret("Anna")],
    )
    .expect("an input");
    let patient = sent_patient(&input).await;
    assert_eq!(patient["resourceType"], "Patient");
    assert_eq!(
        patient["name"],
        serde_json::json!([{"family": "Proefsma", "given": ["Bea", "Anna"]}]),
        "Patient.name, an empty given name left out"
    );
    assert!(patient.get("identifier").is_none(), "no identifier");
}

#[tokio::test]
async fn a_birth_date_alone_is_sent_as_the_patients_birth_date() {
    let patient = sent_patient(&MatchInput::born(&secret("1970-01-02")).expect("an input")).await;
    assert_eq!(patient["birthDate"], "1970-01-02");
    assert!(patient.get("identifier").is_none());
}

#[tokio::test]
async fn the_gender_is_sent_as_its_administrative_gender_code() {
    for (gender, code) in [
        (Gender::Male, "male"),
        (Gender::Female, "female"),
        (Gender::Other, "other"),
        (Gender::Unknown, "unknown"),
    ] {
        let input = MatchInput::named(Some(&secret("Proefsma")), &[])
            .expect("an input")
            .gender(gender);
        assert_eq!(sent_patient(&input).await["gender"], code);
    }
}

#[tokio::test]
async fn the_birth_place_is_sent_in_the_core_birth_place_extension() {
    let place = BirthPlace::new(Some(&secret("Utrecht")), Some(&secret("UT")), Some("NL"))
        .expect("a place");
    let input = MatchInput::born(&secret("1970"))
        .expect("an input")
        .birth_place(place);
    let patient = sent_patient(&input).await;
    assert_eq!(
        patient["extension"],
        serde_json::json!([{
            "url": BIRTH_PLACE,
            "valueAddress": {"city": "Utrecht", "state": "UT", "country": "NL"}
        }])
    );
    let country_only = MatchInput::born(&secret("1970"))
        .expect("an input")
        .birth_place(BirthPlace::new(None, Some(&secret("")), Some("NLD")).expect("a place"));
    assert_eq!(
        sent_patient(&country_only).await["extension"][0]["valueAddress"],
        serde_json::json!({"country": "NLD"}),
        "an empty part is left out"
    );
}

#[tokio::test]
async fn each_nationality_is_sent_in_its_own_core_nationality_extension() {
    let input = MatchInput::born(&secret("1970-01"))
        .expect("an input")
        .nationality("NL")
        .expect("a code")
        .nationality("DE")
        .expect("a code");
    let patient = sent_patient(&input).await;
    let code = |country: &str| {
        serde_json::json!({
            "url": NATIONALITY,
            "extension": [{
                "url": "code",
                "valueCodeableConcept": {
                    "coding": [{"system": "urn:iso:std:iso:3166", "code": country}]
                }
            }]
        })
    };
    assert_eq!(
        patient["extension"],
        serde_json::json!([code("NL"), code("DE")])
    );
}

#[tokio::test]
async fn every_demographic_rides_beside_an_identifier() {
    let input = MatchInput::new(super::DOMAIN, &secret("12345"))
        .expect("an input")
        .name(Some(&secret("Proefsma")), &[secret("Bea")])
        .expect("a name")
        .birth_date(&secret("1970-01-02"))
        .expect("a date")
        .gender(Gender::Female)
        .birth_place(BirthPlace::new(Some(&secret("Utrecht")), None, None).expect("a place"))
        .nationality("NL")
        .expect("a code");
    let patient = sent_patient(&input).await;
    assert_eq!(patient["identifier"][0]["value"], "12345");
    assert_eq!(patient["name"][0]["family"], "Proefsma");
    assert_eq!(patient["birthDate"], "1970-01-02");
    assert_eq!(patient["gender"], "female");
    assert_eq!(patient["extension"][0]["url"], BIRTH_PLACE);
    assert_eq!(patient["extension"][1]["url"], NATIONALITY);
}

#[test]
fn the_match_input_profile_keeps_the_extensions_open_and_admits_each_element() {
    let profile: Value = serde_json::from_str(&vendored(
        "StructureDefinition-IHE.PDQm.MatchInputPatient.json",
    ))
    .expect("JSON");
    let elements = profile["snapshot"]["element"]
        .as_array()
        .expect("a snapshot");
    let element = |id: &str| {
        elements
            .iter()
            .find(|element| element["id"] == id)
            .unwrap_or_else(|| panic!("{id} in the profile"))
    };
    for id in ["Patient.name", "Patient.birthDate", "Patient.gender"] {
        assert_ne!(element(id)["max"], "0", "{id} is admitted");
    }
    assert_eq!(
        element("Patient.extension")["slicing"]["rules"],
        "open",
        "the birth place and nationality extensions ride in the open slicing"
    );
}

#[test]
fn an_input_with_no_demographic_is_refused_at_construction() {
    assert_eq!(
        MatchInput::named(None, &[]).err(),
        Some(InvalidInput::EmptyName)
    );
    assert_eq!(
        MatchInput::named(Some(&secret("")), &[secret("")]).err(),
        Some(InvalidInput::EmptyName)
    );
    assert_eq!(
        MatchInput::born(&secret("")).err(),
        Some(InvalidInput::Date)
    );
}

#[test]
fn a_birth_date_must_be_a_fhir_date() {
    for date in ["1970-01-02T10:00:00Z", "02-01-1970", "1970-13", "yesterday"] {
        assert_eq!(
            MatchInput::born(&secret(date)).err(),
            Some(InvalidInput::Date),
            "{date}"
        );
    }
    for date in ["1970", "1970-01", "1970-01-02"] {
        assert!(MatchInput::born(&secret(date)).is_ok(), "{date}");
    }
}

#[test]
fn a_country_must_be_an_iso_3166_code_and_a_place_must_name_something() {
    let input = || MatchInput::born(&secret("1970")).expect("an input");
    for country in ["nl", "N", "NETH", "N1", ""] {
        assert_eq!(
            input().nationality(country).err(),
            Some(InvalidInput::Country),
            "{country}"
        );
        assert_eq!(
            BirthPlace::new(None, None, Some(country)).err(),
            Some(InvalidInput::Country),
            "{country}"
        );
    }
    assert_eq!(
        BirthPlace::new(None, None, None).err(),
        Some(InvalidInput::EmptyPlace)
    );
    assert_eq!(
        BirthPlace::new(Some(&secret("")), Some(&secret("")), None).err(),
        Some(InvalidInput::EmptyPlace)
    );
}

#[test]
fn no_debug_names_a_demographic() {
    const SENTINEL: &str = "SENTINEL-4712";
    let place = BirthPlace::new(Some(&secret(SENTINEL)), Some(&secret(SENTINEL)), Some("NL"))
        .expect("a place");
    assert!(!format!("{place:?}").contains(SENTINEL));
    let input = MatchInput::new(super::DOMAIN, &secret(SENTINEL))
        .expect("an input")
        .name(Some(&secret(SENTINEL)), &[secret(SENTINEL)])
        .expect("a name")
        .birth_date(&secret("1970-01-02"))
        .expect("a date")
        .gender(Gender::Female)
        .birth_place(place)
        .nationality("NL")
        .expect("a code");
    let shown = format!("{input:?}");
    for value in [SENTINEL, "1970-01-02", "female", "NL"] {
        assert!(!shown.contains(value), "{value} in {shown}");
    }
}
