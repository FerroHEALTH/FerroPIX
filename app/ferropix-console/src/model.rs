// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a page is drawn from, in both halves: the calls the console made to
//! the FerroPIX server and how they went, and the operator API documents
//! they brought back.
//!
//! The server reads them for the first page and the browser takes them
//! serialized with it; after a navigation the browser asks the console's
//! server for the next page's again ([`crate::app::page_now`]), so both
//! halves draw a page from the same values. No specification governs the
//! console: our own design.

use ferrohealth_ui::widget::session::Left;
use ferropix_operator_api::actions::{ActionFilter, Actions};
use ferropix_operator_api::candidate::{Candidate, CandidateQuery, Candidates};
use ferropix_operator_api::configuration::Configuration;
use ferropix_operator_api::domain::{DomainPage, Domains};
use ferropix_operator_api::health::Dependencies;
use ferropix_operator_api::overview::Overview;
use ferropix_operator_api::person::Person;
use ferropix_operator_api::pseudonym::{PseudonymDomains, Resolution};
use ferropix_operator_api::reveal::Revealed;
use ferropix_operator_api::search::Results;
use ferropix_operator_api::source::{SourcePage, Sources};
use http::StatusCode;
use serde::{Deserialize, Serialize};

/// An HTTP status, carried as its number.
mod status {
    use http::StatusCode;
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    /// Writes `status` as its number.
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` calls the writer with a reference to the field"
    )]
    pub(super) fn serialize<S: Serializer>(status: &StatusCode, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_u16(status.as_u16())
    }

    /// Reads a status from its number.
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<StatusCode, D::Error> {
        StatusCode::from_u16(u16::deserialize(input)?).map_err(D::Error::custom)
    }
}

/// Why no answer came back from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    /// The connection was refused or could not be made.
    Connect,
    /// No answer came within the configured timeout.
    Timeout,
    /// The answer could not be read, or the call failed another way.
    Other,
}

/// How the server answered a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reach {
    /// It answered `2xx`.
    Up,
    /// It answered with this other status.
    Refused(#[serde(with = "status")] StatusCode),
    /// No answer came.
    Unreachable(Failure),
}

/// One call to the server and how it went: the route, the outcome, the
/// moment it was made and the request id it carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    /// The route the call took, method and path, as a steward may repeat it.
    pub route: String,
    /// How the server answered.
    pub reach: Reach,
    /// When the call was made, as RFC 3339 text in UTC.
    pub at: String,
    /// The request id the call carried.
    pub request_id: String,
    /// The issues of the `OperationOutcome` a refusal carried, whose
    /// diagnostics the server writes without a value from the request.
    pub issues: Vec<ServerIssue>,
}

/// One issue of the `OperationOutcome` the server refused a call with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerIssue {
    /// `severity`.
    pub severity: String,
    /// `code`.
    pub code: String,
    /// `diagnostics`.
    #[serde(default)]
    pub diagnostics: Option<String>,
}

impl Probe {
    /// Whether the server answered `2xx`.
    #[must_use]
    pub fn is_up(&self) -> bool {
        self.reach == Reach::Up
    }
}

/// The `CapabilityStatement` the server declares, as it was sent, or the
/// call that did not bring one back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    /// The document, as the server sent it.
    Declared(String),
    /// The call that did not answer with one.
    Missing(Probe),
}

/// A document the server sent, or the call that did not bring it.
pub type Read<T> = Result<T, Probe>;

/// Which search a results page answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Segment {
    /// By identifier.
    Identifier,
    /// By demographics.
    Demographics,
}

/// What the top bar's field carried into the demographics form, read by
/// the shape of what was typed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefill {
    /// A family name.
    pub family: Option<String>,
    /// A birth date, `YYYY-MM-DD`.
    pub born: Option<String>,
}

/// What a decision posted on a pair came to, when it did not lead on to
/// the next pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionOutcome {
    /// The server refused it: a `409` when the pair changed under the page.
    Refused(Probe),
    /// It was not sent: a decision needs a reason.
    NoReason,
}

/// The document of one page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Document {
    /// The dashboard: the overview and the readiness of the server's parts.
    Dashboard {
        /// `GET /operator/overview`.
        overview: Read<Overview>,
        /// `GET /operator/dependencies`.
        dependencies: Read<Dependencies>,
    },
    /// The review queue under a filter, `GET /operator/candidates`, with a
    /// deferral just posted.
    Queue {
        /// The filter and the page the steward chose.
        filter: CandidateQuery,
        /// The page of candidates.
        candidates: Read<Candidates>,
        /// The outcome of the deferral just posted.
        deferred: Option<Read<()>>,
    },
    /// One pair to compare and decide, `GET /operator/candidates/{id}`,
    /// with what a decision just posted came to.
    Pair {
        /// The candidate and its evidence.
        candidate: Read<Box<Candidate>>,
        /// What the decision just posted came to.
        outcome: Option<DecisionOutcome>,
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
    /// The pseudonymisation domains, `GET /operator/pseudonym-domains`,
    /// with a write or a resolve just posted.
    PseudonymDomains {
        /// The domains.
        domains: Read<PseudonymDomains>,
        /// The outcome of the create or change just posted.
        written: Option<Read<()>>,
        /// The person a pseudonym just posted resolves to, or the refusal.
        resolved: Option<Read<Resolution>>,
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

/// What a page is drawn from, which the server read before the render.
///
/// It holds the steward's name, the outcome of the FerroPIX server's
/// liveness route, on About its `CapabilityStatement`, and the prefix of
/// the preference cookies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageContext {
    /// The address the context was read for, its path and its query.
    pub path: String,
    /// Where the page sends the browser instead of drawing: the sign-in,
    /// for a request with no live session.
    pub redirect: Option<String>,
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
    pub document: Option<Document>,
    /// Whether the provider granted the steward the server's administrator
    /// scope, so the page draws an administrator's forms.
    pub admin: bool,
    /// The request's preference cookies alone, as one `Cookie` header
    /// value; never the session's cookie.
    pub preferences: String,
    /// The tag of the page's locale.
    pub locale: String,
    /// The tag of the locale the browser asks for, which Your preferences
    /// names as the browser's own.
    pub browser: String,
    /// The FerroPIX server's base address, which the status pill names.
    pub base: String,
    /// The host of the OpenID Provider stewards sign in at.
    pub provider: String,
    /// Whether the console has an OpenID Provider to sign stewards in at.
    pub sign_in: bool,
    /// Today's year, month and day in UTC on the console's server, the last
    /// day the birth date's picker allows.
    pub today: Option<(u16, u8, u8)>,
    /// What the steward's session had left as the server drew the page,
    /// which the session watch counts down from; `None` with no session.
    pub session: Option<Left>,
    /// On a sign-in page, the console's own page its `return` names, which
    /// the sign-in leads back to.
    pub back: Option<String>,
}

impl PageContext {
    /// The context of a page drawn with no session, the sign-in page.
    #[must_use]
    pub const fn signed_out(prefix: String) -> Self {
        Self {
            path: String::new(),
            redirect: None,
            operator: None,
            probe: None,
            capability: None,
            prefix,
            document: None,
            admin: false,
            preferences: String::new(),
            locale: String::new(),
            browser: String::new(),
            base: String::new(),
            provider: String::new(),
            sign_in: false,
            today: None,
            session: None,
            back: None,
        }
    }
}
