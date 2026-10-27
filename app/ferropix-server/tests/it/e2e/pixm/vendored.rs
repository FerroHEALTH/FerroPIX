// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Mohr Alice set of the vendored PIXm 3.1.0 examples, fed by ITI-104
//! and asked by ITI-83.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON values"
)]

use std::collections::BTreeSet;

use ferropix_server::config::section::domain::DomainKind;
use ferropix_testkit::containers::e2e_enabled;
use http::StatusCode;
use serde_json::Value;

use super::{BLUE, GREEN, Harness, RED, identifiers, seed, vendored};

#[tokio::test]
async fn the_vendored_mohr_alice_set_round_trips() {
    if !e2e_enabled() {
        return;
    }
    // NOTE: our own design for the test; Red is authoritative so the Blue and
    // Green feeds that carry IHERED-994 join its person deterministically.
    let harness = Harness::start(vec![
        seed(RED, DomainKind::Authoritative, false),
        seed(GREEN, DomainKind::Local, false),
        seed(BLUE, DomainKind::Local, false),
    ])
    .await;
    let red = harness
        .feed(
            RED,
            "IHERED-994",
            &vendored("Patient-Patient-MohrAlice-Red.json"),
        )
        .await;
    assert_eq!(StatusCode::CREATED, red.status, "{}", red.body);
    let alice = vendored("Patient-Patient-MohrAlice.json");
    for (system, value) in [(BLUE, "IHEBLUE-994"), (GREEN, "IHEGREEN-994")] {
        let fed = harness.feed(system, value, &alice).await;
        assert_eq!(StatusCode::CREATED, fed.status, "{system}: {}", fed.body);
    }
    let request =
        serde_json::to_string(&vendored("Parameters-pixm-request-mohralice-red-all.json"))
            .expect("JSON");
    let all = harness.pix_post(&request).await;
    assert_eq!(StatusCode::OK, all.status, "{}", all.body);
    let response: Value =
        serde_json::to_value(vendored("Parameters-pixm-response-mohralice-red-all.json"))
            .expect("JSON");
    assert_eq!(identifiers(&response), all.identifiers());
    assert_eq!(1, all.target_ids().len(), "one Patient, the person");
    let to_blue = serde_json::to_string(&vendored(
        "Parameters-pixm-request-mohralice-red-to-blue.json",
    ))
    .expect("JSON");
    let in_xml = harness.pix_post(&to_blue).await;
    assert_eq!(StatusCode::OK, in_xml.status, "{}", in_xml.body);
    assert_eq!(
        "application/fhir+xml",
        in_xml.media_type(),
        "the posted _format parameter"
    );
    assert!(in_xml.body.contains("IHEBLUE-994"));

    let maiden = harness
        .feed(
            RED,
            "IHERED-m94",
            &vendored("Patient-Patient-MaidenAlice-Red.json"),
        )
        .await;
    assert_eq!(StatusCode::CREATED, maiden.status, "{}", maiden.body);
    let resolved = harness
        .feed(
            RED,
            "IHERED-m94",
            &vendored("Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json"),
        )
        .await;
    assert_eq!(StatusCode::OK, resolved.status, "{}", resolved.body);
    let subsumed = harness.pix_get(RED, "IHERED-m94", &[]).await;
    assert_eq!(StatusCode::OK, subsumed.status, "{}", subsumed.body);
    assert_eq!(
        BTreeSet::from([
            format!("{RED}|IHERED-994"),
            format!("{GREEN}|IHEGREEN-994"),
            format!("{BLUE}|IHEBLUE-994"),
        ]),
        subsumed.identifiers()
    );
}
