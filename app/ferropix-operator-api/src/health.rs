// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator's health document: `GET {base}/operator/dependencies`, the
//! readiness document of `ferrohealth-health` with every detail, and the
//! last outcome of every upstream FerroPIX sends to.

use ferrohealth_health::document::{Readiness, State};
use serde::{Deserialize, Serialize};

/// What an upstream is to FerroPIX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamKind {
    /// The ATNA Audit Record Repository the BALP records go to.
    AuditRepository,
    /// A PMIR subscriber the ITI-93 feed goes to.
    Subscriber,
    /// An HL7 v2 Patient Identifier Cross-reference Consumer the ITI-10
    /// notifications go to.
    Consumer,
}

/// How the last exchange with an upstream ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// `up` when the upstream took the exchange, `down` when it did not.
    pub state: State,
    /// Why the exchange failed; absent when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Outcome {
    /// Returns the outcome of an exchange the upstream took.
    #[must_use]
    pub const fn up() -> Self {
        Self {
            state: State::Up,
            detail: None,
        }
    }

    /// Returns the outcome of an exchange that failed, with the reason.
    #[must_use]
    pub fn down(detail: impl Into<String>) -> Self {
        Self {
            state: State::Down,
            detail: Some(detail.into()),
        }
    }
}

/// The last exchange with one upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// What the upstream is.
    pub kind: UpstreamKind,
    /// Where it is reached: a URL without its userinfo or query.
    pub endpoint: String,
    /// How the last exchange ended, or `None` before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Outcome>,
    /// When the last exchange ended, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// What `GET {base}/operator/dependencies` answers with, behind the steward
/// scope so an anonymous reader learns no endpoint and no detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    /// The readiness document, each indicator with its detail.
    pub readiness: Readiness,
    /// Every upstream FerroPIX sends to, in a stable order.
    pub upstreams: Vec<Upstream>,
}

#[cfg(test)]
mod tests {
    use ferrohealth_health::document::{IndicatorState, Phase, Readiness, State};

    use super::{Dependencies, Outcome, Upstream, UpstreamKind};

    #[test]
    fn a_dependencies_document_writes_lower_case_states_and_omits_an_absent_detail() {
        let dependencies = Dependencies {
            readiness: Readiness::new(
                Phase::Serving,
                vec![
                    IndicatorState::up("database"),
                    IndicatorState::down("audit_spool", "the spool is full"),
                ],
            ),
            upstreams: vec![Upstream {
                kind: UpstreamKind::AuditRepository,
                endpoint: String::from("https://arr.example.org/fhir"),
                last: Some(Outcome::up()),
                at: None,
            }],
        };
        let text = serde_json::to_string(&dependencies).expect("it serializes");
        assert_eq!(
            r#"{"readiness":{"state":"down","phase":"serving","indicators":[{"name":"audit_spool","state":"down","detail":"the spool is full"},{"name":"database","state":"up"}]},"upstreams":[{"kind":"audit_repository","endpoint":"https://arr.example.org/fhir","last":{"state":"up"}}]}"#,
            text
        );
        let read: Dependencies = serde_json::from_str(&text).expect("it reads back");
        assert_eq!(dependencies, read);
        assert_eq!(State::Down, read.readiness.state);
        assert!(
            serde_json::from_str::<Outcome>(r#"{"state":"up","extra":1}"#).is_err(),
            "an unknown field is refused"
        );
    }
}
