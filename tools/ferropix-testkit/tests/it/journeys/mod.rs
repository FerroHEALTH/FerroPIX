// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator console's browser journeys, behind the `FERROPIX_JOURNEYS`
//! gate (#98).
//!
//! Each journey drives headless Chrome over `WebDriver`, through a
//! chromedriver of its own, against the console run in process over its
//! release site bundle, the test OpenID Provider and a stand-in for the
//! FerroPIX server (`stack`). The waits are FerroTERM's (`browser`): every
//! step waits for the page to hydrate and a press waits for the page it
//! loads, never a timer. A journey fails on anything severe the browser
//! logs.
//!
//! Without `FERROPIX_JOURNEYS=1` each journey returns at once, so the
//! ordinary suite needs no browser; the `journeys` job of
//! `.github/workflows/ci.yml` sets it, installs the Chrome for Testing
//! release `docs/VERSIONS.md` pins and builds the bundle. The guard that
//! every route of the console is walked runs in the ordinary suite. No
//! specification governs the journeys: our own design, after FerroBRIDGE's,
//! FerroFED's and FerroTERM's.

mod accessibility;
mod browser;
mod chromedriver;
mod in_place;
mod keyboard;
mod preferences;
mod pseudo_locale;
mod screenshots;
mod session;
mod sign_in;
mod stack;

/// The environment variable that opts in to the browser journeys.
const JOURNEYS_GATE: &str = "FERROPIX_JOURNEYS";

/// Whether the browser journeys run: only when [`JOURNEYS_GATE`] is `1`.
fn journeys_enabled() -> bool {
    std::env::var(JOURNEYS_GATE).is_ok_and(|value| value == "1")
}

/// The address the not-found page answers, with `404`: a business
/// identifier in an address, which the console refuses to read.
const NOT_FOUND: &str = "/patients/123456789";

/// A person id the console accepts in an address, synthetic.
const PERSON: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

/// Every screen a signed-in steward opens, one address per route of the
/// console's router (`app/ferropix-console/src/app.rs`), each drawing its
/// refused state while the server serves no operator API, and the
/// not-found page an identifier in an address draws.
const SCREENS: [&str; 20] = [
    "/",
    "/queue",
    "/queue/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
    "/search",
    "/search/demographics",
    "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
    "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/merge",
    "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/unmerge",
    "/sources",
    "/sources/EAST.ADT",
    "/domains",
    "/domains/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c",
    "/pseudonym-domains",
    "/configuration",
    "/audit",
    "/about",
    "/preferences",
    "/preferences/language",
    "/preferences/keyboard",
    "/patients/123456789",
];

/// The sign-in page in each state its route names.
const SIGN_IN_SCREENS: [&str; 3] = ["/sign-in", "/sign-in/signed-out", "/sign-in/expired"];

/// The route patterns of the console's pages, from its page list.
fn routes() -> Vec<String> {
    ferropix_console::server::PAGES
        .iter()
        .map(|pattern| {
            pattern
                .split('/')
                .map(|segment| match segment.strip_prefix('{') {
                    Some(name) => format!(":{}", name.trim_end_matches('}')),
                    None => segment.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
}

/// Whether the address `screen` is one the route `pattern` serves: each
/// segment equal, a `:name` segment matching any segment.
fn serves(pattern: &str, screen: &str) -> bool {
    let wanted: Vec<&str> = pattern.split('/').collect();
    let given: Vec<&str> = screen.split('/').collect();
    wanted.len() == given.len()
        && wanted
            .iter()
            .zip(&given)
            .all(|(want, give)| (want.starts_with(':') && !give.is_empty()) || want == give)
}

#[test]
fn every_route_of_the_console_has_a_screen_the_journeys_walk() {
    let routes = routes();
    assert!(
        routes.len() >= SCREENS.len() - 1,
        "the router reads as {} routes, which is fewer than the screens: {routes:?}",
        routes.len()
    );
    let walked: Vec<&str> = SCREENS.iter().chain(&SIGN_IN_SCREENS).copied().collect();
    for route in &routes {
        assert!(
            walked.iter().any(|screen| serves(route, screen)),
            "the route {route} has no screen in journeys::SCREENS, so no journey walks it"
        );
    }
    for screen in &walked {
        assert!(
            routes.iter().any(|route| serves(route, screen)),
            "{screen} is served by no route of the console"
        );
    }
    assert!(
        SCREENS.iter().any(|screen| screen.contains(PERSON)),
        "a person's screens are walked"
    );
}

#[test]
fn a_route_parameter_matches_one_segment_and_no_more() {
    assert!(serves("/patients/:person", "/patients/x"));
    assert!(!serves("/patients/:person", "/patients/x/merge"));
    assert!(!serves("/patients/:person", "/patients/"));
    assert!(serves("/", "/"));
    assert!(!serves("/queue", "/"));
}
