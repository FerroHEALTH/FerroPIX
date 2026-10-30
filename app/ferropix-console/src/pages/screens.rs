// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pages that read the operator API (`docs/architecture.md` §12.4).
//!
//! They are the dashboard, the review queue and one pair, a patient with
//! its merge and unmerge, the sources, the identifier domains, the
//! pseudonymisation domains, the configuration and the audit trail.
//!
//! Each draws its header as the design does, and under it the document the
//! gate read for it ([`crate::document`]), or the state the FerroPIX server
//! left it in ([`super::server_state`]): a page never draws a row the
//! server did not send. An id in a route is checked before the server is
//! asked: a person, a candidate and a domain are UUIDs FerroPIX minted, and
//! a source is a short name; anything else is the not-found page, so no
//! business identifier is ever read from an address.

use ferrohealth_ui::html::Html;
use ferrohealth_ui::view::action::ActionGroup;
use ferrohealth_ui::view::surface::{Crumb, Header, page_heading};

use crate::app::{DOMAINS, QUEUE, SEARCH, SOURCES};
use crate::locale::messages::Msg;
use crate::model::{Document, Read};
use crate::pages::not_found::not_found;
use crate::pages::{Drawn, Needs, server_state};
use crate::visit::Visit;

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

/// One page that reads the operator API: its header and its state.
struct Screen<'a> {
    /// The visit the page is drawn for.
    visit: &'a Visit,
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
    /// The page drawn from the server's document, when the server sent it.
    body: Option<Html>,
    /// The docked decision bar, when the page has one.
    dock: Option<Html>,
}

impl<'a> Screen<'a> {
    /// The screen titled `title` with `subtitle`, reading `route` for `what`.
    fn new(
        visit: &'a Visit,
        title: Msg,
        subtitle: Msg,
        what: Msg,
        route: impl Into<String>,
    ) -> Self {
        Self {
            visit,
            title: visit.tr(title),
            subtitle: visit.tr(subtitle),
            crumbs: Vec::new(),
            mono: false,
            needs: Needs::of(what),
            route: route.into(),
            body: None,
            dock: None,
        }
    }

    /// This screen under the ancestors `crumbs`.
    fn under(self, crumbs: Vec<Crumb>) -> Self {
        Self { crumbs, ..self }
    }

    /// The page drawn from `read` by `draw`, or the refusal of the call
    /// that did not bring the document.
    fn drawn<T>(&self, read: &Read<T>, draw: impl FnOnce(&T) -> Html) -> Html {
        match read {
            Ok(document) => draw(document),
            Err(probe) => server_state(self.visit, Some(probe), self.needs, &self.route),
        }
    }

    /// The page, drawn in the shell.
    fn view(self) -> Drawn {
        let visit = self.visit;
        let heading = page_heading(
            &visit.locale,
            Header {
                title: self.title.clone().into(),
                subtitle: Some(self.subtitle.into()),
                subtitle_view: None,
                crumbs: self.crumbs,
                mono: self.mono,
                badge: None,
            },
            ActionGroup::none(),
        );
        let state = self.body.unwrap_or_else(|| {
            server_state(visit, visit.page.probe.as_ref(), self.needs, &self.route)
        });
        Drawn {
            dock: self.dock,
            ..Drawn::new(self.title, heading.then(state))
        }
    }
}

/// The dashboard at `/` (design 01, 16).
#[must_use]
pub fn dashboard(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::Dashboard,
        Msg::DashboardSub,
        Msg::WhatDashboard,
        "GET /operator/overview",
    );
    screen.needs.away = true;
    if let Some(Document::Dashboard {
        overview,
        dependencies,
    }) = &visit.page.document
    {
        screen.body = Some(crate::pages::dashboard::dashboard(
            visit,
            overview,
            dependencies,
        ));
    }
    screen.view()
}

/// The review queue at `/queue` (design 02, 02.9).
#[must_use]
pub fn queue(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::Queue,
        Msg::QueueSub,
        Msg::WhatQueue,
        "GET /operator/candidates",
    );
    if let Some(Document::Queue {
        filter,
        candidates,
        deferred,
    }) = &visit.page.document
    {
        screen.body = Some(screen.drawn(candidates, |candidates| {
            crate::pages::queue::queue(visit, filter, candidates, deferred.as_ref())
        }));
    }
    screen.view()
}

/// One pair to compare and decide at `/queue/{candidate}` (design 03).
#[must_use]
pub fn pair(visit: &Visit) -> Drawn {
    let Some(candidate) = visit.param("candidate").filter(|id| is_uuid(id)) else {
        return not_found(visit);
    };
    let mut screen = Screen::new(
        visit,
        Msg::Pair,
        Msg::PairSub,
        Msg::WhatPair,
        format!("GET /operator/candidates/{candidate}"),
    )
    .under(vec![Crumb::new(visit.tr(Msg::Queue), QUEUE)]);
    if let Some(Document::Pair { candidate, outcome }) = &visit.page.document {
        match candidate {
            Ok(candidate) => {
                let (body, dock) = crate::pages::pair::pair(visit, candidate, outcome.as_ref());
                screen.body = Some(body);
                screen.dock = dock;
            }
            Err(probe) => {
                screen.body = Some(server_state(
                    visit,
                    Some(probe),
                    screen.needs,
                    &screen.route,
                ));
            }
        }
    }
    screen.view()
}

/// The page of a patient's merge (`unmerge` false) or unmerge.
fn merge_screen(visit: &Visit, unmerge: bool) -> Drawn {
    let Some(person) = visit.param("person").filter(|id| is_uuid(id)) else {
        return not_found(visit);
    };
    let (title, subtitle, what, route) = if unmerge {
        (
            Msg::Unmerge,
            Msg::UnmergeSub,
            Msg::WhatUnmerge,
            format!("GET /operator/persons/{person}"),
        )
    } else {
        (
            Msg::Merge,
            Msg::MergeSub,
            Msg::WhatMerge,
            String::from("GET /operator/merges/preview"),
        )
    };
    let crumbs = vec![
        Crumb::new(visit.tr(Msg::Search), SEARCH),
        Crumb::new(
            visit.tr_with(Msg::PersonId, &[("person", person.clone().into())]),
            format!("/patients/{person}"),
        ),
    ];
    let mut screen = Screen::new(visit, title, subtitle, what, route).under(crumbs);
    screen.body = match &visit.page.document {
        Some(Document::Merge {
            person: id,
            preview,
            refused,
        }) if !unmerge => Some(crate::pages::merge::merge(
            visit,
            id,
            preview.as_ref(),
            refused.as_ref(),
        )),
        Some(Document::Unmerge {
            person: id,
            merge,
            preview,
            refused,
        }) if unmerge => Some(crate::pages::merge::unmerge(
            visit,
            id,
            merge.as_ref(),
            preview.as_ref(),
            refused.as_ref(),
        )),
        _ => None,
    };
    screen.view()
}

/// A patient at `/patients/{person}` (design 05, 05.10).
#[must_use]
pub fn patient(visit: &Visit) -> Drawn {
    let Some(person) = visit.param("person").filter(|id| is_uuid(id)) else {
        return not_found(visit);
    };
    let mut screen = Screen::new(
        visit,
        Msg::Patient,
        Msg::PatientSub,
        Msg::WhatPatient,
        format!("GET /operator/persons/{person}"),
    )
    .under(vec![Crumb::new(visit.tr(Msg::Search), SEARCH)]);
    let tab = visit.query_word("tab").unwrap_or_default();
    if let Some(Document::Person { person, revealed }) = &visit.page.document {
        screen.body = Some(screen.drawn(person, |found| {
            crate::pages::patient::patient(visit, found, tab, revealed.as_ref())
        }));
    }
    screen.title = visit.tr_with(Msg::PersonId, &[("person", person.into())]);
    screen.mono = true;
    screen.view()
}

/// The merge of a patient at `/patients/{person}/merge` (design 08).
#[must_use]
pub fn merge(visit: &Visit) -> Drawn {
    merge_screen(visit, false)
}

/// The unmerge of a patient at `/patients/{person}/unmerge` (design 05.7).
#[must_use]
pub fn unmerge(visit: &Visit) -> Drawn {
    merge_screen(visit, true)
}

/// The sources at `/sources` (design 33).
#[must_use]
pub fn sources(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::Sources,
        Msg::SourcesSub,
        Msg::WhatSources,
        "GET /operator/sources",
    );
    if let Some(Document::Sources(read)) = &visit.page.document {
        screen.body = Some(screen.drawn(read, |sources| {
            crate::pages::registry::sources(visit, sources)
        }));
    }
    screen.view()
}

/// One source at `/sources/{source}` (design 33.1).
#[must_use]
pub fn source(visit: &Visit) -> Drawn {
    let Some(source) = visit.param("source").filter(|name| is_name(name)) else {
        return not_found(visit);
    };
    let mut screen = Screen::new(
        visit,
        Msg::Sources,
        Msg::SourceSub,
        Msg::WhatSource,
        format!("GET /operator/sources/{source}"),
    )
    .under(vec![Crumb::new(visit.tr(Msg::Sources), SOURCES)]);
    if let Some(Document::Source(read)) = &visit.page.document {
        screen.body = Some(screen.drawn(read, |page| crate::pages::registry::source(visit, page)));
    }
    screen.title = source;
    screen.mono = true;
    screen.view()
}

/// The identifier domains at `/domains` (design 10).
#[must_use]
pub fn domains(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::Domains,
        Msg::DomainsSub,
        Msg::WhatDomains,
        "GET /operator/domains",
    );
    if let Some(Document::Domains { domains, written }) = &visit.page.document {
        screen.body = Some(screen.drawn(domains, |domains| {
            crate::pages::registry::domains(visit, domains, written.as_ref(), visit.page.admin)
        }));
    }
    screen.view()
}

/// One identifier domain at `/domains/{domain}` (design 20).
#[must_use]
pub fn domain(visit: &Visit) -> Drawn {
    let Some(domain) = visit.param("domain").filter(|id| is_uuid(id)) else {
        return not_found(visit);
    };
    let mut screen = Screen::new(
        visit,
        Msg::Domains,
        Msg::DomainSub,
        Msg::WhatDomain,
        format!("GET /operator/domains/{domain}"),
    )
    .under(vec![Crumb::new(visit.tr(Msg::Domains), DOMAINS)]);
    let mut title = domain;
    if let Some(Document::Domain { page, written }) = &visit.page.document {
        if let Ok(page) = page {
            title.clone_from(&page.domain.name);
        }
        screen.body = Some(screen.drawn(page, |page| {
            crate::pages::registry::domain(visit, page, written.as_ref(), visit.page.admin)
        }));
    }
    screen.title = title;
    screen.mono = true;
    screen.view()
}

/// The pseudonymisation domains at `/pseudonym-domains` (design 11).
#[must_use]
pub fn pseudonym_domains(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::PseudonymDomains,
        Msg::PseudonymDomainsSub,
        Msg::WhatPseudonymDomains,
        "GET /operator/pseudonym-domains",
    );
    if let Some(Document::PseudonymDomains {
        domains,
        written,
        resolved,
    }) = &visit.page.document
    {
        screen.body = Some(screen.drawn(domains, |domains| {
            crate::pages::pseudonyms::pseudonym_domains(
                visit,
                domains,
                written.as_ref(),
                resolved.as_ref(),
                visit.page.admin,
            )
        }));
    }
    screen.view()
}

/// The configuration at `/configuration` (design 25).
#[must_use]
pub fn configuration(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::Configuration,
        Msg::ConfigurationSub,
        Msg::WhatConfiguration,
        "GET /operator/configuration",
    );
    if let Some(Document::Configuration(read)) = &visit.page.document {
        screen.body = Some(screen.drawn(read, |configuration| {
            crate::pages::registry::configuration(visit, configuration)
        }));
    }
    screen.view()
}

/// The audit trail at `/audit` (design 13).
#[must_use]
pub fn audit(visit: &Visit) -> Drawn {
    let mut screen = Screen::new(
        visit,
        Msg::Audit,
        Msg::AuditSub,
        Msg::WhatAudit,
        "GET /operator/actions",
    );
    if let Some(Document::Trail { filter, actions }) = &visit.page.document {
        screen.body = Some(screen.drawn(actions, |actions| {
            crate::pages::trail::trail(visit, filter, actions)
        }));
    }
    screen.view()
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
