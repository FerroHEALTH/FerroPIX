// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The SAML 2.0 assertion of Cross-Enterprise User Assertion (XUA, ITI-40)
//! that a deployment supplies for an ITI-55 request.
//!
//! The crate signs nothing. A deployment's identity provider or security
//! token service issues and signs the assertion, and the client sends its
//! bytes unchanged inside a WS-Security `Security` header, so its XML
//! signature still verifies at the responding gateway. Whether a network
//! requires an assertion at all is the network's policy: ITI-55 itself
//! requires none (ITI TF-1 Table 27.1.3-1 groups the actors with ATNA and
//! CT only, and §27.4.2 leaves policy to each community).

use std::fmt;
use std::sync::Arc;

use quick_xml::NsReader;
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::name::{NamespaceResolver, ResolveResult};

use super::error::InvalidInput;
use crate::redact::REDACTED;

/// The SAML 2.0 assertion namespace (OASIS SAML 2.0 Core §2.3.3).
const SAML2: &str = "urn:oasis:names:tc:SAML:2.0:assertion";

/// A signed SAML 2.0 assertion, held as the bytes of its one `Assertion`
/// element.
///
/// It is a bearer credential and may name the user: `Debug` shows nothing
/// of it, and there is no `Display`.
#[derive(Clone)]
pub struct XuaAssertion(Arc<str>);

impl XuaAssertion {
    /// Reads an assertion from `xml`: one `saml2:Assertion` element, after an
    /// optional XML declaration, comments and white space, and before
    /// trailing comments and white space only.
    ///
    /// The element's bytes are kept exactly as written; the declaration and
    /// anything around the element are dropped, since the element is
    /// embedded inside the SOAP header.
    ///
    /// # Errors
    /// [`InvalidInput::Assertion`] when `xml` is not well formed, carries a
    /// document type declaration, or holds anything but one SAML 2.0
    /// `Assertion` element.
    pub fn new(xml: &str) -> Result<Self, InvalidInput> {
        let (start, end) = element_span(xml).ok_or(InvalidInput::Assertion)?;
        let element = xml.get(start..end).ok_or(InvalidInput::Assertion)?;
        Ok(Self(Arc::from(element)))
    }

    /// The `Assertion` element, as written.
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for XuaAssertion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("XuaAssertion").field(&REDACTED).finish()
    }
}

/// The byte span of the one SAML 2.0 `Assertion` element `xml` holds, or
/// `None` when it holds anything else.
///
/// The element is spliced into the SOAP header as written, so it must stand
/// on its own: well formed, every namespace prefix it uses declared inside
/// it (an undeclared one would bind to the envelope's own declarations once
/// embedded), every reference one XML predefines or a character reference,
/// and no document type declaration or processing instruction anywhere. No
/// specification governs the check: our own design.
fn element_span(xml: &str) -> Option<(usize, usize)> {
    let mut reader = NsReader::from_str(xml);
    let mut start = None;
    let mut end = None;
    let mut depth = 0_usize;
    loop {
        let before = usize::try_from(reader.buffer_position()).ok()?;
        let (namespace, event) = reader.read_resolved_event().ok()?;
        if matches!(namespace, ResolveResult::Unknown(_)) {
            return None;
        }
        let saml = matches!(namespace, ResolveResult::Bound(ns) if ns.0 == SAML2);
        if let Event::Start(element) | Event::Empty(element) = &event {
            if !attributes_declared(reader.resolver(), element) {
                return None;
            }
            if depth == 0 {
                start = Some(assertion_start(before, end.is_some(), saml, element)?);
            }
        }
        match event {
            Event::Start(_) => depth = depth.checked_add(1)?,
            Event::End(_) => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    end = Some(usize::try_from(reader.buffer_position()).ok()?);
                }
            }
            Event::Empty(_) if depth == 0 => {
                end = Some(usize::try_from(reader.buffer_position()).ok()?);
            }
            Event::Text(text) if depth == 0 => {
                if !text.xml10_content().trim().is_empty() {
                    return None;
                }
            }
            Event::GeneralRef(reference) => {
                if depth == 0 || !reference_resolves(&reference) {
                    return None;
                }
            }
            Event::Decl(_) if start.is_none() && before == 0 => {}
            Event::CData(_) if depth == 0 => return None,
            Event::DocType(_) | Event::Decl(_) | Event::PI(_) => return None,
            Event::Eof => break,
            Event::Empty(_) | Event::Text(_) | Event::CData(_) | Event::Comment(_) => {}
        }
    }
    Some((start?, end?))
}

/// Whether every attribute of `element` is well formed and binds only a
/// namespace prefix that `resolver` has in scope.
fn attributes_declared(resolver: &NamespaceResolver, element: &BytesStart<'_>) -> bool {
    element.attributes().all(|attribute| {
        attribute.is_ok_and(|attribute| {
            let (bound, _) = resolver.resolve_attribute(attribute.key);
            !matches!(bound, ResolveResult::Unknown(_))
        })
    })
}

/// The offset `before` of a top-level `element` when it opens the span: no
/// earlier top-level element has `closed`, and it is a SAML 2.0 `Assertion`.
fn assertion_start(
    before: usize,
    closed: bool,
    saml: bool,
    element: &BytesStart<'_>,
) -> Option<usize> {
    let root = !closed && saml && element.local_name().as_ref() == "Assertion";
    root.then_some(before)
}

/// Whether `reference` is one XML predefines or a character reference.
fn reference_resolves(reference: &BytesRef<'_>) -> bool {
    let predefined =
        quick_xml::escape::resolve_predefined_entity(&reference.xml10_content()).is_some();
    let character = matches!(reference.resolve_char_ref(), Ok(Some(_)));
    predefined || character
}

#[cfg(test)]
mod tests {
    use super::XuaAssertion;

    const ASSERTION: &str = r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion" ID="_a" Version="2.0"><saml2:Issuer>urn:oid:2.999.7</saml2:Issuer></saml2:Assertion>"#;

    #[test]
    fn the_element_is_kept_byte_for_byte_and_the_declaration_dropped() {
        let written =
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- issued -->\n{ASSERTION}\n");
        let read = XuaAssertion::new(&written).map(|assertion| assertion.as_str().to_owned());
        assert_eq!(Ok(ASSERTION.to_owned()), read);
    }

    #[test]
    fn anything_but_one_saml_assertion_is_refused() {
        let open = r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion">"#;
        for written in [
            "",
            "not xml",
            r#"<Assertion xmlns="urn:example"/>"#,
            &format!("{ASSERTION}{ASSERTION}"),
            &format!(
                "{ASSERTION}<wsa:To xmlns:wsa=\"http://www.w3.org/2005/08/addressing\">x</wsa:To>"
            ),
            &format!("<injected/>{ASSERTION}"),
            &format!("{ASSERTION}trailing"),
            &format!("{ASSERTION}<![CDATA[</wsse:Security>]]>"),
            &format!("{ASSERTION}<?pi?>"),
            &format!("<!DOCTYPE a>{ASSERTION}"),
            open,
            &format!("{open}</saml2:Assertion></wsse:Security><wsse:Security>"),
            &format!("{open}<soap:Body/></saml2:Assertion>"),
            &format!("{open}<saml2:Issuer soap:mustUnderstand=\"1\"/></saml2:Assertion>"),
            &format!("{open}&undeclared;</saml2:Assertion>"),
        ] {
            assert!(XuaAssertion::new(written).is_err(), "refused: {written:?}");
        }
    }

    #[test]
    fn references_and_nested_declarations_inside_the_element_are_kept() {
        let written = r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion"><saml2:Subject xmlns:ds="http://www.w3.org/2000/09/xmldsig#" ds:x="a&amp;b&#x41;"><![CDATA[c]]></saml2:Subject></saml2:Assertion>"#;
        let read = XuaAssertion::new(written).map(|assertion| assertion.as_str().to_owned());
        assert_eq!(Ok(written.to_owned()), read);
    }

    #[test]
    fn debug_shows_nothing_of_it() {
        let rendered = XuaAssertion::new(ASSERTION).map(|assertion| format!("{assertion:?}"));
        assert_eq!(Ok("XuaAssertion(\"***\")".to_owned()), rendered);
    }
}
