// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The page tree, which the server draws and the browser hydrates whole and
//! then draws every page after the first, and the shell around it.
//!
//! The document links the library's stylesheet and its FerroPIX theme, which
//! the server serves from the library's constants, and carries the
//! steward's preferences on `<html>` (`data-theme`, `data-density`,
//! `data-motion`, `data-hints`) and the locale as `lang`, so a page arrives
//! in its theme before any script runs. Every page is drawn from its
//! [`PageContext`]: the server reads it for the first page and the browser
//! takes it serialized with the page; after a navigation or a post drawn in
//! the page the browser asks the console's server for the next one
//! ([`page_now`], [`post_now`]), and the old page stays until it comes, so
//! a page is never drawn from another page's document. The shell is
//! FerroUI's, drawn once and following the router (FerroUI
//! `docs/consoles.md`, "How a console builds"): the top bar with the patient
//! search, the server's status pill and the steward's menu, the navigation
//! of `docs/architecture.md` §12.2 as the design groups it, and the footer.
//! No specification governs the console's pages: our own design.
#![cfg_attr(
    target_arch = "wasm32",
    allow(
        clippy::unused_async_trait_impl,
        reason = "the server function macro writes the browser half's call as an async trait method; only that target has it"
    )
)]

use ferrohealth_ui::i18n::{Arg, Locale, provide_locale};
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::island::{FormNavigator, PageAnnouncer};
use ferrohealth_ui::theme::Preferences;
use ferrohealth_ui::view::search::{SearchBox, search_box};
use ferrohealth_ui::view::shell::{Footer, Shell, TopBar, brand_mark, lockup, shell, topbar};
use ferrohealth_ui::view::status::{Reach as PillReach, StatusPill, status_pill};
use ferrohealth_ui::widget::menu::{ThemeChoice, UserMenu, user_menu};
use ferrohealth_ui::widget::nav::{Count, Entry, Group, Rail, Sidebar, sidebar};
use leptos::prelude::*;
use leptos_meta::{Title, provide_meta_context};
use leptos_router::components::Router;
use leptos_router::hooks::use_location;

use crate::locale::{Msg, tr, tr_with};
use crate::model::{PageContext, Probe, Reach};
use crate::paths::{MARK_DARK, MARK_LIGHT};

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

/// The page context the page is drawn from ([`PageContext`]): the one the
/// router's current read brought, inside the page tree.
#[derive(Debug, Clone)]
pub struct Shown(pub PageContext);

/// The route parameters of the page being drawn, by name.
#[derive(Debug, Clone, Default)]
pub struct Params(pub Vec<(String, String)>);

/// The latest page context, which the shell follows.
#[derive(Debug, Clone, Copy)]
pub struct Latest(pub RwSignal<PageContext>);

/// The page context the server read of the request; the browser's own
/// half has none outside the page tree.
fn requested() -> Option<PageContext> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use_context::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<PageContext>().cloned())
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// The context of the page being drawn.
#[must_use]
pub fn page_context() -> PageContext {
    use_context::<Shown>()
        .map(|shown| shown.0)
        .or_else(|| use_context::<Latest>().map(|latest| latest.0.get_untracked()))
        .or_else(requested)
        .unwrap_or_else(|| PageContext::signed_out(String::new()))
}

/// The path of the page being drawn, without its query.
#[must_use]
pub fn current_path() -> String {
    let path = page_context().path;
    path.split(['?', '#']).next().unwrap_or_default().to_owned()
}

/// The query of the page being drawn, without its `?`, empty for none.
#[must_use]
pub fn current_query() -> String {
    let path = page_context().path;
    path.split_once('?')
        .map(|(_, query)| query.split('#').next().unwrap_or_default().to_owned())
        .unwrap_or_default()
}

/// The route parameter `name` of the page being drawn.
#[must_use]
pub fn param(name: &str) -> Option<String> {
    use_context::<Params>().and_then(|params| {
        params
            .0
            .into_iter()
            .find(|(each, _)| each == name)
            .map(|(_, value)| value)
    })
}

/// The steward's preferences the page's context carries.
#[must_use]
pub fn preferences() -> Preferences {
    let page = page_context();
    Preferences::read(&page.preferences, &page.prefix)
}

/// The locale of the page `page`, English for a tag no catalog names.
fn locale_of(page: &PageContext) -> Locale {
    if page.locale == crate::locale::PSEUDO_TAG {
        return crate::locale::pseudo();
    }
    crate::locale::choices()
        .into_iter()
        .find(|locale| locale.tag() == page.locale)
        .unwrap_or_else(Locale::english)
}

/// The whole HTML document the server renders around [`App`], in the
/// preferences the request's cookies keep and the locale they choose.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn document(options: LeptosOptions) -> impl IntoView {
    let page = page_context();
    let chosen = Preferences::read(&page.preferences, &page.prefix);
    let lang = locale_of(&page).tag();
    view! {
        <!DOCTYPE html>
        <html
            lang=lang
            data-theme=chosen.theme.key()
            data-density=chosen.density.key()
            data-motion=chosen.motion.key()
            data-hints=chosen.hints.key()
        >
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <link rel="icon" type="image/svg+xml" href=crate::paths::FAVICON />
                <link rel="stylesheet" href=crate::paths::STYLESHEET />
                <link rel="stylesheet" href=crate::paths::THEME />
                <HydrationScripts options />
                <leptos_meta::MetaTags />
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

/// The page context of the page at `path`, its path and its query, as the
/// request that asks reads it: what the browser draws after a navigation.
///
/// # Errors
///
/// When the console's server cannot be reached or the session store read.
#[server(endpoint = "page")]
pub async fn page_now(
    /// The path of the page the browser navigated to.
    path: String,
) -> Result<PageContext, ServerFnError> {
    if let Some(page) = requested().filter(|page| page.path == path) {
        return Ok(page);
    }
    let state = use_context::<crate::server::ConsoleState>()
        .ok_or_else(|| ServerFnError::new("the console's state is missing"))?;
    let parts = use_context::<http::request::Parts>()
        .ok_or_else(|| ServerFnError::new("the request is missing"))?;
    crate::server::page_for(&state, &parts.headers, &path)
        .await
        .map_err(|error| ServerFnError::new(error.to_string()))
}

/// The page context of the page the form `fields`, posted to `path`, draws:
/// what a search, a trail filter, a reveal or a domain's form draws in the
/// page once it has hydrated, as the plain post draws it with none.
///
/// # Errors
///
/// When the console's server cannot be reached or no form posts to `path`.
#[server(endpoint = "post", input = server_fn::codec::Json)]
pub async fn post_now(
    /// The path the form posts to.
    path: String,
    /// The form's fields, its submitter's last.
    fields: Vec<(String, String)>,
) -> Result<PageContext, ServerFnError> {
    let state = use_context::<crate::server::ConsoleState>()
        .ok_or_else(|| ServerFnError::new("the console's state is missing"))?;
    let parts = use_context::<http::request::Parts>()
        .ok_or_else(|| ServerFnError::new("the request is missing"))?;
    match crate::posts::answer(&state, &parts.headers, &path, &fields).await {
        crate::posts::Answer::Drawn {
            path,
            token,
            steward,
            document,
        } => Ok(crate::posts::posted_context(
            &state,
            &parts.headers,
            &path,
            &token,
            steward,
            document,
        )
        .await),
        crate::posts::Answer::Back(path) => Ok(PageContext {
            path: path.clone(),
            redirect: Some(path),
            ..PageContext::signed_out(state.sessions().preference_prefix())
        }),
        crate::posts::Answer::Unknown => Err(ServerFnError::new("no form posts to this address")),
    }
}

/// The page context a post drawn in the page brought, which the page shows
/// in place of a read until the next navigation.
#[derive(Debug, Clone, Copy)]
pub struct Posted(pub RwSignal<Option<PageContext>>);

/// Posts the form under `event` through [`post_now`] and draws its answer
/// in the page, in place of the plain post's new document.
pub fn post_in_page(event: &leptos::ev::SubmitEvent) {
    #[cfg(target_arch = "wasm32")]
    {
        use leptos::wasm_bindgen::{JsCast as _, JsValue};
        let Some(form) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlFormElement>().ok())
        else {
            return;
        };
        let Some(Posted(posted)) = use_context::<Posted>() else {
            return;
        };
        let Ok(data) = web_sys::FormData::new_with_form(&form) else {
            return;
        };
        event.prevent_default();
        if let Some(by) = event
            .submitter()
            .and_then(|by| by.dyn_into::<web_sys::HtmlElement>().ok())
            && let (Some(name), Some(value)) = (by.get_attribute("name"), by.get_attribute("value"))
            && data.append_with_str(&name, &value).is_err()
        {
            leptos::logging::warn!("the browser refused the submit's value");
        }
        let mut fields = Vec::new();
        if let Ok(Some(entries)) = JsValue::from(data.entries())
            .dyn_into::<web_sys::js_sys::Iterator>()
            .map(Some)
        {
            while let Ok(next) = entries.next() {
                if next.done() {
                    break;
                }
                let pair = web_sys::js_sys::Array::from(&next.value());
                if let (Some(name), Some(value)) =
                    (pair.get(0).as_string(), pair.get(1).as_string())
                {
                    fields.push((name, value));
                }
            }
        }
        let action = form.action();
        let path = web_sys::Url::new(&action).map_or(action, |url| url.pathname());
        leptos::task::spawn_local(async move {
            match post_now(path.clone(), fields).await {
                Ok(page) => {
                    if let Some(to) = &page.redirect {
                        if window().location().assign(to).is_err() {
                            leptos::logging::warn!(
                                "the browser refused the page it is sent back to"
                            );
                        }
                        return;
                    }
                    let shown = page.path.clone();
                    drop(posted.try_set(Some(page)));
                    if window().location().pathname().ok().as_deref() != Some(shown.as_str()) {
                        leptos_router::hooks::use_navigate()(&shown, Default::default());
                    }
                }
                Err(error) => {
                    leptos::logging::warn!("the post did not come back: {error}");
                    if form.submit().is_err() {
                        leptos::logging::warn!("the browser refused the plain post");
                    }
                }
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _: &leptos::ev::SubmitEvent = event;
}

/// The root component: the router, the page tree, and the title.
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn App() -> impl IntoView {
    provide_meta_context();
    // NOTE: the Leptos book, `ssr/22_life_cycle`: a value the server decides reaches the
    // hydrating page serialized, so both halves draw the same page.
    let initial =
        SharedValue::new(|| requested().unwrap_or_else(|| PageContext::signed_out(String::new())));
    let latest = RwSignal::new((*initial).clone());
    provide_context(Latest(latest));
    provide_context(Posted(RwSignal::new(None)));
    provide_locale(locale_of(&initial));
    ferrohealth_ui::widget::hydration::mark();
    view! {
        <Title formatter=title_format(tr(Msg::ConsoleName)) text=String::new() />
        <Router>
            <FormNavigator />
            <PageAnnouncer />
            <Root />
        </Router>
    }
}

/// The page tree inside the router: the shell, drawn once and following
/// the router, around the page the current read draws.
#[component]
fn Root() -> impl IntoView {
    ferrohealth_ui::widget::nav::follow_path();
    let location = use_location();
    let latest = use_context::<Latest>();
    let posted = use_context::<Posted>();
    let key = move || {
        (
            location.pathname.get(),
            location.search.get(),
            ferrohealth_ui::widget::submit::posts().get(),
        )
    };
    let read = Resource::new(key, |(path, query, _)| {
        let path = if query.is_empty() {
            path
        } else {
            format!("{path}?{query}")
        };
        page_now(path)
    });
    // NOTE: no specification governs this: our own design; a post's answer is shown until
    // the steward goes to another address.
    Effect::watch(
        move || (location.pathname.get(), location.search.get()),
        move |now, before, _| {
            if before.is_some_and(|before| before != now)
                && let Some(Posted(posted)) = posted
            {
                drop(posted.try_set(None));
            }
        },
        false,
    );
    let shown = Memo::new(move |_| {
        let read = read.get().and_then(Result::ok);
        let posted = posted.and_then(|Posted(posted)| posted.get());
        posted.or(read)
    });
    Effect::new(move |_| {
        if let (Some(page), Some(Latest(latest))) = (shown.get(), latest) {
            follow(&page);
            if latest.get_untracked() != page {
                latest.set(page);
            }
        }
    });
    let signed_in = Memo::new(move |_| !crate::paths::is_sign_in(&location.pathname.get()));
    move || {
        let content = view! {
            <Transition fallback=|| ()>
                {move || Suspend::new(async move {
                    let read = read.await;
                    let page = shown.get().or_else(|| read.ok());
                    page.map(drawn)
                })}
            </Transition>
        }
        .into_any();
        if signed_in.get() {
            framed(content)
        } else {
            content
        }
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

/// The pages, by the address they answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
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

/// The page the address `path` names, with its parameters by name.
fn route(path: &str) -> Option<(Page, Vec<(String, String)>)> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let one = |name: &str, value: &str| vec![(String::from(name), String::from(value))];
    Some(match segments.as_slice() {
        ["sign-in"] => (Page::SignIn, Vec::new()),
        ["sign-in", state] => (Page::SignIn, one("state", state)),
        [""] => (Page::Dashboard, Vec::new()),
        ["queue"] => (Page::Queue, Vec::new()),
        ["queue", candidate] => (Page::Pair, one("candidate", candidate)),
        ["search"] | ["search", "demographics"] => (Page::Search, Vec::new()),
        ["patients", person] => (Page::Patient, one("person", person)),
        ["patients", person, "merge"] => (Page::Merge, one("person", person)),
        ["patients", person, "unmerge"] => (Page::Unmerge, one("person", person)),
        ["sources"] => (Page::Sources, Vec::new()),
        ["sources", source] => (Page::Source, one("source", source)),
        ["domains"] => (Page::Domains, Vec::new()),
        ["domains", domain] => (Page::Domain, one("domain", domain)),
        ["pseudonym-domains"] => (Page::PseudonymDomains, Vec::new()),
        ["configuration"] => (Page::Configuration, Vec::new()),
        ["audit"] => (Page::Audit, Vec::new()),
        ["about"] => (Page::About, Vec::new()),
        ["preferences"] => (Page::Preferences, Vec::new()),
        ["preferences", section] => (Page::Preferences, one("section", section)),
        _ => return None,
    })
}

/// Draws the page `page` is the context of, by the address it was read for.
fn drawn(page: PageContext) -> AnyView {
    use crate::pages::{about, not_found, preferences, screens, search, sign_in};
    let found = route(&page.path);
    provide_context(Shown(page));
    let Some((page, params)) = found else {
        return view! { <not_found::NotFound /> }.into_any();
    };
    provide_context(Params(params));
    match page {
        Page::SignIn => view! { <sign_in::SignInPage /> }.into_any(),
        Page::Dashboard => view! { <screens::Dashboard /> }.into_any(),
        Page::Queue => view! { <screens::Queue /> }.into_any(),
        Page::Pair => view! { <screens::Pair /> }.into_any(),
        Page::Search => view! { <search::SearchPage /> }.into_any(),
        Page::Patient => view! { <screens::Patient /> }.into_any(),
        Page::Merge => view! { <screens::Merge /> }.into_any(),
        Page::Unmerge => view! { <screens::Unmerge /> }.into_any(),
        Page::Sources => view! { <screens::Sources /> }.into_any(),
        Page::Source => view! { <screens::Source /> }.into_any(),
        Page::Domains => view! { <screens::Domains /> }.into_any(),
        Page::Domain => view! { <screens::Domain /> }.into_any(),
        Page::PseudonymDomains => view! { <screens::PseudonymDomains /> }.into_any(),
        Page::Configuration => view! { <screens::Configuration /> }.into_any(),
        Page::Audit => view! { <screens::Audit /> }.into_any(),
        Page::About => view! { <about::AboutPage /> }.into_any(),
        Page::Preferences => view! { <preferences::PreferencesPage /> }.into_any(),
    }
}

/// Follows the page `page` the browser now draws: sends the browser to the
/// sign-in it names as its redirect, and sets the root element's theme,
/// density, motion and hints from its preferences, as the server writes
/// them on a document it draws; a new language loads the page as a
/// document, since its words are drawn in its locale.
fn follow(page: &PageContext) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(to) = &page.redirect {
            if window().location().assign(to).is_err() {
                leptos::logging::warn!("the browser refused the sign-in");
            }
            return;
        }
        let Some(root) = document().document_element() else {
            return;
        };
        let language = locale_of(page).tag();
        if root
            .get_attribute("lang")
            .is_some_and(|lang| lang != language)
        {
            if window().location().reload().is_err() {
                leptos::logging::warn!("the browser refused to load the page in its language");
            }
            return;
        }
        let chosen = Preferences::read(&page.preferences, &page.prefix);
        for (name, value) in [
            ("data-theme", chosen.theme.key()),
            ("data-density", chosen.density.key()),
            ("data-motion", chosen.motion.key()),
            ("data-hints", chosen.hints.key()),
        ] {
            if root.get_attribute(name).as_deref() != Some(value)
                && root.set_attribute(name, value).is_err()
            {
                leptos::logging::warn!("the browser refused the root's {name}");
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _: &PageContext = page;
}

/// The latest page context, which the shell's parts follow.
fn latest() -> PageContext {
    use_context::<Latest>().map_or_else(page_context, |latest| latest.0.get())
}

/// Returns `content` in the FerroUI shell, drawn once: the navigation and
/// the forms that return to a page follow the router, and the status pill
/// and the steward's menu follow the latest page context.
fn framed(content: AnyView) -> AnyView {
    let first = page_context();
    let nav = sidebar(Sidebar {
        id: SIDEBAR_ID,
        label: tr(Msg::NavLabel),
        groups: navigation(),
        current: first.path.clone(),
        rail: Some(Rail {
            collapsed: crate::preferences::rail_in(&first.preferences, &first.prefix),
            action: String::from(crate::preferences::SIDEBAR_ROUTE),
            back: first.path.clone(),
        }),
        nest: None,
    });
    let menu = move || {
        let page = latest();
        let operator = page
            .operator
            .clone()
            .unwrap_or_else(|| tr(Msg::StewardUnnamed));
        user_menu(UserMenu {
            id: "user-menu",
            label: operator.clone(),
            name: Some(operator.clone()),
            who: operator,
            line: tr_with(
                Msg::MenuLine,
                &[("provider", Arg::from(page.provider.clone()))],
            ),
            preferences: String::from(PREFERENCES),
            theme: ThemeChoice {
                current: Preferences::read(&page.preferences, &page.prefix).theme,
                action: String::from(crate::preferences::PREFERENCE_ROUTE),
                back: page.path.clone(),
            },
            choices: Vec::new(),
            links: Vec::new(),
            shortcuts: Some(format!("{PREFERENCES}/keyboard")),
            sign_out: Some(String::from(SIGN_OUT)),
        })
        .into_any()
    };
    let search = search_box(SearchBox {
        id: String::from(SEARCH_ID),
        label: tr(Msg::SearchLabel),
        placeholder: tr(Msg::SearchPlaceholder),
        action: String::from(SEARCH),
        submit: tr(Msg::SearchFind).into(),
        post: true,
        suggest: Some(String::from(crate::pages::search::SUGGEST)),
        all: Some(ferrohealth_ui::i18n::Phrase::new(tr_with(
            Msg::SearchPatientsFor,
            &[("query", ferrohealth_ui::i18n::Phrase::open("query"))],
        ))),
        ..SearchBox::default()
    });
    let pill = move || {
        let page = latest();
        pill(page.probe.as_ref(), &page.base)
    };
    let end = view! {
        {pill}
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
