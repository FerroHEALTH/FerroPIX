// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The holder's DID document: where its `did:web` DID resolves (the did:web
//! Method Specification, Read (Resolve)), its shape (DID 1.0 §5.2, §5.3,
//! §6.3.1), and that it verifies a presentation the client signed for an
//! access token request (Nuts RFC021 §4.2 item 4).
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the written document and the presentation are read as values"
)]

use ferrofed_testkit::nuts;
use ferrofed_testkit::oauth::{es384_pem, p256_pem};
use jsonwebtoken::{DecodingKey, Validation};
use nl_generic_functions::nuts_auth::did_document::{
    DID_CONTEXT, DidDocument, DocumentError, JSON_WEB_KEY_2020, JWS_2020_CONTEXT,
};
use nl_generic_functions::nuts_auth::holder::{Did, HolderKey};
use secrecy::SecretString;
use serde_json::Value;

use super::{Fixture, HOLDER, HOLDER_KID, PROMPT};

/// The holder key `pem` holds, named `kid`.
fn key(pem: &str, kid: &str) -> HolderKey {
    let did = Did::new(HOLDER).expect("a DID");
    HolderKey::from_pem(&SecretString::from(pem), kid, &did).expect("a holder key")
}

#[test]
fn a_did_web_did_resolves_to_its_well_known_or_path_location() {
    for (did, path) in [
        ("did:web:gateway.example.org", "/.well-known/did.json"),
        (
            "did:web:gateway.example.org%3A8443",
            "/.well-known/did.json",
        ),
        ("did:web:gateway.example.org:org:gw", "/org/gw/did.json"),
        ("did:web:gateway.example.org%3A8443:fed", "/fed/did.json"),
    ] {
        assert_eq!(path, Did::new(did).expect("a DID").document_path(), "{did}");
    }
}

#[test]
fn the_document_names_each_key_once_as_a_json_web_key_and_no_private_member() {
    let pem = p256_pem().expect("a key");
    let first = key(&pem, HOLDER_KID);
    let again = key(&pem, HOLDER_KID);
    let second = key(
        &es384_pem().expect("a key"),
        "did:web:gateway.example.org#key-2",
    );
    let did = Did::new(HOLDER).expect("a DID");
    let document = DidDocument::new(&did, [&first, &again, &second]).expect("a document");
    let json: Value = serde_json::to_value(&document).expect("JSON");
    assert_eq!(json["@context"][0], DID_CONTEXT);
    assert_eq!(json["@context"][1], JWS_2020_CONTEXT);
    assert_eq!(json["id"], HOLDER);
    let methods = json["verificationMethod"].as_array().expect("methods");
    assert_eq!(2, methods.len(), "{methods:?}");
    assert_eq!(methods[0]["id"], HOLDER_KID);
    assert_eq!(methods[0]["type"], JSON_WEB_KEY_2020);
    assert_eq!(methods[0]["controller"], HOLDER);
    assert_eq!(methods[0]["publicKeyJwk"]["crv"], "P-256");
    assert_eq!(methods[1]["publicKeyJwk"]["crv"], "P-384");
    for method in methods {
        assert!(
            method["publicKeyJwk"].get("d").is_none(),
            "no private member"
        );
    }
    let ids = [HOLDER_KID, "did:web:gateway.example.org#key-2"];
    assert_eq!(json["authentication"], serde_json::json!(ids));
    assert_eq!(json["assertionMethod"], serde_json::json!(ids));
}

#[test]
fn a_document_of_no_key_another_dids_key_or_one_kid_twice_is_refused() {
    let did = Did::new(HOLDER).expect("a DID");
    assert_eq!(
        Err(DocumentError::NoKey),
        DidDocument::new(&did, std::iter::empty())
    );
    let other = Did::new("did:web:other.example.org").expect("a DID");
    let theirs = HolderKey::from_pem(
        &SecretString::from(p256_pem().expect("a key")),
        "did:web:other.example.org#key-1",
        &other,
    )
    .expect("a key");
    assert!(matches!(
        DidDocument::new(&did, [&theirs]),
        Err(DocumentError::OtherDid { .. })
    ));
    let one = key(&p256_pem().expect("a key"), HOLDER_KID);
    let two = key(&p256_pem().expect("a key"), HOLDER_KID);
    assert!(matches!(
        DidDocument::new(&did, [&one, &two]),
        Err(DocumentError::SameKeyId { .. })
    ));
}

/// The document built from the holder's key verifies the presentation the
/// client signed for an access token request: its `kid` names a
/// verification method of the document, whose JWK verifies the signature.
#[tokio::test]
async fn the_document_verifies_a_presentation_the_client_signed() {
    let fixture = Fixture::start().await;
    Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    let forms = fixture.node.forms();
    let assertion = forms
        .first()
        .and_then(|form| form.iter().find(|(name, _)| name == "assertion"))
        .map(|(_, value)| value.clone())
        .expect("an assertion");
    let document =
        DidDocument::new(fixture.holder.did(), [fixture.holder.key()]).expect("a document");
    let header = jsonwebtoken::decode_header(&assertion).expect("a JWS");
    let kid = header.kid.expect("a kid");
    let method = document
        .verification_methods()
        .iter()
        .find(|method| method.id() == kid)
        .expect("the kid resolves to a verification method");
    let mut validation = Validation::new(header.alg);
    validation.set_required_spec_claims(&["exp"]);
    validation.validate_aud = false;
    let key = DecodingKey::from_jwk(method.public_key_jwk()).expect("a decoding key");
    jsonwebtoken::decode::<Value>(&assertion, &key, &validation)
        .expect("the presentation verifies");
    let payload: Value =
        serde_json::from_slice(&nuts::payload(&assertion).expect("claims")).expect("JSON");
    assert_eq!(payload["iss"], document.id());
}
