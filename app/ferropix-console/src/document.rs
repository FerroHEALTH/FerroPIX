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

use ferropix_operator_api::actions::ActionFilter;
use ferropix_operator_api::candidate::CandidateQuery;

use crate::model::{Document, Prefill};
use crate::pix::{AccessToken, Pix};

/// Reads the document of the page at `path`, `None` for a page that reads
/// none.
pub async fn load(pix: &Pix, token: &AccessToken, path: &str) -> Option<Document> {
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let document = match segments.as_slice() {
        [""] => Document::Dashboard {
            overview: pix.read("operator/overview", token).await,
            dependencies: pix.read("operator/dependencies", token).await,
        },
        ["queue"] => Document::Queue {
            filter: CandidateQuery::default(),
            candidates: pix.read("operator/candidates", token).await,
            deferred: None,
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
