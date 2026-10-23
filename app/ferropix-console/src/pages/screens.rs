// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pages that read the operator API (`docs/architecture.md` §12.4).
//!
//! They are the dashboard, the review queue and one pair, a patient with
//! its merge and unmerge, the sources, the identifier domains, the
//! pseudonymisation domains, the configuration and the audit trail.
//!
//! Each draws its header as the design does, and under it the state the
//! FerroPIX server leaves it in ([`super::server_state`]): the operator API
//! is #90 and the server it belongs to #57, so no page here draws a row the
//! server did not send. An id in a route is checked before the server is
//! asked: a person and a candidate are UUIDs FerroPIX minted, and a source
//! or a domain is a short name; anything else is the not-found page, so no
//! business identifier is ever read from an address.

use ferrohealth_ui::view::action::ActionGroup;
use ferrohealth_ui::view::surface::{Crumb, Header, page_heading};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::app::{DOMAINS, QUEUE, SEARCH, SOURCES, framed, page_context};
use crate::locale::{Msg, tr, tr_with};
use crate::pages::not_found::NotFound;
use crate::pages::{Needs, server_state, title};

/// Whether `id` reads as a UUID in its canonical text form (RFC 9562 §4),
/// the shape of every person and candidate id FerroPIX mints.
#[must_use]
pub fn is_uuid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.chars().all(|c| c.is_ascii_hexdigit())
        })
}

/// Whether `name` reads as the short name of a source or a domain: letters,
/// digits, `.`, `_` and `-`, at most 64 of them.
#[must_use]
pub fn is_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The route parameter `name`, when the route names one.
fn param(name: &str) -> Option<String> {
    use_params_map().with_untracked(|params| params.get(name))
}

/// One page that reads the operator API: its header and its state.
struct Screen {
    /// The title.
    title: String,
    /// The line under the title.
    subtitle: String,
    /// The page's ancestors.
    crumbs: Vec<Crumb>,
    /// Whether the title is an identifier, drawn in the code face.
    mono: bool,
    /// What the page needs from the server.
    needs: Needs,
    /// The operator API route it reads, as a steward may repeat it.
    route: String,
}

impl Screen {
    /// The screen titled `title` with `subtitle`, reading `route` for `what`.
    fn new(title: Msg, subtitle: Msg, what: Msg, route: impl Into<String>) -> Self {
        Self {
            title: tr(title),
            subtitle: tr(subtitle),
            crumbs: Vec::new(),
            mono: false,
            needs: Needs { what, away: false },
            route: route.into(),
        }
    }

    /// This screen under the ancestors `crumbs`.
    fn under(self, crumbs: Vec<Crumb>) -> Self {
        Self { crumbs, ..self }
    }

    /// The page, drawn in the shell.
    fn view(self) -> AnyView {
        let probe = page_context().probe;
        let heading = page_heading(
            Header {
                title: self.title.clone().into(),
                subtitle: Some(self.subtitle.into()),
                crumbs: self.crumbs,
                mono: self.mono,
                badge: None,
            },
            ActionGroup::none(),
        );
        let state = server_state(probe.as_ref(), self.needs, &self.route);
        framed(
            view! {
                {title(self.title)}
                {heading}
                {state}
            }
            .into_any(),
        )
    }
}

/// The dashboard at `/` (design 01, 16).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Dashboard() -> impl IntoView {
    // TODO(#92): draw the stat cards, the charts and the listeners from the overview.
    let mut screen = Screen::new(
        Msg::Dashboard,
        Msg::DashboardSub,
        Msg::WhatDashboard,
        "GET /operator/overview",
    );
    screen.needs.away = true;
    screen.view()
}

/// The review queue at `/queue` (design 02, 02.9).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Queue() -> impl IntoView {
    // TODO(#109): draw the work-list of candidates.
    Screen::new(
        Msg::Queue,
        Msg::QueueSub,
        Msg::WhatQueue,
        "GET /operator/candidates",
    )
    .view()
}

/// One pair to compare and decide at `/queue/{candidate}` (design 03).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Pair() -> impl IntoView {
    let Some(candidate) = param("candidate").filter(|id| is_uuid(id)) else {
        return view! { <NotFound /> }.into_any();
    };
    // TODO(#110): draw the record compare, the score breakdown and the decision bar.
    Screen::new(
        Msg::Pair,
        Msg::PairSub,
        Msg::WhatPair,
        format!("GET /operator/candidates/{candidate}"),
    )
    .under(vec![Crumb::new(tr(Msg::Queue), QUEUE)])
    .view()
}

/// The patient page of `person`, or of a merge or unmerge from it.
fn patient_screen(title: Msg, subtitle: Msg, what: Msg, route: &str, below: bool) -> AnyView {
    let Some(person) = param("person").filter(|id| is_uuid(id)) else {
        return view! { <NotFound /> }.into_any();
    };
    let patient = format!("/patients/{person}");
    let mut crumbs = vec![Crumb::new(tr(Msg::Search), SEARCH)];
    if below {
        crumbs.push(Crumb::new(
            tr_with(Msg::PersonId, &[("person", person.clone().into())]),
            patient,
        ));
    }
    let mut screen =
        Screen::new(title, subtitle, what, route.replace("{person}", &person)).under(crumbs);
    if !below {
        screen.title = tr_with(Msg::PersonId, &[("person", person.into())]);
        screen.mono = true;
    }
    screen.view()
}

/// A patient at `/patients/{person}` (design 05, 05.10).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Patient() -> impl IntoView {
    // TODO(#94): draw the identifiers, the records side by side, the history and the pseudonyms.
    patient_screen(
        Msg::Patient,
        Msg::PatientSub,
        Msg::WhatPatient,
        "GET /operator/persons/{person}",
        false,
    )
}

/// The merge of a patient at `/patients/{person}/merge` (design 08).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Merge() -> impl IntoView {
    // TODO(#111): draw the merge preview and its confirmation.
    patient_screen(
        Msg::Merge,
        Msg::MergeSub,
        Msg::WhatMerge,
        "GET /operator/merges/preview",
        true,
    )
}

/// The unmerge of a patient at `/patients/{person}/unmerge` (design 05.7).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Unmerge() -> impl IntoView {
    // TODO(#111): draw the latest merge, the preview of its revert and the confirmation.
    patient_screen(
        Msg::Unmerge,
        Msg::UnmergeSub,
        Msg::WhatUnmerge,
        "GET /operator/persons/{person}",
        true,
    )
}

/// The sources at `/sources` (design 33).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Sources() -> impl IntoView {
    // TODO(#120): draw the sources from the operator API.
    Screen::new(
        Msg::Sources,
        Msg::SourcesSub,
        Msg::WhatSources,
        "GET /operator/sources",
    )
    .view()
}

/// One source at `/sources/{source}` (design 33.1).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Source() -> impl IntoView {
    let Some(source) = param("source").filter(|name| is_name(name)) else {
        return view! { <NotFound /> }.into_any();
    };
    // TODO(#120): draw the source's definition and its last messages.
    let mut screen = Screen::new(
        Msg::Sources,
        Msg::SourceSub,
        Msg::WhatSource,
        format!("GET /operator/sources/{source}"),
    )
    .under(vec![Crumb::new(tr(Msg::Sources), SOURCES)]);
    screen.title = source;
    screen.mono = true;
    screen.view()
}

/// The identifier domains at `/domains` (design 10).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Domains() -> impl IntoView {
    // TODO(#95): draw the table of identifier domains.
    Screen::new(
        Msg::Domains,
        Msg::DomainsSub,
        Msg::WhatDomains,
        "GET /operator/domains",
    )
    .view()
}

/// One identifier domain at `/domains/{domain}` (design 20).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Domain() -> impl IntoView {
    let Some(domain) = param("domain").filter(|name| is_name(name)) else {
        return view! { <NotFound /> }.into_any();
    };
    // TODO(#95): draw the domain's definition, its feed activity and its last feeds.
    let mut screen = Screen::new(
        Msg::Domains,
        Msg::DomainSub,
        Msg::WhatDomain,
        format!("GET /operator/domains/{domain}"),
    )
    .under(vec![Crumb::new(tr(Msg::Domains), DOMAINS)]);
    screen.title = domain;
    screen.mono = true;
    screen.view()
}

/// The pseudonymisation domains at `/pseudonym-domains` (design 11).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn PseudonymDomains() -> impl IntoView {
    // TODO(#116): draw the pseudonymisation domains and the resolve form.
    Screen::new(
        Msg::PseudonymDomains,
        Msg::PseudonymDomainsSub,
        Msg::WhatPseudonymDomains,
        "GET /operator/pseudonym-domains",
    )
    .view()
}

/// The configuration at `/configuration` (design 25).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Configuration() -> impl IntoView {
    // TODO(#121): draw the running revision, its tabs and its history.
    Screen::new(
        Msg::Configuration,
        Msg::ConfigurationSub,
        Msg::WhatConfiguration,
        "GET /operator/configuration",
    )
    .view()
}

/// The audit trail at `/audit` (design 13).
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn Audit() -> impl IntoView {
    // TODO(#96): draw the steward actions with their filters and export.
    Screen::new(
        Msg::Audit,
        Msg::AuditSub,
        Msg::WhatAudit,
        "GET /operator/actions",
    )
    .view()
}

#[cfg(test)]
mod tests {
    use super::{is_name, is_uuid};

    #[test]
    fn a_route_id_is_a_uuid_or_a_short_name_and_never_a_business_identifier() {
        assert!(is_uuid("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"));
        assert!(!is_uuid("123456789"), "a nine-digit number names no person");
        assert!(!is_uuid("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5"));
        assert!(!is_uuid("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5g"));
        assert!(is_name("lab-lis"));
        assert!(is_name("hospital.mrn_2"));
        assert!(!is_name(""));
        assert!(!is_name("a b"));
        assert!(!is_name(&"x".repeat(65)));
    }
}
