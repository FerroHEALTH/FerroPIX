// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! GF-Authentication on the Nuts profile (feature `nuts-auth`).
//!
//! The client side of the access token request with a Verifiable
//! Presentation as the authorization grant, and a `DPoP`-bound token (Annex B
//! §B.4 of the Federation Tier specification; the IG's GFI-004 and GFI-005;
//! Nuts RFC021).
//!
//! [`NutsClient::request_access_token`] runs the flow of Nuts RFC021 §1
//! against one authorization server:
//!
//! 1. it reads the server's metadata (RFC 8414 §3) and holds it to the grant:
//!    the `issuer` it was asked of, the same text and the same URL (RFC 8414
//!    §3.3), a `token_endpoint` and a `presentation_definition_endpoint`
//!    (RFC021 §5) on the issuer's origin, `vp_formats` admitting a `jwt_vp`
//!    signed with the holder's algorithm (RFC021 §3.1), and, when listed,
//!    `dpop_signing_alg_values_supported` naming the prover's (RFC 9449
//!    §5.1). Every answer is refused when an object in it repeats a name,
//!    and every endpoint is the parsed URL the request is sent to, never
//!    parsed twice;
//! 2. it reads the Presentation Definition for the scope (RFC021 §5) and
//!    maps the holder's credentials to it in a Presentation Submission
//!    ([`presentation`]);
//! 3. it signs a JWT Verifiable Presentation of every credential (VC Data
//!    Model 1.1 §4.10 and §6.3.1, RFC021 §4.2): `iss` and `sub` the holder's
//!    DID, `kid` the DID URL of its key, `aud` the issuer, `nbf` now and `exp`
//!    five seconds later, a fresh `nonce` and `jti`;
//! 4. it posts the token request, `grant_type` [`GRANT_TYPE`] with the
//!    presentation as `assertion`, the submission and the scope (RFC021 §3),
//!    with a `DPoP` proof of the request from the [`DpopProver`] (RFC 9449
//!    §5), and answers a demanded nonce by sending once more with a new
//!    presentation and a new proof (RFC 9449 §8; RFC021 §4.4 refuses a reused
//!    presentation nonce);
//! 5. it takes the token only when it is `DPoP`-bound (RFC 9449 §5, GFI-004).
//!
//! The crate depends on no application: the key the token is bound to is
//! the caller's, behind [`DpopProver`], and the same key proves every request
//! the token is sent with (GFI-005).
//!
//! No error, `Debug` rendering or log of this module carries a credential,
//! the presentation, a key, a proof or the token. Every answer is read to at
//! most [`RESPONSE_LIMIT`] bytes, and the whole flow is bounded by one
//! timeout.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use nl_generic_functions::nuts_auth::error::ProofError;
//! use nl_generic_functions::nuts_auth::holder::{Did, Holder, HolderKey};
//! use nl_generic_functions::nuts_auth::{DpopProver, Grant, NutsClient};
//! use secrecy::SecretString;
//! use url::Url;
//!
//! struct Prover;
//!
//! impl DpopProver for Prover {
//!     fn algorithm(&self) -> &'static str {
//!         "ES256"
//!     }
//!     fn proof(&self, _method: &http::Method, _url: &Url) -> Result<String, ProofError> {
//!         Ok(String::from("a proof signed with the caller's key"))
//!     }
//!     fn nonce(&self, _url: &Url, _nonce: &str) {}
//! }
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let pem = SecretString::from(std::fs::read_to_string("/run/secrets/nuts-holder.pem")?);
//! let credential = SecretString::from(std::fs::read_to_string("/run/secrets/organization.jwt")?);
//! let did = Did::new("did:web:gateway.example.org")?;
//! let key = HolderKey::from_pem(&pem, "did:web:gateway.example.org#key-1", &did)?;
//! let holder = Holder::new(did, key, vec![(String::from("organization"), credential)])?;
//! let grant = Grant::new("https://nuts.example.org/oauth2/hospital", "openehr-query")?;
//! let http = reqwest::Client::builder()
//!     .redirect(reqwest::redirect::Policy::none())
//!     .build()?;
//! let token = NutsClient::new(http)
//!     .request_access_token(&grant, &holder, &Prover, Duration::from_secs(5))
//!     .await?;
//! let _lifetime = token.expires_in();
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

pub mod error;
pub mod holder;
mod metadata;
pub mod presentation;
mod strict;

use std::fmt;
use std::time::{Duration, Instant};

use http::header::{ACCEPT, CONTENT_TYPE};
use http::{Method, StatusCode};
use jsonwebtoken::Header;
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::{Url, form_urlencoded};

use error::{InvalidInput, Malformation, NutsAuthError, Step, bounded};
use holder::Holder;
use presentation::PresentationDefinition;

/// The `grant_type` of the VP Token Grant Type (Nuts RFC021 §3).
pub const GRANT_TYPE: &str = "vp_token-bearer";

/// The base context of a Verifiable Presentation (VC Data Model 1.1 §4.1).
pub const CREDENTIALS_CONTEXT: &str = "https://www.w3.org/2018/credentials/v1";

/// The type of a Verifiable Presentation (VC Data Model 1.1 §4.10).
pub const PRESENTATION_TYPE: &str = "VerifiablePresentation";

/// The header a `DPoP` proof travels in (RFC 9449 §4.1).
pub const DPOP_HEADER: &str = "DPoP";

/// The header a server sends a `DPoP` nonce in (RFC 9449 §8).
pub const DPOP_NONCE_HEADER: &str = "DPoP-Nonce";

/// The `error` a token endpoint names when it demands a nonce (RFC 9449 §8).
pub const USE_DPOP_NONCE: &str = "use_dpop_nonce";

/// The `token_type` of a `DPoP`-bound access token (RFC 9449 §5).
pub const DPOP_TOKEN_TYPE: &str = "DPoP";

/// The longest a presentation is valid, `exp` less `nbf` (Nuts RFC021 §4.2
/// item 9).
pub const PRESENTATION_LIFETIME: Duration = Duration::from_secs(5);

/// The longest answer the client reads from any endpoint (no specification
/// governs this: our own design).
pub const RESPONSE_LIMIT: usize = 64 * 1024;

/// The media type every answer is read as (RFC 8414 §3.2, RFC 6749 §5.1).
const JSON: &str = "application/json";

/// The signing side of `DPoP` for the token request (RFC 9449 §4): the key
/// the issued token is bound to, held by the caller.
///
/// The caller proves every later request the token is sent with using the
/// same key (RFC 9449 §7, the IG's GFI-005).
pub trait DpopProver: Send + Sync {
    /// The JWS algorithm every proof is signed with, as RFC 7518 §3.1 names
    /// it (`ES256`, `ES384`).
    fn algorithm(&self) -> &'static str;

    /// Returns a proof of a request with `method` to `url` that carries no
    /// access token, naming the nonce the server at `url` sent last, when it
    /// sent one (RFC 9449 §4.2).
    ///
    /// # Errors
    ///
    /// Returns a [`ProofError`](error::ProofError) when no proof can be
    /// signed.
    fn proof(&self, method: &Method, url: &Url) -> Result<String, error::ProofError>;

    /// Keeps `nonce`, the one the authorization server at `url` sent, for the
    /// next proof to it (RFC 9449 §8).
    fn nonce(&self, url: &Url, nonce: &str);
}

/// One authorization server and the scope the token is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    issuer: String,
    issuer_url: Url,
    scope: String,
    client_id: Option<String>,
}

impl Grant {
    /// The grant of `scope` at the authorization server whose issuer
    /// identifier is `authorization_server` (RFC 8414 §2).
    ///
    /// The issuer is an `https` URL (RFC 8414 §2, Nuts RFC021 §7); `http` is
    /// accepted for a test or development setup, and the caller decides
    /// whether a deployment may use it.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidInput::AuthorizationServer`] for an issuer that is no
    /// `http` or `https` URL, carries userinfo, a query or a fragment, or is
    /// not written in the canonical form the URL parser gives it (a lower-case
    /// host, no default port), and
    /// [`InvalidInput::Scope`] for a scope that is empty or holds a character
    /// outside RFC 6749 §3.3.
    pub fn new(authorization_server: &str, scope: &str) -> Result<Self, InvalidInput> {
        let issuer_url = Url::parse(authorization_server)
            .map_err(|_unparsable| InvalidInput::AuthorizationServer)?;
        if !matches!(issuer_url.scheme(), "http" | "https")
            || !issuer_url.username().is_empty()
            || issuer_url.password().is_some()
            || issuer_url.query().is_some()
            || issuer_url.fragment().is_some()
            || !canonical(authorization_server, &issuer_url)
        {
            return Err(InvalidInput::AuthorizationServer);
        }
        // NOTE: RFC 6749 §3.3, a scope token is %x21 / %x23-5B / %x5D-7E, and
        // tokens are separated by single spaces.
        let token = |token: &str| {
            !token.is_empty()
                && token
                    .bytes()
                    .all(|b| b == 0x21 || (0x23..=0x5B).contains(&b) || (0x5D..=0x7E).contains(&b))
        };
        if !scope.split(' ').all(token) {
            return Err(InvalidInput::Scope);
        }
        Ok(Self {
            issuer: authorization_server.to_owned(),
            issuer_url,
            scope: scope.to_owned(),
            client_id: None,
        })
    }

    /// This grant, identifying the client with `client_id` at the token
    /// endpoint (RFC 6749 §3.2.1).
    ///
    /// # Errors
    ///
    /// Returns [`InvalidInput::ClientId`] for an empty value or one with a
    /// control character.
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Result<Self, InvalidInput> {
        let client_id = client_id.into();
        if client_id.is_empty() || client_id.chars().any(char::is_control) {
            return Err(InvalidInput::ClientId);
        }
        self.client_id = Some(client_id);
        Ok(self)
    }

    /// The issuer identifier, as written: the `aud` of every presentation.
    #[must_use]
    pub fn authorization_server(&self) -> &str {
        &self.issuer
    }

    /// The scope the token is asked for.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The `client_id` the request carries, when it carries one.
    #[must_use]
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }
}

/// Whether `text` is the serialization of `url`, the form the URL parser
/// writes, or that form less the `/` it gives an empty path.
///
/// An issuer is compared as text (RFC 8414 §3.3) and fetched as a URL, so
/// only a text the parser leaves unchanged has one meaning for both (no
/// specification governs this: our own design).
fn canonical(text: &str, url: &Url) -> bool {
    text == url.as_str() || (url.path() == "/" && url.as_str().strip_suffix('/') == Some(text))
}

/// A `DPoP`-bound access token the authorization server issued.
///
/// `Debug` shows the lifetime and the scope, never the token.
#[derive(Clone)]
pub struct AccessToken {
    token: SecretString,
    expires_in: Option<Duration>,
    scope: Option<String>,
}

impl AccessToken {
    /// The access token, sent under the `DPoP` scheme with a proof of the
    /// key it is bound to (RFC 9449 §7.1).
    #[must_use]
    pub fn token(&self) -> &SecretString {
        &self.token
    }

    /// The lifetime `expires_in` stated (RFC 6749 §5.1), or `None`.
    #[must_use]
    pub fn expires_in(&self) -> Option<Duration> {
        self.expires_in
    }

    /// The scope the server granted, when it stated one (RFC 6749 §5.1).
    #[must_use]
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken")
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

/// The client side of the Nuts access token request.
///
/// `Debug` leaves out the HTTP client, whose default headers may hold a
/// credential.
#[derive(Clone)]
pub struct NutsClient {
    http: reqwest::Client,
}

impl fmt::Debug for NutsClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NutsClient").finish_non_exhaustive()
    }
}

/// The claims of the Verifiable Presentation (VC Data Model 1.1 §6.3.1,
/// Nuts RFC021 §4.2).
#[derive(Serialize)]
struct PresentationClaims<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    nbf: i64,
    exp: i64,
    jti: String,
    nonce: String,
    vp: Presentation<'a>,
}

/// The `vp` claim (VC Data Model 1.1 §4.10, §6.3.1).
#[derive(Serialize)]
struct Presentation<'a> {
    #[serde(rename = "@context")]
    context: [&'static str; 1],
    #[serde(rename = "type")]
    types: [&'static str; 1],
    #[serde(rename = "verifiableCredential")]
    credentials: Vec<&'a str>,
}

/// An RFC 6749 §5.1 token response, the members the client reads.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
    scope: Option<String>,
}

/// An RFC 6749 §5.2 error response, the members the client reads.
#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    error_description: Option<String>,
}

/// One answer, read: its status, its `DPoP-Nonce`, whether it is JSON, and
/// its body.
struct Answer {
    status: StatusCode,
    nonce: Option<String>,
    json: bool,
    body: Vec<u8>,
}

impl NutsClient {
    /// The client that sends every request through `http`.
    ///
    /// `http` carries the transport the caller chose: TLS and its roots,
    /// a proxy. Build it with `redirect::Policy::none()`: the token request
    /// carries the holder's credentials, and a client that follows redirects
    /// sends them wherever the server points it. Every `3xx` is then an
    /// error like any other unexpected status.
    #[must_use]
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Asks the authorization server of `grant` for a `DPoP`-bound access
    /// token, presenting `holder`'s credentials and proving the request with
    /// `prover`.
    ///
    /// `timeout` bounds the whole flow, metadata, definition and token
    /// request, from connecting until the last answer is read.
    ///
    /// # Errors
    ///
    /// A [`NutsAuthError`] for every flow that ends without a `DPoP`-bound
    /// token: an endpoint that cannot be reached or does not answer in time,
    /// an answer of another status or shape, metadata that does not support
    /// the grant, credentials that do not answer the definition or have
    /// expired, a presentation or a proof that cannot be signed, an RFC 6749
    /// §5.2 refusal, and a token that is not `DPoP`-bound.
    pub async fn request_access_token(
        &self,
        grant: &Grant,
        holder: &Holder,
        prover: &dyn DpopProver,
        timeout: Duration,
    ) -> Result<AccessToken, NutsAuthError> {
        let deadline = Instant::now().checked_add(timeout);
        let algorithm = holder.key().algorithm_name();
        let raw: metadata::Raw = self
            .get(metadata::url(&grant.issuer_url), Step::Metadata, deadline)
            .await?;
        let metadata = metadata::read(
            &raw,
            (&grant.issuer, &grant.issuer_url),
            algorithm,
            prover.algorithm(),
        )?;
        let mut definition_url = metadata.definition_endpoint.clone();
        definition_url
            .query_pairs_mut()
            .append_pair("scope", &grant.scope);
        let definition: PresentationDefinition =
            self.get(definition_url, Step::Definition, deadline).await?;
        let submission =
            presentation::submission(&definition, holder, uuid::Uuid::new_v4().to_string())?;
        let submission = serde_json::to_string(&submission).map_err(NutsAuthError::Submission)?;
        let now = jiff::Timestamp::now().as_second();
        if let Some(expired) = holder
            .credentials()
            .iter()
            .find(|credential| credential.expires().is_some_and(|exp| exp <= now))
        {
            return Err(NutsAuthError::Expired {
                descriptor: expired.descriptor().to_owned(),
            });
        }
        let mut resend = true;
        loop {
            let assertion = sign(holder, grant)?;
            let body = token_form(grant, &assertion, &submission);
            let proof = prover.proof(&Method::POST, &metadata.token_endpoint)?;
            let request = self
                .http
                .post(metadata.token_endpoint.clone())
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ACCEPT, JSON)
                .header(DPOP_HEADER, proof)
                .body(body);
            let answer = send(request, Step::Token, deadline).await?;
            if let Some(nonce) = &answer.nonce {
                prover.nonce(&metadata.token_endpoint, nonce);
            }
            if resend && answer.nonce.is_some() && demands_nonce(&answer) {
                resend = false;
                continue;
            }
            return token(&answer);
        }
    }

    /// Reads the JSON answer of a `GET` of `url` in `step`.
    async fn get<T: DeserializeOwned>(
        &self,
        url: Url,
        step: Step,
        deadline: Option<Instant>,
    ) -> Result<T, NutsAuthError> {
        let answer = send(self.http.get(url).header(ACCEPT, JSON), step, deadline).await?;
        if answer.status != StatusCode::OK {
            return Err(NutsAuthError::Status {
                step,
                status: answer.status,
            });
        }
        json(&answer, step)
    }
}

/// Sends `request` within what is left before `deadline`, and reads its
/// answer to at most [`RESPONSE_LIMIT`] bytes.
async fn send(
    request: reqwest::RequestBuilder,
    step: Step,
    deadline: Option<Instant>,
) -> Result<Answer, NutsAuthError> {
    let remaining = deadline
        .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
        .filter(|remaining| !remaining.is_zero())
        .ok_or(NutsAuthError::Timeout { step })?;
    let mut response = request
        .timeout(remaining)
        .send()
        .await
        .map_err(|error| NutsAuthError::transport(step, error))?;
    let status = response.status();
    let headers = response.headers();
    // NOTE: RFC 9449 §8, a nonce is visible ASCII; one that is not is
    // legitimately unusable, and the answer is read as if it carried none.
    let nonce = headers
        .get(DPOP_NONCE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let json = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case(JSON));
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| NutsAuthError::transport(step, error))?
    {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(NutsAuthError::Malformed {
                step,
                malformation: Malformation::TooLarge {
                    limit: RESPONSE_LIMIT,
                },
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Answer {
        status,
        nonce,
        json,
        body,
    })
}

/// The JSON body of `answer`, read as `T`.
fn json<T: DeserializeOwned>(answer: &Answer, step: Step) -> Result<T, NutsAuthError> {
    let malformed = |malformation| NutsAuthError::Malformed { step, malformation };
    if !answer.json {
        return Err(malformed(Malformation::MediaType));
    }
    serde_json::from_slice::<strict::Unique>(&answer.body).map_err(|error| {
        malformed(if error.is_data() {
            Malformation::RepeatedName {
                line: error.line(),
                column: error.column(),
            }
        } else {
            Malformation::of(&error)
        })
    })?;
    serde_json::from_slice(&answer.body).map_err(|error| malformed(Malformation::of(&error)))
}

/// The Verifiable Presentation of every credential of `holder`, for the
/// authorization server of `grant`, signed with the holder's key.
fn sign(holder: &Holder, grant: &Grant) -> Result<SecretString, NutsAuthError> {
    let nbf = jiff::Timestamp::now().as_second();
    let lifetime = i64::try_from(PRESENTATION_LIFETIME.as_secs()).unwrap_or(i64::MAX);
    let did = holder.did().as_str();
    let claims = PresentationClaims {
        iss: did,
        sub: did,
        aud: &grant.issuer,
        nbf,
        exp: nbf.saturating_add(lifetime),
        jti: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
        nonce: uuid::Uuid::new_v4().simple().to_string(),
        vp: Presentation {
            context: [CREDENTIALS_CONTEXT],
            types: [PRESENTATION_TYPE],
            credentials: holder
                .credentials()
                .iter()
                .map(|credential| credential.jwt().expose_secret())
                .collect(),
        },
    };
    let mut header = Header::new(holder.key().algorithm());
    header.kid = Some(holder.key().kid().to_owned());
    jsonwebtoken::encode(&header, &claims, holder.key().private())
        .map(SecretString::from)
        .map_err(NutsAuthError::Sign)
}

/// The body of the token request (Nuts RFC021 §3).
fn token_form(grant: &Grant, assertion: &SecretString, submission: &str) -> String {
    let mut form = form_urlencoded::Serializer::new(String::new());
    form.append_pair("grant_type", GRANT_TYPE)
        .append_pair("assertion", assertion.expose_secret())
        .append_pair("presentation_submission", submission)
        .append_pair("scope", &grant.scope);
    if let Some(client_id) = &grant.client_id {
        form.append_pair("client_id", client_id);
    }
    form.finish()
}

/// Whether `answer` is the token endpoint's demand for a nonce: a `400`
/// whose RFC 6749 §5.2 error is [`USE_DPOP_NONCE`] (RFC 9449 §8).
fn demands_nonce(answer: &Answer) -> bool {
    answer.status == StatusCode::BAD_REQUEST
        && serde_json::from_slice::<ErrorResponse>(&answer.body)
            .is_ok_and(|refused| refused.error == USE_DPOP_NONCE)
}

/// The token `answer` carries, or the refusal it states.
fn token(answer: &Answer) -> Result<AccessToken, NutsAuthError> {
    let status = answer.status;
    if status == StatusCode::OK {
        let token: TokenResponse = json(answer, Step::Token)?;
        // NOTE: RFC 6749 §5.1 makes token_type case-insensitive; RFC 9449 §5 and
        // the IG's GFI-004 make the token of a proven request DPoP-bound.
        if !token.token_type.eq_ignore_ascii_case(DPOP_TOKEN_TYPE) {
            return Err(NutsAuthError::TokenType {
                token_type: bounded(&token.token_type),
            });
        }
        return Ok(AccessToken {
            token: SecretString::from(token.access_token),
            expires_in: token.expires_in.map(Duration::from_secs),
            scope: token.scope,
        });
    }
    // NOTE: RFC 6749 §5.2 answers 400, or 401 for a failed client authentication,
    // with a JSON error; any other body is answered as its bare status.
    if (status == StatusCode::BAD_REQUEST || status == StatusCode::UNAUTHORIZED)
        && let Ok(refused) = serde_json::from_slice::<ErrorResponse>(&answer.body)
    {
        return Err(NutsAuthError::Refused {
            status,
            error: bounded(&refused.error),
            description: refused.error_description.as_deref().map(bounded),
        });
    }
    Err(NutsAuthError::Status {
        step: Step::Token,
        status,
    })
}
