// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HL7 v3 elements both gateways write: the Send Message Payload
//! wrapper of ITI TF-2 Appendix O.1.1 around a request and an answer, and
//! the data types inside it.

use jiff::Timestamp;
use quick_xml::Writer;
use uuid::Uuid;

use super::HL7;
use super::identifier::HomeCommunityId;
use crate::soap;

/// Which namespace an element is in, of the ones a gateway's reader looks at.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ns {
    Soap,
    Wsa,
    Hl7,
    Other,
}

/// The namespaces a gateway's reader tells apart besides the SOAP 1.2
/// envelope's, by their URIs: WS-Addressing (Appendix V) and HL7 v3.
pub(super) const NAMESPACES: &[(&str, Ns)] = &[(soap::ADDRESSING, Ns::Wsa), (HL7, Ns::Hl7)];

/// The writer of one message.
pub(super) type Out = Writer<Vec<u8>>;

/// The OID of the HL7 interaction and trigger event identifiers
/// (Table 3.55.4.1.2.3-1).
pub(super) const INTERACTION: &str = "2.16.840.1.113883.1.6";

/// The transmission wrapper's head (Appendix O Table O.1.1-1): the message
/// `id`, its `creationTime`, the `interactionId` `interaction`, the
/// `processingCode` `processing`, the `processingModeCode` `T` and the
/// `acceptAckCode` `accept_ack`.
pub(super) fn transmission(
    writer: &mut Out,
    (message, created): (Uuid, Timestamp),
    interaction: &str,
    processing: &str,
    accept_ack: &str,
) -> std::io::Result<()> {
    let id = message.to_string();
    let created = created.strftime("%Y%m%d%H%M%S+0000").to_string();
    ii(writer, "id", &id, None)?;
    writer
        .create_element("creationTime")
        .with_attribute(("value", created.as_str()))
        .write_empty()?;
    ii(writer, "interactionId", INTERACTION, Some(interaction))?;
    code(writer, "processingCode", processing)?;
    code(writer, "processingModeCode", "T")?;
    code(writer, "acceptAckCode", accept_ack)
}

/// An `II` element `name` with `root` and, when given, `extension`.
pub(super) fn ii(
    writer: &mut Out,
    name: &str,
    root: &str,
    extension: Option<&str>,
) -> std::io::Result<()> {
    let element = writer.create_element(name).with_attribute(("root", root));
    match extension {
        Some(extension) => element
            .with_attribute(("extension", extension))
            .write_empty()?,
        None => element.write_empty()?,
    };
    Ok(())
}

/// An element with one `code` attribute.
pub(super) fn code(writer: &mut Out, name: &str, value: &str) -> std::io::Result<()> {
    writer
        .create_element(name)
        .with_attribute(("code", value))
        .write_empty()?;
    Ok(())
}

/// A `receiver` or `sender` with its device `id`, a `nullFlavor` of `NI`
/// when none is known, and the device's organization when a community is
/// named (Appendix O.1.1, §3.55.4.1.2.4, §3.55.4.1.2.5).
pub(super) fn device(
    writer: &mut Out,
    (role, type_code): (&str, &str),
    device: Option<&str>,
    community: Option<&HomeCommunityId>,
) -> std::io::Result<()> {
    writer
        .create_element(role)
        .with_attribute(("typeCode", type_code))
        .write_inner_content(|writer| {
            writer
                .create_element("device")
                .with_attributes([("classCode", "DEV"), ("determinerCode", "INSTANCE")])
                .write_inner_content(|writer| {
                    match device.filter(|device| !device.is_empty()) {
                        Some(device) => ii(writer, "id", device, None)?,
                        None => {
                            writer
                                .create_element("id")
                                .with_attribute(("nullFlavor", "NI"))
                                .write_empty()?;
                        }
                    }
                    if let Some(community) = community {
                        writer
                            .create_element("asAgent")
                            .with_attribute(("classCode", "AGNT"))
                            .write_inner_content(|writer| {
                                writer
                                    .create_element("representedOrganization")
                                    .with_attributes([
                                        ("classCode", "ORG"),
                                        ("determinerCode", "INSTANCE"),
                                    ])
                                    .write_inner_content(|writer| {
                                        ii(writer, "id", community.oid().as_str(), None)
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
