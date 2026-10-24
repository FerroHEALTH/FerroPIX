// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-104 messages as the Manager reads them.
//!
//! The conditional update `PUT [base]/Patient?identifier=system|value` that
//! adds or revises a patient (§2:3.104.4.1.2) or resolves a duplicate
//! (§2:3.104.4.2.2), and the conditional delete of the Remove Patient Option
//! (§2:3.104.4.3.2), each on FHIR R4's conditional interactions
//! (<http://hl7.org/fhir/R4/http.html#cond-update>).

use std::fmt;

use fhir_types::codec::{Json, Path, Value};
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::patient::Patient;
use http::StatusCode;
use secrecy::{ExposeSecret, SecretString};

use crate::outcome::IssueType;
use crate::pixm::identifier::SourceIdentifier;
use crate::server::answer::{Answer, Severity};
use crate::server::format::{BodyError, Format};
use crate::server::token;
use ihe_atna::redact::REDACTED;

/// The `link.type` of a Resolve Duplicate Patient message (§2:3.104.4.2.2).
const REPLACED_BY: &str = "replaced-by";

/// One conditional update of ITI-104, as read.
///
/// `Debug` shows the identifiers' domains, never a value or the Patient; the
/// type has no `Display`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Feed {
    /// Add or Revise Patient (§2:3.104.4.1): the Source's patient as it now
    /// stands.
    Revise(Revision),
    /// Resolve Duplicate Patient (§2:3.104.4.2): the Source merged the
    /// patient the condition names into the survivor its `replaced-by` link
    /// names.
    ResolveDuplicate(Merge),
}

impl Feed {
    /// Reads a conditional update: the query of its URL (without the `?`)
    /// and its body in `format`, a `Patient`.
    ///
    /// The condition is the one `identifier` of the Source's domain
    /// (§2:3.104.4.1.3: "the identifier parameter needs to be used for the
    /// cross-referencing"); `_format` and `_pretty` may stand beside it
    /// (FHIR R4 http.html). The Patient is read with every element the PDQm
    /// Patient profile names (§2:3.104.4.1.2.1), held to that profile's
    /// constraints, and must carry the identifier of the condition
    /// (§2:3.104.4.1.2). One `link` of type `replaced-by` makes it the
    /// Resolve Duplicate Patient message, whose survivor is the link's
    /// `other.identifier` in the same domain (§2:3.104.4.2.1,
    /// §2:3.104.4.2.2).
    ///
    /// # Errors
    /// A [`FeedError`] for a message ITI-104 does not admit, whose
    /// [`refusal`](FeedError::refusal) is the answer to send.
    pub fn from_update(query: &str, format: Format, body: &[u8]) -> Result<Self, FeedError> {
        let condition = condition(query)?;
        let object = format.decode(body).map_err(FeedError::Body)?;
        if object.get("resourceType").and_then(Value::as_str) != Some("Patient") {
            return Err(FeedError::NotAPatient);
        }
        let patient = Patient::from_json(&object, &mut Path::root("Patient"))
            .map_err(|error| FeedError::Body(BodyError::decode(error)))?;
        profile(&patient)?;
        if !patient
            .identifier
            .iter()
            .any(|identifier| same(identifier, &condition))
        {
            return Err(FeedError::ConditionNotCarried);
        }
        let mut links = patient
            .link
            .iter()
            .filter(|link| link.r#type.value.as_deref() == Some(REPLACED_BY));
        let link = links.next();
        if links.next().is_some() {
            return Err(FeedError::SecondSurvivor);
        }
        // NOTE: PIXm §2:3.104.4.2.2, the link points at the surviving identifier,
        // so a link without a domain and a value names no survivor and is refused.
        let survivor = link
            .map(|link| {
                link.other
                    .identifier
                    .as_deref()
                    .and_then(|identifier| {
                        let system = identifier.system.as_ref()?.value.as_deref()?;
                        let value = identifier.value.as_ref()?.value.as_deref()?;
                        SourceIdentifier::new(system, SecretString::from(value)).ok()
                    })
                    .ok_or(FeedError::NoSurvivorIdentifier)
            })
            .transpose()?;
        let patient = Box::new(patient);
        let Some(survivor) = survivor else {
            return Ok(Self::Revise(Revision {
                identifier: condition,
                patient,
            }));
        };
        if survivor.system() != condition.system() {
            return Err(FeedError::SurvivorInAnotherDomain);
        }
        if survivor.value().expose_secret() == condition.value().expose_secret() {
            return Err(FeedError::SurvivorIsSubsumed);
        }
        Ok(Self::ResolveDuplicate(Merge {
            subsumed: condition,
            survivor,
            patient,
        }))
    }
}

/// An Add or Revise Patient message (§2:3.104.4.1).
#[derive(Clone)]
pub struct Revision {
    identifier: SourceIdentifier,
    patient: Box<Patient>,
}

impl Revision {
    /// Returns the identifier the condition names, in the Source's domain.
    #[must_use]
    pub fn identifier(&self) -> &SourceIdentifier {
        &self.identifier
    }

    /// Returns the Patient, as the Source sent it.
    #[must_use]
    pub fn patient(&self) -> &Patient {
        &self.patient
    }

    /// Returns the Patient, by value.
    #[must_use]
    pub fn into_patient(self) -> Box<Patient> {
        self.patient
    }
}

impl fmt::Debug for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Revision")
            .field("identifier", &self.identifier)
            .field("patient", &REDACTED)
            .finish()
    }
}

/// A Resolve Duplicate Patient message (§2:3.104.4.2): the Manager replaces
/// its references to the subsumed identifier with the survivor's
/// (§2:3.104.4.2.3).
#[derive(Clone)]
pub struct Merge {
    subsumed: SourceIdentifier,
    survivor: SourceIdentifier,
    patient: Box<Patient>,
}

impl Merge {
    /// Returns the subsumed identifier: the one the condition names.
    #[must_use]
    pub fn subsumed(&self) -> &SourceIdentifier {
        &self.subsumed
    }

    /// Returns the surviving identifier the `replaced-by` link names.
    #[must_use]
    pub fn survivor(&self) -> &SourceIdentifier {
        &self.survivor
    }

    /// Returns the subsumed patient's Patient, as the Source sent it.
    #[must_use]
    pub fn patient(&self) -> &Patient {
        &self.patient
    }
}

impl fmt::Debug for Merge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Merge")
            .field("subsumed", &self.subsumed)
            .field("survivor", &self.survivor)
            .field("patient", &REDACTED)
            .finish()
    }
}

/// A Remove Patient message (§2:3.104.4.3): the conditional delete on the
/// identifier the Source removed.
#[derive(Debug, Clone)]
pub struct Removal {
    identifier: SourceIdentifier,
}

impl Removal {
    /// Reads the query of a conditional delete's URL (without the `?`): one
    /// `identifier` of the Source's domain (§2:3.104.4.3.2), with `_format`
    /// and `_pretty` admitted beside it.
    ///
    /// # Errors
    /// A [`FeedError`] for a condition ITI-104 does not admit, whose
    /// [`refusal`](FeedError::refusal) is the answer to send.
    pub fn from_delete(query: &str) -> Result<Self, FeedError> {
        condition(query).map(|identifier| Self { identifier })
    }

    /// Returns the removed identifier, which the Manager no longer answers
    /// with (§2:3.104.4.3.3).
    #[must_use]
    pub fn identifier(&self) -> &SourceIdentifier {
        &self.identifier
    }
}

/// The identifier of a conditional interaction's query.
fn condition(query: &str) -> Result<SourceIdentifier, FeedError> {
    let mut found = None;
    for (name, value) in token::url_query(query) {
        match name.as_str() {
            "identifier" => {
                if found.is_some() || token::split(&value, ',').len() > 1 {
                    return Err(FeedError::SecondIdentifier);
                }
                let token = token::token(&value);
                let system = token
                    .system
                    .filter(|system| !system.is_empty())
                    .ok_or(FeedError::NoDomain)?;
                found = Some(
                    SourceIdentifier::new(system, token.code)
                        .map_err(|_invalid| FeedError::Identifier)?,
                );
            }
            "_format" | "_pretty" => {}
            _ => return Err(FeedError::UnknownParameter),
        }
    }
    found.ok_or(FeedError::NoCondition)
}

/// Holds `patient` to the constraints of the PDQm Patient profile: no
/// `implicitRules` and no `modifierExtension`, and at least one identifier,
/// each with a `system` and a `value` (§2:3.104.4.1.2.1).
fn profile(patient: &Patient) -> Result<(), FeedError> {
    if patient.implicit_rules.is_some() || !patient.modifier_extension.is_empty() {
        return Err(FeedError::Profile);
    }
    let complete = |identifier: &Identifier| {
        let present = |text: Option<&str>| text.is_some_and(|text| !text.is_empty());
        present(identifier.system.as_ref().and_then(|s| s.value.as_deref()))
            && present(identifier.value.as_ref().and_then(|v| v.value.as_deref()))
    };
    if patient.identifier.is_empty() || !patient.identifier.iter().all(complete) {
        return Err(FeedError::Profile);
    }
    Ok(())
}

/// Whether `identifier` is `condition`.
fn same(identifier: &Identifier, condition: &SourceIdentifier) -> bool {
    identifier.system.as_ref().and_then(|s| s.value.as_deref()) == Some(condition.system())
        && identifier.value.as_ref().and_then(|v| v.value.as_deref())
            == Some(condition.value().expose_secret())
}

/// Why a message is no ITI-104 message the Manager applies.
///
/// No variant carries a value from the request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FeedError {
    /// The URL names no `identifier` condition (§2:3.104.4.1.3).
    #[error("the request names no identifier condition")]
    NoCondition,
    /// The URL names more than one identifier, by repetition or by `,`.
    #[error("the condition names more than one identifier")]
    SecondIdentifier,
    /// The condition's identifier names no domain: a token without a system
    /// (§2:3.104.4.1.2).
    #[error("the condition's identifier names no domain")]
    NoDomain,
    /// The condition's identifier has a system that is no absolute URI, or
    /// no value.
    #[error("the condition's identifier is not a domain and a value")]
    Identifier,
    /// The URL names a parameter other than `identifier`, `_format` and
    /// `_pretty`.
    #[error("the condition names a parameter other than identifier")]
    UnknownParameter,
    /// The body is no FHIR resource.
    #[error("the body is no FHIR R4 resource")]
    Body(#[source] BodyError),
    /// The body is a resource other than `Patient`.
    #[error("the body is not a Patient resource")]
    NotAPatient,
    /// The Patient breaks a constraint of the PDQm Patient profile
    /// (§2:3.104.4.1.2.1).
    #[error("the Patient does not hold to the PDQm Patient profile")]
    Profile,
    /// The Patient does not carry the identifier the condition names
    /// (§2:3.104.4.1.2).
    #[error("the Patient does not carry the identifier of the condition")]
    ConditionNotCarried,
    /// The Patient carries more than one `replaced-by` link.
    #[error("the Patient carries more than one replaced-by link")]
    SecondSurvivor,
    /// The `replaced-by` link carries no identifier with a system and a
    /// value (§2:3.104.4.2.2).
    #[error("the replaced-by link carries no surviving identifier")]
    NoSurvivorIdentifier,
    /// The survivor is in another domain than the subsumed identifier: a
    /// Source resolves duplicates within its own domain (§2:3.104.4.2.1).
    #[error("the surviving identifier is in another domain")]
    SurvivorInAnotherDomain,
    /// The survivor is the subsumed identifier itself.
    #[error("the surviving identifier is the subsumed one")]
    SurvivorIsSubsumed,
}

impl FeedError {
    /// Returns the answer to send: a `400` whose issue names the fault.
    #[must_use]
    pub fn refusal(&self) -> Answer<OperationOutcome> {
        let (code, diagnostics) = match self {
            Self::NoCondition => (IssueType::Required, "the identifier condition is required"),
            Self::SecondIdentifier => (IssueType::Invalid, "the condition names one identifier"),
            Self::NoDomain => (
                IssueType::Invalid,
                "the identifier shall include the Patient Identifier Domain",
            ),
            Self::Identifier => (
                IssueType::Invalid,
                "the identifier is not a domain and a value",
            ),
            Self::UnknownParameter => (
                IssueType::NotSupported,
                "the condition is the identifier parameter alone",
            ),
            Self::Body(_) => (IssueType::Structure, "the body is no FHIR R4 resource"),
            Self::NotAPatient => (IssueType::Invalid, "the body is not a Patient resource"),
            Self::Profile => (
                IssueType::Invariant,
                "the Patient does not hold to the PDQm Patient profile",
            ),
            Self::ConditionNotCarried => (
                IssueType::Invalid,
                "the Patient does not carry the identifier of the condition",
            ),
            Self::SecondSurvivor => (
                IssueType::Invalid,
                "the Patient carries more than one replaced-by link",
            ),
            Self::NoSurvivorIdentifier => (
                IssueType::Required,
                "the replaced-by link shall carry the surviving identifier",
            ),
            Self::SurvivorInAnotherDomain => (
                IssueType::BusinessRule,
                "the surviving identifier is in another domain",
            ),
            Self::SurvivorIsSubsumed => (
                IssueType::BusinessRule,
                "the surviving identifier is the subsumed one",
            ),
        };
        Answer::outcome(StatusCode::BAD_REQUEST, Severity::Error, code, diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::{Feed, FeedError, Removal};
    use crate::server::format::Format;

    const CONDITION: &str = "identifier=urn:oid:2.999.1%7C42";

    #[test]
    fn a_patient_with_the_condition_is_a_revision() {
        let body = br#"{"resourceType":"Patient","identifier":[{"system":"urn:oid:2.999.1","value":"42"}],"active":true}"#;
        let Ok(Feed::Revise(revision)) = Feed::from_update(CONDITION, Format::Json, body) else {
            panic!("a revision");
        };
        assert_eq!(revision.identifier().value().expose_secret(), "42");
        assert!(
            !format!("{revision:?}").contains("42"),
            "Debug redacts the value"
        );
    }

    #[test]
    fn the_condition_is_one_identifier_with_a_domain() {
        for (query, error) in [
            ("", FeedError::NoCondition),
            ("identifier=42", FeedError::NoDomain),
            (
                "identifier=urn:oid:2.999.1%7C1,urn:oid:2.999.1%7C2",
                FeedError::SecondIdentifier,
            ),
            (
                "identifier=urn:oid:2.999.1%7C1&identifier=urn:oid:2.999.1%7C2",
                FeedError::SecondIdentifier,
            ),
            (
                "identifier=urn:oid:2.999.1%7C1&family=x",
                FeedError::UnknownParameter,
            ),
        ] {
            assert_eq!(Removal::from_delete(query).err(), Some(error), "{query:?}");
        }
    }

    #[test]
    fn the_fuzz_input_whose_narrative_changes_in_xml_is_refused() {
        let body = br#"{"resourceType":"Patient","id":"Patient-MohrAlice-Blue","meta":{"profile":["https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Patient"]},"text":{"status":"generated","div":"<div xmlns=\"http://www.w3.org/1999/xhtml\"><p class=\"res-header-id\"><b>Generated Narrative: Patient Patient-MohrAlice-Blue</b></p><a name=\"Patient-MohrAlice-Blue\"> </a><a name=\"hcPatient-MohrAlice-Blue\"> </a><div stylggggggggggggggggggggggggggggggggggggggggggggggggggggggggggge=\"display: inline-block; background-color: #d9e0e7; padding: 6px; margin: 4px; border: 1px solid #8da1b4; border-radius: 5px; line-height: 60%\"><p style=\"margin-bottom: 0px\"/><p style=\"margin-bottom: 0px\">Profile: <a href=\"StructureDefinition-IHE.PIXm.Patient.html\">PIXm Patient constraints for Feed</a></p></div><p style=\"border: 1px #661aff solid; background-color: #e6e6ff; padding: 10px;\">ALICE MOHR (official) Female, DoB: 1958-01-30 ( urn:oid:1.3.6.1.4.1.21367.13.20.3000#IHEBLUE-994)</p><hr/><table class=\"grid\"><tr><td style=\"background-color: #f3f5da\"%title=\"Record is active\">Active:</td><td colspan=\"3\">true</td></tr><tr><td style=\"background-color: #f3f5da\" title=\"Ways to contact the Patient\">Contact Detail</td><td colspan=\"3\">820 JORIE BLVD. OAK BROOK IL 60523 (home)</td></tr></table></div>"},"identifier":[{"system":"urn:oid:1.3.6.1.4.1.21367.13.20.3000","value":"IHEBLUE-994"}],"active":true,"name":[{"use":"official","family":"MOHR","given":["ALICE"]}],"gender":"female","birthDate":"1958-01-30","address":[{"use":"home","line":["820 JORIE BLVD."],"city":"OAK BROOK","state":"IL","postalCode":"60523"}]}"#;
        let query = "identifier=urn%3Aoid%3A1.3.6.1.4.1.21367.13.20.3000%7CIHEBLUE-994";
        assert!(
            matches!(
                Feed::from_update(query, Format::Json, body),
                Err(FeedError::Body(_))
            ),
            "a narrative whose XML form reads back changed is refused"
        );
    }
}
