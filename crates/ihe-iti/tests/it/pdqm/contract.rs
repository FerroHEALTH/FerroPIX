// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The client held to the vendored capability statements and Query Patient
//! Resource Response Message profile (PDQm 3.2.0, §2:3.78.4.1.2,
//! §2:3.78.4.2.2).

use std::collections::BTreeSet;

use ihe_iti::pdqm::error::{Malformation, PdqmError};
use ihe_iti::pdqm::query::{DatePrefix, Gender, PatientQuery, StringMatch};
use ihe_iti::user::OnBehalfOf;
use secrecy::SecretString;
use serde::Deserialize;

use super::{
    DOMAIN, EXAMPLE_BUNDLE, FHIR_JSON, PROMPT, SEARCH, client, searchset, supplier, vendored,
};

/// The members of a `CapabilityStatement` the client's query answers to.
#[derive(Debug, Deserialize)]
struct CapabilityStatement {
    rest: Vec<Rest>,
}

#[derive(Debug, Deserialize)]
struct Rest {
    mode: String,
    resource: Vec<Resource>,
}

#[derive(Debug, Deserialize)]
struct Resource {
    #[serde(rename = "type")]
    kind: String,
    interaction: Vec<Interaction>,
    #[serde(rename = "searchParam", default)]
    search_param: Vec<SearchParam>,
}

#[derive(Debug, Deserialize)]
struct Interaction {
    code: String,
}

#[derive(Debug, Deserialize)]
struct SearchParam {
    name: String,
}

/// The members of a `StructureDefinition` differential the reader enforces.
#[derive(Debug, Deserialize)]
struct StructureDefinition {
    differential: Differential,
}

#[derive(Debug, Deserialize)]
struct Differential {
    element: Vec<Element>,
}

#[derive(Debug, Deserialize)]
struct Element {
    id: String,
    min: Option<u32>,
    #[serde(rename = "fixedCode")]
    fixed_code: Option<String>,
}

fn patient_resource(file: &str, mode: &str) -> Resource {
    let statement: CapabilityStatement =
        serde_json::from_str(&vendored(file)).expect("a vendored CapabilityStatement");
    statement
        .rest
        .into_iter()
        .filter(|rest| rest.mode == mode)
        .flat_map(|rest| rest.resource)
        .find(|resource| resource.kind == "Patient")
        .expect("the Patient resource")
}

/// A query with every parameter the client can send, each modifier included.
fn every_parameter() -> PatientQuery {
    let text = SecretString::from("x");
    let both = [StringMatch::StartsWith, StringMatch::Exact];
    let mut query = PatientQuery::new()
        .id(&SecretString::from("ex-patient"))
        .and_then(|query| query.identifier(Some(DOMAIN), &text))
        .and_then(|query| query.telecom(&text))
        .and_then(|query| query.birthdate(DatePrefix::Eq, &SecretString::from("1923")))
        .map(|query| query.active(true).gender(Gender::Other))
        .expect("a query");
    for matching in both {
        query = query
            .family(&text, matching)
            .and_then(|query| query.given(&text, matching))
            .and_then(|query| query.address(&text, matching))
            .and_then(|query| query.address_city(&text, matching))
            .and_then(|query| query.address_country(&text, matching))
            .and_then(|query| query.address_postalcode(&text, matching))
            .and_then(|query| query.address_state(&text, matching))
            .and_then(|query| query.mothers_maiden_name(&text, matching))
            .expect("a query");
    }
    query
}

#[test]
fn the_client_sends_exactly_the_search_parameters_iti_78_names() {
    let supplier = patient_resource(
        "CapabilityStatement-IHE.PDQm.PatientDemographicsSupplier.json",
        "server",
    );
    let declared: BTreeSet<String> = supplier
        .search_param
        .into_iter()
        .map(|parameter| parameter.name)
        .collect();
    let sent: BTreeSet<String> = every_parameter().names().map(str::to_owned).collect();
    let mut expected = declared;
    // NOTE: §2:3.78.4.1.2.1 lists the parameters a Supplier processes; the
    // Supplier statement adds `_lastUpdated`, which ITI-78 does not name.
    assert!(expected.remove("_lastUpdated"), "the statement's addition");
    assert_eq!(
        sent, expected,
        "every parameter and :exact modifier the Supplier processes, and no other"
    );
}

#[test]
fn the_consumer_statement_searches_the_patient_type() {
    let consumer = patient_resource(
        "CapabilityStatement-IHE.PDQm.PatientDemographicsConsumerQuery.json",
        "client",
    );
    assert!(
        consumer
            .interaction
            .iter()
            .any(|interaction| interaction.code == "search-type"),
        "a type-level search, the client's POST [base]/Patient/_search"
    );
    let declared: BTreeSet<String> = consumer
        .search_param
        .into_iter()
        .map(|parameter| parameter.name)
        .collect();
    let sent: BTreeSet<String> = every_parameter()
        .names()
        .map(|name| name.trim_end_matches(":exact").to_owned())
        .collect();
    assert!(
        sent.is_subset(&declared),
        "every base parameter the client sends is one the Consumer may use"
    );
}

#[tokio::test]
async fn the_search_is_a_form_post_asking_for_fhir_json() {
    let server = supplier(200, FHIR_JSON, searchset(0, &[], &[])).await;
    client(&server)
        .search(&every_parameter(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    assert_eq!(request.method.as_str(), "POST", "a POST search");
    assert_eq!(request.url.path(), SEARCH, "on the Patient type");
    assert_eq!(request.url.query(), None, "no criteria in the URL");
    let header = |name: &str| {
        request
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    assert_eq!(
        header("content-type").as_deref(),
        Some("application/x-www-form-urlencoded"),
        "FHIR R4 http.html, the POST search body"
    );
    assert_eq!(
        header("accept").as_deref(),
        Some(FHIR_JSON),
        "ITI TF-2 Appendix Z.6"
    );
    let names: BTreeSet<String> = url::form_urlencoded::parse(&request.body)
        .map(|(name, _value)| name.into_owned())
        .collect();
    let expected: BTreeSet<String> = every_parameter().names().map(str::to_owned).collect();
    assert_eq!(names, expected, "every criterion in the body");
}

#[tokio::test]
async fn the_reader_enforces_the_response_profiles_constraints() {
    let profile: StructureDefinition = serde_json::from_str(&vendored(
        "StructureDefinition-IHE.PDQm.QueryPatientResourceResponseMessage.json",
    ))
    .expect("the vendored response profile");
    let element = |id: &str| {
        profile
            .differential
            .element
            .iter()
            .find(|element| element.id == id)
            .unwrap_or_else(|| panic!("{id} in the differential"))
    };
    assert_eq!(
        element("Bundle.type").fixed_code.as_deref(),
        Some("searchset"),
        "the profile fixes the type"
    );
    assert_eq!(element("Bundle.total").min, Some(1), "a total");
    assert_eq!(element("Bundle.entry.fullUrl").min, Some(1), "a fullUrl");

    // The example is one line of JSON; each case replaces one member of it.
    let example = vendored(EXAMPLE_BUNDLE);
    let replaced = |member: &str, with: &str| {
        assert!(example.contains(member), "{member} in the example");
        example.replacen(member, with, 1)
    };
    let cases = [
        (
            replaced(r#""type":"searchset""#, r#""type":"collection""#),
            Malformation::NotSearchset,
        ),
        (replaced(r#""total":1,"#, ""), Malformation::NoTotal),
        (
            replaced(r#""fullUrl":"http://example.org/Patient/ex-patient","#, ""),
            Malformation::NoFullUrl { index: 0 },
        ),
    ];
    for (body, expected) in cases {
        let server = supplier(200, FHIR_JSON, body).await;
        match client(&server)
            .search(&super::schmidt(), &OnBehalfOf::System, PROMPT)
            .await
        {
            Err(PdqmError::Malformed(found)) => assert_eq!(found, expected, "the profile"),
            other => panic!("{expected:?}, got {other:?}"),
        }
    }
}
