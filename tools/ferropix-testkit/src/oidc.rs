// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A test OpenID Provider: a `wiremock` issuer with a JWK Set and a token
//! endpoint, whose signing key is generated per run, so no private key is
//! ever committed.
//!
//! [`Provider`] publishes its key's public half as a JWK Set (RFC 7517 §5),
//! signs the ID Tokens a test chooses (OpenID Connect Core 1.0 §2), and
//! answers the token endpoint's authorization code and refresh token grants
//! (RFC 6749 §4.1.3, §6) with the [`TokenAnswer`] a test mounts, or refuses
//! them (RFC 6749 §5.2). Its authorization and end-session endpoints answer
//! nothing until [`Provider::approve`] makes them sign a real browser in, as
//! the console's browser journeys need; until then the console redirects a
//! browser there and a test reads the redirect. No specification governs
//! the harness: our own design.
#![expect(
    clippy::disallowed_types,
    reason = "a JWT's claims and the token endpoint's answer are a JSON wire seam"
)]

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use aws_lc_rs::digest::{SHA256, digest};
use aws_lc_rs::rand::{SecureRandom as _, SystemRandom};
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::{Jwk, JwkSet, KeyAlgorithm, PublicKeyUse};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The path the key set answers at.
pub const JWKS_PATH: &str = "/jwks.json";

/// The path the authorization server metadata answers at (RFC 8414 §3).
pub const METADATA_PATH: &str = "/.well-known/oauth-authorization-server";

/// The path of the authorization endpoint.
pub const AUTHORIZATION_PATH: &str = "/authorize";

/// The path of the token endpoint.
pub const TOKEN_PATH: &str = "/token";

/// The path of the end-session endpoint.
pub const END_SESSION_PATH: &str = "/logout";

/// The `kid` the provider's key is published under.
pub const KID: &str = "k1";

/// A test provider could not be set up or could not sign.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// The key pair could not be generated.
    #[error("a key pair could not be generated")]
    Generate,
    /// The key could not be described as a JWK, or the token not signed.
    #[error("the token or the key could not be written")]
    Sign(#[source] jsonwebtoken::errors::Error),
    /// A body could not be written as JSON.
    #[error("the body could not be written")]
    Json(#[source] serde_json::Error),
}

/// A test OpenID Provider on its own mock server.
pub struct Provider {
    /// The server every endpoint answers on.
    server: MockServer,
    /// The private half of the signing key.
    encoding: EncodingKey,
    /// The public half, as published.
    jwk: Jwk,
}

impl fmt::Debug for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider")
            .field("issuer", &self.server.uri())
            .finish_non_exhaustive()
    }
}

impl Provider {
    /// Starts a provider with one ES256 key, published as [`KID`], whose
    /// issuer identifier is its server's base URL.
    ///
    /// # Errors
    /// Returns [`ProviderError`] when the key pair cannot be generated or
    /// described.
    pub async fn start() -> Result<Self, ProviderError> {
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .map_err(|_unspecified| ProviderError::Generate)?;
        let encoding = EncodingKey::from_ec_der(pkcs8.as_ref());
        let mut jwk =
            Jwk::from_encoding_key(&encoding, Algorithm::ES256).map_err(ProviderError::Sign)?;
        jwk.common.key_id = Some(KID.to_owned());
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);
        jwk.common.key_algorithm = Some(KeyAlgorithm::ES256);
        let provider = Self {
            server: MockServer::start().await,
            encoding,
            jwk,
        };
        provider.publish().await?;
        Ok(provider)
    }

    /// Returns the issuer identifier, the server's base URL.
    #[must_use]
    pub fn issuer(&self) -> String {
        self.server.uri()
    }

    /// Returns the authorization endpoint's URL.
    #[must_use]
    pub fn authorization_endpoint(&self) -> String {
        format!("{}{AUTHORIZATION_PATH}", self.server.uri())
    }

    /// Returns the token endpoint's URL.
    #[must_use]
    pub fn token_endpoint(&self) -> String {
        format!("{}{TOKEN_PATH}", self.server.uri())
    }

    /// Returns the JWK Set's URL.
    #[must_use]
    pub fn jwks_uri(&self) -> String {
        format!("{}{JWKS_PATH}", self.server.uri())
    }

    /// Returns the end-session endpoint's URL.
    #[must_use]
    pub fn end_session_endpoint(&self) -> String {
        format!("{}{END_SESSION_PATH}", self.server.uri())
    }

    /// Returns the mock server, for a test that mounts an answer of its own.
    #[must_use]
    pub const fn server(&self) -> &MockServer {
        &self.server
    }

    /// Returns the published key set.
    #[must_use]
    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: vec![self.jwk.clone()],
        }
    }

    /// Returns the header the provider signs with: ES256 and [`KID`].
    #[must_use]
    pub fn header() -> Header {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(KID.to_owned());
        header
    }

    /// Signs `claims` as an ID Token under [`Self::header`].
    ///
    /// # Errors
    /// Returns [`ProviderError`] when the claims cannot be written or signed.
    pub fn id_token(&self, claims: &IdTokenClaims) -> Result<String, ProviderError> {
        let payload = serde_json::to_value(claims).map_err(ProviderError::Json)?;
        self.sign(&Self::header(), &payload)
    }

    /// Signs `payload`, whatever it holds, under `header`.
    ///
    /// # Errors
    /// Returns [`ProviderError::Sign`] when the token cannot be signed.
    pub fn sign(
        &self,
        header: &Header,
        payload: &serde_json::Value,
    ) -> Result<String, ProviderError> {
        jsonwebtoken::encode(header, payload, &self.encoding).map_err(ProviderError::Sign)
    }

    /// Answers every authorization code grant at the token endpoint with
    /// `answer`.
    ///
    /// # Errors
    /// Returns [`ProviderError::Json`] when the answer cannot be written.
    pub async fn answer_code(&self, answer: &TokenAnswer) -> Result<(), ProviderError> {
        self.answer_grant("grant_type=authorization_code", answer)
            .await
    }

    /// Answers a refresh token grant that sends `refresh_token` with
    /// `answer`.
    ///
    /// # Errors
    /// Returns [`ProviderError::Json`] when the answer cannot be written.
    pub async fn answer_refresh(
        &self,
        refresh_token: &str,
        answer: &TokenAnswer,
    ) -> Result<(), ProviderError> {
        let body = serde_json::to_string(answer).map_err(ProviderError::Json)?;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains(format!(
                "refresh_token={refresh_token}"
            )))
            .respond_with(json(200, body))
            .mount(&self.server)
            .await;
        Ok(())
    }

    /// Refuses every grant at the token endpoint with `status` and the
    /// error code `error` (RFC 6749 §5.2).
    pub async fn refuse(&self, status: u16, error: &str) {
        let body = serde_json::json!({ "error": error }).to_string();
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(json(status, body))
            .mount(&self.server)
            .await;
    }

    /// Returns every request the token endpoint received, oldest first;
    /// `None` when the server records no requests.
    pub async fn token_requests(&self) -> Option<Vec<Request>> {
        let received = self.server.received_requests().await?;
        Some(
            received
                .into_iter()
                .filter(|request| request.url.path() == TOKEN_PATH)
                .collect(),
        )
    }

    /// Forgets every answer mounted, and publishes the key set again.
    ///
    /// # Errors
    /// Returns [`ProviderError::Json`] when the key set cannot be written.
    pub async fn reset(&self) -> Result<(), ProviderError> {
        self.server.reset().await;
        self.publish().await
    }

    /// Mounts the key set.
    async fn publish(&self) -> Result<(), ProviderError> {
        let body = serde_json::to_string(&self.jwks()).map_err(ProviderError::Json)?;
        Mock::given(method("GET"))
            .and(path(JWKS_PATH))
            .respond_with(json(200, body))
            .mount(&self.server)
            .await;
        Ok(())
    }

    /// Publishes the provider's authorization server metadata (RFC 8414 §3)
    /// at `/.well-known/oauth-authorization-server`, for a resource server
    /// that discovers its key set from the issuer.
    pub async fn publish_metadata(&self) {
        let body = serde_json::json!({
            "issuer": self.issuer(),
            "jwks_uri": self.jwks_uri(),
            "authorization_endpoint": self.authorization_endpoint(),
            "token_endpoint": self.token_endpoint(),
            "end_session_endpoint": self.end_session_endpoint(),
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"],
        })
        .to_string();
        Mock::given(method("GET"))
            .and(path(METADATA_PATH))
            .respond_with(json(200, body))
            .mount(&self.server)
            .await;
    }

    /// Signs an access token in the RFC 9068 profile for `audience`
    /// granting `scope`, valid for an hour.
    ///
    /// # Errors
    /// Returns [`ProviderError::Sign`] when the token cannot be signed.
    pub fn access_token(&self, audience: &str, scope: &str) -> Result<String, ProviderError> {
        let now = jiff::Timestamp::now().as_second();
        let mut header = Self::header();
        header.typ = Some(String::from("at+jwt"));
        let claims = serde_json::json!({
            "iss": self.issuer(),
            "aud": audience,
            "sub": "synthetic-operator",
            "client_id": "ferrobridge-viewer",
            "iat": now,
            "exp": now.saturating_add(3600),
            "jti": "synthetic-operator-access",
            "scope": scope,
        });
        self.sign(&header, &claims)
    }

    /// Makes the provider sign a browser in: the authorization endpoint
    /// approves every request of `client_id` at once and redirects back with
    /// a fresh code and the request's `state` (RFC 6749 §4.1.2), the token
    /// endpoint trades that code once for the verifier whose `S256`
    /// challenge the request carried (RFC 7636 §4.6) and answers with
    /// `access_token` and an ID Token carrying the request's `nonce`
    /// (OpenID Connect Core 1.0 §3.1.3.7), and the end-session endpoint
    /// sends the browser to its `post_logout_redirect_uri` (OpenID Connect
    /// RP-Initiated Logout 1.0 §2).
    pub async fn approve(&self, client_id: &str, access_token: &str) {
        let codes = Codes::default();
        Mock::given(method("GET"))
            .and(path(AUTHORIZATION_PATH))
            .respond_with(Approve {
                issuer: self.issuer(),
                client_id: client_id.to_owned(),
                encoding: self.encoding.clone(),
                codes: Arc::clone(&codes),
            })
            .mount(&self.server)
            .await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .and(body_string_contains("grant_type=authorization_code"))
            .respond_with(Trade {
                codes,
                access_token: access_token.to_owned(),
            })
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path(END_SESSION_PATH))
            .respond_with(EndSession)
            .mount(&self.server)
            .await;
    }

    /// Returns the query pairs of every request the authorization endpoint
    /// received, oldest first; `None` when the server records no requests.
    pub async fn authorization_requests(&self) -> Option<Vec<BTreeMap<String, String>>> {
        let received = self.server.received_requests().await?;
        Some(
            received
                .iter()
                .filter(|request| request.url.path() == AUTHORIZATION_PATH)
                .map(|request| request.url.query_pairs().into_owned().collect())
                .collect(),
        )
    }

    /// Mounts `answer` for a token request whose form carries `grant`.
    async fn answer_grant(&self, grant: &str, answer: &TokenAnswer) -> Result<(), ProviderError> {
        let body = serde_json::to_string(answer).map_err(ProviderError::Json)?;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .and(body_string_contains(grant))
            .respond_with(json(200, body))
            .mount(&self.server)
            .await;
        Ok(())
    }
}

/// What one approved authorization request leaves for the token request:
/// the ID Token to answer with and the PKCE challenge to hold the verifier
/// to.
#[derive(Debug)]
struct Approved {
    /// The signed ID Token.
    id_token: String,
    /// The `code_challenge` the request carried.
    challenge: String,
}

/// The codes the provider has issued and not yet traded.
type Codes = Arc<Mutex<BTreeMap<String, Approved>>>;

/// A plain-text refusal of `status` saying `why`.
fn refused(status: u16, why: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_string(why.to_owned())
}

/// A fresh authorization code: 16 random bytes, base64url.
fn fresh_code() -> Option<String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new().fill(&mut bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}

/// The authorization endpoint of [`Provider::approve`].
struct Approve {
    /// The issuer identifier the ID Token names.
    issuer: String,
    /// The one client it approves.
    client_id: String,
    /// The key the ID Token is signed with.
    encoding: EncodingKey,
    /// Where an approved request waits for its token request.
    codes: Codes,
}

impl Respond for Approve {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let pairs: BTreeMap<String, String> = request.url.query_pairs().into_owned().collect();
        let field = |name: &str| pairs.get(name).map(String::as_str);
        let (Some(redirect), Some(state), Some(nonce), Some(challenge), Some("S256")) = (
            field("redirect_uri"),
            field("state"),
            field("nonce"),
            field("code_challenge"),
            field("code_challenge_method"),
        ) else {
            return refused(400, "the authorization request is incomplete");
        };
        if field("client_id") != Some(self.client_id.as_str())
            || field("response_type") != Some("code")
        {
            return refused(400, "the request names another client or grant");
        }
        let Ok(mut location) = url::Url::parse(redirect) else {
            return refused(400, "the redirect_uri is no URL");
        };
        let claims = IdTokenClaims::new(&self.issuer, &self.client_id, nonce);
        let signed = serde_json::to_value(&claims).ok().and_then(|payload| {
            jsonwebtoken::encode(&Provider::header(), &payload, &self.encoding).ok()
        });
        let (Some(id_token), Some(code)) = (signed, fresh_code()) else {
            return refused(500, "the ID Token or the code could not be made");
        };
        self.codes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                code.clone(),
                Approved {
                    id_token,
                    challenge: challenge.to_owned(),
                },
            );
        location
            .query_pairs_mut()
            .append_pair("code", &code)
            .append_pair("state", state);
        ResponseTemplate::new(303).insert_header("location", location.as_str())
    }
}

/// The token endpoint of [`Provider::approve`].
struct Trade {
    /// The codes the authorization endpoint issued.
    codes: Codes,
    /// The access token every trade answers with.
    access_token: String,
}

impl Respond for Trade {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let form: BTreeMap<String, String> = url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();
        let (Some(code), Some(verifier)) = (form.get("code"), form.get("code_verifier")) else {
            return refused(400, "the token request is incomplete");
        };
        let approved = self
            .codes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(code);
        let Some(approved) = approved else {
            return refused(400, "the code was never issued or is spent");
        };
        let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()));
        if challenge != approved.challenge {
            return refused(400, "the code verifier does not answer the challenge");
        }
        let answer = TokenAnswer {
            expires_in: Some(3600),
            id_token: Some(approved.id_token),
            ..TokenAnswer::bearer(&self.access_token)
        };
        match serde_json::to_string(&answer) {
            Ok(body) => json(200, body),
            Err(_unwritable) => refused(500, "the answer could not be written"),
        }
    }
}

/// The end-session endpoint of [`Provider::approve`].
struct EndSession;

impl Respond for EndSession {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let redirect = request
            .url
            .query_pairs()
            .find(|(name, _)| name == "post_logout_redirect_uri")
            .map(|(_, value)| value.into_owned());
        match redirect {
            Some(redirect) => ResponseTemplate::new(303).insert_header("location", redirect),
            None => refused(400, "no post_logout_redirect_uri"),
        }
    }
}

/// A JSON answer with `status` and `body`.
fn json(status: u16, body: String) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("content-type", "application/json")
        .insert_header("cache-control", "no-store")
        .set_body_string(body)
}

/// The claims of a test ID Token: by default every claim OpenID Connect
/// Core 1.0 §2 requires, valid for five minutes.
#[derive(Debug, Clone, Serialize)]
pub struct IdTokenClaims {
    /// `iss`.
    pub iss: String,
    /// `sub`.
    pub sub: String,
    /// `aud`: one string, or an array.
    pub aud: serde_json::Value,
    /// `exp`, in seconds since the epoch.
    pub exp: i64,
    /// `iat`, in seconds since the epoch.
    pub iat: i64,
    /// `nonce`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// `azp`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,
    /// `name`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `preferred_username`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
}

impl IdTokenClaims {
    /// Returns the claims of an ID Token `issuer` issues to `client_id` for
    /// the sign-in that sent `nonce`, for the synthetic operator.
    #[must_use]
    pub fn new(issuer: &str, client_id: &str, nonce: &str) -> Self {
        let now = jiff::Timestamp::now().as_second();
        Self {
            iss: issuer.to_owned(),
            sub: String::from("synthetic-operator"),
            aud: serde_json::Value::String(client_id.to_owned()),
            exp: now.saturating_add(300),
            iat: now,
            nonce: Some(nonce.to_owned()),
            azp: None,
            name: Some(String::from("Synthetic Operator")),
            preferred_username: None,
        }
    }
}

/// A token endpoint's answer (RFC 6749 §5.1, OpenID Connect Core 1.0
/// §3.1.3.3).
#[derive(Debug, Clone, Serialize)]
pub struct TokenAnswer {
    /// `access_token`.
    pub access_token: String,
    /// `token_type`.
    pub token_type: String,
    /// `expires_in`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    /// `refresh_token`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// `id_token`, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
}

impl TokenAnswer {
    /// Returns a bearer answer carrying `access_token` alone.
    #[must_use]
    pub fn bearer(access_token: &str) -> Self {
        Self {
            access_token: access_token.to_owned(),
            token_type: String::from("Bearer"),
            expires_in: None,
            refresh_token: None,
            id_token: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{DecodingKey, Validation};

    use super::{IdTokenClaims, KID, Provider};

    #[tokio::test]
    async fn a_signed_id_token_verifies_under_the_published_key_set() {
        let provider = Provider::start().await.unwrap();
        let claims = IdTokenClaims::new(&provider.issuer(), "synthetic-client", "n-1");
        let token = provider.id_token(&claims).unwrap();
        let published: jsonwebtoken::jwk::JwkSet = reqwest::get(provider.jwks_uri())
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let key = DecodingKey::from_jwk(published.find(KID).unwrap()).unwrap();
        let mut validation = Validation::new(jsonwebtoken::Algorithm::ES256);
        validation.set_audience(&["synthetic-client"]);
        validation.set_issuer(&[provider.issuer()]);
        let decoded = jsonwebtoken::decode::<serde_json::Value>(&token, &key, &validation).unwrap();
        assert_eq!(Some("n-1"), decoded.claims["nonce"].as_str());
    }
}
