// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator API's guard, methods and wire without a database: every
//! served route refuses a token without its product scope, the searches
//! refuse a `GET`, a posted body is read as JSON alone, and the matching
//! document answers `no-store`. The documents read from the store are
//! driven over PostgreSQL in `e2e::operator`.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON values"
)]

use axum::body::Body;
use ferropix_operator_api::configuration::{Configuration, Mode, Origin};
use ferropix_operator_api::matching::Matching;
use ferropix_server::auth::table::{Need, ROUTES};
use ferropix_server::problem::{self, Kind};
use http::{Method, Request, StatusCode, header};
use tower::ServiceExt as _;

use crate::auth::{Issuer, application, bearer, send, settings};
use crate::support::Secrets;

/// The operator routes this build serves, by interaction.
pub(crate) const SERVED: [&str; 26] = [
    "operator-reveal",
    "operator-domain-create",
    "operator-domain-update",
    "operator-overview",
    "operator-dependencies",
    "operator-search",
    "operator-person",
    "operator-domains",
    "operator-domain",
    "operator-matching",
    "operator-sources",
    "operator-source",
    "operator-configuration",
    "operator-actions",
    "operator-actions-export",
    "operator-candidates",
    "operator-candidate",
    "operator-decide",
    "operator-merge-preview",
    "operator-merge",
    "operator-unmerge",
    "operator-pseudonym-domains",
    "operator-pseudonym-domain-create",
    "operator-pseudonym-domain-update",
    "operator-pseudonym-mint",
    "operator-pseudonym-resolve",
];

/// Sends `method` on `uri` with `authorization`, a body and its media type,
/// and returns the status, the `Cache-Control` and the body.
async fn post(
    app: &axum::Router,
    uri: &str,
    authorization: &str,
    media_type: &str,
    body: &str,
) -> (StatusCode, Option<String>, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::AUTHORIZATION, authorization)
                .header(header::CONTENT_TYPE, media_type)
                .body(Body::from(body.to_owned()))
                .expect("a legal request"),
        )
        .await
        .expect("the router answers");
    let status = response.status();
    let cache = response
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        cache,
        String::from_utf8(bytes.to_vec()).expect("UTF-8"),
    )
}

#[tokio::test]
async fn every_served_operator_route_refuses_a_token_without_its_product_scope() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let consumer = bearer(&issuer.token("east-lab", "system/Patient.cruds"));
    let steward = bearer(&issuer.token("east-lab", "__ferropix.steward"));
    let admin = bearer(&issuer.token("east-lab", "__ferropix.admin"));
    let mut checked = 0;
    for route in ROUTES
        .iter()
        .filter(|route| SERVED.contains(&route.interaction))
    {
        let uri = route
            .path
            .replace("{id}", "00000000-0000-7000-8000-000000000000");
        let label = format!("{} {}", route.method, route.path);
        let refused = send(&app, route.method.clone(), &uri, Some(&consumer)).await;
        assert_eq!(
            StatusCode::FORBIDDEN,
            refused.status,
            "{label}: a FHIR scope"
        );
        let other = match route.need {
            Need::Steward => Some(&admin),
            Need::Admin => Some(&steward),
            _ => None,
        };
        if let Some(other) = other {
            let refused = send(&app, route.method.clone(), &uri, Some(other)).await;
            assert_eq!(
                StatusCode::FORBIDDEN,
                refused.status,
                "{label}: the other product scope"
            );
        }
        checked += 1;
    }
    assert_eq!(27, checked, "every served route was driven");
}

#[tokio::test]
async fn the_searches_and_the_export_refuse_a_get_and_a_body_that_is_not_json() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let steward = bearer(&issuer.token("east-lab", "__ferropix.steward"));
    for path in [
        "/operator/search/identifier",
        "/operator/search/demographics",
        "/operator/actions/export",
    ] {
        let got = send(&app, Method::GET, path, Some(&steward)).await;
        assert_eq!(
            StatusCode::METHOD_NOT_ALLOWED,
            got.status,
            "{path}: an identifier never sits in a URL"
        );
        let (status, cache, body) = post(
            &app,
            path,
            &steward,
            "application/x-www-form-urlencoded",
            "system=x&value=SYNTHETIC-ID-4711",
        )
        .await;
        assert_eq!(StatusCode::UNSUPPORTED_MEDIA_TYPE, status, "{path}");
        assert_eq!(Some("no-store"), cache.as_deref(), "{path}");
        assert!(!body.contains("SYNTHETIC-ID-4711"), "{path}: {body}");
        let (status, _, body) = post(
            &app,
            path,
            &steward,
            "application/json",
            r#"{"system":"x","value":"SYNTHETIC-ID-4711","telecom":"SYNTHETIC-ID-4711"}"#,
        )
        .await;
        assert_eq!(StatusCode::BAD_REQUEST, status, "{path}: {body}");
        assert!(!body.contains("SYNTHETIC-ID-4711"), "{path}: {body}");
        assert!(!body.contains("telecom"), "{path}: {body}");
    }
}

#[tokio::test]
async fn a_demographics_search_that_states_no_field_is_refused_before_it_runs() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let steward = bearer(&issuer.token("east-lab", "__ferropix.steward"));
    let (status, cache, body) = post(
        &app,
        "/operator/search/demographics",
        &steward,
        "application/json",
        r#"{"family":"  ","limit":5}"#,
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, status, "{body}");
    assert_eq!(Some("no-store"), cache.as_deref());
    let problem: serde_json::Value = serde_json::from_str(&body).expect("a problem");
    assert_eq!(
        Kind::InvalidRequest.problem_type().uri(),
        problem["type"],
        "{problem}"
    );
    assert_eq!(400, problem["status"], "{problem}");
}

#[tokio::test]
async fn the_matching_document_names_the_thresholds_keys_and_every_field_and_is_never_cached() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    for scope in ["__ferropix.steward", "__ferropix.admin"] {
        let token = bearer(&issuer.token("east-lab", scope));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/operator/matching")
                    .header(header::AUTHORIZATION, &token)
                    .body(Body::empty())
                    .expect("a legal request"),
            )
            .await
            .expect("the router answers");
        assert_eq!(StatusCode::OK, response.status(), "{scope}");
        assert_eq!(
            Some("no-store"),
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok())
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        let document: Matching = serde_json::from_slice(&bytes).expect("the document reads");
        assert!((document.link_at - 0.95).abs() < f64::EPSILON);
        assert_eq!(3, document.blocking_keys.len());
        assert_eq!(10, document.fields.len());
        assert!(document.fields.iter().all(|field| !field.configured));
    }
}

#[tokio::test]
async fn the_configuration_is_shown_in_file_mode_with_each_origin_and_is_never_cached() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    for scope in ["__ferropix.steward", "__ferropix.admin"] {
        let token = bearer(&issuer.token("east-lab", scope));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/operator/configuration")
                    .header(header::AUTHORIZATION, &token)
                    .body(Body::empty())
                    .expect("a legal request"),
            )
            .await
            .expect("the router answers");
        assert_eq!(StatusCode::OK, response.status(), "{scope}");
        assert_eq!(
            Some("no-store"),
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok())
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        let document: Configuration = serde_json::from_slice(&bytes).expect("the document reads");
        assert_eq!(Mode::File, document.mode);
        let names: Vec<&str> = document.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["thresholds", "weights", "blocking", "rules"]);
        let rules = document.sections.last().expect("the rules");
        assert!(
            rules
                .settings
                .iter()
                .all(|rule| rule.origin == Origin::BuiltIn)
        );
        assert_eq!(
            20,
            document.sections[1].settings.len(),
            "m and u of every field"
        );
    }
}

#[tokio::test]
async fn a_person_or_domain_id_that_is_no_uuid_is_not_found_and_a_bad_trail_filter_is_refused() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let steward = bearer(&issuer.token("east-lab", "__ferropix.steward"));
    for path in ["/operator/persons/p-1", "/operator/domains/d-1"] {
        let answer = send(&app, Method::GET, path, Some(&steward)).await;
        assert_eq!(StatusCode::NOT_FOUND, answer.status, "{path}");
        assert_eq!(
            Kind::NotFound.problem_type().uri(),
            answer.problem().0,
            "{path}"
        );
    }
    for query in ["person=p-1", "from=yesterday", "limit=many", "colour=red"] {
        let answer = send(
            &app,
            Method::GET,
            &format!("/operator/actions?{query}"),
            Some(&steward),
        )
        .await;
        assert_eq!(StatusCode::BAD_REQUEST, answer.status, "{query}");
        assert!(!answer.body.contains("yesterday"), "{query}");
        assert!(!answer.body.contains("colour"), "{query}");
    }
}

/// A made-up identifier and pseudonym no real person carries, which no
/// problem may quote.
const SENTINEL: &str = "Qz7SYNTHETIC-4711";

#[tokio::test]
async fn no_operator_problem_quotes_the_request_an_identifier_or_a_pseudonym() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let steward = bearer(&issuer.token("east-lab", "__ferropix.steward"));
    let person = "00000000-0000-7000-8000-000000000000";
    let cases = [
        (
            Method::POST,
            String::from("/operator/search/identifier"),
            steward.clone(),
            Some(("text/plain", format!("urn:oid:2.999.1.1|{SENTINEL}"))),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            Method::POST,
            String::from("/operator/search/identifier"),
            steward.clone(),
            Some((
                "application/json",
                format!(r#"{{"system":"urn:oid:2.999.1.1","value":"{SENTINEL}","#),
            )),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::GET,
            format!("/operator/persons/{SENTINEL}"),
            steward.clone(),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("/operator/actions?person={SENTINEL}"),
            steward.clone(),
            None,
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            String::from("/operator/pseudonyms/resolve"),
            steward.clone(),
            Some((
                "application/json",
                format!(
                    r#"{{"namespace":"urn:oid:2.999.9","pseudonym":"{SENTINEL}","reason":" "}}"#
                ),
            )),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            format!("/operator/persons/{person}/reveal"),
            steward.clone(),
            Some((
                "application/json",
                format!(r#"{{"target":{{"record":"{SENTINEL}"}},"reason":""}}"#),
            )),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::GET,
            String::from("/operator/overview"),
            bearer(SENTINEL),
            None,
            StatusCode::UNAUTHORIZED,
        ),
    ];
    for (method, uri, authorization, body, status) in cases {
        assert_quotes_nothing(&app, method, &uri, &authorization, body, status).await;
    }
}

/// Sends one request and asserts its answer is an RFC 9457 problem of
/// `status` that quotes nothing of [`SENTINEL`].
async fn assert_quotes_nothing(
    app: &axum::Router,
    method: Method,
    uri: &str,
    authorization: &str,
    body: Option<(&str, String)>,
    status: StatusCode,
) {
    let label = format!("{method} {uri}");
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, authorization);
    let body = match body {
        Some((media_type, text)) => {
            request = request.header(header::CONTENT_TYPE, media_type);
            Body::from(text)
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).expect("a legal request"))
        .await
        .expect("the router answers");
    assert_eq!(status, response.status(), "{label}");
    assert_eq!(
        Some(ferrohealth_server::problem::MEDIA_TYPE),
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        "{label}"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    let text = String::from_utf8(bytes.to_vec()).expect("UTF-8");
    assert!(!text.contains("Qz7"), "{label}: {text}");
    let problem: serde_json::Value = serde_json::from_str(&text).expect("a problem");
    let members = problem.as_object().expect("an object");
    assert!(
        members
            .keys()
            .all(|name| ["type", "title", "status", "detail", "instance"].contains(&name.as_str())),
        "{label}: RFC 9457 §3.1 members alone: {text}"
    );
    assert_eq!(Some(u64::from(status.as_u16())), problem["status"].as_u64());
    let kind = problem["type"].as_str().unwrap_or_default();
    assert!(
        kind == "about:blank" || kind.starts_with(problem::BASE),
        "{label}: {kind}"
    );
}
