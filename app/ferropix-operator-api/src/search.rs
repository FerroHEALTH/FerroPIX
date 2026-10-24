// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The two searches, by identifier and by demographics.
//!
//! `POST {base}/operator/search/identifier` finds one identifier in one
//! domain, and `POST {base}/operator/search/demographics` runs the scorer
//! over what the steward knows. Both are posted, so no value
//! sits in a URL, and both answer [`Results`].

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::patient::{Demographics, DomainRef, Grade, Masked, PersonStatus};

/// The persons a page holds when the request names no limit.
pub const DEFAULT_LIMIT: u32 = 20;

/// The most persons a page holds, whatever the request asks.
pub const MAX_LIMIT: u32 = 50;

/// The body of `POST {base}/operator/search/identifier`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentifierSearch {
    /// The domain's system.
    pub system: String,
    /// The identifier, as the steward typed it.
    pub value: String,
    /// The persons to skip, in id order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// The persons a page holds, at most [`MAX_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl fmt::Debug for IdentifierSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentifierSearch")
            .field("system", &self.system)
            .finish_non_exhaustive()
    }
}

/// The body of `POST {base}/operator/search/demographics`: the fields the
/// steward knows, at least one, and the page.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DemographicsSearch {
    /// The family name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// The given name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
    /// The birth date, `YYYY-MM-DD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birth_date: Option<String>,
    /// The FHIR R4 administrative gender code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    /// The postal code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    /// The persons to skip, the most likely first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// The persons a page holds, at most [`MAX_LIMIT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl fmt::Debug for DemographicsSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DemographicsSearch")
            .field("offset", &self.offset)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

/// What both searches answer with: one page of persons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Results {
    /// Every person the search found.
    pub total: u32,
    /// The persons skipped before this page.
    pub offset: u32,
    /// The offset of the next page, when persons follow this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
    /// The persons of this page: by id for an identifier, the most likely
    /// first for demographics.
    pub persons: Vec<Hit>,
}

/// One person a search found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hit {
    /// The person's id.
    pub id: String,
    /// Whether the person is active or merged.
    pub status: PersonStatus,
    /// The demographics the PDQm Supplier serves for the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading: Option<Demographics>,
    /// The person's identifiers, masked, ordered by domain system.
    pub identifiers: Vec<MaskedIdentifier>,
    /// The scorer's best score of the person's records, for a demographics
    /// search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// The grade of that score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<Grade>,
}

/// One identifier of a found person, masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskedIdentifier {
    /// The identifier's domain.
    pub domain: DomainRef,
    /// The identifier, masked.
    pub masked: Masked,
}

#[cfg(test)]
mod tests {
    use super::{DemographicsSearch, IdentifierSearch};

    #[test]
    fn a_search_body_shows_no_value_in_its_debug_and_refuses_an_unknown_field() {
        let by_identifier = IdentifierSearch {
            system: String::from("urn:oid:2.999.1.1"),
            value: String::from("SYNTHETIC-4711"),
            offset: None,
            limit: None,
        };
        let by_demographics = DemographicsSearch {
            family: Some(String::from("Synthfamily")),
            ..DemographicsSearch::default()
        };
        let shown = format!("{by_identifier:?} {by_demographics:?}");
        assert!(!shown.contains("SYNTHETIC-4711"), "{shown}");
        assert!(!shown.contains("Synthfamily"), "{shown}");
        assert!(serde_json::from_str::<DemographicsSearch>(r#"{"telecom":"x"}"#).is_err());
        assert_eq!(
            DemographicsSearch::default(),
            serde_json::from_str::<DemographicsSearch>("{}").expect("an empty body reads")
        );
    }
}
