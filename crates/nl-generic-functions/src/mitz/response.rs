// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Reading Mitz's answer: a SOAP 1.2 envelope whose body holds one XACML 3.0
//! `Response` with one `Result` per data category asked about, or a SOAP 1.2
//! fault (Implementatiehandleiding Open en gesloten autorisatievraag 3.8.2
//! §3.2.5.2 and the example of §3.2.5.4).

use std::collections::{BTreeMap, BTreeSet};

use http::StatusCode;
use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use secrecy::{ExposeSecret as _, SecretString};

use super::error::{FaultCode, Malformation, MitzError};
use super::question::{ClosedAnswer, ClosedQuestion, DataCategory, Decision};
use super::request::{CATEGORY, HOLDER_ID, PATIENT_ID};
use super::{BSN_ROOT, DATA_CATEGORY_SYSTEM, HL7, SOAP, URA_ROOT, XACML};

/// The most bytes of an answer the client reads (no specification governs
/// this: our own design, room for many more results than one question asks
/// about).
pub(super) const LIMIT: usize = 1024 * 1024;

/// The deepest element nesting the client reads (no specification governs
/// this: our own design; an answer nests about eight deep).
const DEPTH: usize = 64;

/// The XACML 3.0 attribute categories a result's attributes are read in.
const RESOURCE: &str = "urn:oasis:names:tc:xacml:3.0:attribute-category:resource";
const ACTION: &str = "urn:oasis:names:tc:xacml:3.0:attribute-category:action";

/// An HL7 V3 `II` an answer echoes: its root and its extension.
type Identifier = (Option<String>, Option<SecretString>);

/// What one `Result` says.
#[derive(Default)]
struct ResultRead {
    decisions: Vec<String>,
    categories: Vec<(Option<String>, Option<String>)>,
    patients: Vec<Identifier>,
    holders: Vec<Identifier>,
}

/// What the reader collects of an answer.
#[derive(Default)]
struct Read {
    envelope: bool,
    body: bool,
    fault: Option<String>,
    responses: usize,
    results: Vec<ResultRead>,
    category: Option<String>,
    attribute: Option<String>,
}

/// Which namespace an element is in, of the ones the reader looks at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ns {
    Soap,
    Xacml,
    Hl7,
    Other,
}

/// One open element: its namespace and local name.
type Open = (Ns, String);

/// Reads the answer with `status`, media type `media` and `body` to
/// `question`.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    question: &ClosedQuestion,
) -> Result<ClosedAnswer, MitzError> {
    // NOTE: Programma van Eisen AMC AUS-TR-e0900: Mitz uses the HTTP and SOAP error codes, so an
    // answer that is no SOAP message is malformed only on a 200.
    let rejected = |malformed: MitzError| {
        if status == StatusCode::OK {
            malformed
        } else {
            MitzError::Rejected { status }
        }
    };
    if media.map(media_type).as_deref() != Some("application/soap+xml") {
        return Err(rejected(Malformation::NotSoap.into()));
    }
    let read = parse(body).map_err(rejected)?;
    if let Some(code) = read.fault {
        return Err(MitzError::Fault {
            code: FaultCode::of(&code),
            status,
        });
    }
    if status != StatusCode::OK {
        return Err(MitzError::Rejected { status });
    }
    if read.responses != 1 {
        return Err(Malformation::NoResponse.into());
    }
    decide(read.results, question)
}

/// The decision of each result, held to the question asked (§3.2.5.2).
fn decide(results: Vec<ResultRead>, question: &ClosedQuestion) -> Result<ClosedAnswer, MitzError> {
    let mut decided: BTreeMap<DataCategory, Decision> = BTreeMap::new();
    let mut seen: BTreeSet<DataCategory> = BTreeSet::new();
    let mut undecided: Option<MitzError> = None;
    for (index, result) in results.into_iter().enumerate() {
        let category = category_of(&result, index, question)?;
        if !seen.insert(category.clone()) {
            return Err(Malformation::DuplicateResult { result: index }.into());
        }
        // NOTE: §3.2.5.2 requires each result to echo the patient and the data holder, so
        // a result about anyone else is refused, never read as a decision.
        if !about_patient(&result.patients, question) {
            return Err(Malformation::OtherPatient { result: index }.into());
        }
        if !about_holder(&result.holders, question) {
            return Err(Malformation::OtherHolder { result: index }.into());
        }
        let [decision] = result.decisions.as_slice() else {
            return Err(Malformation::NoDecision { result: index }.into());
        };
        match decision.trim() {
            "Permit" => {
                decided.insert(category, Decision::Permit);
            }
            "Deny" => {
                decided.insert(category, Decision::Deny);
            }
            "Indeterminate" => {
                undecided.get_or_insert(MitzError::Indeterminate { category });
            }
            "NotApplicable" => {
                undecided.get_or_insert(MitzError::NotApplicable { category });
            }
            _ => return Err(Malformation::UnknownDecision { result: index }.into()),
        }
    }
    if let Some(undecided) = undecided {
        return Err(undecided);
    }
    if let Some(missing) = question
        .categories()
        .iter()
        .find(|category| !seen.contains(*category))
    {
        return Err(Malformation::MissingResult(missing.clone()).into());
    }
    Ok(ClosedAnswer::new(decided))
}

/// The one asked-about data category `result` names.
fn category_of(
    result: &ResultRead,
    index: usize,
    question: &ClosedQuestion,
) -> Result<DataCategory, MitzError> {
    let [(Some(code), Some(system))] = result.categories.as_slice() else {
        return Err(Malformation::NoCategory { result: index }.into());
    };
    if system != DATA_CATEGORY_SYSTEM {
        return Err(Malformation::NoCategory { result: index }.into());
    }
    question
        .categories()
        .iter()
        .find(|asked| asked.as_str() == code)
        .cloned()
        .ok_or_else(|| Malformation::UnaskedCategory { result: index }.into())
}

/// Whether the result echoes exactly the patient asked about.
fn about_patient(patients: &[Identifier], question: &ClosedQuestion) -> bool {
    let asked = question.patient().value().expose_secret();
    matches!(
        patients,
        [(Some(root), Some(extension))] if root == BSN_ROOT && extension.expose_secret() == asked
    )
}

/// Whether the result echoes exactly the data holder asked about.
fn about_holder(holders: &[Identifier], question: &ClosedQuestion) -> bool {
    let asked = question.holder().organisation().as_str();
    matches!(
        holders,
        [(Some(root), Some(extension))] if root == URA_ROOT && extension.expose_secret() == asked
    )
}

/// The type and subtype of a media type, lowercased, without parameters.
fn media_type(media: &str) -> String {
    media
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Collects what the answer says, refusing what is not well-formed XML.
fn parse(body: &[u8]) -> Result<Read, MitzError> {
    let text = std::str::from_utf8(body).map_err(|error| Malformation::NotXml {
        position: u64::try_from(error.valid_up_to()).unwrap_or(u64::MAX),
    })?;
    let mut reader = NsReader::from_str(text);
    let mut open: Vec<Open> = Vec::new();
    let mut read = Read::default();
    let mut collected = String::new();
    loop {
        let (ns, event) = match reader.read_resolved_event() {
            Ok((namespace, event)) => (kind(&namespace), event),
            Err(_syntax) => {
                return Err(Malformation::NotXml {
                    position: reader.error_position(),
                }
                .into());
            }
        };
        match event {
            Event::Start(element) => {
                if open.len() >= DEPTH {
                    return Err(Malformation::TooDeep { limit: DEPTH }.into());
                }
                open.push((ns, local_name(&element)));
                opened(&mut read, &open, &element)?;
                collected.clear();
            }
            Event::Empty(element) => {
                open.push((ns, local_name(&element)));
                opened(&mut read, &open, &element)?;
                closed(&mut read, &open, "");
                open.pop();
            }
            Event::End(_) => {
                closed(&mut read, &open, &collected);
                collected.clear();
                open.pop();
            }
            Event::Text(text) => collected.push_str(&text.xml10_content()),
            Event::CData(data) => collected.push_str(&data.into_inner()),
            Event::GeneralRef(reference) => {
                let resolved = reference
                    .resolve_char_ref()
                    .ok()
                    .flatten()
                    .map(String::from)
                    .or_else(|| {
                        quick_xml::escape::resolve_predefined_entity(&reference.xml10_content())
                            .map(str::to_owned)
                    })
                    .ok_or(Malformation::NotXml {
                        position: reader.buffer_position(),
                    })?;
                collected.push_str(&resolved);
            }
            Event::DocType(_) => return Err(Malformation::DocumentType.into()),
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }
    if !read.envelope || !read.body {
        return Err(Malformation::NotAnEnvelope.into());
    }
    Ok(read)
}

/// Which of the namespaces the reader looks at `namespace` is.
fn kind(namespace: &ResolveResult<'_>) -> Ns {
    match namespace {
        ResolveResult::Bound(ns) if ns.0 == SOAP => Ns::Soap,
        ResolveResult::Bound(ns) if ns.0 == XACML => Ns::Xacml,
        ResolveResult::Bound(ns) if ns.0 == HL7 => Ns::Hl7,
        _ => Ns::Other,
    }
}

fn local_name(element: &BytesStart<'_>) -> String {
    element.local_name().as_ref().to_owned()
}

/// The value of the attribute `name`, unescaped, if the element has it.
fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, MitzError> {
    let malformed = Malformation::NotXml { position: 0 };
    let Some(found) = element
        .try_get_attribute(name)
        .map_err(|_attribute| malformed.clone())?
    else {
        return Ok(None);
    };
    let value = found
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .map_err(|_escape| malformed)?;
    Ok(Some(value.into_owned()))
}

/// Whether the innermost open element is inside a `Result` of the body.
fn in_result(open: &[Open]) -> bool {
    open.get(2..)
        .unwrap_or_default()
        .iter()
        .any(|(ns, local)| *ns == Ns::Xacml && local == "Result")
}

/// What the element just opened says, given the elements open around it.
fn opened(read: &mut Read, open: &[Open], element: &BytesStart<'_>) -> Result<(), MitzError> {
    let names: Vec<(Ns, &str)> = open
        .iter()
        .map(|(ns, local)| (*ns, local.as_str()))
        .collect();
    match names.as_slice() {
        [(Ns::Soap, "Envelope")] => read.envelope = true,
        [(_, _)] => return Err(Malformation::NotAnEnvelope.into()),
        [(Ns::Soap, "Envelope"), (Ns::Soap, "Body")] => read.body = true,
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Body"),
            (Ns::Soap, "Fault"),
        ] => {
            read.fault = Some(String::new());
        }
        [(Ns::Soap, "Envelope"), (Ns::Soap, "Body"), ..] => {}
        _ => return Ok(()),
    }
    let Some(&(ns, local)) = names.last() else {
        return Ok(());
    };
    match (ns, local) {
        (Ns::Xacml, "Response") => read.responses = read.responses.saturating_add(1),
        (Ns::Xacml, "Result") if !in_result(open.split_last().map_or(&[], |(_, rest)| rest)) => {
            read.results.push(ResultRead::default());
        }
        (Ns::Xacml, "Attributes") if in_result(open) => {
            read.category = attribute(element, "Category")?.map(|text| text.trim().to_owned());
        }
        (Ns::Xacml, "Attribute") if in_result(open) => {
            read.attribute = attribute(element, "AttributeId")?.map(|text| text.trim().to_owned());
        }
        (Ns::Hl7, "CodedValue") if in_result(open) => {
            if is(read.category.as_deref(), ACTION) && is(read.attribute.as_deref(), CATEGORY) {
                let code = attribute(element, "code")?;
                let system = attribute(element, "codeSystem")?;
                if let Some(result) = read.results.last_mut() {
                    result.categories.push((code, system));
                }
            }
        }
        (Ns::Hl7, "InstanceIdentifier") if in_result(open) => {
            let resource = is(read.category.as_deref(), RESOURCE);
            let patient = resource && is(read.attribute.as_deref(), PATIENT_ID);
            let holder = resource && is(read.attribute.as_deref(), HOLDER_ID);
            if patient || holder {
                let root = attribute(element, "root")?.map(|text| text.trim().to_owned());
                let extension = attribute(element, "extension")?.map(SecretString::from);
                if let Some(result) = read.results.last_mut() {
                    if patient {
                        result.patients.push((root, extension));
                    } else {
                        result.holders.push((root, extension));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Whether `value` is `expected`.
fn is(value: Option<&str>, expected: &str) -> bool {
    value == Some(expected)
}

/// What the element about to close held as text, given the elements open.
fn closed(read: &mut Read, open: &[Open], text: &str) {
    let names: Vec<(Ns, &str)> = open
        .iter()
        .map(|(ns, local)| (*ns, local.as_str()))
        .collect();
    if let [
        (Ns::Soap, "Envelope"),
        (Ns::Soap, "Body"),
        (Ns::Soap, "Fault"),
        (Ns::Soap, "Code"),
        (Ns::Soap, "Value"),
    ] = names.as_slice()
    {
        read.fault = Some(text.trim().to_owned());
        return;
    }
    match names.last() {
        Some((Ns::Xacml, "Decision")) if in_result(open) => {
            if let Some(result) = read.results.last_mut() {
                result.decisions.push(text.to_owned());
            }
        }
        Some((Ns::Xacml, "Attributes")) => read.category = None,
        Some((Ns::Xacml, "Attribute")) => read.attribute = None,
        _ => {}
    }
}
