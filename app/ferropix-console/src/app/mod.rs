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
use ferrohealth_ui::island::{FormNavigator, PageAnnouncer, SessionWatch, ShortcutLegend};
use ferrohealth_ui::theme::Preferences;
use ferrohealth_ui::view::kbd::{Shortcut, ShortcutGroup};
use ferrohealth_ui::view::search::{SearchBox, search_box};
use ferrohealth_ui::view::shell::{Shell, TopBar, shell, topbar};
use ferrohealth_ui::widget::menu::{ThemeChoice, UserMenu, user_menu};
use ferrohealth_ui::widget::nav::{Rail, Sidebar, sidebar};
use ferrohealth_ui::widget::session::Watch;
use ferrohealth_ui::widget::shortcuts::LegendWords;
use leptos::prelude::*;
use leptos_meta::{Title, provide_meta_context};
use leptos_router::Lazy;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::path;

use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::PageContext;
use follow::follow;
use in_page::{Posted, listen_posts};
use keys::listen_keys;
use routes::{Aside, Patients, Reads, Registry, Work, content, missing};
use shell::{brand_lockup, footer, navigation, pill};

mod follow;
pub mod in_page;
pub mod keys;
mod routes;
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
    crate::server::context::page_for(&state, &parts.headers, &path)
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
            *document,
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

/// The console's keyboard shortcuts, which the legend lists (FerroPIX
/// design D21).
fn shortcut_groups() -> Vec<ShortcutGroup> {
    let shortcut = |keys: &str, does: Msg| Shortcut {
        keys: String::from(keys),
        does: tr(does),
    };
    let tabs = crate::pages::patient::TABS
        .iter()
        .zip(["1", "2", "3", "4"])
        .map(|((_, words), key)| shortcut(key, *words))
        .collect();
    vec![
        ShortcutGroup {
            title: tr(Msg::ShortcutsEverywhere),
            shortcuts: vec![
                shortcut("/", Msg::ShortcutSearch),
                shortcut("?", Msg::ShortcutLegend),
            ],
        },
        ShortcutGroup {
            title: tr(Msg::ShortcutsLists),
            shortcuts: vec![
                shortcut("j | down", Msg::ShortcutNext),
                shortcut("k | up", Msg::ShortcutPrevious),
                shortcut("enter", Msg::ShortcutOpen),
                shortcut("o | d", Msg::ShortcutGlance),
            ],
        },
        ShortcutGroup {
            title: tr(Msg::ShortcutsPatient),
            shortcuts: tabs,
        },
    ]
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
    listen_posts();
    listen_keys();
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
    // NOTE: no specification governs this: our own design; a post's answer is shown at its
    // own address until the steward goes to another one.
    Effect::watch(
        move || location.pathname.get(),
        move |now, _, _| {
            if let Some(Posted(posted)) = posted
                && posted
                    .get_untracked()
                    .is_some_and(|page| page.path.split('?').next() != Some(now.as_str()))
            {
                drop(posted.try_set(None));
            }
        },
        false,
    );
    let posted_here = Memo::new(move |_| {
        let here = location.pathname.get();
        posted
            .and_then(|Posted(posted)| posted.get())
            .filter(|page| page.path.split('?').next() == Some(here.as_str()))
    });
    // NOTE: leptos_server 0.8 `Resource::try_read_untracked`: a resource is read inside an
    // effect or a `Suspense`, never in a memo, so the page is taken from the read here.
    Effect::new(move |_| {
        let shown = posted_here
            .get()
            .or_else(|| read.get().and_then(Result::ok));
        if let (Some(page), Some(Latest(latest))) = (shown, latest) {
            follow(&page);
            if latest.get_untracked() != page {
                latest.set(page);
            }
        }
    });
    provide_context(Reads {
        read,
        posted: posted_here,
    });
    let signed_in = Memo::new(move |_| !crate::paths::is_sign_in(&location.pathname.get()));
    move || {
        // NOTE: leptos_router 0.8 `Routes`: the router writes a navigation's address once a
        // route has drawn it; each group of pages loads its code from its own chunks.
        let content = view! {
            <Routes fallback=|| content(missing)>
                <Route path=path!("/sign-in") view={Lazy::<Aside>::new()} />
                <Route path=path!("/sign-in/:state") view={Lazy::<Aside>::new()} />
                <Route path=path!("/") view={Lazy::<Work>::new()} />
                <Route path=path!("/queue") view={Lazy::<Work>::new()} />
                <Route path=path!("/queue/:candidate") view={Lazy::<Work>::new()} />
                <Route path=path!("/patients/:person/merge") view={Lazy::<Work>::new()} />
                <Route path=path!("/patients/:person/unmerge") view={Lazy::<Work>::new()} />
                <Route path=path!("/search") view={Lazy::<Patients>::new()} />
                <Route path=path!("/search/demographics") view={Lazy::<Patients>::new()} />
                <Route path=path!("/patients/:person") view={Lazy::<Patients>::new()} />
                <Route path=path!("/sources") view={Lazy::<Registry>::new()} />
                <Route path=path!("/sources/:source") view={Lazy::<Registry>::new()} />
                <Route path=path!("/domains") view={Lazy::<Registry>::new()} />
                <Route path=path!("/domains/:domain") view={Lazy::<Registry>::new()} />
                <Route path=path!("/pseudonym-domains") view={Lazy::<Registry>::new()} />
                <Route path=path!("/configuration") view={Lazy::<Aside>::new()} />
                <Route path=path!("/audit") view={Lazy::<Registry>::new()} />
                <Route path=path!("/about") view={Lazy::<Aside>::new()} />
                <Route path=path!("/preferences") view={Lazy::<Aside>::new()} />
                <Route path=path!("/preferences/:section") view={Lazy::<Aside>::new()} />
            </Routes>
        }
        .into_any();
        if signed_in.get() {
            let legend = view! {
                <ShortcutLegend groups=shortcut_groups() words=LegendWords::of_page() />
            };
            view! {
                {framed(content)}
                {legend}
                {watch()}
            }
            .into_any()
        } else {
            content
        }
    }
}
/// FerroUI's watch over the steward's session, from what it had left as the
/// server drew the first page (FerroUI `docs/consoles.md`, "The session's
/// end"); nothing on a page drawn with no session.
fn watch() -> Option<AnyView> {
    let left = page_context().session?;
    let watch = Watch::new(crate::paths::SESSION, EXPIRED, SIGN_OUT, left);
    Some(view! { <SessionWatch watch /> }.into_any())
}

/// The attribute of the root element that counts the pages the browser has
/// drawn in place, which a journey waits on after a press.
pub const DRAWS: &str = "data-pix-draws";

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
        signed_out: Some(String::from(EXPIRED)),
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
