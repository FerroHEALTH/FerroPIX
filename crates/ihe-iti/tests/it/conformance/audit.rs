// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit corpus: each `AuditEvent` example of the vendored PIXm, PDQm and
//! PMIR packages, against the record `ihe-iti` writes for the exchange the
//! example shows, held to the example's audit profile, the BALP pattern
//! beneath it and the example itself (the checks of #63).
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the records and the vendored examples are compared as JSON values"
)]

use std::sync::Arc;

use ihe_atna::balp::Exchange;
use ihe_atna::user::OnBehalfOf;
use ihe_iti::pdqm::input::MatchInput;
use ihe_iti::pmir::audit::received;
use ihe_iti::pmir::feed::Feed;
use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest};
use secrecy::SecretString;
use serde_json::Value;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{example, examples, is_audit, record, verdict};
use crate::audit::profile::{Kept, holds_to, like, written};

/// The vendored packages whose audit examples the corpus reads.
const PACKAGES: [&str; 3] = ["ihe-pixm", "ihe-pdqm", "ihe-pmir"];

/// The FHIR JSON media type.
const FHIR_JSON: &str = "application/fhir+json";

/// The Consumer's channel endpoint.
const FEED_ENDPOINT: &str = "https://gateway.example.org/pmir/feed";

#[tokio::test]
async fn conformance_the_audit_examples_hold_their_pass_list() {
    let mut cases = Vec::new();
    for package in PACKAGES {
        for id in examples(package).into_iter().filter(|id| is_audit(id)) {
            let case = format!("{package}/{id}");
            cases.push(verdict(case, check(package, id)).await);
        }
    }
    record(
        "audit",
        "The AuditEvent examples of the PIXm, PDQm and PMIR packages, against the record ihe-iti writes for each",
        cases,
    );
}

/// Checks the example `id` of `package` by the audit profile it names.
async fn check(package: &'static str, id: String) {
    let held: Value = serde_json::from_str(&example(package, &id)).expect("JSON");
    let url = held["meta"]["profile"][0]
        .as_str()
        .expect("the example names its profile")
        .to_owned();
    let name = url.rsplit('/').next().unwrap_or_default().to_owned();
    let (exchange, entities) = match name.as_str() {
        "IHE.PIXm.Query.Audit.Consumer" => (pixm_query().await, true),
        "IHE.PDQm.Query.Audit.Consumer" => (pdqm_query().await, true),
        // NOTE: PDQm 3.2.0 §2:3.119.5.1.1; the IG's example names no patient entity, so
        // its entities are not compared, as the client's own audit test does.
        "IHE.PDQm.Match.Audit.Consumer" => (pdqm_match().await, false),
        "IHE.PMIR.Feed.Audit" => (pmir_feed(), true),
        "IHE.PMIR.Audit.Subscription.Create" => (pmir_subscription(0).await, false),
        "IHE.PMIR.Audit.Subscription.Read" => (pmir_subscription(1).await, false),
        "IHE.PMIR.Audit.Subscription.Delete" => (pmir_subscription(2).await, false),
        other if other.ends_with(".Manager") || other.ends_with(".Supplier") => {
            panic!(
                "ihe-iti writes no {other} record: the server side records it in ferropix-server"
            )
        }
        other => panic!("ihe-iti writes no record of the profile {other}"),
    };
    let profile: Value = serde_json::from_str(&example(
        package,
        &format!("StructureDefinition-{name}.json"),
    ))
    .expect("JSON");
    let record = written(&exchange);
    holds_to(&record, &profile);
    like(&record, &held, entities);
}

/// The record of an ITI-83 query the PIXm client posted.
async fn pixm_query() -> Exchange {
    let answer =
        crate::pixm::vendored("example/Parameters-pixm-response-mohralice-red-to-blue.json");
    let server = crate::pixm::posting_manager(200, FHIR_JSON, answer).await;
    let kept = Arc::new(Kept::default());
    crate::pixm::posting_client(&server)
        .audited(kept.clone())
        .cross_reference(
            &crate::pixm::red_source(),
            &[crate::pixm::target(crate::pixm::BLUE)],
            &OnBehalfOf::System,
            crate::pixm::PROMPT,
        )
        .await
        .expect("a cross-reference");
    only(&kept)
}

/// The record of an ITI-78 search of the PDQm client.
async fn pdqm_query() -> Exchange {
    let server = crate::pdqm::supplier(
        200,
        FHIR_JSON,
        crate::pdqm::vendored(crate::pdqm::EXAMPLE_BUNDLE),
    )
    .await;
    let kept = Arc::new(Kept::default());
    crate::pdqm::client(&server)
        .audited(kept.clone())
        .search(
            &crate::pdqm::schmidt(),
            &OnBehalfOf::System,
            crate::pdqm::PROMPT,
        )
        .await
        .expect("a page");
    only(&kept)
}

/// The record of an ITI-119 match of the PDQm client.
async fn pdqm_match() -> Exchange {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fhir/Patient/$match"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            crate::pdqm::vendored("example/Bundle-ex-match-output.json").into_bytes(),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    let input = MatchInput::new(crate::pdqm::DOMAIN, &SecretString::from("12345"))
        .expect("an input")
        .only_certain_matches(true);
    let kept = Arc::new(Kept::default());
    crate::pdqm::client(&server)
        .audited(kept.clone())
        .match_patient(&input, &OnBehalfOf::System, crate::pdqm::PROMPT)
        .await
        .expect("a match");
    only(&kept)
}

/// The record of the vendored create message, as the Consumer received it.
fn pmir_feed() -> Exchange {
    let body = crate::pmir::vendored("example/Bundle-ex-PMIRBundleCreate.json");
    let feed = Feed::read(Some(FHIR_JSON), body.as_bytes()).expect("the example message");
    received(
        &Url::parse("https://pmir.example.org/fhir/").expect("a URL"),
        &Url::parse(FEED_ENDPOINT).expect("a URL"),
        Some(&feed),
    )
}

/// The record of the subscriber's `n`th interaction with a Registry: the
/// create, the read of the status, then the delete.
async fn pmir_subscription(n: usize) -> Exchange {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fhir/Subscription"))
        .respond_with(ResponseTemplate::new(201).insert_header("location", "Subscription/s-1"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!(
                r#"{{"resourceType":"Subscription","id":"s-1","status":"active","reason":"x","criteria":"Patient","channel":{{"type":"message","endpoint":"{FEED_ENDPOINT}","payload":"application/fhir+json"}}}}"#
            ),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let request = SubscriptionRequest::new(
        Criteria::identifier_system(crate::pmir::DOMAIN).expect("an absolute URI"),
        Url::parse(FEED_ENDPOINT).expect("a URL"),
    )
    .expect("a request");
    let kept = Arc::new(Kept::default());
    let client = crate::pmir::subscriber(&server).audited(kept.clone());
    let subscribed = client
        .subscribe(&request, crate::pmir::PROMPT)
        .await
        .expect("created");
    client
        .status(&subscribed, crate::pmir::PROMPT)
        .await
        .expect("a status");
    client
        .unsubscribe(&subscribed, crate::pmir::PROMPT)
        .await
        .expect("deleted");
    kept.taken()
        .into_iter()
        .nth(n)
        .expect("a record of each interaction")
}

/// The one exchange `kept` holds.
fn only(kept: &Kept) -> Exchange {
    let [exchange] = kept.taken().try_into().expect("one record");
    exchange
}
