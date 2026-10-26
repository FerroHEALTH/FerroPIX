// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PID segment as a FHIR R4 `Patient`, and a `Patient` as a PID segment.
//!
//! A feed's PID becomes the `Patient` an ITI-104 Source would send for the
//! same patient, so the v2 feed and the FHIR feed write the same version
//! (ITI TF-2 §3.8.4.1.3; PIXm 3.1.0 §2:3.104.4.1.2.1 names the PDQm Patient
//! elements the Manager accepts). The elements follow the HL7 v2-to-FHIR
//! 1.0.0 maps: `segment-pid-to-patient` for the fields (PID-3 identifier,
//! PID-5 name, PID-6 the `patient-mothersMaidenName` extension, PID-7
//! birthDate, PID-8 gender, PID-11 address, PID-13 and PID-14 telecom), and
//! `datatype-xpn-to-humanname`, `datatype-xad-to-address`,
//! `datatype-xtn-to-contactpoint` and the tables 0001, 0190, 0200, 0201 and
//! 0202 for their parts. A code a table maps to no FHIR code is left out.
//!
//! A query answer's `Patient` is written back as a PID by the inverse of the
//! same maps, with the fields ITI TF-2 §3.21.4.2.2.5 names.

use ferrobridge_hl7v2::parse::{Field, Repetition, Segment};
use fhir_types::codec::{Object, Value};

use crate::hl7v2::authority::Authorities;
use crate::hl7v2::write::Writer;
use crate::matching::subject::MOTHERS_MAIDEN_NAME;

/// One identifier of PID-3 or MRG-1: its domain, when its assigning
/// authority names one the face knows, and its value.
#[derive(Clone, PartialEq, Eq)]
pub struct Cx {
    /// The system of the domain, `None` for an assigning authority that names
    /// none the face knows.
    pub system: Option<String>,
    /// CX-1.
    pub value: String,
}

impl std::fmt::Debug for Cx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cx")
            .field("system", &self.system)
            .finish_non_exhaustive()
    }
}

/// Reads every identifier of `field`, a CX list.
///
/// A repetition with no CX-4 takes `default`, the domain of the sender
/// (ITI TF-2 §3.8.4.1.3: the Manager "shall fill PID-3.4" from the
/// configuration of the Patient Identity Source); a repetition with no CX-1
/// is left out.
#[must_use]
pub fn identifiers(field: Option<&Field>, authorities: &Authorities, default: &str) -> Vec<Cx> {
    let Some(field) = field else {
        return Vec::new();
    };
    field
        .repetitions()
        .iter()
        .filter_map(|repetition| {
            let value = repetition.component(1)?.text()?.to_owned();
            let hd = crate::hl7v2::authority::Hd::of(repetition.component(4));
            let system = if hd.is_empty() {
                Some(default.to_owned())
            } else {
                authorities.system(&hd)
            };
            Some(Cx { system, value })
        })
        .collect()
}

/// Why a PID cannot be read as a `Patient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PidError {
    /// PID-7 is no date.
    #[error("PID-7 is no date")]
    BirthDate,
}

/// Returns the `Patient` of `pid` carrying `identifiers`, each with a
/// system, and `links`, the `Patient.link` entries.
///
/// # Errors
/// Returns [`PidError::BirthDate`] when PID-7 is valued and no date.
pub fn patient(
    pid: &Segment,
    identifiers: &[(String, String)],
    links: Vec<Value>,
) -> Result<Object, PidError> {
    let mut patient = Object::new();
    patient.insert(text("resourceType"), Value::String(text("Patient")));
    patient.insert(
        text("identifier"),
        Value::Array(
            identifiers
                .iter()
                .map(|(system, value)| {
                    object([("system", string(system)), ("value", string(value))])
                })
                .collect(),
        ),
    );
    if let Some(name) = pid.field(6).and_then(|field| field.repetitions().first())
        && let Some(family) = family(name)
    {
        patient.insert(
            text("extension"),
            Value::Array(vec![object([
                ("url", string(MOTHERS_MAIDEN_NAME)),
                ("valueString", string(&family)),
            ])]),
        );
    }
    insert_list(&mut patient, "name", reps(pid, 5).filter_map(human_name));
    if let Some(gender) = pid.text(8).and_then(gender) {
        patient.insert(text("gender"), string(gender));
    }
    if let Some(born) = pid.field(7).and_then(Field::text) {
        patient.insert(
            text("birthDate"),
            Value::String(birth_date(born).ok_or(PidError::BirthDate)?),
        );
    }
    insert_list(&mut patient, "address", reps(pid, 11).filter_map(address));
    let telecom = reps(pid, 13)
        .filter_map(|repetition| contact_point(repetition, "home"))
        .chain(reps(pid, 14).filter_map(|repetition| contact_point(repetition, "work")));
    insert_list(&mut patient, "telecom", telecom);
    if !links.is_empty() {
        patient.insert(text("link"), Value::Array(links));
    }
    Ok(patient)
}

/// Returns the `link` of a Resolve Duplicate Patient message: the survivor
/// `system` and `value` replace the patient (PIXm 3.1.0 §2:3.104.4.2.2).
#[must_use]
pub fn replaced_by(system: &str, value: &str) -> Value {
    object([
        (
            "other",
            object([(
                "identifier",
                object([("system", string(system)), ("value", string(value))]),
            )]),
        ),
        ("type", string("replaced-by")),
    ])
}

/// Returns the FHIR `date` of a v2 `DTM` or `TS`, cut to the precision it
/// states and at most a day: `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
#[must_use]
pub fn birth_date(dtm: &str) -> Option<String> {
    let digits: String = dtm.chars().take_while(char::is_ascii_digit).collect();
    let year = digits.get(..4)?;
    let month = digits.get(4..6);
    let day = digits.get(6..8);
    let valid_month = month.is_none_or(|m| m.parse::<u8>().is_ok_and(|m| (1..=12).contains(&m)));
    let valid_day = day.is_none_or(|d| d.parse::<u8>().is_ok_and(|d| (1..=31).contains(&d)));
    if !valid_month || !valid_day || (digits.len() != 4 && digits.len() < 6) {
        return None;
    }
    Some(match (month, day) {
        (Some(month), Some(day)) => format!("{year}-{month}-{day}"),
        (Some(month), None) => format!("{year}-{month}"),
        _ => year.to_owned(),
    })
}

/// The FHIR `administrative-gender` of a table 0001 code (v2-to-FHIR
/// `table-hl70001-to-administrative-gender`).
fn gender(code: &str) -> Option<&'static str> {
    Some(match code {
        "F" => "female",
        "M" => "male",
        "O" | "A" | "N" => "other",
        "U" => "unknown",
        _ => return None,
    })
}

/// The family name of an XPN: the surname of its first component.
fn family(name: &Repetition) -> Option<String> {
    name.component(1)?.subcomponent(1).map(str::to_owned)
}

/// The `HumanName` of an XPN (`datatype-xpn-to-humanname`), `None` when no
/// part is valued.
fn human_name(name: &Repetition) -> Option<Value> {
    let part = |position: usize| name.component(position).and_then(|c| c.text());
    let mut human = Object::new();
    if let Some(code) = part(7).and_then(name_use) {
        human.insert(text("use"), string(code));
    }
    if let Some(family) = family(name) {
        human.insert(text("family"), string(&family));
    }
    insert_list(
        &mut human,
        "given",
        [part(2), part(3)].into_iter().flatten().map(string),
    );
    insert_list(&mut human, "prefix", part(5).into_iter().map(string));
    insert_list(
        &mut human,
        "suffix",
        [part(4), part(6)].into_iter().flatten().map(string),
    );
    (human.keys().any(|key| key != "use")).then_some(Value::Object(human))
}

/// The `name-use` of a table 0200 code (`table-hl70200-to-name-use`).
fn name_use(code: &str) -> Option<&'static str> {
    Some(match code {
        "BAD" => "old",
        "D" => "usual",
        "L" | "R" => "official",
        "M" => "maiden",
        "MSK" => "anonymous",
        "N" => "nickname",
        "NAV" | "TEMP" => "temp",
        _ => return None,
    })
}

/// The `Address` of an XAD (`datatype-xad-to-address`), `None` when no part
/// is valued.
fn address(address: &Repetition) -> Option<Value> {
    let part = |position: usize| address.component(position).and_then(|c| c.text());
    let mut fhir = Object::new();
    match part(7) {
        Some("M" | "SH") => {
            fhir.insert(text("type"), string("postal"));
        }
        Some(code) => {
            if let Some(code) = address_use(code) {
                fhir.insert(text("use"), string(code));
            }
        }
        None => {}
    }
    insert_list(
        &mut fhir,
        "line",
        [part(1), part(2)].into_iter().flatten().map(string),
    );
    for (position, member) in [(3, "city"), (4, "state"), (5, "postalCode"), (6, "country")] {
        if let Some(value) = part(position) {
            fhir.insert(text(member), string(value));
        }
    }
    (fhir.keys().any(|key| key != "use" && key != "type")).then_some(Value::Object(fhir))
}

/// The `address-use` of a table 0190 code (`table-hl70190-to-address-use`).
fn address_use(code: &str) -> Option<&'static str> {
    Some(match code {
        "BA" => "old",
        "BI" => "billing",
        "C" => "temp",
        "B" | "O" => "work",
        "H" => "home",
        _ => return None,
    })
}

/// The `ContactPoint` of an XTN (`datatype-xtn-to-contactpoint`), with `use`
/// when XTN-2 names none (`segment-pid-to-patient`, PID-13 and PID-14).
fn contact_point(xtn: &Repetition, default_use: &'static str) -> Option<Value> {
    let part = |position: usize| xtn.component(position).and_then(|c| c.text());
    let equipment = part(3);
    let email = matches!(equipment, Some("Internet" | "X.400"));
    let value = if email {
        part(4)
    } else {
        part(12).or_else(|| part(1))
    }?;
    let mut fhir = Object::new();
    // NOTE: v2-to-FHIR table-hl70202-to-contact-point-system maps CP to `mobile`,
    // which R4 ContactPoint.system lacks; a cell phone is a `phone` in `mobile` use.
    let system = match equipment {
        Some("PH" | "CP") | None => Some("phone"),
        Some("FX") => Some("fax"),
        Some("BP") => Some("pager"),
        Some("Internet" | "X.400") => Some("email"),
        Some("MD" | "SAT" | "TDD" | "TTY") => Some("other"),
        Some(_) => None,
    };
    if let Some(system) = system {
        fhir.insert(text("system"), string(system));
    }
    fhir.insert(text("value"), string(value));
    let used = match (equipment, part(2)) {
        (Some("CP"), _) | (_, Some("PRS")) => Some("mobile"),
        (_, Some("PRN")) => Some("home"),
        (_, Some("WPN")) => Some("work"),
        (_, Some(_)) => None,
        (_, None) => Some(default_use),
    };
    if let Some(used) = used {
        fhir.insert(text("use"), string(used));
    }
    Some(Value::Object(fhir))
}

/// The repetitions of field `position` of `segment`.
fn reps(segment: &Segment, position: usize) -> impl Iterator<Item = &Repetition> {
    segment
        .field(position)
        .map(Field::repetitions)
        .unwrap_or_default()
        .iter()
}

/// Inserts `values` under `key` when there is at least one.
fn insert_list(object: &mut Object, key: &str, values: impl IntoIterator<Item = Value>) {
    let values: Vec<Value> = values.into_iter().collect();
    if !values.is_empty() {
        object.insert(text(key), Value::Array(values));
    }
}

/// An owned string.
fn text(value: &str) -> String {
    value.to_owned()
}

/// A JSON string.
fn string(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// A JSON object of `members`.
fn object<const N: usize>(members: [(&str, Value); N]) -> Value {
    Value::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// What a PID written from a `Patient` carries beside its identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// The demographics of ITI TF-2 §3.21.4.2.2.5, for ITI-21.
    Demographics,
    /// No name: an empty first PID-5 and a second whose only component is
    /// the name type `S`, for ITI-9 (ITI TF-2 §3.9.4.2.2.5).
    Pseudonymous,
    /// A single space in PID-5, for ITI-10 (ITI TF-2 §3.10.4.1.2.3).
    Space,
}

/// Writes the PID segment `set_id` of `identifiers` and, for
/// [`Shown::Demographics`], the demographics of `patient`, a `Patient`'s
/// JSON.
#[must_use]
pub fn segment(
    writer: &Writer<'_>,
    authorities: &Authorities,
    set_id: usize,
    identifiers: &[(String, String)],
    patient: Option<&Object>,
    shown: Shown,
) -> String {
    let cx: Vec<String> = identifiers
        .iter()
        .map(|(system, value)| {
            let [namespace, universal, kind] = authorities.of_system(system);
            writer.components(&[
                writer.escape(value),
                String::new(),
                String::new(),
                writer.subcomponents(&[&namespace, &universal, &kind]),
            ])
        })
        .collect();
    let mut fields = vec![String::new(); 14];
    if let Some(slot) = fields.get_mut(0) {
        *slot = set_id.to_string();
    }
    if let Some(slot) = fields.get_mut(2) {
        *slot = writer.repetitions(&cx);
    }
    match (shown, patient) {
        (Shown::Pseudonymous, _) => {
            if let Some(slot) = fields.get_mut(4) {
                let pseudo = writer.components(&[
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::from("S"),
                ]);
                *slot = writer.repetitions(&[String::new(), pseudo]);
            }
        }
        (Shown::Space, _) => {
            if let Some(slot) = fields.get_mut(4) {
                *slot = String::from(" ");
            }
        }
        (Shown::Demographics, Some(patient)) => demographics(writer, patient, &mut fields),
        (Shown::Demographics, None) => {}
    }
    writer.segment("PID", &fields)
}

/// Writes the demographics of `patient` into `fields`, PID-1 at index 0.
fn demographics(writer: &Writer<'_>, patient: &Object, fields: &mut [String]) {
    let mut put = |position: usize, value: String| {
        if let Some(slot) = position
            .checked_sub(1)
            .and_then(|index| fields.get_mut(index))
        {
            *slot = value;
        }
    };
    let names: Vec<String> = list(patient, "name")
        .filter_map(Value::as_object)
        .map(|name| {
            let given: Vec<&str> = strings(name, "given").collect();
            let more = given
                .get(1..)
                .map(|rest| rest.join(" "))
                .unwrap_or_default();
            writer.components(&[
                writer.escape(member(name, "family").unwrap_or_default()),
                writer.escape(given.first().copied().unwrap_or_default()),
                writer.escape(&more),
                writer.escape(&strings(name, "suffix").collect::<Vec<_>>().join(" ")),
                writer.escape(&strings(name, "prefix").collect::<Vec<_>>().join(" ")),
                String::new(),
                member(name, "use")
                    .and_then(name_type)
                    .unwrap_or_default()
                    .to_owned(),
            ])
        })
        .collect();
    put(5, writer.repetitions(&names));
    let maiden = list(patient, "extension")
        .filter_map(Value::as_object)
        .filter(|extension| member(extension, "url") == Some(MOTHERS_MAIDEN_NAME))
        .find_map(|extension| member(extension, "valueString"));
    if let Some(maiden) = maiden {
        put(6, writer.escape(maiden));
    }
    if let Some(born) = member(patient, "birthDate") {
        put(7, born.chars().filter(char::is_ascii_digit).collect());
    }
    if let Some(code) = member(patient, "gender").and_then(administrative_sex) {
        put(8, code.to_owned());
    }
    let addresses: Vec<String> = list(patient, "address")
        .filter_map(Value::as_object)
        .map(|address| {
            let lines: Vec<&str> = strings(address, "line").collect();
            let kind = if member(address, "type") == Some("postal") {
                Some("M")
            } else {
                member(address, "use").and_then(address_type)
            };
            writer.components(&[
                writer.escape(lines.first().copied().unwrap_or_default()),
                writer.escape(
                    &lines
                        .get(1..)
                        .map(|rest| rest.join(" "))
                        .unwrap_or_default(),
                ),
                writer.escape(member(address, "city").unwrap_or_default()),
                writer.escape(member(address, "state").unwrap_or_default()),
                writer.escape(member(address, "postalCode").unwrap_or_default()),
                writer.escape(member(address, "country").unwrap_or_default()),
                kind.unwrap_or_default().to_owned(),
            ])
        })
        .collect();
    put(11, writer.repetitions(&addresses));
    let (work, home): (Vec<&Object>, Vec<&Object>) = list(patient, "telecom")
        .filter_map(Value::as_object)
        .partition(|point| member(point, "use") == Some("work"));
    put(13, telecom(writer, &home));
    put(14, telecom(writer, &work));
}

/// Writes `points` as an XTN list: XTN-2 the use, XTN-3 the equipment, and
/// the value in XTN-4 for an address and XTN-12 for a number.
fn telecom(writer: &Writer<'_>, points: &[&Object]) -> String {
    let written: Vec<String> = points
        .iter()
        .filter_map(|point| {
            let value = member(point, "value")?;
            let system = member(point, "system");
            let used = member(point, "use");
            let equipment = match (system, used) {
                (Some("email"), _) => "Internet",
                (Some("fax"), _) => "FX",
                (Some("pager"), _) => "BP",
                (Some("phone") | None, Some("mobile")) => "CP",
                (Some("phone") | None, _) => "PH",
                (Some(_), _) => return None,
            };
            let use_code = match used {
                Some("work") => "WPN",
                Some("mobile") => "PRS",
                _ if equipment == "Internet" => "NET",
                _ => "PRN",
            };
            let mut parts = vec![String::new(); 12];
            if let Some(slot) = parts.get_mut(1) {
                use_code.clone_into(slot);
            }
            if let Some(slot) = parts.get_mut(2) {
                equipment.clone_into(slot);
            }
            let index = if equipment == "Internet" { 3 } else { 11 };
            if let Some(slot) = parts.get_mut(index) {
                *slot = writer.escape(value);
            }
            Some(writer.components(&parts))
        })
        .collect();
    writer.repetitions(&written)
}

/// The table 0200 code of a `name-use`, the inverse of [`name_use`].
fn name_type(code: &str) -> Option<&'static str> {
    Some(match code {
        "official" => "L",
        "usual" => "D",
        "maiden" => "M",
        "nickname" => "N",
        "anonymous" => "MSK",
        "old" => "BAD",
        "temp" => "TEMP",
        _ => return None,
    })
}

/// The table 0001 code of an `administrative-gender`.
fn administrative_sex(code: &str) -> Option<&'static str> {
    Some(match code {
        "female" => "F",
        "male" => "M",
        "other" => "O",
        "unknown" => "U",
        _ => return None,
    })
}

/// The table 0190 code of an `address-use`, the inverse of
/// [`address_use`].
fn address_type(code: &str) -> Option<&'static str> {
    Some(match code {
        "home" => "H",
        "work" => "B",
        "temp" => "C",
        "old" => "BA",
        "billing" => "BI",
        _ => return None,
    })
}

/// The array member `key` of `object`, empty when absent.
fn list<'o>(object: &'o Object, key: &str) -> impl Iterator<Item = &'o Value> {
    object
        .get(key)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
}

/// The strings of the array member `key` of `object`.
fn strings<'o>(object: &'o Object, key: &str) -> impl Iterator<Item = &'o str> {
    list(object, key).filter_map(Value::as_str)
}

/// The string member `key` of `object`.
fn member<'o>(object: &'o Object, key: &str) -> Option<&'o str> {
    object.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferrobridge_hl7v2::decode::Charset;
    use ferrobridge_hl7v2::parse::lex::lex;
    use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
    use fhir_types::codec::{Json as _, Path};
    use fhir_types::r4::patient::Patient;

    use super::{Shown, birth_date, identifiers, patient, segment};
    use crate::hl7v2::authority::Authorities;
    use crate::hl7v2::write::Writer;

    fn feed() -> String {
        let family = FAMILY_NAMES[0];
        let given = GIVEN_NAMES[0];
        let mother = FAMILY_NAMES[1];
        let bsn = ferropix_testkit::synthetic::bsn(0)
            .expect("a synthetic BSN")
            .digits();
        format!(
            "MSH|^~\\&|ADT|EAST|PIX|NORTH|20261025100000||ADT^A01^ADT_A01|M-1|P|2.5\r\
             EVN||20261025100000\r\
             PID|1||PAT-1^^^EAST~{bsn}^^^&2.16.840.1.113883.2.4.6.3&ISO~X-9^^^NOWHERE||{family}^{given}^B^^Dr^^L~{family}^{given}^^^^^N|{mother}|19800229|F|||Dorpsstraat 1^^Proefdorp^^1234 AB^NL^H||^PRN^PH^^^^^^^^^+31201234567~^NET^Internet^ada@example.org|^WPN^PH^^^^^^^^^+31207654321\r\
             PV1|1|O\r"
        )
    }

    fn authorities() -> Authorities {
        Authorities::new(&BTreeMap::from([(
            HOSPITAL_DOMAIN.to_owned(),
            String::from("EAST"),
        )]))
    }

    #[test]
    fn a_pid_reads_as_the_patient_a_fhir_source_would_send() {
        let lexed = lex(&feed(), Charset::Ascii).expect("the feed lexes");
        let pid = lexed
            .message
            .segments()
            .iter()
            .find(|segment| segment.id() == "PID")
            .expect("a PID");
        let read = identifiers(pid.field(3), &authorities(), HOSPITAL_DOMAIN);
        let systems: Vec<Option<&str>> = read.iter().map(|cx| cx.system.as_deref()).collect();
        assert_eq!(
            systems,
            vec![
                Some(HOSPITAL_DOMAIN),
                Some("urn:oid:2.16.840.1.113883.2.4.6.3"),
                None
            ]
        );
        let known: Vec<(String, String)> = read
            .into_iter()
            .filter_map(|cx| Some((cx.system?, cx.value)))
            .collect();
        let object = patient(pid, &known, Vec::new()).expect("the PID reads");
        let fhir = Patient::from_json(&object, &mut Path::root("Patient")).expect("a Patient");
        assert_eq!(fhir.identifier.len(), 2);
        assert_eq!(
            fhir.birth_date.and_then(|date| date.value).as_deref(),
            Some("1980-02-29")
        );
        assert_eq!(
            fhir.gender.and_then(|code| code.value).as_deref(),
            Some("female")
        );
        let name = fhir.name.first().expect("a name");
        assert_eq!(
            name.family.as_ref().and_then(|f| f.value.as_deref()),
            Some(FAMILY_NAMES[0])
        );
        assert_eq!(name.given.len(), 2);
        assert_eq!(fhir.name.len(), 2);
        assert_eq!(fhir.telecom.len(), 3);
        assert_eq!(fhir.address.len(), 1);
        assert_eq!(fhir.extension.len(), 1, "the mother's maiden name");
    }

    #[test]
    fn a_birth_date_keeps_its_precision_and_refuses_what_is_no_date() {
        assert_eq!(birth_date("19800229").as_deref(), Some("1980-02-29"));
        assert_eq!(birth_date("198002").as_deref(), Some("1980-02"));
        assert_eq!(birth_date("1980").as_deref(), Some("1980"));
        assert_eq!(
            birth_date("19800229123000+0100").as_deref(),
            Some("1980-02-29")
        );
        assert_eq!(birth_date("19801329"), None);
        assert_eq!(birth_date("19"), None);
        assert_eq!(birth_date("19800"), None);
    }

    #[test]
    fn a_patient_writes_back_as_the_pid_it_was_read_from() {
        let lexed = lex(&feed(), Charset::Ascii).expect("the feed lexes");
        let pid = lexed
            .message
            .segments()
            .iter()
            .find(|segment| segment.id() == "PID")
            .expect("a PID");
        let known = vec![(HOSPITAL_DOMAIN.to_owned(), String::from("PAT-1"))];
        let object = patient(pid, &known, Vec::new()).expect("the PID reads");
        let writer = Writer::of(&lexed.message);
        let written = segment(
            &writer,
            &authorities(),
            1,
            &known,
            Some(&object),
            Shown::Demographics,
        );
        let family = FAMILY_NAMES[0];
        let given = GIVEN_NAMES[0];
        let mother = FAMILY_NAMES[1];
        assert_eq!(
            written,
            format!(
                "PID|1||PAT-1^^^EAST&2.999.1.1&ISO||{family}^{given}^B^^Dr^^L~{family}^{given}^^^^^N|{mother}|19800229|F|||Dorpsstraat 1^^Proefdorp^^1234 AB^NL^H||^PRN^PH^^^^^^^^^+31201234567~^NET^Internet^ada@example.org|^WPN^PH^^^^^^^^^+31207654321"
            )
        );
        let pseudonymous = segment(
            &writer,
            &authorities(),
            1,
            &known,
            None,
            Shown::Pseudonymous,
        );
        assert_eq!(pseudonymous, "PID|1||PAT-1^^^EAST&2.999.1.1&ISO||~^^^^^^S");
    }
}
