// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! PIXm ITI-83, Get Corresponding Identifiers, asked by `GET` and by `POST`
//! in JSON and in XML, with every refusal its table names.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON values"
)]

use std::collections::BTreeSet;

use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use fhir_types::codec::Value as Document;
use http::{Method, StatusCode, header};
use serde_json::Value;

use super::{
    Harness, LAB, carrying, feed_both, fixture, hospital_identifiers, identifiers, pix_parameters,
    pix_uri, synthetic_seeds, xml,
};
use crate::e2e::BSN_SYSTEM;

#[tokio::test]
async fn a_fed_patient_is_answered_alike_by_get_and_post_in_json_and_xml() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, bsn) = hospital_identifiers();
    let created = harness
        .feed(HOSPITAL_DOMAIN, &hospital, &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::CREATED, created.status, "{}", created.body);
    assert_eq!(
        (
            "informational".to_owned(),
            "the identifier is added to the cross-reference".to_owned()
        ),
        created.issue()
    );
    let location = created
        .headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a Location");
    assert!(
        location.starts_with("http://localhost:8080/fhir/Patient/"),
        "{location}"
    );
    let revised = harness
        .feed(HOSPITAL_DOMAIN, &hospital, &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::OK, revised.status, "FHIR R4 http.html §3.1.0.4");
    let laboratory = carrying(&fixture(Fixture::FeedLaboratory), BSN_SYSTEM, &bsn);
    let in_xml = harness
        .send(
            Method::PUT,
            &format!(
                "/fhir/Patient?{}&_format=xml",
                crate::e2e::feeds::condition(LABORATORY_DOMAIN, LAB)
            ),
            Some(&harness.source()),
            &[(header::CONTENT_TYPE, "application/fhir+xml")],
            Some(xml(&laboratory)),
        )
        .await;
    assert_eq!(StatusCode::CREATED, in_xml.status, "{}", in_xml.body);
    assert_eq!("application/fhir+xml", in_xml.media_type());
    assert!(in_xml.body.contains("<OperationOutcome"), "{}", in_xml.body);

    let expected = identifiers(&serde_json::to_value(fixture(Fixture::PixQueryOut)).expect("JSON"));
    let got = harness
        .pix_get(LABORATORY_DOMAIN, LAB, &[HOSPITAL_DOMAIN, BSN_SYSTEM])
        .await;
    assert_eq!(StatusCode::OK, got.status, "{}", got.body);
    assert_eq!(expected, got.identifiers(), "PIXm 3.1.0 §2:3.83.4.2.2.1");
    let ids = got.target_ids();
    assert_eq!(1, ids.len(), "the person is the one Patient");
    assert_eq!(
        Some(location.trim_start_matches("http://localhost:8080/fhir/")),
        ids.first().map(String::as_str)
    );
    let posted = harness
        .pix_post(&Fixture::PixQueryIn.text().expect("the fixture reads"))
        .await;
    assert_eq!(StatusCode::OK, posted.status, "{}", posted.body);
    assert_eq!(
        got.body, posted.body,
        "POST answers exactly what GET answers (#52)"
    );

    let posted_xml = harness
        .send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&harness.consumer()),
            &[
                (header::CONTENT_TYPE, "application/fhir+xml"),
                (header::ACCEPT, "application/fhir+xml"),
            ],
            Some(xml(&fixture(Fixture::PixQueryIn))),
        )
        .await;
    assert_eq!(StatusCode::OK, posted_xml.status, "{}", posted_xml.body);
    assert_eq!("application/fhir+xml", posted_xml.media_type());
    let decoded = fhir_types::xml::from_xml(&fhir_types::r4::schema::SCHEMAS, &posted_xml.body)
        .expect("the answer is FHIR XML");
    let decoded: Value = serde_json::to_value(Document::Object(decoded)).expect("JSON");
    assert_eq!(got.json(), decoded, "the XML answer is the JSON answer");

    let every = harness.pix_get(LABORATORY_DOMAIN, LAB, &[]).await;
    assert_eq!(
        BTreeSet::from([
            format!("{HOSPITAL_DOMAIN}|{hospital}"),
            format!("{BSN_SYSTEM}|{bsn}")
        ]),
        every.identifiers(),
        "every identifier but the one asked (§2:3.83.4.2.2.1)"
    );
    assert_eq!(7, harness.records(), "one BALP record per transaction");
}

#[tokio::test]
async fn every_refusal_of_iti_83_is_the_one_the_table_names() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    let before = harness.records();
    for (label, uri_targets, source, status, code, diagnostics) in [
        (
            "an unknown source domain",
            &[][..],
            ("urn:oid:2.999.9.9", "X-1"),
            StatusCode::BAD_REQUEST,
            "code-invalid",
            "sourceIdentifier Assigning Authority not found",
        ),
        (
            "an unknown target domain",
            &["urn:oid:2.999.9.9"][..],
            (HOSPITAL_DOMAIN, hospital.as_str()),
            StatusCode::FORBIDDEN,
            "code-invalid",
            "targetSystem not found",
        ),
        (
            "an unknown identifier",
            &[][..],
            (HOSPITAL_DOMAIN, "H-100777"),
            StatusCode::NOT_FOUND,
            "not-found",
            "sourceIdentifier Patient Identifier not found",
        ),
    ] {
        let got = harness.pix_get(source.0, source.1, uri_targets).await;
        assert_eq!(status, got.status, "GET: {label}");
        assert_eq!(
            (code.to_owned(), diagnostics.to_owned()),
            got.issue(),
            "GET: {label}"
        );
        let posted = harness
            .pix_post(&pix_parameters(source.0, source.1, uri_targets))
            .await;
        assert_eq!(status, posted.status, "POST: {label}");
        assert_eq!(got.body, posted.body, "POST: {label}");
    }
    assert_eq!(
        before + 6,
        harness.records(),
        "each refused query is recorded"
    );
    let in_xml = harness
        .send(
            Method::GET,
            &format!("{}&_format=xml", pix_uri(HOSPITAL_DOMAIN, "H-100777", &[])),
            Some(&harness.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(StatusCode::NOT_FOUND, in_xml.status);
    assert!(
        in_xml.body.contains("<code value=\"not-found\"/>"),
        "{}",
        in_xml.body
    );
    let missing = harness
        .send(
            Method::GET,
            "/fhir/Patient/$ihe-pix",
            Some(&harness.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(StatusCode::BAD_REQUEST, missing.status);
    assert_eq!("required", missing.issue().0, "§2:3.83.4.1.2.1");
    let unacceptable = harness
        .send(
            Method::GET,
            &format!(
                "{}&_format=text/turtle",
                pix_uri(HOSPITAL_DOMAIN, &hospital, &[])
            ),
            Some(&harness.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(StatusCode::NOT_ACCEPTABLE, unacceptable.status);
    assert_eq!("not-supported", unacceptable.issue().0);
    let plain = harness
        .send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&harness.consumer()),
            &[(header::CONTENT_TYPE, "text/plain")],
            Some(String::from("sourceIdentifier")),
        )
        .await;
    assert_eq!(StatusCode::UNSUPPORTED_MEDIA_TYPE, plain.status);
}

#[tokio::test]
async fn a_carried_bsn_finds_its_person_and_answers_among_its_identifiers() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, bsn) = feed_both(&harness).await;
    let by_bsn = harness.pix_get(BSN_SYSTEM, &bsn, &[]).await;
    assert_eq!(StatusCode::OK, by_bsn.status, "{}", by_bsn.body);
    assert_eq!(
        BTreeSet::from([
            format!("{HOSPITAL_DOMAIN}|{hospital}"),
            format!("{LABORATORY_DOMAIN}|{LAB}"),
        ]),
        by_bsn.identifiers(),
        "the BSN itself is left out of its own answer (§2:3.83.4.2.2.1)"
    );
    let posted = harness
        .pix_post(&pix_parameters(BSN_SYSTEM, &bsn, &[HOSPITAL_DOMAIN]))
        .await;
    assert_eq!(StatusCode::OK, posted.status, "{}", posted.body);
    assert_eq!(
        BTreeSet::from([format!("{HOSPITAL_DOMAIN}|{hospital}")]),
        posted.identifiers()
    );

    let mut elsewhere: Value = serde_json::to_value(carrying(
        &fixture(Fixture::FeedLaboratory),
        BSN_SYSTEM,
        &bsn,
    ))
    .expect("JSON");
    elsewhere["identifier"][0] =
        serde_json::json!({"system": PRACTICE_DOMAIN, "value": "P-300001"});
    elsewhere["birthDate"] = Value::String(String::from("1980-02-02"));
    let elsewhere: Document = serde_json::from_value(elsewhere).expect("a document");
    let fed = harness.feed(PRACTICE_DOMAIN, "P-300001", &elsewhere).await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    let practice = harness.pix_get(PRACTICE_DOMAIN, "P-300001", &[]).await;
    assert_eq!(StatusCode::OK, practice.status, "{}", practice.body);
    assert!(
        practice
            .identifiers()
            .contains(&format!("{BSN_SYSTEM}|{bsn}")),
        "the practice's own person carries the BSN too"
    );
    let ambiguous = harness.pix_get(BSN_SYSTEM, &bsn, &[]).await;
    assert_eq!(StatusCode::CONFLICT, ambiguous.status, "{}", ambiguous.body);
    assert_eq!("multiple-matches", ambiguous.issue().0);
}
