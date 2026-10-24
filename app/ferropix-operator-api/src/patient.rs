// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The values several documents share: a person's demographics, a masked
//! identifier, the domain it belongs to and a match grade.
//!
//! Every type that holds a name, a birth date, an address, a telecom value or
//! an identifier writes a `Debug` that shows none of them, so a document
//! logged by mistake leaks nothing.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The text a `Debug` rendering shows in place of a value.
const REDACTED: &str = "***";

/// One name of a person.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Name {
    /// The family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// The given names, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub given: Vec<String>,
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Name")
            .field("family", &self.family.as_ref().map(|_| REDACTED))
            .field("given", &self.given.len())
            .finish()
    }
}

/// One address of a person.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Address {
    /// The street lines, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// The city.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// The postal code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    /// The country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Address")
            .field("lines", &self.lines.len())
            .finish_non_exhaustive()
    }
}

/// One telecom value of a person: a telephone number or an email address.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telecom {
    /// The FHIR R4 `ContactPoint.system`: `phone`, `email` and the others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// The value.
    pub value: String,
}

impl fmt::Debug for Telecom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Telecom")
            .field("system", &self.system)
            .field("value", &REDACTED)
            .finish()
    }
}

/// The demographics one record states, the elements of the PDQm Patient
/// profile the store keeps.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demographics {
    /// The names, the leading one first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<Name>,
    /// The birth date as FHIR writes it, at its precision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth_date: Option<String>,
    /// The FHIR R4 administrative gender code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    /// The addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<Address>,
    /// The telephone numbers and email addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub telecom: Vec<Telecom>,
}

impl fmt::Debug for Demographics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Demographics")
            .field("names", &self.names.len())
            .field("birth_date", &self.birth_date.as_ref().map(|_| REDACTED))
            .field("gender", &self.gender)
            .field("addresses", &self.addresses.len())
            .field("telecom", &self.telecom.len())
            .finish()
    }
}

/// An identifier shown masked: its last three characters, at most half of
/// it, behind a `•` for every other character (`docs/architecture.md` §12.5).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Masked(pub String);

impl fmt::Debug for Masked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Masked(***)")
    }
}

/// What an identifier domain is, as the store spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainKind {
    /// A local numbering, such as a hospital's patient numbers.
    Local,
    /// An authoritative numbering, such as the BSN, which links on agreement.
    Authoritative,
    /// The Master Patient Identification Domain.
    Master,
}

/// The domain an identifier or a record belongs to, as a list row names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRef {
    /// The domain's id.
    pub id: String,
    /// The domain's system, the FHIR `Identifier.system`.
    pub system: String,
    /// The domain's name.
    pub name: String,
    /// What the domain is.
    pub kind: DomainKind,
}

/// The grade of a match score against the `[matching]` thresholds, the
/// PDQm 3.2.0 `match-grade` codes a candidate can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Grade {
    /// Below the possible threshold.
    CertainlyNot,
    /// At or above `possible_at`.
    Possible,
    /// At or above `review_at`.
    Probable,
    /// At or above `link_at`.
    Certain,
}

/// Whether a person is the cross-reference or was merged into another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonStatus {
    /// The person is active.
    Active,
    /// The person was merged into a survivor.
    Merged,
}

/// The channel a record version arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// The FHIR face, ITI-104.
    Fhir,
    /// The HL7 v2 face, ITI-8.
    V2,
    /// The PMIR feed, ITI-93.
    Pmir,
}

#[cfg(test)]
mod tests {
    use super::{Address, Demographics, Masked, Name, Telecom};

    #[test]
    fn no_debug_rendering_shows_a_name_a_birth_date_an_address_or_an_identifier() {
        let demographics = Demographics {
            names: vec![Name {
                family: Some(String::from("Synthfamily")),
                given: vec![String::from("Synthgiven")],
            }],
            birth_date: Some(String::from("1970-01-01")),
            gender: Some(String::from("other")),
            addresses: vec![Address {
                lines: vec![String::from("Synthstreet 1")],
                city: Some(String::from("Synthcity")),
                postal_code: Some(String::from("9999 ZZ")),
                country: None,
            }],
            telecom: vec![Telecom {
                system: Some(String::from("phone")),
                value: String::from("+31600000001"),
            }],
        };
        let shown = format!("{demographics:?} {:?}", Masked(String::from("••••••123")));
        for value in [
            "Synthfamily",
            "Synthgiven",
            "1970-01-01",
            "Synthstreet",
            "Synthcity",
            "9999 ZZ",
            "+31600000001",
            "123",
        ] {
            assert!(!shown.contains(value), "{value} in {shown}");
        }
    }

    #[test]
    fn an_empty_demographics_document_writes_an_empty_object() {
        let text = serde_json::to_string(&Demographics::default()).expect("it serializes");
        assert_eq!("{}", text);
        assert!(serde_json::from_str::<Demographics>(r#"{"extra":1}"#).is_err());
    }
}
