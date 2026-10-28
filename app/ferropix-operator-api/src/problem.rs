// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The RFC 9457 problem details document the server refuses an operator
//! call with (<https://www.rfc-editor.org/rfc/rfc9457>), as the console
//! reads it.
//!
//! The server defines its problem types under [`BASE`], the book page that
//! documents each one; a problem whose status says all there is to say is
//! `about:blank`. No member carries a value of the request (RFC 9457 §5).

use serde::{Deserialize, Serialize};

/// The media type of a problem details document (RFC 9457 §3).
pub const MEDIA_TYPE: &str = "application/problem+json";

/// The base URI of every FerroPIX problem type: the book page with one
/// section per type, the type's slug its fragment.
pub const BASE: &str = "https://ferropix.eu/docs/operate/problems.html#";

/// The `type` of a problem that adds nothing to its status, and the value
/// of an absent `type` (RFC 9457 §4.2.1).
pub const ABOUT_BLANK: &str = "about:blank";

/// A problem details object: the members RFC 9457 §3.1 defines, each
/// optional on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Problem {
    /// `type`: the URI of the problem type; `about:blank` when absent
    /// (RFC 9457 §3.1.1).
    #[serde(rename = "type")]
    pub kind: String,
    /// `title`: the type's summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `status`: the HTTP status of the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// `detail`: what to correct, in a fixed sentence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// `instance`: the occurrence, the server's request id as a `urn:uuid:`
    /// URI on a refusal of the server's stack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl Default for Problem {
    fn default() -> Self {
        Self {
            kind: String::from(ABOUT_BLANK),
            title: None,
            status: None,
            detail: None,
            instance: None,
        }
    }
}

impl Problem {
    /// The slug of a FerroPIX problem type, the fragment of its `type`
    /// under [`BASE`]; `None` for `about:blank` or another type.
    #[must_use]
    pub fn slug(&self) -> Option<&str> {
        self.kind.strip_prefix(BASE).filter(|slug| !slug.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{ABOUT_BLANK, Problem};

    #[test]
    fn a_problem_reads_its_members_and_its_slug() {
        let problem: Problem = serde_json::from_str(
            r#"{"type":"https://ferropix.eu/docs/operate/problems.html#stale","title":"Stale","status":409,"detail":"reload it"}"#,
        )
        .expect("a problem");
        assert_eq!(Some("stale"), problem.slug());
        assert_eq!(Some(409), problem.status);
        assert_eq!(Some("reload it"), problem.detail.as_deref());
        assert_eq!(None, problem.instance);
    }

    #[test]
    fn an_absent_type_is_about_blank() {
        let problem: Problem =
            serde_json::from_str(r#"{"status":413,"title":"Payload Too Large"}"#)
                .expect("a problem");
        assert_eq!(ABOUT_BLANK, problem.kind);
        assert_eq!(None, problem.slug());
    }
}
