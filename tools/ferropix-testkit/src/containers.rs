// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The container harness behind the `FERROPIX_E2E` gate.
//!
//! A container-backed test asks [`e2e_enabled`] first and returns without
//! touching Docker when the gate is unset, so the ordinary suite stays
//! offline. Every image the harness starts is a [`PinnedImage`] in
//! [`IMAGES`], pinned by tag and by the digest of its image index, and
//! repeated as a row of the container image table of `docs/VERSIONS.md`,
//! which the pin-freshness lane reads against its registry.
//!
//! No specification governs the harness: our own design, after FerroFED's
//! and FerroBRIDGE's.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use testcontainers::core::{Healthcheck, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ContainerRequest, GenericImage, ImageExt};

/// The environment variable that admits the container-backed tests.
pub const E2E_GATE: &str = "FERROPIX_E2E";

/// The value of [`E2E_GATE`] that admits them.
pub const E2E_GATE_VALUE: &str = "1";

/// The PostgreSQL port inside its container.
const POSTGRES_PORT: u16 = 5432;

/// The HTTP port the stub provider listens on inside its container, the
/// image's default `SERVER_PORT`.
const PROVIDER_PORT: u16 = 8080;

/// The superuser and its password in the PostgreSQL container. It is not
/// `ferropix`, the role the store's grants name, so a suite can create that
/// role as the server's and connect as it.
const POSTGRES_ROLE: &str = "pix_super";

/// The database in the PostgreSQL container.
const POSTGRES_DB: &str = "ferropix";

/// The issuer id the stub provider is configured under: the first path
/// segment of every URL it serves.
pub const STUB_ISSUER_ID: &str = "ferropix";

/// How long the readiness poll waits before it gives up.
const READINESS_BUDGET: Duration = Duration::from_secs(120);

/// How long the readiness poll sleeps between two probes.
const READINESS_INTERVAL: Duration = Duration::from_millis(250);

/// How often a container's own health check runs, and its per-run budget.
const HEALTH_INTERVAL: Duration = Duration::from_secs(1);

/// How many consecutive health-check failures make a container unhealthy.
const HEALTH_RETRIES: u32 = 30;

/// Returns whether the container-backed tests may run.
///
/// They run when `FERROPIX_E2E` is exactly `1`. A test that returns early on
/// a `false` here is reported by `cargo nextest` as passed, the deliberate
/// trade: the gate keeps the ordinary suite offline, and the end-to-end run
/// is where these tests do their work.
#[must_use]
pub fn e2e_enabled() -> bool {
    std::env::var(E2E_GATE).is_ok_and(|value| value == E2E_GATE_VALUE)
}

/// One container image, pinned by tag and by digest.
///
/// The digest is what Docker resolves; the tag travels beside it so a reader
/// sees which release the digest is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedImage {
    /// The row of `docs/VERSIONS.md` that repeats this pin.
    pub item: &'static str,
    /// The repository, registry included for anything but Docker Hub.
    pub repository: &'static str,
    /// The tag the digest was published under.
    pub tag: &'static str,
    /// The `sha256:` digest of the image index.
    pub digest: &'static str,
}

impl PinnedImage {
    /// Returns the reference Docker resolves this image by.
    ///
    /// # Examples
    ///
    /// ```
    /// let reference = ferropix_testkit::containers::POSTGRES.reference();
    /// assert!(reference.starts_with("postgres:18."));
    /// assert!(reference.contains("@sha256:"));
    /// ```
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{}:{}@{}", self.repository, self.tag, self.digest)
    }

    /// Returns the image with the digest in the tag position, which is how
    /// `testcontainers` spells a reference (`name:tag`).
    fn image(&self) -> GenericImage {
        GenericImage::new(
            self.repository.to_owned(),
            format!("{}@{}", self.tag, self.digest),
        )
    }
}

/// PostgreSQL 18, the one store of FerroPIX (`docs/architecture.md` §4),
/// as the official image publishes it.
pub const POSTGRES: PinnedImage = PinnedImage {
    item: "PostgreSQL test image",
    repository: "postgres",
    tag: "18.6",
    digest: "sha256:edb0c08dc33befd0262ab3b5cf1c4f3f8cfd8b09e88c05b45adee29d6761ddb6",
};

/// The stub OpenID Provider: NAV's `mock-oauth2-server` (MIT), which serves
/// the discovery document, the key set and a token endpoint that signs a
/// token for any client.
pub const STUB_PROVIDER: PinnedImage = PinnedImage {
    item: "Stub OpenID Provider image",
    repository: "ghcr.io/navikt/mock-oauth2-server",
    tag: "6.1.3",
    digest: "sha256:2a813381bf282da46ff36ff386059e7903e25865056af8149f4fb3f1441916b8",
};

/// Every image the harness starts.
pub const IMAGES: [PinnedImage; 2] = [POSTGRES, STUB_PROVIDER];

/// A container could not be started, or did not become usable.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HarnessError {
    /// Docker refused to start or to inspect a container.
    #[error("the {image} container could not be started")]
    Container {
        /// The image that was being started.
        image: &'static str,
        /// What Docker reported.
        #[source]
        source: testcontainers::TestcontainersError,
    },
    /// An HTTP request to a started container could not be sent or read.
    #[error("the request to {url} failed")]
    Http {
        /// The endpoint that was called.
        url: String,
        /// What the HTTP stack reported.
        #[source]
        source: reqwest::Error,
    },
    /// A started container answered with a status other than success.
    #[error("{url} answered {status}")]
    Status {
        /// The endpoint that was called.
        url: String,
        /// The status it answered with.
        status: reqwest::StatusCode,
    },
    /// The token response of the stub provider could not be read.
    #[error("the token response of {url} could not be read")]
    TokenResponse {
        /// The token endpoint.
        url: String,
        /// What the JSON reader reported.
        #[source]
        source: serde_json::Error,
    },
    /// The stub provider's configuration could not be written.
    #[error("the stub provider's configuration could not be written")]
    Config(#[source] serde_json::Error),
    /// The service did not answer its readiness probe inside the budget.
    #[error("{url} did not become ready within {}s", budget.as_secs())]
    NotReady {
        /// The endpoint that was probed.
        url: String,
        /// How long the probe waited.
        budget: Duration,
    },
}

/// A started PostgreSQL, torn down when it is dropped.
#[derive(Debug)]
pub struct Postgres {
    /// The container, owned so that its lifetime is this value's.
    container: ContainerAsync<GenericImage>,
    /// The connection URL of the login role.
    url: String,
}

impl Postgres {
    /// Returns the connection URL of the login role, which owns its
    /// database.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the container, for a caller that needs to inspect it.
    #[must_use]
    pub fn container(&self) -> &ContainerAsync<GenericImage> {
        &self.container
    }
}

/// Starts PostgreSQL 18 and returns it with the URL of its login role.
///
/// The role and its password are `pix_super`, the database `ferropix`, and
/// the role is the database's superuser, so a suite can create the restricted
/// roles the store's boot gate expects and the server's role the grants name. The server offers no TLS, so the URL says
/// `sslmode=disable`.
///
/// # Errors
///
/// Returns [`HarnessError::Container`] when Docker refuses the container or
/// the mapped port cannot be read.
pub async fn postgres() -> Result<Postgres, HarnessError> {
    let container = postgres_request()
        .start()
        .await
        .map_err(|source| container_error(POSTGRES, source))?;
    let host = container
        .get_host()
        .await
        .map_err(|source| container_error(POSTGRES, source))?;
    let port = container
        .get_host_port_ipv4(POSTGRES_PORT.tcp())
        .await
        .map_err(|source| container_error(POSTGRES, source))?;
    let url = format!(
        "postgres://{POSTGRES_ROLE}:{POSTGRES_ROLE}@{host}:{port}/{POSTGRES_DB}?sslmode=disable"
    );
    Ok(Postgres { container, url })
}

/// Returns the request that starts PostgreSQL.
fn postgres_request() -> ContainerRequest<GenericImage> {
    POSTGRES
        .image()
        .with_wait_for(WaitFor::healthcheck())
        .with_exposed_port(POSTGRES_PORT.tcp())
        .with_health_check(postgres_health_check())
        .with_env_var("POSTGRES_USER", POSTGRES_ROLE)
        .with_env_var("POSTGRES_PASSWORD", POSTGRES_ROLE)
        .with_env_var("POSTGRES_DB", POSTGRES_DB)
}

/// Returns the health check that answers when PostgreSQL accepts TCP
/// connections.
///
/// `pg_isready` is PostgreSQL's own readiness utility
/// (<https://www.postgresql.org/docs/18/app-pg-isready.html>), and the host
/// is named because the entrypoint's initialisation server listens on the
/// Unix socket alone; a socket probe would answer before the port does.
fn postgres_health_check() -> Healthcheck {
    Healthcheck::cmd([
        "pg_isready",
        "-h",
        "127.0.0.1",
        "-U",
        POSTGRES_ROLE,
        "-d",
        POSTGRES_DB,
    ])
    .with_interval(HEALTH_INTERVAL)
    .with_timeout(HEALTH_INTERVAL)
    .with_retries(HEALTH_RETRIES)
}

/// A started stub OpenID Provider, torn down when it is dropped.
#[derive(Debug)]
pub struct StubProvider {
    /// The container, owned so that its lifetime is this value's.
    container: ContainerAsync<GenericImage>,
    /// The origin the provider is reachable at, with no path.
    origin: String,
    /// The HTTP client the token requests go through.
    client: reqwest::Client,
}

impl StubProvider {
    /// Returns the issuer identifier, the `iss` of every token it signs.
    #[must_use]
    pub fn issuer(&self) -> String {
        format!("{}/{STUB_ISSUER_ID}", self.origin)
    }

    /// Returns the URL of the issuer's OpenID Provider metadata.
    #[must_use]
    pub fn discovery_url(&self) -> String {
        format!("{}/.well-known/openid-configuration", self.issuer())
    }

    /// Returns the URL of the issuer's token endpoint.
    #[must_use]
    pub fn token_url(&self) -> String {
        format!("{}/token", self.issuer())
    }

    /// Returns the container, for a caller that needs to inspect it.
    #[must_use]
    pub fn container(&self) -> &ContainerAsync<GenericImage> {
        &self.container
    }

    /// Asks the provider for an access token by the client credentials
    /// grant (RFC 6749 §4.4) and returns the signed JWT.
    ///
    /// The token carries `client_id` as its `sub` and `client_id` claims,
    /// the audience the provider was started with as `aud`, and `scope` as
    /// its `scope` claim, so a suite chooses exactly what the resource
    /// server sees.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::Http`] when the request fails,
    /// [`HarnessError::Status`] when the provider refuses it, and
    /// [`HarnessError::TokenResponse`] when the answer is not a token
    /// response.
    pub async fn client_credentials(
        &self,
        client_id: &str,
        scope: &str,
    ) -> Result<String, HarnessError> {
        let url = self.token_url();
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", "synthetic"),
            ("scope", scope),
        ];
        let response = self
            .client
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|source| HarnessError::Http {
                url: url.clone(),
                source,
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(HarnessError::Status { url, status });
        }
        let body = response.text().await.map_err(|source| HarnessError::Http {
            url: url.clone(),
            source,
        })?;
        let token: TokenResponse = serde_json::from_str(&body)
            .map_err(|source| HarnessError::TokenResponse { url, source })?;
        Ok(token.access_token)
    }
}

/// The member of a token response the harness reads (RFC 6749 §5.1).
#[derive(Deserialize)]
struct TokenResponse {
    /// The access token.
    access_token: String,
}

/// The stub provider's `JSON_CONFIG`: no login page, and one token callback
/// for the client credentials grant.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderConfig {
    /// Whether the authorize endpoint shows a login page.
    interactive_login: bool,
    /// The token callbacks, one per issuer.
    token_callbacks: Vec<TokenCallback>,
}

/// The claims an issuer signs for a matching token request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenCallback {
    /// The issuer the callback serves.
    issuer_id: &'static str,
    /// The request parameter matches and the claims each one signs.
    request_mappings: Vec<RequestMapping>,
}

/// One request parameter match and the claims it signs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestMapping {
    /// The form parameter that is matched.
    request_param: &'static str,
    /// The value it must carry.
    #[serde(rename = "match")]
    matches: &'static str,
    /// The claims of the signed token.
    claims: Claims,
}

/// The claims of a client credentials token, with the provider's template
/// variables filled from the token request.
#[derive(Debug, Serialize)]
struct Claims {
    /// The subject, the requesting client.
    sub: &'static str,
    /// The requesting client.
    client_id: &'static str,
    /// The audience the provider was started with.
    aud: Vec<String>,
    /// The scope the token request asked for.
    scope: &'static str,
}

/// Returns the `JSON_CONFIG` the stub provider starts with.
fn provider_config(audience: &str) -> Result<String, HarnessError> {
    let config = ProviderConfig {
        interactive_login: false,
        token_callbacks: vec![TokenCallback {
            issuer_id: STUB_ISSUER_ID,
            request_mappings: vec![RequestMapping {
                request_param: "grant_type",
                matches: "client_credentials",
                claims: Claims {
                    sub: "${clientId}",
                    client_id: "${clientId}",
                    aud: vec![audience.to_owned()],
                    scope: "${scope}",
                },
            }],
        }],
    };
    serde_json::to_string(&config).map_err(HarnessError::Config)
}

/// Returns the request that starts the stub provider for `audience`.
fn provider_request(audience: &str) -> Result<ContainerRequest<GenericImage>, HarnessError> {
    Ok(STUB_PROVIDER
        .image()
        .with_exposed_port(PROVIDER_PORT.tcp())
        .with_env_var("JSON_CONFIG", provider_config(audience)?))
}

/// Starts the stub OpenID Provider, whose tokens name `audience`, and waits
/// for its readiness endpoint.
///
/// The issuer is [`StubProvider::issuer`], under the issuer id
/// [`STUB_ISSUER_ID`]; the provider generates its signing key when it
/// starts, so no key is ever committed.
///
/// # Errors
///
/// Returns [`HarnessError::Config`] when the configuration cannot be
/// written, [`HarnessError::Container`] when Docker refuses the container,
/// [`HarnessError::Http`] when the readiness probe cannot be built, and
/// [`HarnessError::NotReady`] when the provider does not answer it in time.
pub async fn stub_provider(audience: &str) -> Result<StubProvider, HarnessError> {
    let container = provider_request(audience)?
        .start()
        .await
        .map_err(|source| container_error(STUB_PROVIDER, source))?;
    let host = container
        .get_host()
        .await
        .map_err(|source| container_error(STUB_PROVIDER, source))?;
    let port = container
        .get_host_port_ipv4(PROVIDER_PORT.tcp())
        .await
        .map_err(|source| container_error(STUB_PROVIDER, source))?;
    let origin = format!("http://{host}:{port}");
    let client = http_client(&origin)?;
    await_readiness(&client, &format!("{origin}/isalive")).await?;
    Ok(StubProvider {
        container,
        origin,
        client,
    })
}

/// Returns the HTTP client the harness talks to a container with.
fn http_client(origin: &str) -> Result<reqwest::Client, HarnessError> {
    reqwest::Client::builder()
        .timeout(READINESS_INTERVAL.saturating_mul(40))
        .build()
        .map_err(|source| HarnessError::Http {
            url: origin.to_owned(),
            source,
        })
}

/// Polls `url` until it answers `200`, or the budget runs out.
async fn await_readiness(client: &reqwest::Client, url: &str) -> Result<(), HarnessError> {
    let deadline = std::time::Instant::now().checked_add(READINESS_BUDGET);
    loop {
        // NOTE: no specification governs this: our own design. A refused
        // probe is the expected answer while the container starts.
        if let Ok(response) = client.get(url).send().await
            && response.status() == reqwest::StatusCode::OK
        {
            return Ok(());
        }
        if deadline.is_none_or(|deadline| std::time::Instant::now() >= deadline) {
            return Err(HarnessError::NotReady {
                url: url.to_owned(),
                budget: READINESS_BUDGET,
            });
        }
        tokio::time::sleep(READINESS_INTERVAL).await;
    }
}

/// Wraps a `testcontainers` failure with the image it concerns.
fn container_error(
    image: PinnedImage,
    source: testcontainers::TestcontainersError,
) -> HarnessError {
    HarnessError::Container {
        image: image.repository,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::{IMAGES, PinnedImage, postgres_request, provider_config, provider_request};
    use testcontainers::Image;

    #[test]
    fn every_image_names_a_tag_and_a_sha256_digest() {
        for image in IMAGES {
            assert!(!image.tag.is_empty(), "{} has no tag", image.repository);
            let hex = image.digest.strip_prefix("sha256:").unwrap_or_default();
            assert!(
                hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
                "{} is not pinned by a sha256 digest",
                image.repository
            );
        }
    }

    #[test]
    fn a_reference_is_the_repository_tag_and_digest() {
        let image = PinnedImage {
            item: "x",
            repository: "postgres",
            tag: "18.6",
            digest: "sha256:0000",
        };
        assert_eq!("postgres:18.6@sha256:0000", image.reference());
    }

    #[test]
    fn the_postgres_request_names_the_pinned_digest() {
        let request = postgres_request();
        assert_eq!(
            "18.6@sha256:edb0c08dc33befd0262ab3b5cf1c4f3f8cfd8b09e88c05b45adee29d6761ddb6",
            request.image().tag(),
            "the request must resolve the image by digest"
        );
    }

    #[test]
    fn the_provider_signs_the_requested_scope_for_the_audience() {
        let config = provider_config("ferropix-test").unwrap();
        assert!(config.contains(r#""interactiveLogin":false"#), "{config}");
        assert!(config.contains(r#""issuerId":"ferropix""#), "{config}");
        assert!(config.contains(r#""aud":["ferropix-test"]"#), "{config}");
        assert!(config.contains(r#""scope":"${scope}""#), "{config}");
    }

    #[test]
    fn the_provider_request_carries_its_configuration() {
        let request = provider_request("ferropix-test").unwrap();
        let names: Vec<String> = request
            .env_vars()
            .map(|(name, _)| name.into_owned())
            .collect();
        assert!(
            names.iter().any(|name| name == "JSON_CONFIG"),
            "the provider starts with its configuration, got {names:?}"
        );
    }
}
