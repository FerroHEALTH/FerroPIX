// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The search the client sends, held to the vendored Localization Service
//! capability statement and localization record profile of the IG.

use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, PATIENT, PROMPT, SEARCH, client, patient, searchset};
use crate::ig::{has_rule, source};

#[test]
fn the_capability_statement_offers_the_search_the_client_makes() {
    let statement = source("input/fsh/capabilitystatement-localization-repository.fsh");
    for rule in [
        "* type = #DocumentReference",
        "* code = #search-type",
        "* name = \"patient.identifier\"",
        "* name = \"type\"",
    ] {
        assert!(has_rule(&statement, rule), "the statement declares {rule}");
    }
}

#[test]
fn the_profile_fixes_the_systems_the_client_reads() {
    let profile = source("input/fsh/nl_gf_localization_document_reference.fsh");
    for rule in [
        "* subject.identifier.system = \"http://fhir.nl/fhir/NamingSystem/pseudo-bsn\"",
        "* custodian.identifier.system = \"http://fhir.nl/fhir/NamingSystem/ura\"",
    ] {
        assert!(has_rule(&profile, rule), "the profile fixes {rule}");
    }
    let example = source("input/fsh/examples/gf-localization.fsh");
    assert!(
        has_rule(
            &example,
            "* type = $loinc#55188-7 \"Patient data Document\""
        ),
        "the example's record type is the one the client asks for"
    );
}

#[tokio::test]
async fn the_search_names_the_pseudonym_and_the_record_type() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(query_param(
            "patient.identifier",
            format!("http://fhir.nl/fhir/NamingSystem/pseudo-bsn|{PATIENT}"),
        ))
        .and(query_param("type", "http://loinc.org|55188-7"))
        .and(header("accept", FHIR_JSON))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(searchset(Vec::new(), None).to_string(), FHIR_JSON),
        )
        .expect(1)
        .mount(&server)
        .await;
    client(&server)
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    let requests = server.received_requests().await.expect("recorded requests");
    let pairs = requests
        .first()
        .expect("one request")
        .url
        .query_pairs()
        .count();
    assert_eq!(pairs, 2, "the search carries nothing else");
}
