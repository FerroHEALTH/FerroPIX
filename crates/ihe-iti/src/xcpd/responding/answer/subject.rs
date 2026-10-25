// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The subjects of an answer: the `RegistrationEvent` of each matching
//! patient (Cases 1 and 2) and the `DetectedIssueEvent` of Cases 3 and 5.

use quick_xml::events::BytesText;
use secrecy::ExposeSecret as _;

use super::super::super::discovery::RequestedAttribute;
use super::super::super::error::DetectedIssue;
use super::super::super::{ISSUE_SYSTEM, REQUEST_SYSTEM};
use super::{ACT_CODE, Candidate, LOCATOR_SYSTEM, Out, Outcome, code, ii};

/// One `RegistrationEvent` with its `Patient` and its custodian's
/// `homeCommunityId` (§3.55.4.2.2.2, §3.55.4.2.2.4, §3.55.4.2.2.5).
pub(super) fn registration(writer: &mut Out, candidate: &Candidate) -> std::io::Result<()> {
    writer
        .create_element("subject")
        .with_attribute(("typeCode", "SUBJ"))
        .write_inner_content(|writer| {
            writer
                .create_element("registrationEvent")
                .with_attributes([("classCode", "REG"), ("moodCode", "EVN")])
                .write_inner_content(|writer| {
                    writer
                        .create_element("id")
                        .with_attribute(("nullFlavor", "NA"))
                        .write_empty()?;
                    code(writer, "statusCode", "active")?;
                    writer
                        .create_element("subject1")
                        .with_attribute(("typeCode", "SBJ"))
                        .write_inner_content(|writer| patient(writer, candidate))?;
                    writer
                        .create_element("custodian")
                        .with_attribute(("typeCode", "CST"))
                        .write_inner_content(|writer| {
                            writer
                                .create_element("assignedEntity")
                                .with_attribute(("classCode", "ASSIGNED"))
                                .write_inner_content(|writer| {
                                    writer
                                        .create_element("id")
                                        .with_attribute((
                                            "root",
                                            candidate.community.oid().as_str(),
                                        ))
                                        .write_empty()?;
                                    writer
                                        .create_element("code")
                                        .with_attributes([
                                            ("code", "NotHealthDataLocator"),
                                            ("codeSystem", LOCATOR_SYSTEM),
                                        ])
                                        .write_empty()?;
                                    Ok(())
                                })?;
                            Ok(())
                        })?;
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(())
}

/// The `Patient` role with its `Person`.
///
/// No `QueryMatchObservation` is written: Table 3.55.4.2.2.2-1 fixes no code
/// for it, and the quality of match is optional to the Initiating Gateway.
fn patient(writer: &mut Out, candidate: &Candidate) -> std::io::Result<()> {
    writer
        .create_element("patient")
        .with_attribute(("classCode", "PAT"))
        .write_inner_content(|writer| {
            let (authority, value) = &candidate.patient_id;
            ii(
                writer,
                "id",
                authority.as_str(),
                Some(value.expose_secret()),
            )?;
            code(writer, "statusCode", "active")?;
            writer
                .create_element("patientPerson")
                .with_attributes([("classCode", "PSN"), ("determinerCode", "INSTANCE")])
                .write_inner_content(|writer| person(writer, candidate))?;
            Ok(())
        })?;
    Ok(())
}

/// The `Person` of a candidate, with the other identifiers as `asOtherIDs`,
/// the `OtherIDs` class of Table 3.55.4.2.2.2-1.
fn person(writer: &mut Out, candidate: &Candidate) -> std::io::Result<()> {
    if candidate.names.is_empty() {
        writer
            .create_element("name")
            .with_attribute(("nullFlavor", "NA"))
            .write_empty()?;
    }
    for name in &candidate.names {
        writer
            .create_element("name")
            .write_inner_content(|writer| {
                for given in &name.given {
                    writer
                        .create_element("given")
                        .write_text_content(BytesText::new(given))?;
                }
                for family in &name.family {
                    writer
                        .create_element("family")
                        .write_text_content(BytesText::new(family))?;
                }
                Ok(())
            })?;
    }
    if let Some(gender) = &candidate.gender {
        writer
            .create_element("administrativeGenderCode")
            .with_attribute(("code", gender.as_str()))
            .write_empty()?;
    }
    if let Some(birth) = &candidate.birth_time {
        writer
            .create_element("birthTime")
            .with_attribute(("value", birth.as_str()))
            .write_empty()?;
    }
    for address in &candidate.addresses {
        writer
            .create_element("addr")
            .write_inner_content(|writer| {
                for line in &address.lines {
                    writer
                        .create_element("streetAddressLine")
                        .write_text_content(BytesText::new(line))?;
                }
                for (part, value) in [
                    ("city", &address.city),
                    ("state", &address.state),
                    ("postalCode", &address.postal_code),
                    ("country", &address.country),
                ] {
                    if let Some(value) = value {
                        writer
                            .create_element(part)
                            .write_text_content(BytesText::new(value))?;
                    }
                }
                Ok(())
            })?;
    }
    for (authority, value) in &candidate.other_ids {
        writer
            .create_element("asOtherIDs")
            .with_attribute(("classCode", "PAT"))
            .write_inner_content(|writer| {
                ii(
                    writer,
                    "id",
                    authority.as_str(),
                    Some(value.expose_secret()),
                )?;
                writer
                    .create_element("scopingOrganization")
                    .with_attributes([("classCode", "ORG"), ("determinerCode", "INSTANCE")])
                    .write_inner_content(|writer| ii(writer, "id", authority.as_str(), None))?;
                Ok(())
            })?;
    }
    Ok(())
}

/// The `DetectedIssueEvent` of Case 3 (§3.55.4.2.2.6) or Case 5
/// (§3.55.4.2.2.7); nothing for the other cases.
pub(super) fn detected_issue(writer: &mut Out, outcome: &Outcome) -> std::io::Result<()> {
    let (requested, issues): (&[RequestedAttribute], &[DetectedIssue]) = match outcome {
        Outcome::MoreAttributes(requested) if !requested.is_empty() => (requested, &[]),
        Outcome::Unable(issues) if !issues.is_empty() => (&[], issues),
        _ => return Ok(()),
    };
    writer
        .create_element("reasonOf")
        .with_attribute(("typeCode", "RSON"))
        .write_inner_content(|writer| {
            writer
                .create_element("detectedIssueEvent")
                .with_attributes([("classCode", "ALRT"), ("moodCode", "EVN")])
                .write_inner_content(|writer| {
                    writer
                        .create_element("code")
                        .with_attributes([
                            ("code", "_ActAdministrativeDetectedIssueManagementCode"),
                            ("codeSystem", ACT_CODE),
                        ])
                        .write_empty()?;
                    for attribute in requested {
                        let Some(code) = requested_code(*attribute) else {
                            continue;
                        };
                        writer
                            .create_element("triggerFor")
                            .with_attribute(("typeCode", "TRIG"))
                            .write_inner_content(|writer| {
                                writer
                                    .create_element("actOrderRequired")
                                    .with_attributes([("classCode", "ACT"), ("moodCode", "RQO")])
                                    .write_inner_content(|writer| {
                                        writer
                                            .create_element("code")
                                            .with_attributes([
                                                ("code", code),
                                                ("codeSystem", REQUEST_SYSTEM),
                                            ])
                                            .write_empty()?;
                                        Ok(())
                                    })?;
                                Ok(())
                            })?;
                    }
                    for issue in issues {
                        let Some(code) = issue_code(*issue) else {
                            continue;
                        };
                        writer
                            .create_element("mitigatedBy")
                            .with_attribute(("typeCode", "MITGT"))
                            .write_inner_content(|writer| {
                                writer
                                    .create_element("detectedIssueManagement")
                                    .with_attributes([("classCode", "ACT"), ("moodCode", "EVN")])
                                    .write_inner_content(|writer| {
                                        writer
                                            .create_element("code")
                                            .with_attributes([
                                                ("code", code),
                                                ("codeSystem", ISSUE_SYSTEM),
                                            ])
                                            .write_empty()?;
                                        Ok(())
                                    })?;
                                Ok(())
                            })?;
                    }
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(())
}

/// The code of Table 3.55.4.2.2.6-1 for `attribute`.
fn requested_code(attribute: RequestedAttribute) -> Option<&'static str> {
    Some(match attribute {
        RequestedAttribute::AdministrativeGender => "LivingSubjectAdministrativeGenderRequested",
        RequestedAttribute::PatientAddress => "PatientAddressRequested",
        RequestedAttribute::PatientTelecom => "PatientTelecomRequested",
        RequestedAttribute::BirthPlaceName => "LivingSubjectBirthPlaceNameRequested",
        RequestedAttribute::BirthPlaceAddress => "LivingSubjectBirthPlaceAddressRequested",
        RequestedAttribute::MothersMaidenName => "MothersMaidenNameRequested",
        RequestedAttribute::Other => return None,
    })
}

/// The code of Table 3.55.4.2.2.7-1 for `issue`.
fn issue_code(issue: DetectedIssue) -> Option<&'static str> {
    Some(match issue {
        DetectedIssue::ResponderBusy => "ResponderBusy",
        DetectedIssue::AnswerNotAvailable => "AnswerNotAvailable",
        DetectedIssue::InternalError => "InternalError",
        DetectedIssue::Other => return None,
    })
}
