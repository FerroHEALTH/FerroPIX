// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's route table and its four lazy route groups.
//!
//! Every page is named by the address it answers, and each group is a chunk
//! of its own under `--split`. No specification governs the console's
//! pages: our own design.

use leptos::prelude::*;
use leptos_router::{LazyRoute, lazy_route};

use super::{Params, Shown};
use crate::model::PageContext;

/// The pages, by the address they answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
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

/// The read of the page in view and the page context shown, which every
/// group of pages draws from.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reads {
    /// The read of the page at the address.
    pub(super) read: Resource<Result<PageContext, ServerFnError>>,
    /// A post's answer at its own address, shown in place of the read.
    pub(super) posted: Memo<Option<PageContext>>,
}

/// The page in view, drawn by `draw` once its read has arrived; the page
/// before stays until then.
pub(super) fn content(draw: fn(Page) -> AnyView) -> AnyView {
    let Some(Reads { read, posted }) = use_context::<Reads>() else {
        return ().into_any();
    };
    view! {
        <Transition fallback=|| ()>
            {move || Suspend::new(async move {
                let read = read.await;
                let page = posted.get().or_else(|| read.ok());
                page.map(|page| drawn(page, draw))
            })}
        </Transition>
    }
    .into_any()
}

/// Draws the page `page` is the context of, by the address it was read
/// for, with `draw`; an address no page answers is the not-found page.
fn drawn(page: PageContext, draw: fn(Page) -> AnyView) -> AnyView {
    let found = route(&page.path);
    provide_context(Shown(page));
    let Some((page, params)) = found else {
        return missing(Page::Dashboard);
    };
    provide_context(Params(params));
    draw(page)
}

/// The not-found page, whatever the page.
pub(super) fn missing(_: Page) -> AnyView {
    view! { <crate::pages::not_found::NotFound /> }.into_any()
}

/// Declares the lazy route `$name` of a group of pages, drawing each page
/// `$page` by its component `$component`, so its chunks hold their code
/// and no other group's.
macro_rules! group {
    ($(#[$doc:meta])* $name:ident { $($page:ident => $component:path),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub(super) struct $name;

        #[lazy_route]
        impl LazyRoute for $name {
            fn data() -> Self {
                Self
            }

            fn view(this: Self) -> AnyView {
                let _: $name = this;
                content(|page| match page {
                    $(Page::$page => view! { <$component /> }.into_any(),)+
                    _ => missing(page),
                })
            }
        }
    };
}

group!(
    /// The review work: the dashboard, the queue, a pair, a merge and an
    /// unmerge.
    Work {
        Dashboard => crate::pages::screens::Dashboard,
        Queue => crate::pages::screens::Queue,
        Pair => crate::pages::screens::Pair,
        Merge => crate::pages::screens::Merge,
        Unmerge => crate::pages::screens::Unmerge,
    }
);

group!(
    /// The search and the patient record.
    Patients {
        Search => crate::pages::search::SearchPage,
        Patient => crate::pages::screens::Patient,
    }
);

group!(
    /// The registry and the trail: the sources, the identifier and
    /// pseudonymisation domains, and the audit trail.
    Registry {
        Sources => crate::pages::screens::Sources,
        Source => crate::pages::screens::Source,
        Domains => crate::pages::screens::Domains,
        Domain => crate::pages::screens::Domain,
        PseudonymDomains => crate::pages::screens::PseudonymDomains,
        Audit => crate::pages::screens::Audit,
    }
);

group!(
    /// The pages beside the work: sign-in, the configuration, About and
    /// Your preferences.
    Aside {
        SignIn => crate::pages::sign_in::SignInPage,
        Configuration => crate::pages::screens::Configuration,
        About => crate::pages::about::AboutPage,
        Preferences => crate::pages::preferences::PreferencesPage,
    }
);
