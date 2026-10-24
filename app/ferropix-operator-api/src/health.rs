// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The health documents: `GET {base}/health/readiness`, the state of every
//! indicator, and `GET {base}/operator/dependencies`, the same report with
//! the last outcome of every upstream FerroPIX sends to.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The state of one indicator, or of the aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// The subsystem serves.
    Up,
    /// The subsystem does not serve.
    Down,
}

/// The outcome of one indicator check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndicatorState {
    /// Whether the subsystem serves.
    pub state: State,
    /// Why it does not, when it does not: never a body, a credential, an
    /// identifier or an endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl IndicatorState {
    /// Returns the state of a subsystem that serves.
    #[must_use]
    pub const fn up() -> Self {
        Self {
            state: State::Up,
            detail: None,
        }
    }

    /// Returns the state of a subsystem that does not serve, with the reason.
    #[must_use]
    pub fn down(detail: impl Into<String>) -> Self {
        Self {
            state: State::Down,
            detail: Some(detail.into()),
        }
    }
}

/// What `GET {base}/health/readiness` answers with: `200` while the
/// aggregate is up and `503` while it is down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    /// The aggregate: down while any indicator is down.
    pub state: State,
    /// Every indicator by name, in name order.
    pub indicators: BTreeMap<String, IndicatorState>,
}

/// What an upstream is to FerroPIX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamKind {
    /// The ATNA Audit Record Repository the BALP records go to.
    AuditRepository,
    /// A PMIR subscriber the ITI-93 feed goes to.
    Subscriber,
}

/// The last exchange with one upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// What the upstream is.
    pub kind: UpstreamKind,
    /// Where it is reached: a URL without its userinfo or query.
    pub endpoint: String,
    /// The state of the last exchange, or `None` before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<IndicatorState>,
    /// When the last exchange ended, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// What `GET {base}/operator/dependencies` answers with, behind the steward
/// scope so an anonymous reader learns no endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    /// The readiness report.
    pub readiness: Readiness,
    /// Every upstream FerroPIX sends to, in a stable order.
    pub upstreams: Vec<Upstream>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{IndicatorState, Readiness, State};

    #[test]
    fn a_readiness_document_writes_its_states_in_lower_case_and_omits_an_absent_detail() {
        let readiness = Readiness {
            state: State::Down,
            indicators: BTreeMap::from([
                (String::from("database"), IndicatorState::up()),
                (
                    String::from("audit_spool"),
                    IndicatorState::down("the spool is full"),
                ),
            ]),
        };
        let text = serde_json::to_string(&readiness).expect("it serializes");
        assert_eq!(
            r#"{"state":"down","indicators":{"audit_spool":{"state":"down","detail":"the spool is full"},"database":{"state":"up"}}}"#,
            text
        );
        let read: Readiness = serde_json::from_str(&text).expect("it reads back");
        assert_eq!(readiness, read);
        assert!(
            serde_json::from_str::<Readiness>(r#"{"state":"up","indicators":{},"extra":1}"#)
                .is_err()
        );
    }
}
