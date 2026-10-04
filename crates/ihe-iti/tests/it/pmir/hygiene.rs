// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifiers an ITI-93 message carries reach the caller only through
//! `ExposeSecret`: no `Debug` of a feed, an event or an identity shows one,
//! no refusal's `Display`, `Debug` or body quotes one, and no demographic is
//! kept at all; nor does a subscriber or a subscription error show the
//! Registry's credentials or text.

use ihe_iti::pmir::PmirSubscriber;
use ihe_iti::pmir::error::FeedError;
use ihe_iti::pmir::feed::{Feed, refusal};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use url::Url;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, PROMPT, entry, merged, message, patient, subscriber};
use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest};

/// A value that stands for a patient identifier.
const SENTINEL: &str = "SENTINEL-4711";

/// A value that stands for a Patient's resource id at the Registry.
const SENTINEL_ID: &str = "sentinel-id-4712";

/// A value that stands for a demographic.
const SENTINEL_NAME: &str = "Sentinelname";

fn carries_none(text: &str) {
    for sentinel in [SENTINEL, SENTINEL_ID, SENTINEL_NAME] {
        assert!(!text.contains(sentinel), "{sentinel} in {text}");
    }
}

#[test]
fn no_rendering_of_a_feed_shows_an_identifier_or_a_demographic() {
    let resource =
        merged(SENTINEL_ID, SENTINEL, "p-survivor").replacen("Synthetic", SENTINEL_NAME, 1);
    let body = message(&[
        entry(
            "PUT",
            &format!("Patient/{SENTINEL_ID}"),
            Some(&resource),
            "200",
        ),
        entry(
            "POST",
            "Patient",
            Some(&patient("p-2", SENTINEL, "").replacen("Synthetic", SENTINEL_NAME, 1)),
            "201",
        ),
    ]);
    let feed = Feed::read(Some(FHIR_JSON), body.as_bytes()).expect("a feed");
    carries_none(&format!("{feed:?}"));
    for event in feed.events() {
        carries_none(&format!("{event:?}"));
    }
}

#[test]
fn no_refusal_quotes_the_message() {
    // Each message breaks a rule next to a value a careless reader would quote.
    let bodies = [
        message(&[entry(
            "PUT",
            &format!("Patient/{SENTINEL_ID}"),
            Some(&patient("other", SENTINEL, "")),
            "200",
        )]),
        message(&[entry(
            "PUT",
            "Patient/p-1",
            Some(&patient(
                "p-1",
                SENTINEL,
                &format!(r#","{SENTINEL_NAME}":1"#),
            )),
            "200",
        )]),
        format!("{{\"{SENTINEL}\": [}}"),
        message(&[entry(
            "PUT",
            "Patient/p-1",
            Some(&patient("p-1", SENTINEL, "")),
            &format!("500 {SENTINEL}"),
        )]),
    ];
    for body in bodies {
        let error: FeedError = Feed::read(Some(FHIR_JSON), body.as_bytes()).expect_err("refused");
        carries_none(&format!("{error} {error:?}"));
        let written = String::from_utf8(refusal(&error).expect("an outcome")).expect("UTF-8");
        carries_none(&written);
    }
}

#[tokio::test]
async fn no_subscription_error_carries_the_registrys_text_or_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_raw(
            format!(
                r#"{{"resourceType":"OperationOutcome","issue":[{{"severity":"error","code":"invalid","diagnostics":"{SENTINEL}"}}]}}"#
            ),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    let request = SubscriptionRequest::new(
        Criteria::AllPatients,
        Url::parse("https://gateway.example.org/pmir/feed").expect("a URL"),
    )
    .expect("a request");
    let error = subscriber(&server)
        .subscribe(&request, PROMPT)
        .await
        .expect_err("refused");
    carries_none(&format!("{error} {error:?}"));

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer Qz7token"));
    let http = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("an HTTP client");
    let base = Url::parse("https://Qz7user:Qz7password@pmir.example.org/fhir/").expect("a URL");
    let shown = format!(
        "{:?}",
        PmirSubscriber::new(base, http).expect("a subscriber")
    );
    for secret in ["Qz7user", "Qz7password", "Qz7token"] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }
}
