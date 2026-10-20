// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The vendored texts the access token request is held to, read: each is the
//! pinned document, and each value the client writes is the one its text
//! defines.

use std::path::Path;

use nl_generic_functions::nuts_auth::holder::DID_WEB_PREFIX;
use nl_generic_functions::nuts_auth::presentation::{JWT_VC, JWT_VP};
use nl_generic_functions::nuts_auth::{
    CREDENTIALS_CONTEXT, DPOP_NONCE_HEADER, DPOP_TOKEN_TYPE, GRANT_TYPE, PRESENTATION_TYPE,
    USE_DPOP_NONCE,
};

/// The text of the vendored file at `path` under `docs/specs/`.
fn vendored(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/specs");
    std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("the vendored file {path} reads: {error}"))
}

#[test]
fn the_grant_type_is_rfc021s() {
    let rfc021 = vendored("nuts-rfc/rfc/rfc021-vp_token-grant-type.md");
    assert!(rfc021.starts_with("# RFC021 VP Token Grant Type"));
    assert!(rfc021.contains(&format!("The value of the `grant_type` is `{GRANT_TYPE}`.")));
    assert!(rfc021.contains("The `presentation_submission` parameter MUST be used"));
    assert!(rfc021.contains("presentation_definition_endpoint"));
}

#[test]
fn the_igs_access_token_request_is_vendored() {
    let gfi004 = vendored("nl-gf/input/pagecontent/GFI-004.md");
    assert!(gfi004.contains("Must be set to `Bearer` or `DPoP`"));
    assert!(gfi004.contains(
        "If the client provided a DPoP header in the access token request, the authorization server must issue a DPoP-bound access token."
    ));
    let gfi005 = vendored("nl-gf/input/pagecontent/GFI-005.md");
    assert!(gfi005.contains("Authorization: DPoP"));
    let authentication = vendored("nl-gf/input/pagecontent/authentication.md");
    assert!(authentication.contains("[did:web method](https://w3c-ccg.github.io/did-method-web/)"));
}

#[test]
fn the_dpop_names_are_rfc_9449s() {
    let rfc9449 = vendored("ietf-oauth/rfc9449.txt");
    assert!(rfc9449.contains("Request for Comments: 9449"));
    assert!(rfc9449.contains(&format!("\"{USE_DPOP_NONCE}\"")));
    assert!(rfc9449.contains(&format!("{DPOP_NONCE_HEADER}:")));
    assert!(rfc9449.contains(&format!("\"token_type\": \"{DPOP_TOKEN_TYPE}\"")));
}

#[test]
fn the_metadata_location_is_rfc_8414s() {
    let rfc8414 = vendored("ietf-oauth/rfc8414.txt");
    assert!(rfc8414.contains("Request for Comments: 8414"));
    assert!(rfc8414.contains("/.well-known/oauth-authorization-server"));
}

#[test]
fn every_vendored_rfc_is_the_one_its_name_says() {
    for number in [6749, 7519, 7521, 7523, 7662, 8414, 9126, 9396, 9449] {
        let text = vendored(&format!("ietf-oauth/rfc{number}.txt"));
        assert!(
            text.contains(&format!("Request for Comments: {number}")),
            "rfc{number}.txt"
        );
    }
}

#[test]
fn the_presentation_shape_is_the_vc_data_models() {
    let model = vendored("w3c-did-vc/vc-data-model-1.1.html");
    assert!(model.contains("Verifiable Credentials Data Model v1.1"));
    assert!(model.contains(CREDENTIALS_CONTEXT));
    assert!(model.contains(PRESENTATION_TYPE));
    for (path, title) in [
        (
            "w3c-did-vc/did-core-1.0.html",
            "Decentralized Identifiers (DIDs) v1.0",
        ),
        ("w3c-did-vc/did-resolution-1.0.html", "DID Resolution"),
        (
            "w3c-did-vc/vc-bitstring-status-list-1.0.html",
            "Bitstring Status List",
        ),
    ] {
        assert!(vendored(path).contains(title), "{path}");
    }
}

#[test]
fn the_holder_identifier_is_the_did_web_methods() {
    let method = vendored("w3c-did-vc/did-method-web/index.html");
    assert!(method.contains(DID_WEB_PREFIX));
}

#[test]
fn the_formats_are_presentation_exchanges() {
    let spec = vendored("dif-pe/spec/v2.0.0/spec.md");
    assert!(spec.contains(&format!("`{JWT_VC}`")));
    assert!(spec.contains(&format!("`{JWT_VP}`")));
    assert!(spec.contains("submission_requirements"));
    for schema in [
        "presentation-definition.json",
        "presentation-submission.json",
        "submission-requirement.json",
    ] {
        let text = vendored(&format!("dif-pe/schemas/v2.0.0/{schema}"));
        assert!(text.contains("\"$schema\""), "{schema}");
    }
}

#[test]
fn the_rfc003_and_rfc022_texts_are_the_pinned_documents() {
    assert!(
        vendored("nuts-rfc/rfc/rfc003-oauth2-authorization.md").contains("OAuth2 Authorization")
    );
    assert!(vendored("nuts-rfc/rfc/rfc022-discovery-service.md").contains("Discovery Service"));
}
