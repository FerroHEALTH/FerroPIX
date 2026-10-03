// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The patient identifier, the identifiers a community answers with and the
//! XUA assertion appear in no `Debug` rendering and no error: they travel in
//! the request body to the responding gateway only.

use ihe_iti::xcpd::security::XuaAssertion;

use super::{
    PATIENT_VALUE, PROMPT, SOAP_XML, Templated, answering, client, fixture, gateway, query,
    responding,
};

/// A secret-bearing value inside the synthetic assertion.
const ASSERTION_SECRET: &str = "SYNTH-SUBJECT-NAME-7";

#[test]
fn no_rendering_of_the_query_shows_the_identifier() {
    let query = query();
    for rendered in [format!("{query:?}"), format!("{:?}", query.patient())] {
        assert!(
            !rendered.contains(PATIENT_VALUE),
            "the identifier in {rendered}"
        );
        assert!(
            rendered.contains("2.999.1"),
            "the authority shows: {rendered}"
        );
    }
}

#[test]
fn no_rendering_of_an_assertion_shows_it() {
    let written = format!(
        r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion"><saml2:Subject>{ASSERTION_SECRET}</saml2:Subject></saml2:Assertion>"#
    );
    let assertion = XuaAssertion::new(&written).expect("an assertion");
    assert!(!format!("{assertion:?}").contains(ASSERTION_SECRET));
}

#[tokio::test]
async fn no_rendering_of_a_match_shows_the_patient_ids() {
    let answer = client()
        .discover(
            &responding(&answering("match.xml").await),
            &query(),
            None,
            PROMPT,
        )
        .await
        .expect("a match");
    let rendered = format!("{answer:?}");
    assert!(!rendered.contains("PID-50-0001"), "{rendered}");
    assert!(!rendered.contains(PATIENT_VALUE), "{rendered}");
}

#[tokio::test]
async fn no_error_carries_the_gateway_text_or_the_identifier() {
    let server = gateway(Templated::new(500, SOAP_XML, fixture("fault.xml"))).await;
    let error = client()
        .discover(&responding(&server), &query(), None, PROMPT)
        .await
        .expect_err("a fault");
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains(PATIENT_VALUE), "{rendered}");
    assert!(
        !rendered.contains("lookup of"),
        "the fault reason: {rendered}"
    );
}
