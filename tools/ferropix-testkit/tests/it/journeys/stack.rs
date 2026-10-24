// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The stack a journey runs against, each part on loopback: the test OpenID
//! Provider of `ferropix_testkit::oidc`, which signs a real browser in, a
//! stand-in for the FerroPIX server, and the console itself, run in process
//! over its release site bundle so the browser hydrates the pages its server
//! drew.
//!
//! The stand-in answers the liveness route the console probes and refuses
//! every other route with `404`, which is the server as the console meets it
//! until the operator API exists (#90): every screen draws its refused state.
//! No specification governs the stack: our own design, after FerroBRIDGE's.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ferropix_testkit::oidc::Provider;
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::journeys::browser::workspace;

/// The environment variable naming the site bundle's directory, when it is
/// not the workspace's `target/site`.
const SITE_ENV: &str = "FERROPIX_JOURNEYS_SITE";

/// The console's client id at the provider.
const CLIENT_ID: &str = "ferropix-console";

/// The synthetic access token the provider grants the steward, which never
/// reaches the browser.
const ACCESS_TOKEN: &str = "synthetic-journey-access-token";

/// The name the provider's ID Token gives the steward.
pub(crate) const STEWARD: &str = "Synthetic Operator";

/// The prefix of every preference cookie the console sets over plain HTTP
/// (`.claude/rules/leptos-ui.md` §4).
pub(crate) const PREFERENCE_PREFIX: &str = "ferropix_console_";

/// The running stack.
#[derive(Debug)]
pub(crate) struct Stack {
    /// The provider, kept so it outlives the console.
    _provider: Provider,
    /// The stand-in for the FerroPIX server, kept so it outlives the console.
    _server: MockServer,
    /// The console's base URL.
    console: String,
}

impl Stack {
    /// Starts the provider, the stand-in and the console.
    pub(crate) async fn start() -> Self {
        let site = site_root();
        let provider = Provider::start()
            .await
            .unwrap_or_else(|error| panic!("the test provider did not start: {error}"));
        provider.approve(CLIENT_ID, ACCESS_TOKEN).await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"pass\"}"))
            .mount(&server)
            .await;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port for the console");
        let address = listener.local_addr().expect("the console's address");
        let console = format!("http://{address}");
        let text = format!(
            "[server]\nlisten = \"{address}\"\nsite_root = {site}\n\n\
             [pix]\nbase_url = \"{server}/\"\ntimeout_ms = 2000\n\n\
             [session]\nsecure_cookie = false\n\n\
             [oidc]\nissuer = \"{issuer}\"\nauthorization_endpoint = \"{authorization}\"\n\
             token_endpoint = \"{token}\"\njwks_uri = \"{jwks}\"\nclient_id = \"{CLIENT_ID}\"\n\
             redirect_uri = \"{console}/auth/callback\"\n\
             end_session_endpoint = \"{end_session}\"\npost_logout_redirect_uri = \"{console}/sign-in/signed-out\"\n",
            site = toml_string(&site.display().to_string()),
            server = server.uri(),
            issuer = provider.issuer(),
            authorization = provider.authorization_endpoint(),
            token = provider.token_endpoint(),
            jwks = provider.jwks_uri(),
            end_session = provider.end_session_endpoint(),
        );
        let settings =
            ferropix_console::config::Config::from_sources(Some(&text), &BTreeMap::new())
                .expect("the journey configuration reads")
                .resolve()
                .expect("the journey configuration resolves");
        let state = ferropix_console::server::ConsoleState::new(settings)
            .expect("the console's state builds");
        tokio::spawn(ferropix_console::server::serve(
            listener,
            ferropix_console::server::router(state),
            std::future::pending(),
        ));
        Self {
            _provider: provider,
            _server: server,
            console,
        }
    }

    /// The URL of `path` on the console.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.console)
    }
}

/// `text` as a TOML basic string.
fn toml_string(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// The site bundle's directory: [`SITE_ENV`], or the workspace's
/// `target/site`, which must hold the WebAssembly cargo-leptos wrote.
fn site_root() -> PathBuf {
    let site =
        std::env::var_os(SITE_ENV).map_or_else(|| workspace().join("target/site"), PathBuf::from);
    let bundle = site.join("pkg/ferropix-console.wasm");
    assert!(
        bundle.is_file(),
        "{} holds no site bundle: build it with scripts/release/console-site.sh --release, or name its directory in {SITE_ENV}",
        site.display()
    );
    site
}
