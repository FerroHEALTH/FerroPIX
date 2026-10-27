// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The stack a journey runs against, each part on loopback: the test OpenID
//! Provider of `ferropix_testkit::oidc`, which signs a real browser in, a
//! stand-in for the FerroPIX server, and the console itself, run in process
//! over its release site bundle so the browser hydrates the pages its server
//! drew.
//!
//! The stand-in answers the liveness route the console probes. Started with
//! [`Server::Refusing`] it refuses every other route with `404`, so every
//! screen draws its refused state; with [`Server::Filled`] it answers the
//! operator API routes the screens read with the synthetic documents under
//! `fixtures/operator/`, so every screen draws its filled state. No
//! specification governs the stack: our own design, after FerroBRIDGE's.

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

/// The directory of the synthetic operator API documents.
const DOCUMENTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/operator");

/// A person the filled stand-in serves, synthetic.
pub(crate) const FILLED_PERSON: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

/// The routes the filled stand-in answers: the method, the path and the
/// document under [`DOCUMENTS`] it answers with.
pub(crate) const FILLED: [(&str, &str, &str); 14] = [
    ("GET", "/operator/overview", "overview"),
    ("GET", "/operator/dependencies", "dependencies"),
    (
        "GET",
        "/operator/persons/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
        "person",
    ),
    ("GET", "/operator/sources", "sources"),
    ("GET", "/operator/sources/EAST.ADT", "source-page"),
    ("GET", "/operator/domains", "domains"),
    (
        "GET",
        "/operator/domains/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c",
        "domain-page",
    ),
    ("GET", "/operator/pseudonym-domains", "pseudonym-domains"),
    ("POST", "/operator/pseudonyms/resolve", "resolution"),
    ("GET", "/operator/configuration", "configuration"),
    ("GET", "/operator/actions", "actions"),
    (
        "POST",
        "/operator/persons/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/reveal",
        "revealed",
    ),
    ("POST", "/operator/search/identifier", "results"),
    ("POST", "/operator/search/demographics", "results"),
];

/// The text of the synthetic document `name`.
pub(crate) fn document(name: &str) -> String {
    let file = format!("{DOCUMENTS}/{name}.json");
    std::fs::read_to_string(&file).unwrap_or_else(|error| panic!("reading {file}: {error}"))
}

/// What the stand-in for the FerroPIX server answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Server {
    /// Liveness alone: every operator API route is refused with `404`.
    Refusing,
    /// The operator API routes of [`FILLED`] with their documents.
    Filled,
}

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
    /// Starts the provider, a refusing stand-in and the console.
    pub(crate) async fn start() -> Self {
        Self::start_with(Server::Refusing).await
    }

    /// Starts the provider, a stand-in answering as `answers` says and the
    /// console.
    pub(crate) async fn start_with(answers: Server) -> Self {
        Self::start_session(answers, "").await
    }

    /// Starts the provider, a refusing stand-in and a console whose
    /// sessions idle out after `seconds`.
    pub(crate) async fn start_idle(seconds: u64) -> Self {
        Self::start_session(Server::Refusing, &format!("idle_timeout_s = {seconds}\n")).await
    }

    /// Starts the provider, a stand-in answering as `answers` says and the
    /// console, with the TOML lines `session` added to its `[session]`.
    async fn start_session(answers: Server, session: &str) -> Self {
        let site = site_root();
        let provider = Provider::start()
            .await
            .unwrap_or_else(|error| panic!("the test provider did not start: {error}"));
        provider.approve(CLIENT_ID, ACCESS_TOKEN).await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health/liveness"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        if answers == Server::Filled {
            for (verb, route, name) in FILLED {
                Mock::given(method(verb))
                    .and(path(route))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .insert_header("content-type", "application/json")
                            .set_body_string(document(name)),
                    )
                    .mount(&server)
                    .await;
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port for the console");
        let address = listener.local_addr().expect("the console's address");
        let console = format!("http://{address}");
        let text = format!(
            "[server]\nlisten = \"{address}\"\nsite_root = {site}\n\n\
             [pix]\nbase_url = \"{server}/\"\ntimeout_ms = 2000\n\n\
             [session]\nsecure_cookie = false\n{session}\n\
             [oidc]\nissuer = \"{issuer}\"\nauthorization_endpoint = \"{authorization}\"\n\
             token_endpoint = \"{token}\"\njwks_uri = \"{jwks}\"\nclient_id = \"{CLIENT_ID}\"\n\
             redirect_uri = \"{console}/auth/callback\"\nscopes = [\"openid\", \"__ferropix.admin\"]\n\
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
        let lifecycle = state.lifecycle().clone();
        tokio::spawn(ferropix_console::server::serve(
            listener,
            ferropix_console::server::router(state),
            lifecycle,
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

#[cfg(test)]
mod tests {
    use ferropix_operator_api::actions::Actions;
    use ferropix_operator_api::configuration::Configuration;
    use ferropix_operator_api::domain::{DomainPage, Domains};
    use ferropix_operator_api::health::Dependencies;
    use ferropix_operator_api::overview::Overview;
    use ferropix_operator_api::person::Person;
    use ferropix_operator_api::pseudonym::{PseudonymDomains, Resolution};
    use ferropix_operator_api::reveal::Revealed;
    use ferropix_operator_api::search::Results;
    use ferropix_operator_api::source::{SourcePage, Sources};
    use serde::de::DeserializeOwned;

    use super::{FILLED, document};

    /// Whether the document `name` reads as a `T`.
    fn reads<T: DeserializeOwned>(name: &str) -> bool {
        serde_json::from_str::<T>(&document(name)).is_ok()
    }

    #[test]
    fn every_filled_document_reads_as_the_operator_api_type_its_route_answers() {
        for (_, route, name) in FILLED {
            let read = match name {
                "overview" => reads::<Overview>(name),
                "dependencies" => reads::<Dependencies>(name),
                "person" => reads::<Person>(name),
                "sources" => reads::<Sources>(name),
                "source-page" => reads::<SourcePage>(name),
                "domains" => reads::<Domains>(name),
                "domain-page" => reads::<DomainPage>(name),
                "configuration" => reads::<Configuration>(name),
                "actions" => reads::<Actions>(name),
                "results" => reads::<Results>(name),
                "revealed" => reads::<Revealed>(name),
                "pseudonym-domains" => reads::<PseudonymDomains>(name),
                "resolution" => reads::<Resolution>(name),
                other => panic!("{other} names no operator API type"),
            };
            assert!(read, "{name}.json does not read as the answer of {route}");
        }
    }
}
