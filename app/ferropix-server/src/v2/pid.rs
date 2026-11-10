// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PID segment as a FHIR R4 `Patient`, and a `Patient` as a PID segment.
//!
//! A feed's PID becomes the `Patient` an ITI-104 Source would send for the
//! same patient, so the v2 feed and the FHIR feed write the same version
//! (ITI TF-2 §3.8.4.1.3; PIXm 3.1.0 §2:3.104.4.1.2.1 names the PDQm Patient
//! elements the Manager accepts). The PID maps through the HL7 v2-to-FHIR
//! 1.0.0 structure maps `ferrobridge-hl7v2` is built with, FerroBRIDGE's
//! corrections over them: `segment-pid-to-patient` and its data type maps,
//! with every table value translated by `ConceptMap/$translate` on the
//! terminology server `[terminology]` names. A query answer's `Patient` is
//! written back as a PID by the inverse of the same maps, with the fields
//! ITI TF-2 §3.21.4.2.2.5 names. PID-1 and PID-3 are FerroPIX's own, from
//! its identifier domains, and so are the PID-5 forms of ITI-9 and ITI-10.

use ferrobridge_hl7v2::map::inverse::{self, InverseError};
use ferrobridge_hl7v2::map::maps::Maps;
use ferrobridge_hl7v2::map::single::{self, SegmentError};
use ferrobridge_hl7v2::map::{MapError, Outcome};
use ferrobridge_hl7v2::parse::structure::segment_of;
use ferrobridge_hl7v2::parse::{self, Parsed};
use ferrobridge_hl7v2::write::value::{Component, Field, Repetition, Segment};
use ferrobridge_term::client::Client;
use fhir_types::codec::{Object, Value};

use crate::config::section::hl7v2::authority::Authorities;

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
pub fn identifiers(
    field: Option<&parse::Field>,
    authorities: &Authorities,
    default: &str,
) -> Vec<Cx> {
    let Some(field) = field else {
        return Vec::new();
    };
    field
        .repetitions()
        .iter()
        .filter_map(|repetition| {
            let value = repetition.component(1)?.subcomponent(1)?.to_owned();
            let hd = crate::config::section::hl7v2::authority::Hd::of(repetition.component(4));
            let system = if hd.is_empty() {
                Some(default.to_owned())
            } else {
                authorities.system(&hd)
            };
            Some(Cx { system, value })
        })
        .collect()
}

/// Why a PID cannot be read as a `Patient`, or a `Patient` written as a PID.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PidError {
    /// PID-7 is no date.
    #[error("PID-7 is no date")]
    BirthDate,
    /// No placed PID segment is at the index read.
    #[error("the message places no PID at the index read")]
    Unplaced,
    /// The terminology server refused a translation or could not be
    /// reached, so the PID or the `Patient` was not mapped; a later try may
    /// pass.
    #[error("the terminology server did not translate a table value")]
    Terminology(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// The segment map refused the PID.
    #[error("the PID could not be mapped")]
    Map(#[source] Box<SegmentError>),
    /// The segment map refused to write the `Patient` back.
    #[error("the Patient could not be written as a PID")]
    Inverse(#[source] Box<InverseError>),
}

/// Returns the PID of the HL7 v2.5 tables, the version every answer is
/// written in.
fn pid_2_5() -> Result<&'static hl7v2_types::model::Segment, PidError> {
    hl7v2_types::legacy::find("ADT_A01", "2.5")
        .and_then(|structure| segment_of(structure, "PID"))
        .ok_or(PidError::Unplaced)
}

/// Returns the `Patient` of the PID at `index` of `parsed`, carrying
/// `identifiers`, each with a system, and `links`, the `Patient.link`
/// entries.
///
/// The PID maps through `segment-pid-to-patient` of the HL7 v2-to-FHIR
/// 1.0.0 maps, with every table value translated on `terminology`. PID-3 is
/// FerroPIX's own: the Manager reads each CX-4 against its identifier
/// domains (ITI TF-2 §3.8.4.1.3), so `identifiers` replace the map's
/// `identifier`. A table value the server translates to nothing is left
/// out and counted, as a value the hand-written map knew no code for was.
///
/// # Errors
/// Returns [`PidError::BirthDate`] when PID-7 is valued and no date,
/// [`PidError::Terminology`] when the terminology server refuses a
/// translation or cannot be reached, and the other variants when the maps
/// refuse the PID.
pub async fn patient(
    parsed: &Parsed,
    index: usize,
    identifiers: &[(String, String)],
    links: Vec<Value>,
    terminology: &Client,
) -> Result<Object, PidError> {
    let mapped = single::segment(
        parsed,
        index,
        "Patient",
        Maps::corrected(),
        Some(terminology),
    )
    .await
    .map_err(|error| match error {
        SegmentError::Map(MapError::Terminology { source, .. }) => PidError::Terminology(source),
        other => PidError::Map(Box::new(other)),
    })?;
    if mapped.outcomes().iter().any(birth_date_refused) {
        return Err(PidError::BirthDate);
    }
    for outcome in mapped.outcomes() {
        tracing::debug!(kind = outcome.kind(), "a PID element is not mapped");
    }
    let Some(Value::Object(mut patient)) = mapped.resource().map(|built| built.resource.clone())
    else {
        return Err(PidError::Unplaced);
    };
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
    if !links.is_empty() {
        patient.insert(text("link"), Value::Array(links));
    }
    Ok(patient)
}

/// Whether `outcome` is PID-7 left out as no date.
fn birth_date_refused(outcome: &Outcome) -> bool {
    match outcome {
        Outcome::InvalidValue { at, .. } | Outcome::Unconvertible { at, .. } => {
            at.segment == "PID" && at.field == Some(7)
        }
        _ => false,
    }
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
    /// No name: an empty first PID-5 and a second whose only component is
    /// the name type `S`, for ITI-9 (ITI TF-2 §3.9.4.2.2.5).
    Pseudonymous,
    /// A single space in PID-5, for ITI-10 (ITI TF-2 §3.10.4.1.2.3).
    Space,
}

/// Writes the PID segment `set_id` of `identifiers` with no demographics,
/// PID-5 as `shown` names it.
#[must_use]
pub fn segment(
    authorities: &Authorities,
    set_id: usize,
    identifiers: &[(String, String)],
    shown: Shown,
) -> Segment {
    let mut pid = Segment::new("PID", Vec::<Field>::new());
    identify(&mut pid, authorities, set_id, identifiers);
    match shown {
        Shown::Pseudonymous => {
            let pseudo = Repetition::from_iter(["", "", "", "", "", "", "S"]);
            pid.set(5, Field::from_iter([Repetition::default(), pseudo]));
        }
        Shown::Space => pid.set(5, Field::from(" ")),
    }
    pid
}

/// Writes the PID segment `set_id` of `identifiers` with the demographics
/// of `patient`, a `Patient`'s JSON, for ITI-21 (ITI TF-2 §3.21.4.2.2.5).
///
/// The `Patient` is written back through the inverse of the maps
/// [`patient`] reads with, into the PID of HL7 v2.5. PID-1 and PID-3 are
/// FerroPIX's own: the set id, and each identifier with the assigning
/// authority of its domain.
///
/// # Errors
/// Returns [`PidError::Terminology`] when the terminology server refuses a
/// translation or cannot be reached, and [`PidError::Inverse`] when the
/// maps refuse the `Patient`.
pub async fn demographics(
    authorities: &Authorities,
    set_id: usize,
    identifiers: &[(String, String)],
    patient: &Object,
    terminology: &Client,
) -> Result<Segment, PidError> {
    let mut demographics = patient.clone();
    demographics.insert(text("resourceType"), string("Patient"));
    demographics.remove("identifier");
    demographics.remove("link");
    let inverted = inverse::segment(
        &Value::Object(demographics),
        pid_2_5()?,
        Maps::corrected(),
        Some(terminology),
    )
    .await
    .map_err(|error| match error {
        InverseError::Terminology { source, .. } => PidError::Terminology(source),
        other => PidError::Inverse(Box::new(other)),
    })?;
    for outcome in inverted.outcomes() {
        tracing::debug!(
            kind = outcome.kind(),
            "a Patient element is not written back into PID"
        );
    }
    let mut pid = inverted.into_segment();
    identify(&mut pid, authorities, set_id, identifiers);
    Ok(pid)
}

/// Sets PID-1 to `set_id` and PID-3 to `identifiers`, each CX-4 the
/// assigning authority of its domain.
fn identify(
    pid: &mut Segment,
    authorities: &Authorities,
    set_id: usize,
    identifiers: &[(String, String)],
) {
    let cx: Field = identifiers
        .iter()
        .map(|(system, value)| {
            let [namespace, universal, kind] = authorities.of_system(system);
            Repetition::from_iter([
                Component::from(value.as_str()),
                Component::default(),
                Component::default(),
                Component::from_iter([namespace, universal, kind]),
            ])
        })
        .collect();
    pid.set(1, Field::from(set_id.to_string()));
    pid.set(3, cx);
}

/// A client of a terminology stub holding the PID rows, with the stub and
/// its server, which the suites keep alive while they translate.
#[cfg(test)]
pub(crate) async fn stub_terminology() -> (
    wiremock::MockServer,
    ferropix_testkit::terminology::Translations,
    Client,
) {
    let stub = ferropix_testkit::terminology::Translations::v2_pid();
    let (server, base) = stub.serve().await.expect("the stub serves");
    let client = {
        let mut section = crate::config::section::terminology::Terminology::default();
        section.endpoint.url = ferrohealth_secret::secret_url::SecretUrl::new(base.to_string());
        section
    }
    .resolve(crate::config::Profile::Development)
    .expect("the section resolves")
    .expect("a terminology server")
    .client;
    (server, stub, client)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferrobridge_hl7v2::decode::Charset;
    use ferrobridge_hl7v2::parse::lex::lex;
    use ferrobridge_hl7v2::parse::structure::structure_declared;
    use ferrobridge_hl7v2::parse::{Parsed, grouping, profile};
    use ferrobridge_hl7v2::write::Writer;
    use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
    use fhir_types::codec::{Json as _, Path, Value};
    use fhir_types::r4::patient::Patient;

    use super::{
        PidError, Shown, birth_date, demographics, identifiers, patient, segment,
        stub_terminology as terminology,
    };
    use crate::config::section::hl7v2::authority::Authorities;

    fn feed() -> String {
        let family = FAMILY_NAMES[0];
        let given = GIVEN_NAMES[0];
        let mother = FAMILY_NAMES[1];
        let bsn = ferropix_testkit::synthetic::bsn(0)
            .expect("a synthetic BSN")
            .digits();
        message(&format!(
            "PID|1||PAT-1^^^EAST~{bsn}^^^&2.16.840.1.113883.2.4.6.3&ISO~X-9^^^NOWHERE||{family}^{given}^B^^Dr^^L~{family}^{given}^^^^^N|{mother}|19800229|F|||Dorpsstraat 1^^Proefdorp^^1234 AB^NL^H||^PRN^PH^^^^^^^^^+31201234567~^NET^Internet^ada@example.org|^WPN^PH^^^^^^^^^+31207654321"
        ))
    }

    /// An ADT^A01 of HL7 v2.5 around `pid`, the third segment.
    fn message(pid: &str) -> String {
        format!(
            "MSH|^~\\&|ADT|EAST|PIX|NORTH|20261025100000||ADT^A01^ADT_A01|M-1|P|2.5\r\
             EVN||20261025100000\r\
             {pid}\r\
             PV1|1|O\r"
        )
    }

    /// `text` grouped by the structure its MSH declares.
    fn parsed(text: &str) -> Parsed {
        let lexed = lex(text, Charset::Ascii).expect("the feed lexes");
        let structure = structure_declared(&lexed.message).expect("a structure");
        grouping::group(lexed, structure, &profile::Profile::BASE)
    }

    fn authorities() -> Authorities {
        Authorities::new(&BTreeMap::from([(
            HOSPITAL_DOMAIN.to_owned(),
            String::from("EAST"),
        )]))
    }

    /// The `Patient` of the PID of `text`, carrying the identifiers the
    /// face reads from PID-3 in the hospital domain.
    async fn read(text: &str) -> fhir_types::codec::Object {
        let (_server, stub, client) = terminology().await;
        let parsed = parsed(text);
        let pid = parsed.message().segments().get(2).expect("a PID").clone();
        let known: Vec<(String, String)> =
            identifiers(pid.field(3), &authorities(), HOSPITAL_DOMAIN)
                .into_iter()
                .filter_map(|cx| Some((cx.system?, cx.value)))
                .collect();
        let read = patient(&parsed, 2, &known, Vec::new(), &client)
            .await
            .expect("the PID reads");
        assert!(
            stub.asked().iter().any(|(map, code)| map
                .ends_with("table-hl70001-to-administrative-gender")
                && code == "F"),
            "PID-8 is translated on the terminology server"
        );
        read
    }

    #[tokio::test]
    async fn a_pid_reads_as_the_patient_a_fhir_source_would_send() {
        let text = feed();
        let parsed = parsed(&text);
        let pid = parsed.message().segments().get(2).expect("a PID");
        let read_ids = identifiers(pid.field(3), &authorities(), HOSPITAL_DOMAIN);
        let systems: Vec<Option<&str>> = read_ids.iter().map(|cx| cx.system.as_deref()).collect();
        assert_eq!(
            systems,
            vec![
                Some(HOSPITAL_DOMAIN),
                Some("urn:oid:2.16.840.1.113883.2.4.6.3"),
                None
            ]
        );
        let object = read(&text).await;
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

    #[tokio::test]
    async fn the_telecom_follows_the_v2_to_fhir_maps_and_holds_r4() {
        let object = read(&message(
            "PID|1||PAT-1^^^EAST||Proefsma|||F|||||^PRS^CP^^^^^^^^^+31600000000~^^FX^^^^^^^^^+31200000001~^^^^^^^^^^^+31200000003",
        ))
        .await;
        let telecom: Vec<String> = object
            .get("telecom")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|point| serde_json::to_string(point).expect("a ContactPoint serializes"))
            .collect();
        assert_eq!(
            telecom,
            [
                r#"{"system":"phone","use":"mobile","value":"+31600000000"}"#,
                r#"{"system":"fax","use":"home","value":"+31200000001"}"#,
                r#"{"_system":{"extension":[{"url":"http://hl7.org/fhir/StructureDefinition/data-absent-reason","valueCode":"unknown"}]},"use":"home","value":"+31200000003"}"#,
            ]
        );
    }

    #[tokio::test]
    async fn a_pid_7_that_is_no_date_is_refused() {
        let (_server, _stub, client) = terminology().await;
        let parsed = parsed(&message("PID|1||PAT-1^^^EAST||Proefsma||19801329|F"));
        let refused = patient(&parsed, 2, &[], Vec::new(), &client).await;
        assert!(matches!(refused, Err(PidError::BirthDate)), "{refused:?}");
    }

    #[tokio::test]
    async fn a_terminology_server_that_does_not_answer_is_a_terminology_refusal() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free port");
        let client = {
            let mut section = crate::config::section::terminology::Terminology::default();
            section.endpoint.url =
                ferrohealth_secret::secret_url::SecretUrl::new(format!("http://{closed}/r4"));
            section
        }
        .resolve(crate::config::Profile::Development)
        .expect("the section resolves")
        .expect("a terminology server")
        .client;
        let parsed = parsed(&message("PID|1||PAT-1^^^EAST||Proefsma||19800229|F"));
        let refused = patient(&parsed, 2, &[], Vec::new(), &client).await;
        assert!(
            matches!(refused, Err(PidError::Terminology(_))),
            "{refused:?}"
        );
        let mut found = fhir_types::codec::Object::new();
        found.insert(
            String::from("gender"),
            Value::String(String::from("female")),
        );
        let refused = demographics(&authorities(), 1, &[], &found, &client).await;
        assert!(
            matches!(refused, Err(PidError::Terminology(_))),
            "{refused:?}"
        );
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

    #[tokio::test]
    async fn a_patient_writes_back_as_the_pid_it_was_read_from() {
        let text = feed();
        let known = vec![(HOSPITAL_DOMAIN.to_owned(), String::from("PAT-1"))];
        let object = read(&text).await;
        let (_server, stub, client) = terminology().await;
        let lexed = lex(&text, Charset::Ascii).expect("the feed lexes");
        let writer = Writer::new(*lexed.message.delimiters());
        let written = writer.segment(
            &demographics(&authorities(), 1, &known, &object, &client)
                .await
                .expect("the Patient writes back"),
        );
        assert!(
            stub.asked().iter().any(|(map, code)| map
                .ends_with("table-administrative-gender-to-hl70001")
                && code == "female"),
            "the gender is translated back on the terminology server"
        );
        let family = FAMILY_NAMES[0];
        let given = GIVEN_NAMES[0];
        let mother = FAMILY_NAMES[1];
        // NOTE: v2-to-FHIR table-hl70201-to-contact-point-use maps no NET, so the email has no
        // `use` and its XTN-2, optional in the HL7 v2.5 XTN data type, stays empty.
        assert_eq!(
            written,
            format!(
                "PID|1||PAT-1^^^EAST&2.999.1.1&ISO||{family}^{given}^B^^Dr^^L~{family}^{given}^^^^^N|{mother}|19800229|F|||Dorpsstraat 1^^Proefdorp^^1234 AB^NL^H||^PRN^PH^^^^^^^^^+31201234567~^^Internet^ada@example.org|^WPN^PH^^^^^^^^^+31207654321"
            )
        );
        let pseudonymous = writer.segment(&segment(&authorities(), 1, &known, Shown::Pseudonymous));
        assert_eq!(pseudonymous, "PID|1||PAT-1^^^EAST&2.999.1.1&ISO||~^^^^^^S");
    }
}
