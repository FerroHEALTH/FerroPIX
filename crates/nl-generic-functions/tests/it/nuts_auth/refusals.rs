// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every flow that ends without a token, each a typed error, and the ones
//! decided before the token request with nothing sent to the token endpoint.

use std::time::Duration;

use http::StatusCode;
use nl_generic_functions::nuts_auth::error::{Malformation, MetadataError, NutsAuthError, Step};
use nl_generic_functions::nuts_auth::presentation::Mismatch;
use nl_generic_functions::nuts_auth::{Grant, RESPONSE_LIMIT};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Fixture, PROMPT, SCOPE};

#[tokio::test]
async fn a_refused_grant_is_the_servers_rfc_6749_error() {
    let fixture = Fixture::start().await;
    fixture
        .node
        .refuse(400, "invalid_request", "the credentials are not trusted");
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("refused");
    match error {
        NutsAuthError::Refused {
            status,
            error,
            description,
        } => {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(error, "invalid_request");
            assert_eq!(
                description.as_deref(),
                Some("the credentials are not trusted")
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_token_endpoint_that_does_not_answer_in_time_is_a_timeout() {
    let fixture = Fixture::start().await;
    fixture.node.delay(Duration::from_secs(3));
    let error = Fixture::client()
        .request_access_token(
            &fixture.grant,
            &fixture.holder,
            &fixture.prover,
            Duration::from_millis(500),
        )
        .await
        .expect_err("timed out");
    assert!(
        matches!(error, NutsAuthError::Timeout { step: Step::Token }),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_bearer_token_is_refused() {
    let fixture = Fixture::start().await;
    fixture.node.issue_bearer();
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("not DPoP-bound");
    assert!(
        matches!(&error, NutsAuthError::TokenType { token_type } if token_type == "Bearer"),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_expired_credential_is_never_sent() {
    let fixture = Fixture::with_expiry(jiff::Timestamp::now().as_second() - 60).await;
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("expired");
    assert!(matches!(error, NutsAuthError::Expired { .. }), "{error:?}");
    assert!(
        fixture.node.forms().is_empty(),
        "nothing reached the token endpoint"
    );
}

#[tokio::test]
async fn an_unanswered_descriptor_is_never_sent() {
    let fixture = Fixture::start().await;
    fixture.node.define(
        r#"{"id": "pd_two", "input_descriptors": [
            {"id": "organization_credential"},
            {"id": "employee_credential"}
        ]}"#,
    );
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("unanswered");
    assert!(
        matches!(&error, NutsAuthError::Definition(Mismatch::Unanswered { descriptor }) if descriptor == "employee_credential"),
        "{error:?}"
    );
    assert!(fixture.node.forms().is_empty());
}

#[tokio::test]
async fn a_credential_for_a_descriptor_the_definition_lacks_is_never_sent() {
    let fixture = Fixture::start().await;
    fixture
        .node
        .define(r#"{"id": "pd_other", "input_descriptors": [{"id": "other_credential"}]}"#);
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("unknown descriptor");
    assert!(
        matches!(
            error,
            NutsAuthError::Definition(Mismatch::UnknownDescriptor { .. })
        ),
        "{error:?}"
    );
    assert!(fixture.node.forms().is_empty());
}

#[tokio::test]
async fn a_pick_requirement_the_credentials_meet_reaches_the_token_endpoint() {
    let fixture = Fixture::start().await;
    fixture.node.define(
        r#"{"id": "pd_pick",
            "submission_requirements": [{"rule": "pick", "count": 1, "from": "A"}],
            "input_descriptors": [
              {"id": "organization_credential", "group": ["A"]},
              {"id": "other_credential", "group": ["A"]}
            ]}"#,
    );
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await;
    // The harness requires every descriptor answered and refuses; the test
    // holds that the client's requirement check let the request go.
    assert!(
        matches!(error, Err(NutsAuthError::Refused { .. })),
        "{error:?}"
    );
    assert_eq!(fixture.node.forms().len(), 1);
}

#[tokio::test]
async fn an_unmet_requirement_is_never_sent() {
    let fixture = Fixture::start().await;
    fixture.node.define(
        r#"{"id": "pd_all",
            "submission_requirements": [{"rule": "all", "from": "A"}],
            "input_descriptors": [
              {"id": "organization_credential", "group": ["A"]},
              {"id": "other_credential", "group": ["A"]}
            ]}"#,
    );
    let error = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("unmet");
    assert!(
        matches!(
            error,
            NutsAuthError::Definition(Mismatch::Requirement { index: 0 })
        ),
        "{error:?}"
    );
    assert!(fixture.node.forms().is_empty());
}

/// A stub authorization server whose metadata is `metadata`, at the issuer
/// `{uri}/oauth2/stub`.
async fn stub(metadata: impl Fn(&str) -> String) -> (MockServer, Grant) {
    let server = MockServer::start().await;
    let issuer = format!("{}/oauth2/stub", server.uri());
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server/oauth2/stub"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(metadata(&issuer), "application/json"),
        )
        .mount(&server)
        .await;
    let grant = Grant::new(&issuer, SCOPE).expect("a grant");
    (server, grant)
}

/// Metadata of `issuer` that supports the grant, with `extra` members
/// spliced in after the issuer.
fn metadata(issuer: &str, extra: &str) -> String {
    format!(
        r#"{{"issuer": "{issuer}", {extra}
            "token_endpoint": "{issuer}/token",
            "presentation_definition_endpoint": "{issuer}/presentation_definition",
            "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
    )
}

/// The error a grant at a stub serving `server_metadata` ends in, having
/// sent no request that carries a credential: nothing but the metadata read,
/// to the stub or to the harness node.
async fn metadata_error(server_metadata: impl Fn(&str) -> String) -> NutsAuthError {
    let fixture = Fixture::start().await;
    let (server, grant) = stub(server_metadata).await;
    let error = Fixture::client()
        .request_access_token(&grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("refused metadata");
    let received = server.received_requests().await.expect("recording is on");
    assert!(
        received
            .iter()
            .all(|request| request.method == "GET"
                && request.url.path().starts_with("/.well-known/")),
        "only the metadata was read"
    );
    assert!(fixture.node.forms().is_empty(), "no token request was sent");
    error
}

#[tokio::test]
async fn a_token_endpoint_on_another_origin_is_refused_with_nothing_sent() {
    let elsewhere = MockServer::start().await;
    let other = elsewhere.uri();
    let error = metadata_error(|issuer| {
        format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": "{other}/token",
                "presentation_definition_endpoint": "{issuer}/pd",
                "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
        )
    })
    .await;
    assert!(
        matches!(error, NutsAuthError::Metadata(MetadataError::OtherOrigin)),
        "{error:?}"
    );
    let reached = elsewhere
        .received_requests()
        .await
        .expect("recording is on");
    assert!(reached.is_empty(), "the other origin was sent nothing");
}

#[tokio::test]
async fn a_token_endpoint_of_another_scheme_is_refused_with_nothing_sent() {
    let error = metadata_error(|issuer| {
        let other = issuer.replacen("http://", "https://", 1);
        format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": "{other}/token",
                "presentation_definition_endpoint": "{issuer}/pd",
                "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
        )
    })
    .await;
    assert!(
        matches!(error, NutsAuthError::Metadata(MetadataError::OtherOrigin)),
        "{error:?}"
    );
}

#[tokio::test]
async fn metadata_that_repeats_a_name_is_refused_with_nothing_sent() {
    let elsewhere = MockServer::start().await;
    let other = elsewhere.uri();
    let error = metadata_error(|issuer| {
        format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": "{issuer}/token",
                "token_endpoint": "{other}/token",
                "presentation_definition_endpoint": "{issuer}/pd",
                "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
        )
    })
    .await;
    assert!(
        matches!(
            error,
            NutsAuthError::Malformed {
                step: Step::Metadata,
                malformation: Malformation::RepeatedName { .. }
            }
        ),
        "{error:?}"
    );
    let reached = elsewhere
        .received_requests()
        .await
        .expect("recording is on");
    assert!(reached.is_empty(), "the repeated endpoint was sent nothing");
}

#[tokio::test]
async fn metadata_whose_endpoint_is_not_a_string_is_refused() {
    let error = metadata_error(|issuer| {
        format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": 7,
                "presentation_definition_endpoint": "{issuer}/pd",
                "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
        )
    })
    .await;
    assert!(
        matches!(
            error,
            NutsAuthError::Malformed {
                malformation: Malformation::Shape { .. },
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_issuer_not_in_canonical_form_is_refused_before_anything_is_sent() {
    for issuer in [
        "https://AS.example.org/oauth2",
        "https://as.example.org:443/oauth2",
        "https://as.example.org/a/../oauth2",
    ] {
        assert!(Grant::new(issuer, SCOPE).is_err(), "{issuer}");
    }
    assert!(Grant::new("https://as.example.org", SCOPE).is_ok());
    assert!(Grant::new("https://as.example.org/oauth2", SCOPE).is_ok());
}

#[tokio::test]
async fn metadata_of_another_issuer_is_refused() {
    let error = metadata_error(|issuer| metadata(&format!("{issuer}-other"), "")).await;
    assert!(
        matches!(error, NutsAuthError::Metadata(MetadataError::Issuer)),
        "{error:?}"
    );
}

#[tokio::test]
async fn metadata_without_a_definition_endpoint_is_refused() {
    let error = metadata_error(|issuer| {
        format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": "{issuer}/token",
                "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
        )
    })
    .await;
    assert!(
        matches!(
            error,
            NutsAuthError::Metadata(MetadataError::DefinitionEndpoint)
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn metadata_that_admits_no_jwt_vp_of_the_holders_algorithm_is_refused() {
    let error = metadata_error(|issuer| {
        format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": "{issuer}/token",
                "presentation_definition_endpoint": "{issuer}/pd",
                "vp_formats": {{"jwt_vp": {{"alg": ["EdDSA"]}}, "ldp_vp": {{}}}}}}"#
        )
    })
    .await;
    assert!(
        matches!(
            error,
            NutsAuthError::Metadata(MetadataError::PresentationFormat { .. })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn metadata_whose_dpop_algorithms_exclude_the_provers_is_refused() {
    let error = metadata_error(|issuer| {
        metadata(issuer, r#""dpop_signing_alg_values_supported": ["RS256"],"#)
    })
    .await;
    assert!(
        matches!(
            error,
            NutsAuthError::Metadata(MetadataError::ProofAlgorithm { .. })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn missing_metadata_is_its_status() {
    let fixture = Fixture::start().await;
    let server = MockServer::start().await;
    let grant = Grant::new(&format!("{}/oauth2/none", server.uri()), SCOPE).expect("a grant");
    let error = Fixture::client()
        .request_access_token(&grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("no metadata");
    assert!(
        matches!(
            error,
            NutsAuthError::Status {
                step: Step::Metadata,
                status: StatusCode::NOT_FOUND
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let fixture = Fixture::start().await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", fixture.node.uri().as_str()),
        )
        .mount(&server)
        .await;
    let grant = Grant::new(&format!("{}/oauth2/moved", server.uri()), SCOPE).expect("a grant");
    let error = Fixture::client()
        .request_access_token(&grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("redirected");
    assert!(
        matches!(
            error,
            NutsAuthError::Status {
                status: StatusCode::FOUND,
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_oversized_answer_is_refused() {
    let fixture = Fixture::start().await;
    let (_server, grant) = stub(|_issuer| "x".repeat(RESPONSE_LIMIT + 1)).await;
    let error = Fixture::client()
        .request_access_token(&grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("too large");
    assert!(
        matches!(
            error,
            NutsAuthError::Malformed {
                malformation: Malformation::TooLarge { .. },
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_answer_that_is_not_json_is_refused() {
    let fixture = Fixture::start().await;
    let (_server, grant) = stub(|_issuer| String::from("{ not json")).await;
    let error = Fixture::client()
        .request_access_token(&grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("not JSON");
    assert!(
        matches!(
            error,
            NutsAuthError::Malformed {
                step: Step::Metadata,
                malformation: Malformation::NotJson { .. }
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_issuer_with_a_query_is_refused_before_anything_is_sent() {
    assert!(Grant::new("https://as.example.org/oauth2?tenant=a", SCOPE).is_err());
    assert!(Grant::new("https://user:pw@as.example.org/oauth2", SCOPE).is_err());
    assert!(Grant::new("ftp://as.example.org/oauth2", SCOPE).is_err());
    assert!(Grant::new("https://as.example.org/oauth2", "").is_err());
    assert!(Grant::new("https://as.example.org/oauth2", "a\"b").is_err());
    assert!(Grant::new("https://as.example.org/oauth2", "a  b").is_err());
    assert!(Grant::new("https://as.example.org/oauth2", "a b").is_ok());
}
