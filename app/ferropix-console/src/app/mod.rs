// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's pages, each drawn whole on the server as one document,
//! and the shell around them.
//!
//! The document links FerroUI's stylesheet, its FerroPIX theme and its one
//! loader script, served under FerroUI's versioned path, and carries the
//! steward's preferences on `<html>` (`data-theme`, `data-density`,
//! `data-motion`, `data-hints`) and the locale as `lang`, so a page arrives
//! in its theme before any script runs. Every page is drawn from the
//! [`Visit`] the server read for its request, and a link or a form loads
//! the next document (FerroUI `docs/consoles.md`, "How a console builds").
//! The shell is FerroUI's, drawn by every page: the top bar with the
//! patient search, the server's status pill and the steward's menu, the
//! navigation of `docs/architecture.md` §12.2 as the design groups it, and
//! the footer. No specification governs the console's pages: our own
//! design.

use ferrohealth_ui::html::Html;
use ferrohealth_ui::i18n::{Arg, Phrase};
use ferrohealth_ui::serve::Assets;
use ferrohealth_ui::style::Brand;
use ferrohealth_ui::theme::Preferences;
use ferrohealth_ui::view::kbd::{Shortcut, ShortcutGroup};
use ferrohealth_ui::view::menu::{ThemeChoice, UserMenu, user_menu};
use ferrohealth_ui::view::nav::{Rail, Sidebar, sidebar};
use ferrohealth_ui::view::search::{SearchBox, search_box};
use ferrohealth_ui::view::session::{Watch, session_watch};
use ferrohealth_ui::view::shell::{Document, Shell, TopBar, centred, document, shell, topbar};
use ferrohealth_ui::view::shortcuts::shortcut_legend;
use ferrohealth_ui::view::toast::toast_region;
use http::StatusCode;

use crate::locale::messages::Msg;
use crate::markup::{h, t};
use crate::model::PageContext;
use crate::pages::Drawn;
use crate::visit::Visit;
use shell::{brand_lockup, footer, navigation, pill};

pub mod shell;

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

/// The sign-in page a request whose session has ended is sent to, which
/// the session watch loads with the page's address as its return.
pub const EXPIRED: &str = "/sign-in/expired";

/// The id of the navigation, which the drawer button opens.
const SIDEBAR_ID: &str = "sidebar";

/// The id of the top bar's search field.
const SEARCH_ID: &str = "pix-search";

/// The pages, by the address they answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `/sign-in` and `/sign-in/:state`.
    SignIn,
    /// `/`.
    Dashboard,
    /// `/queue`.
    Queue,
    /// `/queue/:candidate`.
    Pair,
    /// `/search` and `/search/demographics`.
    Search,
    /// `/patients/:person`.
    Patient,
    /// `/patients/:person/merge`.
    Merge,
    /// `/patients/:person/unmerge`.
    Unmerge,
    /// `/sources`.
    Sources,
    /// `/sources/:source`.
    Source,
    /// `/domains`.
    Domains,
    /// `/domains/:domain`.
    Domain,
    /// `/pseudonym-domains`.
    PseudonymDomains,
    /// `/configuration`.
    Configuration,
    /// `/audit`.
    Audit,
    /// `/about`.
    About,
    /// `/preferences` and `/preferences/:section`.
    Preferences,
}

/// The page the address `path` names, with its parameters by name; `None`
/// for an address no page answers.
#[must_use]
pub fn route(path: &str) -> Option<(Route, Vec<(String, String)>)> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let one = |name: &str, value: &str| vec![(String::from(name), String::from(value))];
    Some(match segments.as_slice() {
        ["sign-in"] => (Route::SignIn, Vec::new()),
        ["sign-in", state] => (Route::SignIn, one("state", state)),
        [""] => (Route::Dashboard, Vec::new()),
        ["queue"] => (Route::Queue, Vec::new()),
        ["queue", candidate] => (Route::Pair, one("candidate", candidate)),
        ["search"] | ["search", "demographics"] => (Route::Search, Vec::new()),
        ["patients", person] => (Route::Patient, one("person", person)),
        ["patients", person, "merge"] => (Route::Merge, one("person", person)),
        ["patients", person, "unmerge"] => (Route::Unmerge, one("person", person)),
        ["sources"] => (Route::Sources, Vec::new()),
        ["sources", source] => (Route::Source, one("source", source)),
        ["domains"] => (Route::Domains, Vec::new()),
        ["domains", domain] => (Route::Domain, one("domain", domain)),
        ["pseudonym-domains"] => (Route::PseudonymDomains, Vec::new()),
        ["configuration"] => (Route::Configuration, Vec::new()),
        ["audit"] => (Route::Audit, Vec::new()),
        ["about"] => (Route::About, Vec::new()),
        ["preferences"] => (Route::Preferences, Vec::new()),
        ["preferences", section] => (Route::Preferences, one("section", section)),
        _ => return None,
    })
}

/// The page of `route` drawn from `visit`.
fn drawn(route: Route, visit: &Visit) -> Drawn {
    use crate::pages::{about, preferences, screens, search, sign_in};
    match route {
        Route::SignIn => sign_in::sign_in_page(visit),
        Route::Dashboard => screens::dashboard(visit),
        Route::Queue => screens::queue(visit),
        Route::Pair => screens::pair(visit),
        Route::Search => search::search_page(visit),
        Route::Patient => screens::patient(visit),
        Route::Merge => screens::merge(visit),
        Route::Unmerge => screens::unmerge(visit),
        Route::Sources => screens::sources(visit),
        Route::Source => screens::source(visit),
        Route::Domains => screens::domains(visit),
        Route::Domain => screens::domain(visit),
        Route::PseudonymDomains => screens::pseudonym_domains(visit),
        Route::Configuration => screens::configuration(visit),
        Route::Audit => screens::audit(visit),
        Route::About => about::about_page(visit),
        Route::Preferences => preferences::preferences_page(visit),
    }
}

/// The whole document of the page `page` is the context of, with the
/// status it is answered with.
#[must_use]
pub fn page(page: PageContext) -> (StatusCode, Html) {
    let found = route(&page.path);
    let signed_in = !crate::paths::is_sign_in(&page.path);
    let mut visit = Visit::new(page);
    let drawn = if let Some((route, params)) = found {
        visit = visit.with_params(params);
        drawn(route, &visit)
    } else {
        crate::pages::not_found::not_found(&visit)
    };
    let status = drawn.status;
    let title = if drawn.title.is_empty() {
        visit.tr(Msg::ConsoleName)
    } else {
        format!("{} · {}", drawn.title, visit.tr(Msg::ConsoleName))
    };
    let body = if signed_in {
        framed(&visit, drawn)
    } else {
        centred(
            &visit.locale,
            drawn.body,
            toast_region(&visit.locale, Vec::new()),
            footer(&visit.locale),
        )
    };
    let html = document(Document {
        locale: visit.locale.clone(),
        preferences: visit.preferences(),
        title,
        assets: Assets::of(Brand::Pix),
        nonce: None,
        head: h(format!(
            "<link rel=\"icon\" type=\"image/svg+xml\" href=\"{}\">",
            t(crate::paths::FAVICON)
        )),
        body,
    });
    (status, html)
}

/// The console's keyboard shortcuts, which the legend lists (FerroPIX
/// design D21).
// TODO(#390): list `o | d` for the focused row's side panel once FerroUI binds them.
fn shortcut_groups(visit: &Visit) -> Vec<ShortcutGroup> {
    let shortcut = |keys: &str, does: Msg| Shortcut {
        keys: String::from(keys),
        does: visit.tr(does),
    };
    let tabs = crate::pages::patient::TABS
        .iter()
        .zip(["1", "2", "3", "4"])
        .map(|((_, words), key)| shortcut(key, *words))
        .collect();
    vec![
        ShortcutGroup {
            title: visit.tr(Msg::ShortcutsEverywhere),
            shortcuts: vec![
                shortcut("/", Msg::ShortcutSearch),
                shortcut("?", Msg::ShortcutLegend),
            ],
        },
        ShortcutGroup {
            title: visit.tr(Msg::ShortcutsLists),
            shortcuts: vec![
                shortcut("j | down", Msg::ShortcutNext),
                shortcut("k | up", Msg::ShortcutPrevious),
                shortcut("enter", Msg::ShortcutOpen),
            ],
        },
        ShortcutGroup {
            title: visit.tr(Msg::ShortcutsPatient),
            shortcuts: tabs,
        },
    ]
}

/// FerroUI's watch over the steward's session, from what it had left as the
/// server drew the page (FerroUI `docs/consoles.md`, "The session's end");
/// nothing on a page drawn with no session.
fn watch(visit: &Visit) -> Html {
    visit.page.session.map_or_else(Html::empty, |left| {
        session_watch(&Watch::new(
            &visit.locale,
            crate::paths::SESSION,
            EXPIRED,
            SIGN_OUT,
            left,
        ))
    })
}

/// Returns `drawn` in the FerroUI shell: the top bar with the search, the
/// status pill and the steward's menu, the navigation marked at the page,
/// the shortcut legend and the session watch.
fn framed(visit: &Visit, drawn: Drawn) -> Html {
    let locale = &visit.locale;
    let page = &visit.page;
    let here = page.path.clone();
    let nav = sidebar(
        locale,
        Sidebar {
            id: SIDEBAR_ID,
            label: visit.tr(Msg::NavLabel),
            groups: navigation(locale),
            current: visit.path().to_owned(),
            rail: Some(Rail {
                collapsed: crate::preferences::rail_in(&page.preferences, &page.prefix),
                action: String::from(crate::preferences::SIDEBAR_ROUTE),
                back: here.clone(),
            }),
            nest: None,
        },
    );
    let operator = page
        .operator
        .clone()
        .unwrap_or_else(|| visit.tr(Msg::StewardUnnamed));
    let menu = user_menu(
        locale,
        UserMenu {
            id: "user-menu",
            label: operator.clone(),
            name: Some(operator.clone()),
            who: operator,
            line: visit.tr_with(
                Msg::MenuLine,
                &[("provider", Arg::from(page.provider.clone()))],
            ),
            preferences: String::from(PREFERENCES),
            theme: ThemeChoice {
                current: Preferences::read(&page.preferences, &page.prefix).theme,
                action: String::from(crate::preferences::PREFERENCE_ROUTE),
                back: here,
            },
            choices: Vec::new(),
            links: Vec::new(),
            shortcuts: Some(format!("{PREFERENCES}/keyboard")),
            sign_out: Some(String::from(SIGN_OUT)),
        },
    );
    let search = search_box(
        locale,
        &SearchBox {
            id: String::from(SEARCH_ID),
            label: visit.tr(Msg::SearchLabel),
            placeholder: visit.tr(Msg::SearchPlaceholder),
            action: String::from(SEARCH),
            submit: Some(visit.tr(Msg::SearchFind).into()),
            post: true,
            suggest: Some(String::from(crate::pages::search::SUGGEST)),
            signed_out: Some(String::from(EXPIRED)),
            all: Some(Phrase::new(visit.tr_with(
                Msg::SearchPatientsFor,
                &[("query", Phrase::open("query"))],
            ))),
            ..SearchBox::default()
        },
    );
    let end = pill(locale, page.probe.as_ref(), &page.base).then(menu);
    let framed = shell(
        locale,
        Shell {
            topbar: topbar(
                locale,
                TopBar {
                    sidebar: SIDEBAR_ID,
                    lockup: brand_lockup(locale),
                    search: Some(search),
                    end,
                },
            ),
            banner: None,
            sidebar: nav,
            content: drawn.body,
            dock: drawn.dock,
            toasts: toast_region(locale, Vec::new()),
            footer: footer(locale),
        },
    );
    framed
        .then(shortcut_legend(locale, shortcut_groups(visit)))
        .then(watch(visit))
}

#[cfg(test)]
mod tests {
    use super::{Route, route};

    #[test]
    fn every_page_is_named_by_its_address_and_another_address_by_none() {
        assert_eq!(Some(Route::Dashboard), route("/").map(|(route, _)| route));
        assert_eq!(
            Some((
                Route::Pair,
                vec![(String::from("candidate"), String::from("c"))]
            )),
            route("/queue/c?x=1")
        );
        assert_eq!(
            Some(Route::Unmerge),
            route("/patients/p/unmerge").map(|(route, _)| route)
        );
        assert_eq!(
            Some(Route::Search),
            route("/search/demographics").map(|(route, _)| route)
        );
        assert_eq!(None, route("/patients/p/merge/again"));
        assert_eq!(None, route("/nowhere"));
    }
}
