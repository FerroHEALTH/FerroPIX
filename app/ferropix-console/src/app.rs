// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The page tree the server renders, and the shell every signed-in page is
//! drawn in.
//!
//! The document links the library's stylesheet and its FerroPIX theme, which
//! the server serves from the library's constants, and carries the
//! steward's preferences on `<html>` (`data-theme`, `data-density`,
//! `data-motion`, `data-hints`) and the locale as `lang`, so a page arrives
//! in its theme before any script runs. The shell is FerroUI's: the top bar
//! with the patient search, the server's status pill and the steward's
//! menu, the navigation of `docs/architecture.md` §12.2 as the design
//! groups it, and the footer. No specification governs the console's pages:
//! our own design.

use ferrohealth_ui::i18n::{Arg, provide_locale};
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::theme::Preferences;
use ferrohealth_ui::view::search::{SearchBox, search_box};
use ferrohealth_ui::view::shell::{Footer, Shell, TopBar, brand_mark, lockup, shell, topbar};
use ferrohealth_ui::view::status::{Reach as PillReach, StatusPill, status_pill};
use ferrohealth_ui::widget::menu::{ThemeChoice, UserMenu, user_menu};
use ferrohealth_ui::widget::nav::{Count, Entry, Group, Rail, Sidebar, sidebar};
use leptos::prelude::*;
use leptos_meta::{MetaTags, Title, provide_meta_context};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::locale::{Msg, chosen, tr, tr_with};
use crate::pix::{Capability, Probe, Reach};
use crate::server::{ConsoleState, FAVICON, MARK_DARK, MARK_LIGHT, STYLESHEET, THEME};

/// The console's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The sign-in page.
pub const SIGN_IN: &str = "/sign-in";

/// The sign-out route, a form post.
pub const SIGN_OUT: &str = "/logout";

/// The dashboard.
pub const DASHBOARD: &str = "/";

/// The review queue.
pub const QUEUE: &str = "/queue";

/// The search page, by identifier.
pub const SEARCH: &str = "/search";

/// The search page, by demographics.
pub const SEARCH_DEMOGRAPHICS: &str = "/search/demographics";

/// The sources.
pub const SOURCES: &str = "/sources";

/// The identifier domains.
pub const DOMAINS: &str = "/domains";

/// The pseudonymisation domains.
pub const PSEUDONYM_DOMAINS: &str = "/pseudonym-domains";

/// The configuration.
pub const CONFIGURATION: &str = "/configuration";

/// The audit trail.
pub const AUDIT: &str = "/audit";

/// The About page.
pub const ABOUT: &str = "/about";

/// Your preferences, on its Appearance tab.
pub const PREFERENCES: &str = "/preferences";

/// The id of the navigation, which the drawer button opens.
const SIDEBAR_ID: &str = "sidebar";

/// The id of the top bar's search field.
const SEARCH_ID: &str = "pix-search";

/// What a page is drawn from, which the server read before the render.
///
/// It holds the steward's name, the outcome of the FerroPIX server's
/// liveness route, on About its `CapabilityStatement`, and the prefix of
/// the preference cookies.
#[derive(Debug, Clone)]
pub struct PageContext {
    /// The steward's name as the provider gave it.
    pub operator: Option<String>,
    /// The call to the server's liveness route; `None` on a page drawn with
    /// no session.
    pub probe: Option<Probe>,
    /// The server's `CapabilityStatement`, read for About alone.
    pub capability: Option<Capability>,
    /// The prefix every preference cookie's name takes.
    pub prefix: String,
    /// The operator API document the page is drawn from, when it reads one.
    pub document: Option<crate::document::Document>,
}

impl PageContext {
    /// The context of a page drawn with no session, the sign-in page.
    #[must_use]
    pub const fn signed_out(prefix: String) -> Self {
        Self {
            operator: None,
            probe: None,
            capability: None,
            prefix,
            document: None,
        }
    }
}

/// The request's parts, which the server provides to every render.
fn parts() -> Option<http::request::Parts> {
    use_context::<http::request::Parts>()
}

/// The path of the page being drawn.
#[must_use]
pub fn current_path() -> String {
    parts()
        .map(|parts| parts.uri.path().to_owned())
        .unwrap_or_default()
}

/// The query of the page being drawn, empty for none.
#[must_use]
pub fn current_query() -> String {
    parts()
        .and_then(|parts| parts.uri.query().map(str::to_owned))
        .unwrap_or_default()
}

/// The context of the page being drawn.
#[must_use]
pub fn page_context() -> PageContext {
    use_context::<PageContext>().unwrap_or_else(|| PageContext::signed_out(String::new()))
}

/// The steward's preferences the request keeps.
#[must_use]
pub fn preferences() -> Preferences {
    let prefix = page_context().prefix;
    parts().map_or_else(Preferences::default, |parts| {
        crate::preferences::read(&parts.headers, &prefix)
    })
}

/// The whole HTML document the server renders around [`App`], in the
/// preferences the request's cookies keep and the locale they choose.
#[must_use]
pub fn document(options: LeptosOptions) -> impl IntoView {
    let preferences = preferences();
    let configured = use_context::<ConsoleState>()
        .map(|state| state.settings().locale.clone())
        .unwrap_or_default();
    let locale = parts().map_or(configured.clone(), |parts| {
        chosen(&preferences, &parts.headers, configured)
    });
    let lang = locale.tag();
    provide_locale(locale);
    view! {
        <!DOCTYPE html>
        <html
            lang=lang
            data-theme=preferences.theme.key()
            data-density=preferences.density.key()
            data-motion=preferences.motion.key()
            data-hints=preferences.hints.key()
        >
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <link rel="icon" type="image/svg+xml" href=FAVICON />
                <link rel="stylesheet" href=STYLESHEET />
                <link rel="stylesheet" href=THEME />
                <HydrationScripts options islands=true />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

/// The root's one title format: a page's words, then the console's name.
fn title_format(product: String) -> impl Fn(String) -> String + Send + Sync + 'static {
    move |page| {
        if page.is_empty() {
            product.clone()
        } else {
            format!("{page} · {product}")
        }
    }
}

/// The root component: the routes and the title.
///
/// It runs on the server alone: the console renders in islands mode, so
/// the browser hydrates FerroUI's islands of a page and nothing else.
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn App() -> impl IntoView {
    use crate::pages::{about, not_found, preferences, screens, search, sign_in};
    provide_meta_context();
    view! {
        <Title formatter=title_format(tr(Msg::ConsoleName)) text=String::new() />
        <Router>
            <Routes fallback=|| view! { <not_found::NotFound /> }>
                <Route path=path!("/sign-in") view=sign_in::SignInPage />
                <Route path=path!("/sign-in/:state") view=sign_in::SignInPage />
                <Route path=path!("/") view=screens::Dashboard />
                <Route path=path!("/queue") view=screens::Queue />
                <Route path=path!("/queue/:candidate") view=screens::Pair />
                <Route path=path!("/search") view=search::SearchPage />
                <Route path=path!("/search/demographics") view=search::SearchPage />
                <Route path=path!("/patients/:person") view=screens::Patient />
                <Route path=path!("/patients/:person/merge") view=screens::Merge />
                <Route path=path!("/patients/:person/unmerge") view=screens::Unmerge />
                <Route path=path!("/sources") view=screens::Sources />
                <Route path=path!("/sources/:source") view=screens::Source />
                <Route path=path!("/domains") view=screens::Domains />
                <Route path=path!("/domains/:domain") view=screens::Domain />
                <Route path=path!("/pseudonym-domains") view=screens::PseudonymDomains />
                <Route path=path!("/configuration") view=screens::Configuration />
                <Route path=path!("/audit") view=screens::Audit />
                <Route path=path!("/about") view=about::AboutPage />
                <Route path=path!("/preferences") view=preferences::PreferencesPage />
                <Route path=path!("/preferences/:section") view=preferences::PreferencesPage />
            </Routes>
        </Router>
    }
}

/// The footer every page carries.
#[must_use]
pub fn footer() -> Footer {
    Footer {
        product: tr(Msg::ConsoleName).into(),
        version: VERSION.into(),
        about: ABOUT.into(),
    }
}

/// The FerroPIX lockup, its marks in both themes, leading to `href`.
#[must_use]
pub fn brand_lockup() -> AnyView {
    lockup(
        DASHBOARD,
        brand_mark(MARK_LIGHT, MARK_DARK),
        tr(Msg::Product),
        tr(Msg::ConsoleWord),
    )
    .into_any()
}

/// One entry of the navigation.
fn entry(href: &'static str, label: Msg, glyph: Glyph, exact: bool) -> Entry {
    Entry {
        href: href.into(),
        label: tr(label).into(),
        glyph,
        exact,
        count: None,
    }
}

/// The navigation's groups, as the design draws them.
fn navigation() -> Vec<Group> {
    // NOTE: FerroUI COMPONENTS.md R12: a count the console does not know is a muted
    // dash with its reason, never a zero; the queue's depth is the operator API's.
    let queue = Entry {
        count: Some(Count::Unknown {
            why: tr(Msg::QueueDepthUnknown),
        }),
        ..entry(QUEUE, Msg::Queue, Glyph::LIST_CHECKS, false)
    };
    vec![
        Group {
            id: "nav-work",
            title: tr(Msg::NavWork).into(),
            entries: vec![
                entry(DASHBOARD, Msg::Dashboard, Glyph::LAYOUT_DASHBOARD, true),
                queue,
            ],
        },
        Group {
            id: "nav-patients",
            title: tr(Msg::NavPatients).into(),
            entries: vec![entry(SEARCH, Msg::Search, Glyph::USER_ROUND_SEARCH, false)],
        },
        Group {
            id: "nav-registry",
            title: tr(Msg::NavRegistry).into(),
            entries: vec![
                entry(SOURCES, Msg::Sources, Glyph::RADIO_TOWER, false),
                entry(DOMAINS, Msg::Domains, Glyph::TAGS, false),
                entry(
                    PSEUDONYM_DOMAINS,
                    Msg::PseudonymDomains,
                    Glyph::SHIELD_CHECK,
                    false,
                ),
            ],
        },
        Group {
            id: "nav-trail",
            title: tr(Msg::NavTrail).into(),
            entries: vec![entry(AUDIT, Msg::Audit, Glyph::FILE_CLOCK, false)],
        },
        Group {
            id: "nav-console",
            title: tr(Msg::NavConsole).into(),
            entries: vec![
                entry(CONFIGURATION, Msg::Configuration, Glyph::FILE_TEXT, false),
                entry(ABOUT, Msg::About, Glyph::INFO, true),
            ],
        },
    ]
}

/// The top bar's status pill for the call `probe` to the server's liveness
/// route.
fn pill(probe: Option<&Probe>, base: &str) -> AnyView {
    let (reach, words) = match probe.map(|probe| &probe.reach) {
        Some(Reach::Up) => (PillReach::Up, tr(Msg::PillUp)),
        Some(Reach::Refused(status))
            if *status == http::StatusCode::UNAUTHORIZED
                || *status == http::StatusCode::FORBIDDEN =>
        {
            (
                PillReach::Refused,
                tr_with(
                    Msg::PillRefused,
                    &[("status", Arg::from(status.as_u16().to_string()))],
                ),
            )
        }
        Some(Reach::Refused(status)) => (
            PillReach::Degraded,
            tr_with(
                Msg::PillDegraded,
                &[("status", Arg::from(status.as_u16().to_string()))],
            ),
        ),
        Some(Reach::Unreachable(_)) | None => (PillReach::Unreachable, tr(Msg::PillUnreachable)),
    };
    status_pill(StatusPill {
        reach,
        words,
        detail: Some(tr_with(
            Msg::PillDetail,
            &[("base", Arg::from(base.to_owned()))],
        )),
    })
    .into_any()
}

/// The host of the OpenID Provider the steward signed in at, as the user
/// menu names it.
fn provider_host() -> String {
    use_context::<ConsoleState>()
        .and_then(|state| {
            state
                .settings()
                .oidc
                .as_ref()
                .and_then(|oidc| oidc.authorization_endpoint.host_str().map(str::to_owned))
        })
        .unwrap_or_default()
}

/// Returns a signed-in page: `content` in the FerroUI shell, the navigation
/// marking the page at the request's path.
#[must_use]
pub fn framed(content: AnyView) -> AnyView {
    let page = page_context();
    let path = current_path();
    let base = use_context::<ConsoleState>()
        .map(|state| state.pix().base().to_string())
        .unwrap_or_default();
    let rail = parts().is_some_and(|parts| crate::preferences::rail(&parts.headers, &page.prefix));
    let nav = sidebar(Sidebar {
        id: SIDEBAR_ID,
        label: tr(Msg::NavLabel),
        groups: navigation(),
        current: path.clone(),
        rail: Some(Rail {
            collapsed: rail,
            action: String::from(crate::preferences::SIDEBAR_ROUTE),
            back: path.clone(),
        }),
        nest: None,
    });
    let operator = page
        .operator
        .clone()
        .unwrap_or_else(|| tr(Msg::StewardUnnamed));
    let menu = user_menu(UserMenu {
        id: "user-menu",
        label: operator.clone(),
        name: Some(operator.clone()),
        who: operator,
        line: tr_with(Msg::MenuLine, &[("provider", Arg::from(provider_host()))]),
        preferences: String::from(PREFERENCES),
        theme: ThemeChoice {
            current: preferences().theme,
            action: String::from(crate::preferences::PREFERENCE_ROUTE),
            back: path,
        },
        choices: Vec::new(),
        links: Vec::new(),
        // TODO(#204): pass the shortcut legend once FerroUI carries it.
        shortcuts: None,
        sign_out: Some(String::from(SIGN_OUT)),
    });
    let search = search_box(
        SearchBox {
            id: String::from(SEARCH_ID),
            label: tr(Msg::SearchLabel),
            placeholder: tr(Msg::SearchPlaceholder),
            action: String::from(SEARCH),
            submit: tr(Msg::SearchFind).into(),
            post: true,
            ..SearchBox::default()
        },
        None,
    );
    let end = view! {
        {pill(page.probe.as_ref(), &base)}
        {menu}
    }
    .into_any();
    shell(Shell {
        topbar: topbar(TopBar {
            sidebar: SIDEBAR_ID,
            lockup: brand_lockup(),
            search: Some(search),
            end,
        }),
        banner: None,
        sidebar: nav,
        content,
        dock: None,
        footer: footer(),
    })
}
