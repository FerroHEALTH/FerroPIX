// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's client of the FerroPIX server, the one place the console
//! reaches it.
//!
//! The console calls the server server to server, with the signed-in
//! steward's own access token, so the server records the steward as the
//! actor of every call (`docs/architecture.md` §12.1). Each call carries an
//! `X-Request-Id` the console mints, which a page shows beside a failure so
//! a steward can quote it. Two routes are read today: the server's liveness
//! route, `GET {base}/health` (`docs/architecture.md` §9), and its FHIR
//! `CapabilityStatement`, `GET {base}/fhir/metadata` (HL7 FHIR R4, the REST
//! interaction `capabilities`). The operator API the other pages read is
//! #90, and until the server serves it every such page draws its refusal
//! from the [`Probe`] of the liveness route. No specification governs the client:
//! our own design.

use std::fmt;
use std::time::Duration;

use http::StatusCode;
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;

use crate::config::settings::PixSettings;

/// The header that carries the request id the console mints for a call.
pub const REQUEST_ID: &str = "x-request-id";

/// The liveness route of the server, under its base.
pub const HEALTH: &str = "health";

/// The FHIR capability route of the server, under its base.
pub const METADATA: &str = "fhir/metadata";

/// How many bytes of an answer the console reads at most: a
/// `CapabilityStatement` is a few kilobytes, so a larger answer is refused
/// rather than drawn.
pub const BODY_LIMIT: usize = 1 << 20;

/// The steward's access token, which the console sends to the server and
/// never renders.
#[derive(Clone)]
pub struct AccessToken(SecretString);

impl AccessToken {
    /// Wraps the token a session holds.
    #[must_use]
    pub const fn new(token: SecretString) -> Self {
        Self(token)
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(..)")
    }
}

/// Why no answer came back from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The connection was refused or could not be made.
    Connect,
    /// No answer came within the configured timeout.
    Timeout,
    /// The answer could not be read, or the call failed another way.
    Other,
}

impl Failure {
    /// The failure `error` names.
    fn of(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connect
        } else {
            Self::Other
        }
    }
}

/// How the server answered a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// It answered `2xx`.
    Up,
    /// It answered with this other status.
    Refused(StatusCode),
    /// No answer came.
    Unreachable(Failure),
}

/// One call to the server and how it went: the route, the outcome, the
/// moment it was made and the request id it carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The route the call took, method and path, as a steward may repeat it.
    pub route: String,
    /// How the server answered.
    pub reach: Reach,
    /// When the call was made, as RFC 3339 text in UTC.
    pub at: String,
    /// The request id the call carried.
    pub request_id: String,
}

impl Probe {
    /// Whether the server answered `2xx`.
    #[must_use]
    pub fn is_up(&self) -> bool {
        self.reach == Reach::Up
    }
}

/// The `CapabilityStatement` the server declares, as it was sent, or the
/// call that did not bring one back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// The document, as the server sent it.
    Declared(String),
    /// The call that did not answer with one.
    Missing(Probe),
}

/// A client of the FerroPIX server could not be built.
#[derive(Debug, thiserror::Error)]
#[error("the client of the FerroPIX server could not be built")]
pub struct ClientError(#[from] reqwest::Error);

/// The client of one FerroPIX server.
#[derive(Debug, Clone)]
pub struct Pix {
    base: Url,
    http: reqwest::Client,
}

impl Pix {
    /// The client of the server `settings` names.
    ///
    /// # Errors
    /// Returns [`ClientError`] when the HTTP client cannot be built.
    pub fn new(settings: &PixSettings) -> Result<Self, ClientError> {
        // NOTE: no specification governs this: our own design; a call is never
        // redirected elsewhere, so the steward's token reaches only the base.
        let http = reqwest::Client::builder()
            .timeout(settings.timeout)
            .connect_timeout(settings.timeout.min(Duration::from_secs(5)))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(ClientError::from)?;
        Ok(Self {
            base: settings.base.clone(),
            http,
        })
    }

    /// The server's base URL.
    #[must_use]
    pub const fn base(&self) -> &Url {
        &self.base
    }

    /// Asks the server's liveness route whether it answers.
    pub async fn probe(&self, token: &AccessToken) -> Probe {
        self.get(HEALTH, token).await.0
    }

    /// Reads the server's `CapabilityStatement`.
    pub async fn capability_statement(&self, token: &AccessToken) -> Capability {
        match self.get(METADATA, token).await {
            (probe, Some(text)) if probe.is_up() => Capability::Declared(text),
            (probe, _) => Capability::Missing(probe),
        }
    }

    /// `GET {base}/{route}` with the steward's token: the call's [`Probe`]
    /// and, for a `2xx` answer within [`BODY_LIMIT`], its body as text.
    async fn get(&self, route: &'static str, token: &AccessToken) -> (Probe, Option<String>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let at = jiff::Timestamp::now()
            .round(jiff::Unit::Second)
            .map_or_else(|_| jiff::Timestamp::now().to_string(), |at| at.to_string());
        let probe = |reach| Probe {
            route: format!("GET /{route}"),
            reach,
            at: at.clone(),
            request_id: request_id.clone(),
        };
        let Ok(url) = self.base.join(route) else {
            return (probe(Reach::Unreachable(Failure::Other)), None);
        };
        let sent = self
            .http
            .get(url)
            .bearer_auth(token.0.expose_secret())
            .header(REQUEST_ID, &request_id)
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(route, %request_id, failure = ?Failure::of(&error), "the FerroPIX server did not answer");
                return (probe(Reach::Unreachable(Failure::of(&error))), None);
            }
        };
        let status = response.status();
        if !status.is_success() {
            tracing::warn!(route, %request_id, %status, "the FerroPIX server refused a call");
            return (probe(Reach::Refused(status)), None);
        }
        let too_long = response.content_length().is_some_and(|length| {
            usize::try_from(length).map_or(true, |length| length > BODY_LIMIT)
        });
        if too_long {
            return (probe(Reach::Unreachable(Failure::Other)), None);
        }
        match response.text().await {
            Ok(text) if text.len() <= BODY_LIMIT => (probe(Reach::Up), Some(text)),
            Ok(_) => (probe(Reach::Unreachable(Failure::Other)), None),
            Err(error) => (probe(Reach::Unreachable(Failure::of(&error))), None),
        }
    }
}
