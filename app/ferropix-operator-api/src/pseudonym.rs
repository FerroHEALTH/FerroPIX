// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudonymisation domains and the pseudonyms.
//!
//! `GET {base}/operator/pseudonym-domains` lists the domains with their
//! counts; behind the administrator scope, `POST` adds one and `PATCH
//! {base}/operator/pseudonym-domains/{id}` renames it, names another
//! consumer or retires it. `POST {base}/operator/pseudonyms` mints a
//! person's pseudonym in a domain when it has none, and answers whether it
//! minted, never the value, which the reveal shows. `POST
//! {base}/operator/pseudonyms/resolve` answers the person a pseudonym
//! resolves to, with the steward's reason, as a disclosure.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The most characters a reason holds.
pub const MAX_REASON: usize = 500;

/// One pseudonymisation domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PseudonymDomain {
    /// The domain's id.
    pub id: String,
    /// The namespace, the pseudonyms' system.
    pub namespace: String,
    /// The domain's name.
    pub name: String,
    /// The client id a pseudonym is minted for when it asks through ITI-83.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_client_id: Option<String>,
    /// When the domain was made, RFC 3339 UTC.
    pub created_at: String,
    /// When the domain was retired, RFC 3339 UTC; a retired domain mints
    /// nothing and answers as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<String>,
    /// The pseudonyms minted in it.
    pub pseudonyms: u64,
}

/// What `GET {base}/operator/pseudonym-domains` answers with, by namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PseudonymDomains {
    /// The domains.
    pub domains: Vec<PseudonymDomain>,
}

/// The body of `POST {base}/operator/pseudonym-domains`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewPseudonymDomain {
    /// The namespace, an absolute URI no identifier domain holds.
    pub namespace: String,
    /// The domain's name.
    pub name: String,
    /// The client id a pseudonym is minted for through ITI-83.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_client_id: Option<String>,
    /// Why, in the administrator's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The body of `PATCH {base}/operator/pseudonym-domains/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PseudonymDomainChange {
    /// The new name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The new consumer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_client_id: Option<String>,
    /// Whether the domain is retired, which cannot be undone.
    #[serde(default)]
    pub retire: bool,
    /// Why, in the administrator's words: required.
    pub reason: String,
}

/// The body of `POST {base}/operator/pseudonyms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mint {
    /// The person's id; a merged person is resolved to its survivor.
    pub person: String,
    /// The pseudonymisation domain's id.
    pub domain: String,
}

/// What `POST {base}/operator/pseudonyms` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Minted {
    /// The person the pseudonym belongs to: the survivor.
    pub person: String,
    /// The pseudonymisation domain's id.
    pub domain: String,
    /// Whether this request made it; `false` when the person had one.
    pub created: bool,
}

/// The body of `POST {base}/operator/pseudonyms/resolve`, which no `Debug`
/// rendering shows the pseudonym of.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolve {
    /// The pseudonymisation domain's namespace.
    pub namespace: String,
    /// The pseudonym.
    pub pseudonym: String,
    /// Why, in the steward's words: required, at most [`MAX_REASON`]
    /// characters.
    pub reason: String,
}

impl fmt::Debug for Resolve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolve")
            .field("namespace", &self.namespace)
            .field("reason", &self.reason.chars().count())
            .finish_non_exhaustive()
    }
}

/// What `POST {base}/operator/pseudonyms/resolve` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    /// The person the pseudonym resolves to: the survivor.
    pub person: String,
    /// The steward action that records the disclosure.
    pub action: String,
}

#[cfg(test)]
mod tests {
    use super::Resolve;

    #[test]
    fn a_resolve_shows_no_pseudonym_in_its_debug() {
        let resolve = Resolve {
            namespace: String::from("urn:oid:2.999.9.1"),
            pseudonym: String::from("3f0c2a5e-0000-4000-8000-000000000001"),
            reason: String::from("Synthfamily asked by phone"),
        };
        assert!(!format!("{resolve:?}").contains("3f0c2a5e"));
    }
}
