// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The span export to `[telemetry] otlp_endpoint`: requests through the
//! family's middleware under a subscriber that exports to the testkit's
//! in-process OTLP/gRPC collector, what reaches it, and what no exported span
//! carries. No specification governs the export: our own design.

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::routing::get;
use ferropix_server::config::section::server::ServerSettings;
use ferropix_server::config::section::telemetry::{Format, TelemetrySettings};
use ferropix_server::http::with_middleware;
use ferropix_server::telemetry::{Rendering, Traces, subscriber};
use ferropix_testkit::otlp::Collector;
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use http::{Request, StatusCode};
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::trace::v1::Span;
use tower::ServiceExt as _;

/// The synthetic identifier value the ITI-83 query carries.
const IDENTIFIER: &str = "Qz7pt";

/// The synthetic family name a handler event carries.
const FAMILY: &str = "Vxqrbn";

/// The bearer token the requests carry.
const TOKEN: &str = "tok-Wq9zKv";

/// Every attribute key an exported span may carry: the request span's
/// fields, and the three the export layer adds.
const ALLOWED: [&str; 6] = [
    "target",
    "busy_ns",
    "idle_ns",
    "http.request.method",
    "http.route",
    "http.response.status_code",
];

fn server() -> ServerSettings {
    ServerSettings {
        listen: "127.0.0.1:0".parse().expect("a socket address"),
        base_path: String::new(),
        public_base_url: url::Url::parse("http://localhost:8080").expect("a URL"),
        body_limit: 1024,
        request_timeout: Duration::from_secs(5),
        drain_timeout: Duration::from_millis(300),
    }
}

fn telemetry() -> TelemetrySettings {
    TelemetrySettings {
        filter: String::from("info"),
        format: Format::Json,
        otlp_endpoint: None,
    }
}

/// An ITI-83 route whose handler logs the identifier and a demographic in
/// an event, and answers 404 as a Manager does for an unknown identifier.
fn app() -> Router {
    with_middleware(
        Router::new().route(
            "/fhir/Patient/$ihe-pix",
            get(|| async {
                tracing::info!(identifier = IDENTIFIER, family = FAMILY, "looked up");
                StatusCode::NOT_FOUND
            }),
        ),
        &server(),
    )
}

/// Sends the ITI-83 `GET` with the identifier in its query, and a request
/// no route matches with the identifier in its path, both with the token.
async fn requests() {
    for uri in [
        format!("/fhir/Patient/$ihe-pix?sourceIdentifier={HOSPITAL_DOMAIN}%7C{IDENTIFIER}"),
        format!("/fhir/Patient/{IDENTIFIER}"),
    ] {
        let request = Request::get(uri)
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .expect("a request");
        let response = app().oneshot(request).await.expect("an answer");
        assert_eq!(StatusCode::NOT_FOUND, response.status());
    }
}

/// The text of an attribute value, or its `Debug` form when it is no string.
fn text(value: Option<&opentelemetry_proto::tonic::common::v1::AnyValue>) -> String {
    match value.and_then(|value| value.value.as_ref()) {
        Some(Value::StringValue(text)) => text.clone(),
        other => format!("{other:?}"),
    }
}

/// The value of the attribute `key` of `span`, as text.
fn attribute(span: &Span, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|pair| pair.key == key)
        .map(|pair| text(pair.value.as_ref()))
}

/// Every piece of text `span` carries: its name, status, attributes, events
/// and links.
fn carried(span: &Span) -> String {
    let mut carried = vec![span.name.clone(), format!("{:?}", span.status)];
    for pair in &span.attributes {
        carried.push(pair.key.clone());
        carried.push(text(pair.value.as_ref()));
    }
    for event in &span.events {
        carried.push(event.name.clone());
        carried.extend(event.attributes.iter().map(|pair| format!("{pair:?}")));
    }
    for link in &span.links {
        carried.extend(link.attributes.iter().map(|pair| format!("{pair:?}")));
    }
    carried.join("\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_spans_reach_the_collector_and_carry_no_identifier_demographic_token_or_query()
{
    let collector = Collector::start().expect("a collector");
    let endpoint = url::Url::parse(collector.endpoint()).expect("a URL");
    let traces = Traces::new(&endpoint).expect("an exporter");
    let exporting = subscriber(
        &telemetry(),
        Rendering::Json,
        false,
        std::io::sink,
        Some(traces.tracer()),
    )
    .expect("a subscriber");
    {
        let _default = tracing::subscriber::set_default(exporting);
        requests().await;
    }
    tokio::task::spawn_blocking(move || traces.shutdown())
        .await
        .expect("the flush ran")
        .expect("the export flushed");

    let spans = collector.spans();
    let routes: Vec<_> = spans
        .iter()
        .map(|span| {
            (
                span.name.clone(),
                attribute(span, "http.route"),
                attribute(span, "http.response.status_code"),
            )
        })
        .collect();
    assert_eq!(
        routes,
        [
            (
                String::from("request"),
                Some(String::from("/fhir/Patient/$ihe-pix")),
                Some(String::from("404")),
            ),
            (
                String::from("request"),
                Some(String::from("<unmatched>")),
                Some(String::from("404")),
            ),
        ],
        "one request span per request, under its route template"
    );
    for span in &spans {
        for pair in &span.attributes {
            assert!(
                ALLOWED.contains(&pair.key.as_str()),
                "{} is no attribute a span may carry",
                pair.key
            );
        }
        let carried = carried(span);
        for forbidden in [IDENTIFIER, FAMILY, TOKEN, "Bearer", "sourceIdentifier", "?"] {
            assert!(
                !carried.contains(forbidden),
                "a span carries {forbidden:?}: {carried}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_an_endpoint_no_span_is_exported() {
    assert_eq!(None, telemetry().otlp_endpoint, "no endpoint is configured");
    let collector = Collector::start().expect("a collector");
    let endpoint = url::Url::parse(collector.endpoint()).expect("a URL");
    let traces = Traces::new(&endpoint).expect("an exporter");
    let console = subscriber(&telemetry(), Rendering::Json, false, std::io::sink, None)
        .expect("a subscriber");
    {
        let _default = tracing::subscriber::set_default(console);
        requests().await;
    }
    tokio::task::spawn_blocking(move || traces.shutdown())
        .await
        .expect("the flush ran")
        .expect("the export flushed");
    assert!(collector.spans().is_empty(), "no span left the process");
}
