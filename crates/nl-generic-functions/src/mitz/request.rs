// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The closed authorization question on the wire: a SOAP 1.2 envelope with
//! its WS-Addressing headers around one XACML 3.0 `XACMLAuthzDecisionQuery`
//! (Implementatiehandleiding Open en gesloten autorisatievraag 3.8.2
//! §3.2.4.2 and the example of §3.2.4.4).

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesText, Event};
use secrecy::ExposeSecret as _;
use url::Url;
use uuid::Uuid;

use super::question::{ClosedQuestion, ProfessionalId};
use super::{
    ACTION, BSN_ROOT, CARE_PROVIDER_TYPE_SYSTEM, DATA_CATEGORY_SYSTEM, HL7, PURPOSE_SYSTEM,
    ROLE_SYSTEM, SOAP, URA_ROOT, WSA, XACML, XACML_SAML_PROTOCOL,
};

/// The WS-Addressing anonymous address: the answer comes back on the
/// request's own connection.
const ANONYMOUS: &str = "http://www.w3.org/2005/08/addressing/anonymous";

/// The XACML 3.0 attribute categories the question fills (§3.2.4.2).
const RESOURCE: &str = "urn:oasis:names:tc:xacml:3.0:attribute-category:resource";
const ACTION_CATEGORY: &str = "urn:oasis:names:tc:xacml:3.0:attribute-category:action";
const SUBJECT: &str = "urn:oasis:names:tc:xacml:1.0:subject-category:access-subject";
const ENVIRONMENT: &str = "urn:oasis:names:tc:xacml:3.0:attribute-category:environment";

/// The attribute identifiers of the question (§3.2.4.2).
pub(super) const PATIENT_ID: &str = "urn:oasis:names:tc:xacml:2.0:resource:resource-id";
pub(super) const HOLDER_TYPE: &str =
    "urn:ihe:iti:appc:2016:document-entry:healthcare-facility-type-code";
pub(super) const HOLDER_ID: &str = "urn:ihe:iti:appc:2016:author-institution:id";
pub(super) const CATEGORY: &str = "urn:ihe:iti:appc:2016:document-entry:event-code";
const ROLE: &str = "urn:oasis:names:tc:xacml:2.0:subject:role";
const MANDATED: &str = "urn:nl:otv:names:tc:1.0:subject:mandated";
const RESPONSIBLE: &str = "urn:ihe:iti:xua:2017:subject:provider-identifier";
const USER_ID: &str = "urn:nl:otv:names:tc:1.0:subject:provider-institution";
const USER_TYPE: &str = "urn:nl:otv:names:tc:1.0:subject:consulting-healthcare-facility-type-code";
const PURPOSE: &str = "urn:oasis:names:tc:xspa:1.0:subject:purposeofuse";

/// The XACML data types of the HL7 V3 values (§3.2.4.2).
const II: &str = "urn:hl7-org:v3#II";
const CV: &str = "urn:hl7-org:v3#CV";

type Out = Writer<Vec<u8>>;

/// The SOAP 1.2 envelope of `question` to `endpoint`, its WS-Addressing
/// `MessageID` the `urn:uuid:` form of `message` (§3.2.4.4, §6).
///
/// The envelope holds the BSN, so it is handed to the HTTP client and never
/// kept, logged or put into an error.
pub(super) fn envelope(
    question: &ClosedQuestion,
    endpoint: &Url,
    message: Uuid,
) -> std::io::Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    writer
        .create_element("soap:Envelope")
        .with_attributes([("xmlns:soap", SOAP), ("xmlns:wsa", WSA)])
        .write_inner_content(|writer| {
            writer
                .create_element("soap:Header")
                .write_inner_content(|writer| header(writer, endpoint, message))?;
            writer
                .create_element("soap:Body")
                .write_inner_content(|writer| query(writer, question))?;
            Ok(())
        })?;
    Ok(writer.into_inner())
}

/// The WS-Addressing headers of §3.2.4.4, with the `MessageID` §6 equates
/// with the `X-Request-Id`.
fn header(writer: &mut Out, endpoint: &Url, message: Uuid) -> std::io::Result<()> {
    // NOTE: Implementatiehandleiding §3.2.4.2 holds the question to ITI TF-2x Appendix V,
    // under which every wsa:Action carries mustUnderstand (IHE-WSA101).
    writer
        .create_element("wsa:Action")
        .with_attribute(("soap:mustUnderstand", "1"))
        .write_text_content(BytesText::new(ACTION))?;
    writer
        .create_element("wsa:MessageID")
        .write_text_content(BytesText::new(&format!("urn:uuid:{message}")))?;
    writer
        .create_element("wsa:To")
        .write_text_content(BytesText::new(endpoint.as_str()))?;
    writer
        .create_element("wsa:ReplyTo")
        .write_inner_content(|writer| {
            writer
                .create_element("wsa:Address")
                .write_text_content(BytesText::new(ANONYMOUS))?;
            Ok(())
        })?;
    Ok(())
}

/// The `XACMLAuthzDecisionQuery` with its one `Request`: the resource, one
/// action per data category, the subject and the environment (§3.2.4.2).
fn query(writer: &mut Out, question: &ClosedQuestion) -> std::io::Result<()> {
    writer
        .create_element("xacml-samlp:XACMLAuthzDecisionQuery")
        .with_attributes([
            ("xmlns:xacml-samlp", XACML_SAML_PROTOCOL),
            ("xmlns:xacml", XACML),
            ("xmlns:hl7", HL7),
        ])
        .write_inner_content(|writer| {
            writer
                .create_element("xacml:Request")
                .with_attributes([
                    ("ReturnPolicyIdList", "false"),
                    ("CombinedDecision", "false"),
                ])
                .write_inner_content(|writer| {
                    resource(writer, question)?;
                    for (index, category) in question.categories().iter().enumerate() {
                        let id = format!("action{index}");
                        attributes(writer, ACTION_CATEGORY, &id, |writer| {
                            coded(
                                writer,
                                (CATEGORY, true),
                                category.as_str(),
                                DATA_CATEGORY_SYSTEM,
                            )
                        })?;
                    }
                    subject(writer, question)?;
                    attributes(writer, ENVIRONMENT, "environment", |writer| {
                        coded(
                            writer,
                            (PURPOSE, false),
                            question.purpose().code(),
                            PURPOSE_SYSTEM,
                        )
                    })?;
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(())
}

/// The resource: the patient by BSN, and the data holder by category and
/// URA, each returned in the result so a decision names what it is about.
fn resource(writer: &mut Out, question: &ClosedQuestion) -> std::io::Result<()> {
    let holder = question.holder();
    attributes(writer, RESOURCE, "resource", |writer| {
        identified(
            writer,
            (PATIENT_ID, true),
            BSN_ROOT,
            question.patient().value().expose_secret(),
        )?;
        coded(
            writer,
            (HOLDER_TYPE, true),
            holder.kind().as_str(),
            CARE_PROVIDER_TYPE_SYSTEM,
        )?;
        identified(
            writer,
            (HOLDER_ID, true),
            URA_ROOT,
            holder.organisation().as_str(),
        )
    })
}

/// The subject: the responsible professional's role and identification, the
/// person consulting under mandate when there is one, and the data user by
/// category and URA.
fn subject(writer: &mut Out, question: &ClosedQuestion) -> std::io::Result<()> {
    let user = question.user();
    attributes(writer, SUBJECT, "subject", |writer| {
        coded(writer, (ROLE, true), user.role().as_str(), ROLE_SYSTEM)?;
        professional(writer, (RESPONSIBLE, true), user.responsible())?;
        if let Some(mandated) = user.mandated() {
            professional(writer, (MANDATED, false), mandated)?;
        }
        coded(
            writer,
            (USER_TYPE, false),
            user.kind().as_str(),
            CARE_PROVIDER_TYPE_SYSTEM,
        )?;
        identified(
            writer,
            (USER_ID, false),
            URA_ROOT,
            user.organisation().as_str(),
        )
    })
}

/// One `Attributes` element of `category`, with `xml:id` `id`.
fn attributes(
    writer: &mut Out,
    category: &str,
    id: &str,
    content: impl FnOnce(&mut Out) -> std::io::Result<()>,
) -> std::io::Result<()> {
    writer
        .create_element("xacml:Attributes")
        .with_attributes([("Category", category), ("xml:id", id)])
        .write_inner_content(content)?;
    Ok(())
}

/// One `Attribute` named `id`, returned in the result when `included`, around
/// one value of `data_type`.
fn attribute(
    writer: &mut Out,
    (id, included): (&str, bool),
    data_type: &str,
    value: impl FnOnce(&mut Out) -> std::io::Result<()>,
) -> std::io::Result<()> {
    writer
        .create_element("xacml:Attribute")
        .with_attributes([
            ("AttributeId", id),
            ("IncludeInResult", if included { "true" } else { "false" }),
        ])
        .write_inner_content(|writer| {
            writer
                .create_element("xacml:AttributeValue")
                .with_attribute(("DataType", data_type))
                .write_inner_content(value)?;
            Ok(())
        })?;
    Ok(())
}

/// An HL7 V3 `II` attribute: `extension` under `root`.
fn identified(
    writer: &mut Out,
    named: (&str, bool),
    root: &str,
    extension: &str,
) -> std::io::Result<()> {
    attribute(writer, named, II, |writer| {
        writer
            .create_element("hl7:InstanceIdentifier")
            .with_attributes([("root", root), ("extension", extension)])
            .write_empty()?;
        Ok(())
    })
}

/// A professional's identification as an HL7 V3 `II` attribute.
fn professional(writer: &mut Out, named: (&str, bool), id: &ProfessionalId) -> std::io::Result<()> {
    identified(writer, named, id.root(), id.extension())
}

/// An HL7 V3 `CV` attribute: `code` of `system`.
fn coded(writer: &mut Out, named: (&str, bool), code: &str, system: &str) -> std::io::Result<()> {
    attribute(writer, named, CV, |writer| {
        writer
            .create_element("hl7:CodedValue")
            .with_attributes([("code", code), ("codeSystem", system)])
            .write_empty()?;
        Ok(())
    })
}
