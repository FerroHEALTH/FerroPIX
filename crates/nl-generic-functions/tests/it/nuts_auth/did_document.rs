// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The holder's DID document: where its `did:web` DID resolves (the did:web
//! Method Specification, Read (Resolve)), its shape (DID 1.0 §5.2, §5.3,
//! §6.3.1), and that it verifies a presentation the client signed for an
//! access token request (Nuts RFC021 §4.2 item 4).
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the written document and the presentation are read as values"
)]

use crate::testkit::keys::{es384_pem, p256_pem};
use crate::testkit::nuts;
use jsonwebtoken::{DecodingKey, Validation};
use nl_generic_functions::nuts_auth::did_document::{
    DID_CONTEXT, DidDocument, DocumentError, JSON_WEB_KEY_2020, JWS_2020_CONTEXT,
};
use nl_generic_functions::nuts_auth::holder::{Did, HolderError, HolderKey};
use secrecy::SecretString;
use serde_json::Value;

use super::{Fixture, HOLDER, HOLDER_KID, PROMPT};

/// The holder key `pem` holds, named `kid`.
fn key(pem: &str, kid: &str) -> HolderKey {
    let did = Did::new(HOLDER).expect("a DID");
    HolderKey::from_pem(&SecretString::from(pem), kid, &did).expect("a holder key")
}

/// The PKCS#8 PEM of a fresh key `document` holds.
fn pkcs8_pem(document: &[u8]) -> SecretString {
    SecretString::from(pem::encode(&pem::Pem::new(
        "PRIVATE KEY",
        document.to_vec(),
    )))
}

// RFC 7518 §3.4 and RFC 9964 §5: a holder signs ES256, ES384 or ML-DSA, so a
// P-521 key is another curve and an Ed25519 key, or text that holds no key,
// is no key a holder signs with.
#[test]
fn a_key_that_signs_neither_es256_nor_es384_is_refused() {
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{ECDSA_P521_SHA512_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair};
    let did = Did::new(HOLDER).expect("a DID");
    let random = SystemRandom::new();
    let p521 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P521_SHA512_FIXED_SIGNING, &random)
        .expect("a P-521 key");
    assert!(matches!(
        HolderKey::from_pem(&pkcs8_pem(p521.as_ref()), HOLDER_KID, &did),
        Err(HolderError::Curve(_))
    ));
    let ed25519 = Ed25519KeyPair::generate_pkcs8(&random).expect("an Ed25519 key");
    assert!(matches!(
        HolderKey::from_pem(&pkcs8_pem(ed25519.as_ref()), HOLDER_KID, &did),
        Err(HolderError::Pem(_))
    ));
    assert!(matches!(
        HolderKey::from_pem(&SecretString::from("no key"), HOLDER_KID, &did),
        Err(HolderError::Pem(_))
    ));
}

// RFC 9964 §5 and RFC 9881 §6: an ML-DSA key in PKCS#8 signs its parameter
// set, and the DID document carries it as an `AKP` key naming that `alg`.
#[test]
fn an_ml_dsa_key_signs_its_parameter_set_and_publishes_an_akp_key() {
    use aws_lc_rs::signature::{ML_DSA_65_SIGNING, PqdsaKeyPair};
    let did = Did::new(HOLDER).expect("a DID");
    let pkcs8 = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING)
        .expect("an ML-DSA-65 key")
        .to_pkcs8v1()
        .expect("its PKCS#8 form");
    let key = HolderKey::from_pem(&pkcs8_pem(pkcs8.as_ref()), HOLDER_KID, &did)
        .expect("an ML-DSA-65 holder key");
    assert_eq!("ML-DSA-65", key.algorithm_name());
    let public = key.public().to_value();
    assert_eq!(Some("AKP"), public.get("kty").and_then(Value::as_str));
    assert_eq!(Some("ML-DSA-65"), public.get("alg").and_then(Value::as_str));
    assert!(public.get("priv").is_none(), "no private member");
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
    let published = serde_json::from_value(method.public_key_jwk().to_value()).expect("a JWK");
    let key = DecodingKey::from_jwk(&published).expect("a decoding key");
    jsonwebtoken::decode::<Value>(&assertion, &key, &validation)
        .expect("the presentation verifies");
    let payload: Value =
        serde_json::from_slice(&nuts::payload(&assertion).expect("claims")).expect("JSON");
    assert_eq!(payload["iss"], document.id());
}
