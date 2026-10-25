// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator API documents a page is drawn from, read before the render.
//!
//! The `gate` middleware reads the document of the page a request asks for
//! once, with the steward's own token, and hands it to the render in the
//! page's context, so a page reads nothing while it renders
//! (`.claude/rules/leptos-ui.md` §1). A page whose document the server
//! refused or did not send draws that call's refusal in place. A posted
//! form (a search, a trail filter, a reveal) is read the same way by its
//! handler and drawn at the same address, so nothing a steward typed
//! reaches an address. No specification governs the console: our own
//! design.

use ferropix_operator_api::actions::{ActionFilter, Actions};
use ferropix_operator_api::configuration::Configuration;
use ferropix_operator_api::domain::{DomainPage, Domains};
use ferropix_operator_api::health::Dependencies;
use ferropix_operator_api::overview::Overview;
use ferropix_operator_api::person::Person;
use ferropix_operator_api::reveal::Revealed;
use ferropix_operator_api::search::Results;
use ferropix_operator_api::source::{SourcePage, Sources};

use crate::pix::{AccessToken, Pix, Probe};

/// A document the server sent, or the call that did not bring it.
pub type Read<T> = Result<T, Probe>;

/// Which search a results page answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// By identifier.
    Identifier,
    /// By demographics.
    Demographics,
}

/// What the top bar's field carried into the demographics form, read by
/// the shape of what was typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefill {
    /// A family name.
    pub family: Option<String>,
    /// A birth date, `YYYY-MM-DD`.
    pub born: Option<String>,
}

/// The document of one page.
#[derive(Debug, Clone)]
pub enum Document {
    /// The dashboard: the overview and the readiness of the server's parts.
    Dashboard {
        /// `GET /operator/overview`.
        overview: Read<Overview>,
        /// `GET /operator/dependencies`.
        dependencies: Read<Dependencies>,
    },
    /// A person, `GET /operator/persons/{id}`, with a reveal the steward
    /// just asked for.
    Person {
        /// The person.
        person: Read<Box<Person>>,
        /// The value a reveal unmasked, or the call that refused it.
        revealed: Option<Read<Revealed>>,
    },
    /// The sources, `GET /operator/sources`.
    Sources(Read<Sources>),
    /// One source, `GET /operator/sources/{id}`.
    Source(Read<SourcePage>),
    /// The identifier domains, `GET /operator/domains`, with a domain just
    /// created.
    Domains {
        /// The domains.
        domains: Read<Domains>,
        /// The outcome of the create just posted.
        written: Option<Read<()>>,
    },
    /// One identifier domain, `GET /operator/domains/{id}`, with a change
    /// just posted.
    Domain {
        /// The domain.
        page: Read<DomainPage>,
        /// The outcome of the change just posted.
        written: Option<Read<()>>,
    },
    /// The configuration, `GET /operator/configuration`.
    Configuration(Read<Configuration>),
    /// The audit trail under a filter, `GET /operator/actions`.
    Trail {
        /// The filter the steward chose.
        filter: ActionFilter,
        /// The page of actions.
        actions: Read<Actions>,
    },
    /// The search page: the domains its identifier field chooses from and,
    /// after a post, the results.
    Search {
        /// The identifier domains.
        domains: Read<Domains>,
        /// The results of the search just posted.
        results: Option<(Segment, Read<Results>)>,
        /// What the top bar's field carried into the demographics form.
        prefill: Prefill,
    },
}

/// Reads the document of the page at `path`, `None` for a page that reads
/// none.
pub async fn load(pix: &Pix, token: &AccessToken, path: &str) -> Option<Document> {
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let document = match segments.as_slice() {
        [""] => Document::Dashboard {
            overview: pix.read("operator/overview", token).await,
            dependencies: pix.read("operator/dependencies", token).await,
        },
        ["patients", person] if crate::pages::screens::is_uuid(person) => Document::Person {
            person: pix.read(&format!("operator/persons/{person}"), token).await,
            revealed: None,
        },
        ["sources"] => Document::Sources(pix.read("operator/sources", token).await),
        ["sources", source] if crate::pages::screens::is_name(source) => {
            Document::Source(pix.read(&format!("operator/sources/{source}"), token).await)
        }
        ["domains"] => Document::Domains {
            domains: pix.read("operator/domains?limit=200", token).await,
            written: None,
        },
        ["domains", domain] if crate::pages::screens::is_uuid(domain) => Document::Domain {
            page: pix.read(&format!("operator/domains/{domain}"), token).await,
            written: None,
        },
        ["configuration"] => {
            Document::Configuration(pix.read("operator/configuration", token).await)
        }
        ["audit"] => trail(pix, token, ActionFilter::default()).await,
        ["search"] | ["search", "demographics"] => Document::Search {
            domains: pix.read("operator/domains?limit=200", token).await,
            results: None,
            prefill: Prefill::default(),
        },
        _ => return None,
    };
    Some(document)
}

/// Reads the page of the audit trail `filter` names.
pub async fn trail(pix: &Pix, token: &AccessToken, filter: ActionFilter) -> Document {
    let query = {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in [
            ("actor", filter.actor.as_deref()),
            ("kind", filter.kind.as_deref()),
            ("person", filter.person.as_deref()),
            ("from", filter.from.as_deref()),
            ("to", filter.to.as_deref()),
            ("after", filter.after.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                query.append_pair(name, value);
            }
        }
        query.finish()
    };
    let route = if query.is_empty() {
        String::from("operator/actions")
    } else {
        format!("operator/actions?{query}")
    };
    let actions = pix.read(&route, token).await;
    Document::Trail { filter, actions }
}
