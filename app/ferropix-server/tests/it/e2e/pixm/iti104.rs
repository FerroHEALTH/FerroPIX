// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! PIXm ITI-104, Patient Identity Feed: every refusal its table names, a
//! resolved duplicate and a removed identifier.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON values"
)]

use std::collections::BTreeSet;

use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use fhir_types::codec::Value as Document;
use http::{Method, StatusCode, header};
use serde_json::Value;

use super::{Harness, LAB, STRANGER, carrying, feed_both, fixture, synthetic_seeds};
use crate::auth::bearer;
use crate::e2e::BSN_SYSTEM;

#[tokio::test]
async fn every_refusal_of_iti_104_is_the_one_the_table_names() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    let stranger = bearer(&harness.issuer.token(STRANGER, "system/Patient.cud"));
    let foreign = harness
        .send(
            Method::PUT,
            &format!(
                "/fhir/Patient?{}",
                crate::e2e::feeds::condition(HOSPITAL_DOMAIN, &hospital)
            ),
            Some(&stranger),
            &[(header::CONTENT_TYPE, "application/fhir+json")],
            Some(serde_json::to_string(&fixture(Fixture::FeedHospital)).expect("JSON")),
        )
        .await;
    assert_eq!(StatusCode::FORBIDDEN, foreign.status, "§2:3.104.5");
    assert_eq!("forbidden", foreign.issue().0);
    let uncarried = harness
        .feed(HOSPITAL_DOMAIN, "H-100555", &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::BAD_REQUEST, uncarried.status, "§2:3.104.4.1.2");
    assert_eq!("invalid", uncarried.issue().0);
    let unknown = harness
        .feed(
            "urn:oid:2.999.9.9",
            "X-1",
            &carrying(&fixture(Fixture::FeedHospital), "urn:oid:2.999.9.9", "X-1"),
        )
        .await;
    assert_eq!(StatusCode::BAD_REQUEST, unknown.status);
    assert_eq!(
        (
            "code-invalid".to_owned(),
            "identifier Assigning Authority not found".to_owned()
        ),
        unknown.issue()
    );
    let broken_bsn = "999990001";
    let refused = harness
        .feed(
            BSN_SYSTEM,
            broken_bsn,
            &carrying(&fixture(Fixture::FeedLaboratory), BSN_SYSTEM, broken_bsn),
        )
        .await;
    assert_eq!(
        StatusCode::UNPROCESSABLE_ENTITY,
        refused.status,
        "{}",
        refused.body
    );
    assert_eq!("business-rule", refused.issue().0);
}

#[tokio::test]
async fn a_resolved_duplicate_answers_the_survivors_set_and_a_removed_identifier_is_not_found() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, bsn) = feed_both(&harness).await;
    let duplicate_value = "H-100099";
    let mut duplicate: Value =
        serde_json::to_value(fixture(Fixture::FeedDuplicateResolved)).expect("JSON");
    let resolved: Document = serde_json::from_value(duplicate.clone()).expect("a document");
    duplicate.as_object_mut().expect("an object").remove("link");
    duplicate["active"] = Value::Bool(true);
    let duplicate: Document = serde_json::from_value(duplicate).expect("a document");
    let fed = harness
        .feed(HOSPITAL_DOMAIN, duplicate_value, &duplicate)
        .await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    let merged = harness
        .feed(HOSPITAL_DOMAIN, duplicate_value, &resolved)
        .await;
    assert_eq!(
        StatusCode::OK,
        merged.status,
        "§2:3.104.4.2.4: {}",
        merged.body
    );
    let subsumed = harness.pix_get(HOSPITAL_DOMAIN, duplicate_value, &[]).await;
    assert_eq!(StatusCode::OK, subsumed.status, "{}", subsumed.body);
    assert_eq!(
        BTreeSet::from([
            format!("{HOSPITAL_DOMAIN}|{hospital}"),
            format!("{BSN_SYSTEM}|{bsn}"),
            format!("{LABORATORY_DOMAIN}|{LAB}"),
        ]),
        subsumed.identifiers(),
        "§2:3.104.4.2.3: the survivor's references"
    );
    let survivor = harness.pix_get(HOSPITAL_DOMAIN, &hospital, &[]).await;
    assert!(
        !survivor
            .identifiers()
            .contains(&format!("{HOSPITAL_DOMAIN}|{duplicate_value}")),
        "the subsumed identifier is replaced"
    );
    let revised = harness
        .feed(HOSPITAL_DOMAIN, duplicate_value, &duplicate)
        .await;
    assert_eq!(StatusCode::UNPROCESSABLE_ENTITY, revised.status);

    let removal = format!(
        "/fhir/Patient?{}",
        crate::e2e::feeds::condition(LABORATORY_DOMAIN, LAB)
    );
    let removed = harness
        .send(Method::DELETE, &removal, Some(&harness.source()), &[], None)
        .await;
    assert_eq!(StatusCode::OK, removed.status, "{}", removed.body);
    assert_eq!(
        "the identifier is removed from the cross-reference",
        removed.issue().1
    );
    let gone = harness.pix_get(LABORATORY_DOMAIN, LAB, &[]).await;
    assert_eq!(StatusCode::NOT_FOUND, gone.status, "§2:3.104.4.3.3");
    let survivor = harness.pix_get(HOSPITAL_DOMAIN, &hospital, &[]).await;
    assert!(
        !survivor
            .identifiers()
            .contains(&format!("{LABORATORY_DOMAIN}|{LAB}")),
        "the removed identifier is answered nowhere"
    );
    let again = harness
        .send(Method::DELETE, &removal, Some(&harness.source()), &[], None)
        .await;
    assert_eq!(StatusCode::OK, again.status, "FHIR R4 http.html §3.1.0.7");
    assert_eq!(
        "the cross-reference holds no such identifier",
        again.issue().1
    );
}
