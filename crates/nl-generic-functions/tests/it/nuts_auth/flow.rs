// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The flow of Nuts RFC021 §1 that ends in a `DPoP`-bound token.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the sent presentation and submission are read as values"
)]

use std::path::Path;

use ferrofed_testkit::nuts::{self, Verdict};
use nl_generic_functions::nuts_auth::{
    CREDENTIALS_CONTEXT, GRANT_TYPE, PRESENTATION_LIFETIME, PRESENTATION_TYPE,
};
use secrecy::ExposeSecret;
use serde_json::Value;

use super::{DESCRIPTOR, Fixture, HOLDER, HOLDER_KID, PROMPT, SCOPE};

/// The URI the submission schema references the claim format designations
/// by.
const CLAIM_FORMATS: &str = "https://identity.foundation/claim-format-registry/schemas/presentation-submission-claim-format-designations.json";

/// The vendored JSON document at `path` under `docs/specs/`.
fn vendored_json(path: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/specs")
        .join(path);
    serde_json::from_str(&std::fs::read_to_string(file).expect("the vendored file"))
        .expect("the vendored file is JSON")
}

/// The form parameter `name` of `form`.
fn field<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn a_valid_presentation_gets_a_dpop_bound_token() {
    let fixture = Fixture::start().await;
    let token = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    assert_eq!(fixture.node.verdicts(), vec![Verdict::Issued]);
    assert!(!token.token().expose_secret().is_empty());
    assert_eq!(
        token.expires_in(),
        Some(std::time::Duration::from_secs(3600))
    );
    assert_eq!(token.scope(), Some(SCOPE));
}

#[tokio::test]
async fn the_request_is_the_vp_token_bearer_grant() {
    let fixture = Fixture::start().await;
    Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    let forms = fixture.node.forms();
    let form = forms.first().expect("one request");
    assert_eq!(field(form, "grant_type"), Some(GRANT_TYPE));
    assert_eq!(field(form, "scope"), Some(SCOPE));
    assert_eq!(field(form, "client_id"), None);
    assert!(field(form, "assertion").is_some());
    assert!(field(form, "presentation_submission").is_some());
}

#[tokio::test]
async fn the_presentation_holds_to_rfc021_section_4_2() {
    let fixture = Fixture::start().await;
    Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    let forms = fixture.node.forms();
    let assertion = field(forms.first().expect("a request"), "assertion").expect("an assertion");
    let header = jsonwebtoken::decode_header(assertion).expect("a JWS");
    assert_eq!(header.kid.as_deref(), Some(HOLDER_KID));
    let claims: Value =
        serde_json::from_slice(&nuts::payload(assertion).expect("claims")).expect("JSON");
    assert_eq!(claims["iss"], HOLDER);
    assert_eq!(claims["sub"], HOLDER);
    assert_eq!(claims["aud"], fixture.node.issuer().as_str());
    let lifetime = claims["exp"].as_i64().expect("exp") - claims["nbf"].as_i64().expect("nbf");
    assert!(lifetime > 0);
    assert!(lifetime <= i64::try_from(PRESENTATION_LIFETIME.as_secs()).expect("seconds"));
    assert!(
        claims["nonce"]
            .as_str()
            .is_some_and(|nonce| !nonce.is_empty())
    );
    assert!(claims["jti"].as_str().is_some_and(|jti| !jti.is_empty()));
    assert_eq!(claims["vp"]["@context"][0], CREDENTIALS_CONTEXT);
    assert_eq!(claims["vp"]["type"][0], PRESENTATION_TYPE);
    assert_eq!(
        claims["vp"]["verifiableCredential"][0],
        fixture.credential.as_str()
    );
}

#[tokio::test]
async fn the_submission_holds_to_the_presentation_exchange_schema() {
    let fixture = Fixture::start().await;
    Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    let forms = fixture.node.forms();
    let text =
        field(forms.first().expect("a request"), "presentation_submission").expect("a submission");
    let submission: Value = serde_json::from_str(text).expect("JSON");
    let schema = vendored_json("dif-pe/schemas/v2.0.0/presentation-submission.json");
    let formats = vendored_json(
        "dif-pe/claim-format-registry/schemas/presentation-submission-claim-format-designations.json",
    );
    let registry = jsonschema::Registry::new()
        .add(CLAIM_FORMATS, formats)
        .expect("the registry takes the schema")
        .prepare()
        .expect("the registry prepares");
    let validator = jsonschema::options()
        .with_registry(&registry)
        .build(&schema)
        .expect("a validator");
    // The schema validates the submission as its envelope carries it
    // (Presentation Exchange 2.0.0 §Presentation Submission).
    let envelope = serde_json::json!({ "presentation_submission": submission });
    let errors: Vec<String> = validator
        .iter_errors(&envelope)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(submission["definition_id"], "pd_synthetic_organization");
    assert_eq!(submission["descriptor_map"][0]["id"], DESCRIPTOR);
    assert_eq!(submission["descriptor_map"][0]["format"], "jwt_vc");
    assert_eq!(
        submission["descriptor_map"][0]["path"],
        "$.verifiableCredential[0]"
    );
}

#[tokio::test]
async fn a_client_id_is_sent_when_the_grant_names_one() {
    let fixture = Fixture::start().await;
    let client_id = format!("{}/client", fixture.node.uri());
    fixture.node.expect_client_id(&client_id);
    let grant = fixture
        .grant
        .clone()
        .with_client_id(client_id.clone())
        .expect("a client id");
    Fixture::client()
        .request_access_token(&grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    let forms = fixture.node.forms();
    assert_eq!(
        field(forms.first().expect("a request"), "client_id"),
        Some(client_id.as_str())
    );
}

#[tokio::test]
async fn a_demanded_nonce_is_answered_with_a_new_presentation() {
    let fixture = Fixture::start().await;
    fixture.node.require_nonce("synthetic-nonce-1");
    Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token after the nonce");
    assert_eq!(
        fixture.node.verdicts(),
        vec![
            Verdict::Refused(String::from("use_dpop_nonce")),
            Verdict::Issued
        ]
    );
    assert_eq!(
        *fixture.prover.nonces.lock().expect("lock"),
        vec![String::from("synthetic-nonce-1")]
    );
    let forms = fixture.node.forms();
    let nonces: Vec<Value> = forms
        .iter()
        .map(|form| {
            let assertion = field(form, "assertion").expect("an assertion");
            let claims: Value =
                serde_json::from_slice(&nuts::payload(assertion).expect("claims")).expect("JSON");
            claims["nonce"].clone()
        })
        .collect();
    assert_eq!(nonces.len(), 2);
    assert_ne!(nonces.first(), nonces.get(1), "RFC021 §4.4: a nonce once");
}
