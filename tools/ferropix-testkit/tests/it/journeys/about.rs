// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! About draws the server's `CapabilityStatement` (HL7 FHIR R4, the REST
//! interaction `capabilities`) with the server and FHIR versions it
//! declares, and no refusal, when the server answers `GET /fhir/metadata`.
//! No specification governs the console: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{Server, Stack};

/// What the page's main content says: one line per finding, empty when
/// About draws the statement, its two versions and no `Not known`.
const DRAWN: &str = r"
const main = document.querySelector('main');
const text = main.innerText;
const findings = [];
if (main.querySelector('a[href=\'/about/capability-statement.json\']') === null) {
  findings.push('no Download of the CapabilityStatement');
}
for (const wanted of arguments[0].split('\n')) {
  if (!text.includes(wanted)) { findings.push('the page does not say ' + wanted); }
}
if (text.includes('Not known')) { findings.push('the page says Not known'); }
if (main.querySelector('[role=alert]') !== null) { findings.push('the page draws an alert'); }
return findings.join('\n');
";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn about_draws_the_capability_statement_the_server_declares() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/about")).await;
        journey.element(By::Css("h1"), "the page's heading").await;
        let wanted = format!(
            "FerroPIX {}\n4.0.1\n\"resourceType\"",
            env!("CARGO_PKG_VERSION")
        );
        let findings = journey
            .evaluate_with(DRAWN, wanted)
            .await
            .expect("the page reads its content");
        assert!(findings.is_empty(), "About: {findings}");
    })
    .await;
}
