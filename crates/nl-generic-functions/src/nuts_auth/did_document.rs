// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The holder's DID document: the document its `did:web` DID resolves to,
//! naming the keys its presentations are signed with.
//!
//! An authorization server resolves the DID URL a presentation's `kid` names
//! to a verification method of the holder's DID document (Nuts RFC021 §4.2
//! item 4). Under the `did:web` method that document is served at the HTTPS
//! URL the DID names ([`Did::document_path`]; the did:web Method
//! Specification, Read (Resolve)), and its `id` is the DID itself.
//!
//! A [`DidDocument`] is built from the holder keys alone, so it carries the
//! public half of each key the holder signs with and nothing else: one
//! `JsonWebKey2020` verification method per key, its `id` the key's DID URL,
//! its `controller` the DID and its `publicKeyJwk` the key's public JWK (DID
//! 1.0 §5.2, §5.2.1), referenced from `authentication` and `assertionMethod`
//! (DID 1.0 §5.3.1, §5.3.2). The document is the JSON-LD representation,
//! with the DID 1.0 context first and the JSON Web Signature 2020 context
//! that defines `JsonWebKey2020` (DID 1.0 §6.3.1), served as
//! [`MEDIA_TYPE`].

use jsonwebtoken::jwk::Jwk;
use serde::Serialize;

use crate::nuts_auth::holder::{Did, HolderKey};

/// The media type of a DID document in its JSON-LD representation (DID 1.0
/// §6.3.1, Appendix E.2).
pub const MEDIA_TYPE: &str = "application/did+ld+json";

/// The context every DID document's JSON-LD representation names first
/// (DID 1.0 §6.3.1).
pub const DID_CONTEXT: &str = "https://www.w3.org/ns/did/v1";

/// The context that defines the `JsonWebKey2020` verification method type
/// (DID 1.0 §5.2.1, Example 13).
pub const JWS_2020_CONTEXT: &str = "https://w3id.org/security/suites/jws-2020/v1";

/// The verification method type of a key given as a JWK (DID 1.0 §5.2.1).
pub const JSON_WEB_KEY_2020: &str = "JsonWebKey2020";

/// A DID document that cannot be built from its keys.
///
/// No variant carries key material; a DID URL names a key and is public.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentError {
    /// The document is given no key.
    #[error("the DID document names no key")]
    NoKey,
    /// A key's DID URL is not one of the document's DID.
    #[error("the key {kid} is not a key of {did}")]
    OtherDid {
        /// The key's DID URL.
        kid: String,
        /// The document's DID.
        did: String,
    },
    /// Two different keys are named by one DID URL, so a presentation's
    /// `kid` would name either.
    #[error("two different keys are named {kid}")]
    SameKeyId {
        /// The DID URL both keys are named by.
        kid: String,
    },
}

/// One verification method of a DID document (DID 1.0 §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationMethod {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    controller: String,
    #[serde(rename = "publicKeyJwk")]
    public_key_jwk: Jwk,
}

impl VerificationMethod {
    /// The DID URL of the method, the `kid` of every presentation it
    /// verifies.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The public key, as a JWK with no private member (DID 1.0 §5.2.1).
    #[must_use]
    pub fn public_key_jwk(&self) -> &Jwk {
        &self.public_key_jwk
    }
}

/// The DID document a `did:web` DID resolves to, built from the keys its
/// holder signs with (DID 1.0 §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DidDocument {
    #[serde(rename = "@context")]
    context: [&'static str; 2],
    id: String,
    #[serde(rename = "verificationMethod")]
    verification_method: Vec<VerificationMethod>,
    authentication: Vec<String>,
    #[serde(rename = "assertionMethod")]
    assertion_method: Vec<String>,
}

impl DidDocument {
    /// The document of `did`, naming each of `keys` once, in the order
    /// given; a key given twice is named once.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::NoKey`] for no key,
    /// [`DocumentError::OtherDid`] for a key whose DID URL is not `did`
    /// followed by `#`, and [`DocumentError::SameKeyId`] for two different
    /// keys named by one DID URL.
    pub fn new<'a>(
        did: &Did,
        keys: impl IntoIterator<Item = &'a HolderKey>,
    ) -> Result<Self, DocumentError> {
        let mut methods: Vec<VerificationMethod> = Vec::new();
        for key in keys {
            let of_did = key
                .kid()
                .strip_prefix(did.as_str())
                .is_some_and(|rest| rest.starts_with('#'));
            if !of_did {
                return Err(DocumentError::OtherDid {
                    kid: key.kid().to_owned(),
                    did: did.as_str().to_owned(),
                });
            }
            match methods.iter().find(|method| method.id == key.kid()) {
                Some(known) if known.public_key_jwk == *key.public() => {}
                Some(_) => {
                    return Err(DocumentError::SameKeyId {
                        kid: key.kid().to_owned(),
                    });
                }
                None => methods.push(VerificationMethod {
                    id: key.kid().to_owned(),
                    kind: JSON_WEB_KEY_2020,
                    controller: did.as_str().to_owned(),
                    public_key_jwk: key.public().clone(),
                }),
            }
        }
        if methods.is_empty() {
            return Err(DocumentError::NoKey);
        }
        // NOTE: Nuts RFC021 §4.2 item 4 names no verification relationship, so each
        // key is referenced from the two DID 1.0 §5.3 gives a presentation's signer.
        let referenced: Vec<String> = methods.iter().map(|method| method.id.clone()).collect();
        Ok(Self {
            context: [DID_CONTEXT, JWS_2020_CONTEXT],
            id: did.as_str().to_owned(),
            verification_method: methods,
            authentication: referenced.clone(),
            assertion_method: referenced,
        })
    }

    /// The DID the document describes, its `id`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The verification methods, one per key.
    #[must_use]
    pub fn verification_methods(&self) -> &[VerificationMethod] {
        &self.verification_method
    }
}
