// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The GF-Authentication access token request (Annex B §B.4; the IG's
//! GFI-004; Nuts RFC021) against the harness Nuts node, with synthetic
//! values only: `did:web` identifiers under `example.org`, keys generated at
//! run time, and credentials minted for the test.

mod corpus;
mod did_document;
mod flow;
mod hygiene;
mod refusals;

use std::sync::Mutex;
use std::time::Duration;

use ferrofed_testkit::nuts::{self, NutsNode};
use ferrofed_testkit::oauth::p256_pem;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use nl_generic_functions::nuts_auth::error::ProofError;
use nl_generic_functions::nuts_auth::holder::{Did, Holder, HolderKey};
use nl_generic_functions::nuts_auth::{DpopProver, Grant, NutsClient};
use secrecy::SecretString;
use serde::Serialize;
use url::Url;

/// The holder: the gateway's organisation.
pub(crate) const HOLDER: &str = "did:web:gateway.example.org";

/// The DID URL of the holder's key.
pub(crate) const HOLDER_KID: &str = "did:web:gateway.example.org#key-1";

/// The credential issuer.
pub(crate) const ISSUER: &str = "did:web:issuer.example.org";

/// The DID URL of the issuer's key.
pub(crate) const ISSUER_KID: &str = "did:web:issuer.example.org#key-1";

/// The scope every grant asks for.
pub(crate) const SCOPE: &str = "openehr-query";

/// The input descriptor the organisation credential answers.
pub(crate) const DESCRIPTOR: &str = "organization_credential";

/// A timeout no harness answer comes near.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// The Presentation Definition the harness serves unless a test sets
/// another: one input descriptor.
pub(crate) const DEFINITION: &str = r#"{
  "id": "pd_synthetic_organization",
  "input_descriptors": [
    {
      "id": "organization_credential",
      "constraints": {
        "fields": [
          {"path": ["$.type"], "filter": {"type": "string", "const": "SyntheticOrganizationCredential"}}
        ]
      }
    }
  ]
}"#;

/// A `DPoP` prover over a key generated for the test (RFC 9449 §4.2).
pub(crate) struct TestProver {
    key: EncodingKey,
    jwk: Jwk,
    nonce: Mutex<Option<String>>,
    pub(crate) nonces: Mutex<Vec<String>>,
}

#[derive(Serialize)]
struct ProofClaims {
    jti: String,
    htm: String,
    htu: String,
    iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
}

impl TestProver {
    pub(crate) fn new() -> Self {
        let pem = p256_pem().expect("a key");
        let key = EncodingKey::from_ec_pem(pem.as_bytes()).expect("the key reads");
        let jwk = Jwk::from_encoding_key(&key, Algorithm::ES256).expect("the public key");
        Self {
            key,
            jwk,
            nonce: Mutex::new(None),
            nonces: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("the test proof could not be signed")]
struct Unsigned(#[source] jsonwebtoken::errors::Error);

impl DpopProver for TestProver {
    fn algorithm(&self) -> &'static str {
        "ES256"
    }

    fn proof(&self, method: &http::Method, url: &Url) -> Result<String, ProofError> {
        let mut htu = url.clone();
        htu.set_query(None);
        htu.set_fragment(None);
        let claims = ProofClaims {
            jti: uuid::Uuid::new_v4().to_string(),
            htm: method.as_str().to_owned(),
            htu: htu.to_string(),
            iat: jiff::Timestamp::now().as_second(),
            nonce: self.nonce.lock().expect("lock").clone(),
        };
        let mut header = Header::new(Algorithm::ES256);
        header.typ = Some(String::from("dpop+jwt"));
        header.jwk = Some(self.jwk.clone());
        jsonwebtoken::encode(&header, &claims, &self.key)
            .map_err(|error| ProofError::new(Unsigned(error)))
    }

    fn nonce(&self, _url: &Url, nonce: &str) {
        *self.nonce.lock().expect("lock") = Some(nonce.to_owned());
        self.nonces.lock().expect("lock").push(nonce.to_owned());
    }
}

/// One harness node and a holder it trusts.
pub(crate) struct Fixture {
    pub(crate) node: NutsNode,
    pub(crate) holder: Holder,
    pub(crate) grant: Grant,
    pub(crate) prover: TestProver,
    pub(crate) credential: String,
    pub(crate) holder_pem: String,
}

impl Fixture {
    /// A node trusting a fresh holder key and issuer key, serving
    /// [`DEFINITION`], and a holder with one credential valid for an hour.
    pub(crate) async fn start() -> Self {
        Self::with_expiry(jiff::Timestamp::now().as_second() + 3600).await
    }

    /// As [`Fixture::start`], with the credential expiring at `exp`.
    pub(crate) async fn with_expiry(exp: i64) -> Self {
        let node = NutsNode::start("hospital-a", SCOPE, Some(3600)).await;
        node.define(DEFINITION);
        let holder_pem = p256_pem().expect("a holder key");
        let issuer_pem = p256_pem().expect("an issuer key");
        node.trust_holder(
            HOLDER,
            HOLDER_KID,
            nuts::public_jwk(&holder_pem, Algorithm::ES256).expect("jwk"),
        );
        node.trust_issuer(
            ISSUER,
            ISSUER_KID,
            nuts::public_jwk(&issuer_pem, Algorithm::ES256).expect("jwk"),
        );
        let credential = nuts::credential(
            (&issuer_pem, ISSUER_KID),
            ISSUER,
            HOLDER,
            (
                "SyntheticOrganizationCredential",
                "Synthetic Care Organisation",
            ),
            exp,
        )
        .expect("a credential");
        let holder = holder(&holder_pem, &credential);
        let grant = Grant::new(&node.issuer(), SCOPE).expect("a grant");
        Self {
            node,
            holder,
            grant,
            prover: TestProver::new(),
            credential,
            holder_pem,
        }
    }

    /// The client every request goes through: no redirects.
    pub(crate) fn client() -> NutsClient {
        NutsClient::new(
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("a client"),
        )
    }
}

/// The holder [`HOLDER`] with the key `pem` and the one `credential`.
pub(crate) fn holder(pem: &str, credential: &str) -> Holder {
    let did = Did::new(HOLDER).expect("a DID");
    let key =
        HolderKey::from_pem(&SecretString::from(pem), HOLDER_KID, &did).expect("a holder key");
    Holder::new(
        did,
        key,
        vec![(DESCRIPTOR.to_owned(), SecretString::from(credential))],
    )
    .expect("a holder")
}
