// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HL7 v2 face over PostgreSQL 18 (#85, #86, #87, #88), behind
//! `FERROPIX_E2E`: the MLLP listener driven by the testkit's sender, ITI-8
//! fed beside ITI-104, ITI-9 asked beside ITI-83 and ITI-21 beside ITI-78,
//! on one store.
//!
//! Every message is synthetic: the testkit's domains, names and BSNs.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the audit records and the FHIR answers are read as JSON values"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferropix_server::audit::Audit;
use ferropix_server::config::section::hl7v2::{Hl7v2, Sender};
use ferropix_server::hl7v2::cursor::Cursors;
use ferropix_server::hl7v2::{Face, Listener, Listening, serve_until};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::mllp;
use ferropix_testkit::synthetic::{
    FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};
use http::{Method, StatusCode};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::BSN_SYSTEM;
use super::pixm::{Harness, synthetic_seeds};

/// The namespace id of each synthetic domain.
const NAMESPACES: [(&str, &str); 3] = [
    (HOSPITAL_DOMAIN, "HOSP"),
    (LABORATORY_DOMAIN, "LAB"),
    (PRACTICE_DOMAIN, "GP"),
];

/// The frame ceiling and the frame timeout the listener runs with.
const FRAME_LIMIT: usize = 8 * 1024;
const FRAME_TIMEOUT_MS: u64 = 400;

/// A running face beside the FHIR router, on one store.
struct V2 {
    /// The FHIR router and its store.
    pix: Harness,
    /// The listener's address.
    address: SocketAddr,
    /// The readiness indicator.
    listening: Listening,
    /// The stop signal.
    stop: Option<oneshot::Sender<()>>,
    /// The listener's task.
    served: Option<JoinHandle<std::io::Result<()>>>,
    /// The face's audit spool's parent directory.
    spool: tempfile::TempDir,
}

impl V2 {
    /// Starts PostgreSQL, the router and the face, its pointers kept in
    /// `cursors`.
    async fn start(cursors: Cursors) -> Self {
        let pix = Harness::start(synthetic_seeds()).await;
        let section = Hl7v2 {
            enabled: true,
            listen: String::from("127.0.0.1:0"),
            frame_limit_bytes: FRAME_LIMIT,
            frame_timeout_ms: FRAME_TIMEOUT_MS,
            senders: vec![
                sender("ADT", "EAST", HOSPITAL_DOMAIN),
                sender("LIS", "WEST", LABORATORY_DOMAIN),
                sender("PORTAL", "NORTH", ""),
            ],
            namespaces: NAMESPACES
                .iter()
                .map(|(system, namespace)| ((*system).to_owned(), (*namespace).to_owned()))
                .collect(),
            ..Hl7v2::default()
        };
        let settings = section
            .resolve()
            .expect("the section resolves")
            .expect("the face is on");
        let spool = tempfile::tempdir().expect("a temporary directory");
        let mut audit_settings = pix.audit_settings.clone();
        audit_settings.spool_dir = spool.path().join("spool");
        let audit =
            Audit::open(&audit_settings, &pix.public_base_url).expect("the audit spool opens");
        let face = Face::new(
            &pix.store,
            &pix.matching,
            pix.deprecated,
            &pix.public_base_url,
            &audit,
            &settings,
        )
        .with_cursors(cursors);
        let socket = TcpListener::bind(settings.listen)
            .await
            .expect("the listener binds");
        let address = socket.local_addr().expect("a local address");
        let listening = Listening::default();
        let (stop, stopped) = oneshot::channel::<()>();
        let served = tokio::spawn(serve_until(
            socket,
            Arc::new(face),
            Listener::of(&settings, Duration::from_secs(5)),
            listening.clone(),
            async move {
                let _stopped = stopped.await;
            },
        ));
        Self {
            pix,
            address,
            listening,
            stop: Some(stop),
            served: Some(served),
            spool,
        }
    }

    /// Sends `message` and returns the answer.
    async fn send(&self, message: &str) -> String {
        mllp::send(self.address, message)
            .await
            .expect("the face answers")
    }

    /// The face's audit records.
    fn records(&self) -> Vec<Value> {
        std::fs::read_dir(self.spool.path().join("spool"))
            .expect("the spool reads")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                serde_json::from_str(&std::fs::read_to_string(entry.path()).expect("a record"))
                    .expect("a record is JSON")
            })
            .collect()
    }

    /// Counts the rows of `table` in schema `pix`.
    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM pix.{table}"
        )))
        .fetch_one(self.pix.store.pool())
        .await
        .expect("the table counts")
    }
}

/// One accepted sender.
fn sender(application: &str, facility: &str, domain: &str) -> Sender {
    Sender {
        application: application.to_owned(),
        facility: facility.to_owned(),
        domain: domain.to_owned(),
    }
}

/// A header from `application` at `facility` of `kind`, MSH-10 `control`.
fn msh(application: &str, facility: &str, kind: &str, control: &str) -> String {
    format!(
        "MSH|^~\\&|{application}|{facility}|FERROPIX|MPI|20261025100000||{kind}|{control}|P|2.5"
    )
}

/// An ADT message of `event` from the hospital, its PID-3 `pid3`, the
/// synthetic name at `index`, born on `born`.
fn adt(event: &str, control: &str, pid3: &str, index: usize, born: &str) -> String {
    let structure = if event == "A40" { "ADT_A39" } else { "ADT_A01" };
    format!(
        "{}\rEVN||20261025100000\rPID|1||{pid3}||{}^{}^^^^^L||{born}|F\rPV1|1|O\r",
        msh("ADT", "EAST", &format!("ADT^{event}^{structure}"), control),
        FAMILY_NAMES[index % FAMILY_NAMES.len()],
        GIVEN_NAMES[index % GIVEN_NAMES.len()],
    )
}

/// The synthetic BSN at `index`.
fn bsn(index: usize) -> String {
    ferropix_testkit::synthetic::bsn(index)
        .expect("a synthetic BSN")
        .digits()
}

/// A PID-3 repetition of the BSN at `index`.
fn bsn_cx(index: usize) -> String {
    format!("{}^^^&{BSN_SYSTEM}&URI", bsn(index))
}

/// The segments of `answer`.
fn segments(answer: &str) -> Vec<&str> {
    answer
        .split('\r')
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// MSA-1 of `answer`.
fn code(answer: &str) -> String {
    segments(answer)
        .iter()
        .find(|segment| segment.starts_with("MSA|"))
        .and_then(|msa| msa.split('|').nth(1))
        .unwrap_or_default()
        .to_owned()
}

/// QAK-2 of `answer`.
fn status(answer: &str) -> String {
    segments(answer)
        .iter()
        .find(|segment| segment.starts_with("QAK|"))
        .and_then(|qak| qak.split('|').nth(2))
        .unwrap_or_default()
        .to_owned()
}

/// ERR-2 and ERR-3.1 of every ERR of `answer`.
fn errors(answer: &str) -> Vec<(String, String)> {
    segments(answer)
        .iter()
        .filter(|segment| segment.starts_with("ERR|"))
        .map(|err| {
            let fields: Vec<&str> = err.split('|').collect();
            (
                fields.get(2).copied().unwrap_or_default().to_owned(),
                fields
                    .get(3)
                    .and_then(|code| code.split('^').next())
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

/// The identifiers of every PID-3 of `answer`, as `system|value`.
fn pid_identifiers(answer: &str) -> Vec<BTreeSet<String>> {
    let systems: BTreeMap<&str, &str> = NAMESPACES
        .iter()
        .map(|(system, namespace)| (*namespace, *system))
        .collect();
    segments(answer)
        .iter()
        .filter(|segment| segment.starts_with("PID|"))
        .map(|pid| {
            pid.split('|')
                .nth(3)
                .unwrap_or_default()
                .split('~')
                .filter(|cx| !cx.is_empty())
                .map(|cx| {
                    let parts: Vec<&str> = cx.split('^').collect();
                    let value = parts.first().copied().unwrap_or_default();
                    let hd: Vec<&str> = parts
                        .get(3)
                        .copied()
                        .unwrap_or_default()
                        .split('&')
                        .collect();
                    let system = match (hd.first(), hd.get(1), hd.get(2)) {
                        (_, Some(oid), Some(&"ISO")) => format!("urn:oid:{oid}"),
                        (_, Some(uri), Some(&"URI")) => (*uri).to_owned(),
                        (Some(namespace), _, _) => systems
                            .get(namespace)
                            .copied()
                            .unwrap_or_default()
                            .to_owned(),
                        _ => String::new(),
                    };
                    format!("{system}|{value}")
                })
                .collect()
        })
        .collect()
}

/// A PIX query from the portal for `qpd3`, wanting `qpd4`.
fn pix_query(control: &str, qpd3: &str, qpd4: &str) -> String {
    format!(
        "{}\rQPD|IHE PIX Query|T-{control}|{qpd3}|{qpd4}\rRCP|I\r",
        msh("PORTAL", "NORTH", "QBP^Q23^QBP_Q21", control)
    )
}

/// A demographics query from the portal with `qpd3`, `qpd8`, the RCP-2
/// `increment` and a DSC naming `pointer`.
fn pdq_query(
    control: &str,
    tag: &str,
    qpd3: &str,
    qpd8: &str,
    rcp2: &str,
    pointer: Option<&str>,
) -> String {
    let dsc = pointer
        .map(|pointer| format!("DSC|{pointer}|I\r"))
        .unwrap_or_default();
    format!(
        "{}\rQPD|IHE PDQ Query|{tag}|{qpd3}|||||{qpd8}\rRCP|I|{rcp2}\r{dsc}",
        msh("PORTAL", "NORTH", "QBP^Q22^QBP_Q21", control)
    )
}

/// The DSC-1 of `answer`.
fn pointer(answer: &str) -> Option<String> {
    segments(answer)
        .iter()
        .find(|segment| segment.starts_with("DSC|"))
        .and_then(|dsc| dsc.split('|').nth(1))
        .map(str::to_owned)
}

#[tokio::test]
async fn the_listener_answers_a_frame_and_refuses_what_it_cannot_read_or_accept() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    let answer = v2
        .send(&adt("A01", "F-1", "H-1^^^HOSP", 0, "19800229"))
        .await;
    assert_eq!(code(&answer), "AA", "{answer}");

    let header = msh("ADT", "EAST", "ADT^A01^ADT_A01", "F-2");
    let mut unopened = header.clone().into_bytes();
    unopened.extend_from_slice(&[mllp::END_BLOCK, mllp::CARRIAGE_RETURN]);
    let reply = mllp::exchange(v2.address, &unopened)
        .await
        .expect("the exchange runs")
        .expect("a malformed frame is answered");
    assert_eq!(code(&String::from_utf8(reply).expect("text")), "AR");

    let mut oversize = vec![mllp::START_BLOCK];
    oversize.extend_from_slice(header.as_bytes());
    oversize.extend_from_slice(b"\rNTE|1||");
    oversize.extend(std::iter::repeat_n(b'x', FRAME_LIMIT * 2));
    let reply = mllp::exchange(v2.address, &oversize)
        .await
        .expect("the exchange runs")
        .expect("an oversize frame is answered");
    assert_eq!(code(&String::from_utf8(reply).expect("text")), "AR");

    let mut stalled = vec![mllp::START_BLOCK];
    stalled.extend_from_slice(header.as_bytes());
    let reply = mllp::exchange(v2.address, &stalled)
        .await
        .expect("the exchange runs")
        .expect("a stalled frame is answered once its timeout passes");
    assert_eq!(code(&String::from_utf8(reply).expect("text")), "AR");

    let records = v2.count("source_record").await;
    let stranger =
        adt("A01", "F-3", "H-9^^^HOSP", 1, "19800229").replace("|ADT|EAST|", "|ADT|SOUTH|");
    let answer = v2.send(&stranger).await;
    assert_eq!(code(&answer), "AR", "{answer}");
    assert_eq!(
        v2.count("source_record").await,
        records,
        "nothing is applied"
    );
}

#[tokio::test]
async fn a_message_sent_again_is_acknowledged_again_and_changes_nothing() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    let message = adt("A01", "R-1", "H-1^^^HOSP", 0, "19800229");
    assert_eq!(code(&v2.send(&message).await), "AA");
    let versions = v2.count("source_record_version").await;
    let again = v2.send(&message).await;
    assert_eq!(code(&again), "AA", "{again}");
    assert_eq!(v2.count("source_record_version").await, versions);

    let refused = adt("A01", "R-2", "L-1^^^LAB", 1, "19800229");
    let first = v2.send(&refused).await;
    assert_eq!(code(&first), "AE", "{first}");
    assert_eq!(code(&v2.send(&refused).await), "AE");
    assert_eq!(v2.count("source_record_version").await, versions);
}

#[tokio::test]
async fn the_listener_stops_on_the_signal_and_finishes_the_frame_in_flight() {
    if !e2e_enabled() {
        return;
    }
    let mut v2 = V2::start(Cursors::default()).await;
    for _ in 0..100 {
        if v2.listening.is_up() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(v2.listening.is_up(), "readiness reports the socket");
    let address = v2.address;
    let in_flight = tokio::spawn(async move {
        mllp::send(address, &adt("A01", "D-1", "H-1^^^HOSP", 0, "19800229")).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    v2.stop
        .take()
        .expect("a stop")
        .send(())
        .expect("the face listens");
    let answer = in_flight
        .await
        .expect("the sender runs")
        .expect("the frame in flight is answered");
    assert_eq!(code(&answer), "AA", "{answer}");
    v2.served
        .take()
        .expect("the task")
        .await
        .expect("the listener's task ends")
        .expect("the listener drains");
    assert!(!v2.listening.is_up(), "readiness reports the socket closed");
    assert!(
        mllp::send(address, &adt("A08", "D-2", "H-1^^^HOSP", 0, "19800229"))
            .await
            .is_err(),
        "no connection is taken after the drain"
    );
}

#[tokio::test]
async fn each_trigger_event_writes_the_version_a_fhir_feed_writes() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    for (event, control) in [
        ("A01", "T-1"),
        ("A04", "T-2"),
        ("A05", "T-3"),
        ("A08", "T-4"),
    ] {
        let answer = v2
            .send(&adt(event, control, "H-1^^^HOSP", 0, "19800229"))
            .await;
        assert_eq!(code(&answer), "AA", "{event}: {answer}");
    }
    let body: fhir_types::codec::Value = serde_json::from_value(serde_json::json!({
        "resourceType": "Patient",
        "identifier": [{"system": LABORATORY_DOMAIN, "value": "L-1"}],
        "name": [{"use": "official", "family": FAMILY_NAMES[0], "given": [GIVEN_NAMES[0]]}],
        "gender": "female",
        "birthDate": "1980-02-29"
    }))
    .expect("a document");
    let fed = v2.pix.feed(LABORATORY_DOMAIN, "L-1", &body).await;
    assert_eq!(fed.status, StatusCode::CREATED, "{}", fed.body);
    let heads: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT d.system, v.channel, v.demographics::text
         FROM pix.source_record r
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         ORDER BY d.system",
    )
    .fetch_all(v2.pix.store.pool())
    .await
    .expect("the heads read");
    let by: BTreeMap<String, (String, Value)> = heads
        .into_iter()
        .map(|(system, channel, text)| {
            (
                system,
                (channel, serde_json::from_str(&text).expect("JSON")),
            )
        })
        .collect();
    let (v2_channel, from_v2) = by.get(HOSPITAL_DOMAIN).expect("the v2 record");
    let (fhir_channel, from_fhir) = by.get(LABORATORY_DOMAIN).expect("the FHIR record");
    assert_eq!(v2_channel, "v2");
    assert_eq!(fhir_channel, "fhir");
    assert_eq!(from_v2, from_fhir, "both faces write the same demographics");
    let versions: i32 = sqlx::query_scalar(
        "SELECT max(version) FROM pix.source_record_version WHERE channel = 'v2'",
    )
    .fetch_one(v2.pix.store.pool())
    .await
    .expect("the versions read");
    assert_eq!(versions, 4, "each trigger event writes a version");
}

#[tokio::test]
async fn an_a40_merges_as_the_replaced_by_feed_does() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    assert_eq!(
        code(
            &v2.send(&adt("A01", "M-1", "H-1^^^HOSP", 0, "19800229"))
                .await
        ),
        "AA"
    );
    assert_eq!(
        code(
            &v2.send(&adt("A01", "M-2", "H-2^^^HOSP", 3, "19700101"))
                .await
        ),
        "AA"
    );
    assert_eq!(v2.count("person WHERE status = 'active'").await, 2);
    let merge = adt("A40", "M-3", "H-1^^^HOSP", 0, "19800229")
        .replace("PV1|1|O\r", "MRG|H-2^^^HOSP\rPV1|1|O\r");
    let answer = v2.send(&merge).await;
    assert_eq!(code(&answer), "AA", "{answer}");
    assert_eq!(v2.count("source_record WHERE NOT active").await, 1);
    assert_eq!(v2.count("person WHERE status = 'active'").await, 1);

    let pix = v2.send(&pix_query("M-4", "H-2^^^HOSP", "")).await;
    let iti_83 = v2.pix.pix_get(HOSPITAL_DOMAIN, "H-2", &[]).await;
    assert_eq!(iti_83.status, StatusCode::OK, "{}", iti_83.body);
    assert_eq!(code(&pix), "AA", "{pix}");
    assert_eq!(
        pid_identifiers(&pix).first().cloned().unwrap_or_default(),
        iti_83.identifiers(),
        "a merged identifier answers the survivor's identifiers on both faces"
    );
    let actions: Vec<String> = v2
        .records()
        .iter()
        .filter(|record| record["subtype"][0]["code"] == "ITI-8")
        .filter_map(|record| record["action"].as_str().map(str::to_owned))
        .collect();
    assert!(
        actions.contains(&String::from("D")),
        "the subsumed side: {actions:?}"
    );
}

#[tokio::test]
async fn a_feed_outside_the_sender_s_domain_without_pid_3_or_of_another_event_is_refused() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    let elsewhere = v2
        .send(&adt("A01", "E-1", "L-1^^^LAB", 0, "19800229"))
        .await;
    assert_eq!(code(&elsewhere), "AE", "{elsewhere}");
    assert_eq!(
        errors(&elsewhere),
        vec![(String::from("PID^1^3^1^4"), String::from("204"))]
    );

    let missing = v2.send(&adt("A01", "E-2", "", 0, "19800229")).await;
    assert_eq!(code(&missing), "AE", "{missing}");
    assert!(
        errors(&missing).iter().any(|(_, code)| code == "101"),
        "{missing}"
    );

    let discharge = adt("A01", "E-3", "H-1^^^HOSP", 0, "19800229")
        .replace("ADT^A01^ADT_A01", "ADT^A03^ADT_A03");
    let answer = v2.send(&discharge).await;
    assert_eq!(code(&answer), "AR", "{answer}");
    assert_eq!(errors(&answer), vec![(String::new(), String::from("200"))]);
    assert_eq!(v2.count("source_record").await, 0);
}

#[tokio::test]
async fn the_record_of_a_feed_names_the_patient_as_its_subject() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    assert_eq!(
        code(
            &v2.send(&adt("A01", "B-1", "H-1^^^HOSP", 0, "19800229"))
                .await
        ),
        "AA"
    );
    let records = v2.records();
    let record = records
        .iter()
        .find(|record| record["subtype"][0]["code"] == "ITI-8")
        .expect("the ITI-8 record");
    assert_eq!(record["resourceType"], "AuditEvent");
    assert_eq!(record["type"]["code"], "110110");
    assert_eq!(record["action"], "C");
    assert_eq!(record["outcome"], "0");
    let patient = record["entity"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entity| entity["role"]["code"] == "1")
        .expect("the patient entity");
    assert_eq!(patient["what"]["identifier"]["system"], HOSPITAL_DOMAIN);
    assert_eq!(patient["what"]["identifier"]["value"], "H-1");
    let source = record["agent"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|agent| agent["type"]["coding"][0]["code"] == "110153")
        .expect("the source agent");
    assert_eq!(source["who"]["display"], "EAST|ADT");
}

/// Feeds the hospital's H-1 and the laboratory's L-1, both carrying the
/// BSN at 0, so the deterministic match joins them.
async fn linked(v2: &V2) {
    let hospital = adt(
        "A01",
        "X-1",
        &format!("H-1^^^HOSP~{}", bsn_cx(0)),
        0,
        "19800229",
    );
    assert_eq!(code(&v2.send(&hospital).await), "AA");
    let laboratory = adt(
        "A01",
        "X-2",
        &format!("L-1^^^LAB~{}", bsn_cx(0)),
        0,
        "19800229",
    )
    .replace("|ADT|EAST|", "|LIS|WEST|");
    let answer = v2.send(&laboratory).await;
    assert_eq!(code(&answer), "AA", "{answer}");
}

#[tokio::test]
async fn a_pix_query_answers_what_iti_83_answers() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    linked(&v2).await;
    for (qpd4, targets) in [("", vec![]), ("^^^LAB", vec![LABORATORY_DOMAIN])] {
        let answer = v2.send(&pix_query("P-1", "H-1^^^HOSP", qpd4)).await;
        assert_eq!(code(&answer), "AA", "{answer}");
        assert_eq!(status(&answer), "OK");
        let iti_83 = v2.pix.pix_get(HOSPITAL_DOMAIN, "H-1", &targets).await;
        assert_eq!(iti_83.status, StatusCode::OK, "{}", iti_83.body);
        assert_eq!(
            pid_identifiers(&answer)
                .first()
                .cloned()
                .unwrap_or_default(),
            iti_83.identifiers(),
            "QPD-4 {qpd4:?}"
        );
        assert!(
            answer.contains("||~^^^^^^S"),
            "PID-5 is the pseudo-name: {answer}"
        );
    }
    let bsn_query = v2.send(&pix_query("P-2", &bsn_cx(0), "^^^HOSP")).await;
    assert_eq!(
        pid_identifiers(&bsn_query)
            .first()
            .cloned()
            .unwrap_or_default(),
        BTreeSet::from([format!("{HOSPITAL_DOMAIN}|H-1")])
    );
    let record = v2
        .records()
        .into_iter()
        .find(|record| record["subtype"][0]["code"] == "ITI-9")
        .expect("the ITI-9 record");
    assert_eq!(record["type"]["code"], "110112");
    assert!(
        record["entity"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entity| entity["role"]["code"] == "24" && entity["query"].is_string()),
        "the record carries the query"
    );
}

#[tokio::test]
async fn an_unknown_identifier_domain_or_target_domain_answers_the_case_the_text_names() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    linked(&v2).await;
    let unknown = v2.send(&pix_query("U-1", "H-404^^^HOSP", "")).await;
    assert_eq!(
        (code(&unknown).as_str(), status(&unknown).as_str()),
        ("AE", "AE")
    );
    assert_eq!(
        errors(&unknown),
        vec![(String::from("QPD^1^3^1^1"), String::from("204"))]
    );
    let domain = v2.send(&pix_query("U-2", "H-1^^^NOWHERE", "")).await;
    assert_eq!(
        errors(&domain),
        vec![(String::from("QPD^1^3^1^4"), String::from("204"))]
    );
    let target = v2
        .send(&pix_query("U-3", "H-1^^^HOSP", "^^^LAB~^^^NOWHERE"))
        .await;
    assert_eq!(
        errors(&target),
        vec![(String::from("QPD^1^4^2"), String::from("204"))]
    );
    let nothing = v2.send(&pix_query("U-4", "H-1^^^HOSP", "^^^GP")).await;
    assert_eq!(
        (code(&nothing).as_str(), status(&nothing).as_str()),
        ("AA", "NF")
    );
    assert_eq!(pid_identifiers(&nothing), Vec::<BTreeSet<String>>::new());
}

/// The persons ITI-78 finds for `query`, by the hospital identifiers of
/// their Patients.
async fn iti_78(v2: &V2, query: &str) -> BTreeSet<String> {
    let answer = v2
        .pix
        .send(
            Method::GET,
            &format!("/fhir/Patient?{query}"),
            Some(&v2.pix.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    answer.json()["entry"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["search"]["mode"] == "match")
        .flat_map(|entry| {
            entry["resource"]["identifier"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|identifier| identifier["system"] == HOSPITAL_DOMAIN)
        .filter_map(|identifier| identifier["value"].as_str().map(str::to_owned))
        .collect()
}

/// The hospital identifiers of the PIDs of `answer`.
fn hospital_ids(answer: &str) -> BTreeSet<String> {
    pid_identifiers(answer)
        .into_iter()
        .flatten()
        .filter_map(|identifier| {
            identifier
                .strip_prefix(&format!("{HOSPITAL_DOMAIN}|"))
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn a_demographics_query_finds_the_persons_iti_78_finds() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    for (index, (name, born)) in [(0, "19800229"), (0, "19700101"), (1, "19800229")]
        .iter()
        .enumerate()
    {
        let answer = v2
            .send(&adt(
                "A01",
                &format!("D-{index}"),
                &format!("H-{index}^^^HOSP"),
                *name,
                born,
            ))
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
    }
    let family = FAMILY_NAMES[0];
    for (qpd3, query) in [
        (format!("@PID.5.1.1^{family}"), format!("family={family}")),
        (
            format!("@PID.5.1.1^{family}~@PID.7^19800229"),
            format!("family={family}&birthdate=1980-02-29"),
        ),
        (
            String::from("@PID.3.1^H-2~@PID.3.4.1^HOSP"),
            super::feeds::condition(HOSPITAL_DOMAIN, "H-2"),
        ),
    ] {
        let answer = v2.send(&pdq_query("Q-1", "T-1", &qpd3, "", "", None)).await;
        assert_eq!(code(&answer), "AA", "{answer}");
        assert_eq!(hospital_ids(&answer), iti_78(&v2, &query).await, "{qpd3}");
        assert!(!hospital_ids(&answer).is_empty(), "{qpd3}");
    }
    let filtered = v2
        .send(&pdq_query(
            "Q-2",
            "T-2",
            &format!("@PID.5.1.1^{family}"),
            "^^^GP",
            "",
            None,
        ))
        .await;
    assert_eq!(
        (code(&filtered).as_str(), status(&filtered).as_str()),
        ("AA", "NF")
    );
    let unknown = v2
        .send(&pdq_query(
            "Q-3",
            "T-3",
            &format!("@PID.5.1.1^{family}"),
            "^^^HOSP~^^^NOWHERE",
            "",
            None,
        ))
        .await;
    assert_eq!(
        errors(&unknown),
        vec![(String::from("QPD^1^8^2"), String::from("204"))]
    );
    assert!(
        v2.records()
            .iter()
            .any(|record| record["subtype"][0]["code"] == "ITI-21"),
        "the record of each query"
    );
}

#[tokio::test]
async fn a_query_continues_over_three_increments_and_a_pointer_past_its_bound_is_refused() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    for index in 0..5 {
        let born = format!("1970010{}", index + 1);
        let answer = v2
            .send(
                &adt(
                    "A01",
                    &format!("C-{index}"),
                    &format!("H-{index}^^^HOSP"),
                    0,
                    &born,
                )
                .replace(
                    &format!("^{}^^^^^L", GIVEN_NAMES[0]),
                    &format!("^{}^^^^^L", GIVEN_NAMES[index % GIVEN_NAMES.len()]),
                ),
            )
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
    }
    let qpd3 = format!("@PID.5.1.1^{}", FAMILY_NAMES[0]);
    let mut seen = BTreeSet::new();
    let mut next: Option<String> = None;
    let mut increments = Vec::new();
    for page in 0..3 {
        let answer = v2
            .send(&pdq_query(
                &format!("N-{page}"),
                "T-N",
                &qpd3,
                "",
                "2^RD",
                next.as_deref(),
            ))
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
        let ids = hospital_ids(&answer);
        increments.push(ids.len());
        seen.extend(ids);
        next = pointer(&answer);
    }
    assert_eq!(increments, vec![2, 2, 1]);
    assert_eq!(seen.len(), 5);
    assert_eq!(next, None, "the last increment names no pointer");

    let short = V2::start(Cursors::with_lifetime(Duration::ZERO)).await;
    for index in 0..3 {
        let answer = short
            .send(
                &adt(
                    "A01",
                    &format!("S-{index}"),
                    &format!("H-{index}^^^HOSP"),
                    0,
                    "19700101",
                )
                .replace(
                    &format!("^{}^^^^^L", GIVEN_NAMES[0]),
                    &format!("^{}^^^^^L", GIVEN_NAMES[index % GIVEN_NAMES.len()]),
                ),
            )
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
    }
    let first = short
        .send(&pdq_query("O-1", "T-O", &qpd3, "", "1^RD", None))
        .await;
    let stale = pointer(&first).expect("a pointer");
    for control in ["O-2", "O-3"] {
        let answer = short
            .send(&pdq_query(control, "T-O", &qpd3, "", "1^RD", Some(&stale)))
            .await;
        assert_eq!(
            (code(&answer).as_str(), status(&answer).as_str()),
            ("AE", "AE")
        );
        assert_eq!(
            errors(&answer),
            vec![(String::from("DSC^1^1"), String::from("204"))]
        );
    }
}

/// A log sink the case reads back.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// NOTE: ITI TF-2 §3.22.4.2.2.7 (a PV1 per patient) and Table 3.22-4 (a visit field searched):
// FerroPIX keeps no visit data, so ITI-22 is refused and ITI-21 is the demographics query.
#[tokio::test]
async fn a_demographics_and_visit_query_is_refused_as_unsupported() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    let before = v2.records().len();
    for (control, qpd3) in [
        ("Z-1", "@PID.5.1.1^Proefsma"),
        ("Z-2", "@PV1.3.2^389~@PV1.3.3^2"),
    ] {
        let query = pdq_query(control, &format!("T-{control}"), qpd3, "", "", None)
            .replace("QBP^Q22^QBP_Q21", "QBP^ZV1^QBP_Q21");
        let answer = v2.send(&query).await;
        assert_eq!(code(&answer), "AR", "{answer}");
        assert_eq!(errors(&answer), vec![(String::new(), String::from("200"))]);
        assert!(
            answer.contains("ITI-21"),
            "the refusal names the query to use: {answer}"
        );
        assert!(
            !segments(&answer)
                .iter()
                .any(|segment| segment.starts_with("PID|")),
            "no patient is answered: {answer}"
        );
    }
    assert_eq!(v2.records().len(), before, "a refused query is no query");
}

#[tokio::test]
async fn no_identifier_or_name_of_a_message_reaches_a_log_line() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let sink = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();
    let v2 = V2::start(Cursors::default()).await;
    let _subscriber = tracing::subscriber::set_default(subscriber);
    linked(&v2).await;
    let _pix = v2.send(&pix_query("G-1", "H-1^^^HOSP", "")).await;
    let _pdq = v2
        .send(&pdq_query(
            "L-2",
            "T-L",
            &format!("@PID.5.1.1^{}", FAMILY_NAMES[0]),
            "",
            "",
            None,
        ))
        .await;
    let mut written = capture.0.lock().unwrap().clone();
    let _flushed = written.flush();
    let log = String::from_utf8_lossy(&written);
    assert!(
        log.contains("message answered"),
        "the face logs its answers"
    );
    for secret in [
        bsn(0).as_str(),
        "H-1",
        "L-1",
        FAMILY_NAMES[0],
        GIVEN_NAMES[0],
    ] {
        assert!(!log.contains(secret), "the log shows a value of a message");
    }
}
