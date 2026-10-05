// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The holder: the entity whose Verifiable Credentials the access token
//! request presents, named by a `did:web` identifier, with the key that signs
//! its presentations and the credentials it holds.
//!
//! GF-Authentication names every entity by a `did:web` Decentralized
//! Identifier (the IG's Authentication page, Entity Identifiers; GFI-001) and
//! encodes credentials and presentations as JWTs (GFI-004, VC Data Model 1.1
//! §6.3.1). The presentation is signed with a key its `kid` names by a DID URL
//! whose DID is the holder's (Nuts RFC021 §4.2, items 3 and 4), and every
//! credential is issued to the holder (§4.2 item 5).
//!
//! No type here renders a key or a credential: `Debug` shows the holder's
//! identifier, the key's DID URL and the input descriptor each credential
//! answers.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, EncodingKey};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::IgnoredAny;

/// The prefix of every identifier of the `did:web` method (DID 1.0 §3.1, the
/// did:web Method Specification §3.1).
pub const DID_WEB_PREFIX: &str = "did:web:";

/// A holder that cannot present.
///
/// No variant carries a key or a credential, or any part of one.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HolderError {
    /// The identifier is not a `did:web` DID (DID 1.0 §3.1, the did:web
    /// Method Specification §3.1).
    #[error("the holder identifier is not a did:web DID")]
    Did,
    /// The key's `kid` is not a DID URL of the holder's DID with a fragment
    /// (Nuts RFC021 §4.2 item 4).
    #[error("the key id is not a DID URL of the holder's DID with a fragment")]
    KeyId,
    /// The text is not an EC private key in PKCS#8 PEM.
    #[error("the holder key is not an EC private key in PKCS#8 PEM")]
    Pem(#[source] jsonwebtoken::errors::Error),
    /// The key is on neither P-256 (ES256) nor P-384 (ES384) (RFC 7518 §3.4).
    #[error("the holder key is on neither P-256 (ES256) nor P-384 (ES384)")]
    Curve(#[source] jsonwebtoken::errors::Error),
    /// The input descriptor a credential answers is empty.
    #[error("credential {index} names no input descriptor")]
    Descriptor {
        /// The credential's position.
        index: usize,
    },
    /// A credential is not a JWS in compact serialization with a JSON claims
    /// set (RFC 7515 §7.1, VC Data Model 1.1 §6.3.1).
    #[error("credential {index} is not a JWT-encoded credential")]
    NotAJwt {
        /// The credential's position.
        index: usize,
    },
    /// A credential's claims carry no `vc` claim (VC Data Model 1.1
    /// §6.3.1).
    #[error("credential {index} carries no vc claim")]
    NotACredential {
        /// The credential's position.
        index: usize,
    },
    /// A credential's `sub`, its `credentialSubject.id`, is not the holder
    /// (VC Data Model 1.1 §6.3.1, Nuts RFC021 §4.2 item 5).
    #[error("credential {index} is not issued to the holder")]
    OtherSubject {
        /// The credential's position.
        index: usize,
    },
    /// The holder presents no credential.
    #[error("the holder holds no credential")]
    NoCredential,
    /// Two credentials answer the same input descriptor.
    #[error("credentials {first} and {second} answer the same input descriptor")]
    SameDescriptor {
        /// The first credential's position.
        first: usize,
        /// The second credential's position.
        second: usize,
    },
}

/// A `did:web` Decentralized Identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Did(String);

impl Did {
    /// Reads `text` as a `did:web` DID: `did:web:` followed by one or more
    /// colon-separated segments of the DID 1.0 `idchar` set or
    /// percent-encoded octets (DID 1.0 §3.1, the did:web Method Specification
    /// §3.1).
    ///
    /// # Errors
    ///
    /// Returns [`HolderError::Did`] for any other text.
    pub fn new(text: impl Into<String>) -> Result<Self, HolderError> {
        let text = text.into();
        let id = text.strip_prefix(DID_WEB_PREFIX).ok_or(HolderError::Did)?;
        if id.is_empty() || !id.split(':').all(segment) {
            return Err(HolderError::Did);
        }
        Ok(Self(text))
    }

    /// The identifier as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path of the URL the DID resolves to, where its DID document is
    /// served: `/.well-known/did.json` for a DID that names a host alone,
    /// and each further colon-separated segment as a path segment before
    /// `/did.json` otherwise (the did:web Method Specification, Read
    /// (Resolve)).
    ///
    /// # Examples
    ///
    /// ```
    /// use nl_generic_functions::nuts_auth::holder::Did;
    ///
    /// let host = Did::new("did:web:gateway.example.org%3A8443")?;
    /// assert_eq!("/.well-known/did.json", host.document_path());
    /// let path = Did::new("did:web:gateway.example.org:fed:nuts")?;
    /// assert_eq!("/fed/nuts/did.json", path.document_path());
    /// # Ok::<(), nl_generic_functions::nuts_auth::holder::HolderError>(())
    /// ```
    #[must_use]
    pub fn document_path(&self) -> String {
        // NOTE: the did:web Method Specification, Read (Resolve): the segment after
        // the method name is the host, with its port, and every later one a path.
        let path: Vec<&str> = self.0.split(':').skip(3).collect();
        if path.is_empty() {
            String::from("/.well-known/did.json")
        } else {
            format!("/{}/did.json", path.join("/"))
        }
    }
}

impl fmt::Display for Did {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `segment` is a non-empty run of DID 1.0 `idchar`s, each a letter,
/// a digit, `.`, `-`, `_` or a `%` with two hex digits (DID 1.0 §3.1).
fn segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let encoded = bytes
                .get(index.saturating_add(1)..index.saturating_add(3))
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit));
            if !encoded {
                return false;
            }
            index = index.saturating_add(3);
        } else if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            index = index.saturating_add(1);
        } else {
            return false;
        }
    }
    !bytes.is_empty()
}

/// The key the holder signs its presentations with, and the DID URL that
/// names its verification method.
///
/// `Debug` shows the DID URL and the algorithm alone.
pub struct HolderKey {
    kid: String,
    algorithm: Algorithm,
    private: EncodingKey,
    public: Jwk,
}

impl HolderKey {
    /// Reads the private key `pem` holds in PKCS#8 PEM, named by the DID URL
    /// `kid` of `did`: a P-256 key signs ES256, a P-384 key ES384.
    ///
    /// # Errors
    ///
    /// Returns [`HolderError::KeyId`] when `kid` is not `did` followed by `#`
    /// and a fragment, [`HolderError::Pem`] for text that is no EC private
    /// key in PKCS#8 PEM, and [`HolderError::Curve`] for a key on another
    /// curve.
    pub fn from_pem(pem: &SecretString, kid: &str, did: &Did) -> Result<Self, HolderError> {
        let fragment = kid
            .strip_prefix(did.as_str())
            .and_then(|rest| rest.strip_prefix('#'))
            .ok_or(HolderError::KeyId)?;
        if fragment.is_empty()
            || fragment
                .chars()
                .any(|c| c == '#' || c.is_whitespace() || c.is_control())
        {
            return Err(HolderError::KeyId);
        }
        let private =
            EncodingKey::from_ec_pem(pem.expose_secret().as_bytes()).map_err(HolderError::Pem)?;
        let (algorithm, public) = match Jwk::from_encoding_key(&private, Algorithm::ES256) {
            Ok(public) => (Algorithm::ES256, public),
            Err(_not_p256) => (
                Algorithm::ES384,
                Jwk::from_encoding_key(&private, Algorithm::ES384).map_err(HolderError::Curve)?,
            ),
        };
        Ok(Self {
            kid: kid.to_owned(),
            algorithm,
            private,
            public,
        })
    }

    /// The public half of the key, as the JWK its verification method in
    /// the holder's DID document carries (DID 1.0 §5.2.1): the curve and
    /// coordinates, and `alg`, never a private member.
    #[must_use]
    pub fn public(&self) -> &Jwk {
        &self.public
    }

    /// The DID URL of the key's verification method, the `kid` of every
    /// presentation.
    #[must_use]
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The JWS algorithm every presentation is signed with.
    #[must_use]
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The algorithm's name as RFC 7518 §3.1 writes it, `ES256` or `ES384`.
    #[must_use]
    pub fn algorithm_name(&self) -> &'static str {
        if self.algorithm == Algorithm::ES256 {
            "ES256"
        } else {
            "ES384"
        }
    }

    pub(super) fn private(&self) -> &EncodingKey {
        &self.private
    }
}

impl fmt::Debug for HolderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HolderKey")
            .field("kid", &self.kid)
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

/// One JWT-encoded Verifiable Credential the holder holds, and the input
/// descriptor of the Presentation Definition it answers (Presentation
/// Exchange 2.0.0 §Input Descriptor Object).
///
/// `Debug` shows the input descriptor alone.
#[derive(Clone)]
pub struct HeldCredential {
    descriptor: String,
    jwt: SecretString,
    expires: Option<i64>,
}

impl HeldCredential {
    /// The input descriptor this credential answers.
    #[must_use]
    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }

    /// The credential's `exp`, in seconds since the epoch, when it has one.
    #[must_use]
    pub fn expires(&self) -> Option<i64> {
        self.expires
    }

    pub(super) fn jwt(&self) -> &SecretString {
        &self.jwt
    }
}

impl fmt::Debug for HeldCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeldCredential")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

/// The claims of a JWT-encoded credential this crate reads (VC Data Model
/// 1.1 §6.3.1).
#[derive(Deserialize)]
struct CredentialClaims {
    sub: Option<String>,
    exp: Option<i64>,
    vc: Option<IgnoredAny>,
}

/// The holder: its DID, its signing key and the credentials it presents.
///
/// `Debug` shows the DID, the key's DID URL and the input descriptors.
#[derive(Debug)]
pub struct Holder {
    did: Did,
    key: HolderKey,
    credentials: Vec<HeldCredential>,
}

impl Holder {
    /// The holder `did`, signing with `key` and presenting `credentials`,
    /// each a JWT-encoded Verifiable Credential (VC Data Model 1.1 §6.3.1)
    /// with the input descriptor it answers.
    ///
    /// The credentials are read for their claims and never verified: the
    /// authorization server verifies them (Nuts RFC021 §4).
    ///
    /// # Errors
    ///
    /// Returns [`HolderError::KeyId`] when the key is not one of `did`'s,
    /// [`HolderError::NoCredential`] for no credential, and for each
    /// credential [`HolderError::Descriptor`] for an empty descriptor,
    /// [`HolderError::NotAJwt`] for text that is no compact JWS with JSON
    /// claims, [`HolderError::NotACredential`] for claims without `vc`,
    /// [`HolderError::OtherSubject`] for a `sub` other than `did`, and
    /// [`HolderError::SameDescriptor`] for a descriptor answered twice.
    pub fn new(
        did: Did,
        key: HolderKey,
        credentials: Vec<(String, SecretString)>,
    ) -> Result<Self, HolderError> {
        if !key
            .kid()
            .strip_prefix(did.as_str())
            .is_some_and(|rest| rest.starts_with('#'))
        {
            return Err(HolderError::KeyId);
        }
        if credentials.is_empty() {
            return Err(HolderError::NoCredential);
        }
        let mut held: Vec<HeldCredential> = Vec::with_capacity(credentials.len());
        for (index, (descriptor, jwt)) in credentials.into_iter().enumerate() {
            if descriptor.trim().is_empty() {
                return Err(HolderError::Descriptor { index });
            }
            if let Some(first) = held.iter().position(|held| held.descriptor == descriptor) {
                return Err(HolderError::SameDescriptor {
                    first,
                    second: index,
                });
            }
            let claims = claims(jwt.expose_secret()).ok_or(HolderError::NotAJwt { index })?;
            if claims.vc.is_none() {
                return Err(HolderError::NotACredential { index });
            }
            if claims.sub.as_deref() != Some(did.as_str()) {
                return Err(HolderError::OtherSubject { index });
            }
            held.push(HeldCredential {
                descriptor,
                jwt,
                expires: claims.exp,
            });
        }
        Ok(Self {
            did,
            key,
            credentials: held,
        })
    }

    /// The holder's DID, the `iss` and `sub` of every presentation.
    #[must_use]
    pub fn did(&self) -> &Did {
        &self.did
    }

    /// The key every presentation is signed with.
    #[must_use]
    pub fn key(&self) -> &HolderKey {
        &self.key
    }

    /// The credentials, in the order they were given.
    #[must_use]
    pub fn credentials(&self) -> &[HeldCredential] {
        &self.credentials
    }
}

/// The claims of the compact JWS `jwt`, or `None` when it is not one with a
/// JSON object as its payload (RFC 7515 §7.1).
fn claims(jwt: &str) -> Option<CredentialClaims> {
    let mut parts = jwt.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if header.is_empty() || signature.is_empty() {
        return None;
    }
    // NOTE: RFC 7515 §7.1, a part that is not base64url or a payload that is not
    // a JSON object is legitimately not a JWT credential, refused as such.
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_did_web_identifier_is_read() {
        for text in [
            "did:web:example.org",
            "did:web:example.org%3A8443",
            "did:web:example.org:org:ura-0001",
        ] {
            assert!(Did::new(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for text in [
            "did:web:",
            "did:key:z6Mk",
            "did:web:exa mple.org",
            "did:web:example.org::x",
            "did:web:example.org%3",
            "did:web:example.org/path",
            "https://example.org",
        ] {
            assert!(matches!(Did::new(text), Err(HolderError::Did)), "{text}");
        }
    }

    #[test]
    fn a_compact_jws_is_read_for_its_claims() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"did:web:example.org","vc":{}}"#);
        let read = claims(&format!("e30.{payload}.c2ln")).expect("claims");
        assert_eq!(read.sub.as_deref(), Some("did:web:example.org"));
        assert!(read.vc.is_some());
        assert!(claims("e30.e30").is_none());
        assert!(claims("e30.e30.c2ln.x").is_none());
        assert!(claims("e30.!!.c2ln").is_none());
    }
}
