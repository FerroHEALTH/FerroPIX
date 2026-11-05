// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Reading Mitz's answer: a SOAP 1.2 envelope whose body holds one XACML 3.0
//! `Response` with one `Result` per data category asked about, or a SOAP 1.2
//! fault (Implementatiehandleiding Open en gesloten autorisatievraag 3.8.2
//! §3.2.5.2 and the example of §3.2.5.4).

use std::collections::{BTreeMap, BTreeSet};

use http::StatusCode;
use ihe_iti::soap::{self, Answer, Element, Visitor};
use secrecy::{ExposeSecret as _, SecretString};

use super::error::{Malformation, MitzError};
use super::question::{ClosedAnswer, ClosedQuestion, DataCategory, Decision};
use super::request::{CATEGORY, HOLDER_ID, PATIENT_ID};
use super::{BSN_ROOT, DATA_CATEGORY_SYSTEM, HL7, URA_ROOT, XACML};

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

/// Reads the answer with `status`, media type `media` and `body` to
/// `question`.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    question: &ClosedQuestion,
) -> Result<ClosedAnswer, MitzError> {
    // NOTE: Programma van Eisen AMC AUS-TR-e0900: Mitz uses the HTTP and SOAP error codes, so an
    // answer that is no SOAP message is malformed only on a 200, as `soap::answer` reads it.
    let mut read = Read::default();
    match soap::answer(status, media, body, DEPTH, &mut read)? {
        Answer::Envelope => {}
        Answer::Fault { code, status } => return Err(MitzError::Fault { code, status }),
        Answer::Rejected { status } => return Err(MitzError::Rejected { status }),
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

/// Whether the innermost open element is inside a `Result` of the body.
fn in_result(open: &[(Ns, &str)]) -> bool {
    open.get(2..)
        .unwrap_or_default()
        .iter()
        .any(|(ns, local)| *ns == Ns::Xacml && *local == "Result")
}

/// Whether `value` is `expected`.
fn is(value: Option<&str>, expected: &str) -> bool {
    value == Some(expected)
}

impl Visitor for Read {
    type Namespace = Ns;
    type Error = Malformation;
    const SOAP: Ns = Ns::Soap;
    const NAMESPACES: &[(&str, Ns)] = &[(XACML, Ns::Xacml), (HL7, Ns::Hl7)];
    const OTHER: Ns = Ns::Other;

    fn opened(&mut self, open: &[(Ns, &str)], element: &Element<'_>) -> Result<(), Malformation> {
        let [(Ns::Soap, "Envelope"), (Ns::Soap, "Body"), ..] = open else {
            return Ok(());
        };
        let Some(&(ns, local)) = open.last() else {
            return Ok(());
        };
        match (ns, local) {
            (Ns::Xacml, "Response") => self.responses = self.responses.saturating_add(1),
            (Ns::Xacml, "Result")
                if !in_result(open.split_last().map_or(&[], |(_, rest)| rest)) =>
            {
                self.results.push(ResultRead::default());
            }
            (Ns::Xacml, "Attributes") if in_result(open) => {
                self.category = element
                    .attribute("Category")?
                    .map(|text| text.trim().to_owned());
            }
            (Ns::Xacml, "Attribute") if in_result(open) => {
                self.attribute = element
                    .attribute("AttributeId")?
                    .map(|text| text.trim().to_owned());
            }
            (Ns::Hl7, "CodedValue") if in_result(open) => {
                if is(self.category.as_deref(), ACTION) && is(self.attribute.as_deref(), CATEGORY) {
                    let code = element.attribute("code")?;
                    let system = element.attribute("codeSystem")?;
                    if let Some(result) = self.results.last_mut() {
                        result.categories.push((code, system));
                    }
                }
            }
            (Ns::Hl7, "InstanceIdentifier") if in_result(open) => {
                let resource = is(self.category.as_deref(), RESOURCE);
                let patient = resource && is(self.attribute.as_deref(), PATIENT_ID);
                let holder = resource && is(self.attribute.as_deref(), HOLDER_ID);
                if patient || holder {
                    let root = element
                        .attribute("root")?
                        .map(|text| text.trim().to_owned());
                    let extension = element.attribute("extension")?.map(SecretString::from);
                    if let Some(result) = self.results.last_mut() {
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

    fn closed(&mut self, open: &[(Ns, &str)], text: Option<&str>, _at: usize) {
        let text = text.unwrap_or_default();
        match open.last() {
            Some((Ns::Xacml, "Decision")) if in_result(open) => {
                if let Some(result) = self.results.last_mut() {
                    result.decisions.push(text.to_owned());
                }
            }
            Some((Ns::Xacml, "Attributes")) => self.category = None,
            Some((Ns::Xacml, "Attribute")) => self.attribute = None,
            _ => {}
        }
    }
}
