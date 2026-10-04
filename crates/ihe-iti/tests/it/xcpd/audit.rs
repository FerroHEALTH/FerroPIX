// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-55 Initiating Gateway audit message (ITI TF-2 §3.55.5.1.1): one
//! per exchange, its outcome, destination and query parameters, and the
//! patient identifier only inside the redacted query.

use std::sync::{Arc, Mutex, PoisonError};

use ihe_iti::xcpd::audit::{
    AuditError, AuditEvent, AuditRecorder, EventOutcome, NetworkAccessPoint,
};
use ihe_iti::xcpd::error::XcpdError;
use ihe_iti::xcpd::identifier::HomeCommunityId;
use ihe_iti::xcpd::request::RespondingGateway;
use secrecy::ExposeSecret;
use url::Url;

use super::{
    PATIENT_VALUE, PROMPT, RECEIVER, SOAP_XML, Templated, answering, client, fixture, gateway, oid,
    query, responding,
};

/// A recorder that keeps every event.
#[derive(Default)]
struct Kept(Mutex<Vec<AuditEvent>>);

#[async_trait::async_trait]
impl AuditRecorder for Kept {
    async fn record(&self, event: AuditEvent) -> Result<(), AuditError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
        Ok(())
    }
}

/// A recorder that accepts nothing.
struct Refusing;

/// The failure the refusing recorder reports.
#[derive(Debug, thiserror::Error)]
#[error("synthetic audit repository outage")]
struct Outage;

#[async_trait::async_trait]
impl AuditRecorder for Refusing {
    async fn record(&self, _event: AuditEvent) -> Result<(), AuditError> {
        Err(AuditError(Box::new(Outage)))
    }
}

/// A recorder that never accepts, as a spool whose disk stalled does not.
struct Stalled;

#[async_trait::async_trait]
impl AuditRecorder for Stalled {
    async fn record(&self, _event: AuditEvent) -> Result<(), AuditError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn a_match_whose_event_is_not_stored_within_the_exchange_s_time_is_not_used() {
    let budget = std::time::Duration::from_millis(300);
    let slack = std::time::Duration::from_secs(3);
    let audited = client().audited(Arc::new(Stalled));
    let matched = answering("match.xml").await;
    let asked = std::time::Instant::now();
    let answer = audited
        .discover(&responding(&matched), &query(), None, budget)
        .await;
    assert!(asked.elapsed() < budget + slack, "{:?}", asked.elapsed());
    match answer {
        Err(XcpdError::Audit(AuditError(source))) => {
            assert!(source.is::<ihe_iti::recording::Late>(), "{source:?}");
        }
        other => panic!("a match not audited in time is not used (§3.55.5.1): {other:?}"),
    }
}

#[tokio::test]
async fn an_event_the_recorder_refuses_fails_the_discovery() {
    let audited = client().audited(Arc::new(Refusing));
    let matched = answering("match.xml").await;
    let answer = audited
        .discover(&responding(&matched), &query(), None, PROMPT)
        .await;
    match answer {
        Err(error @ XcpdError::Audit(_)) => {
            assert_eq!(None, error.status(), "a failure on the gateway's side");
        }
        other => panic!("a match whose audit was refused is not used (§3.55.5.1): {other:?}"),
    }
}

#[tokio::test]
async fn the_recorded_query_is_the_query_sent() {
    let kept = Arc::new(Kept::default());
    let audited = client().audited(kept.clone());
    let server = answering("no-match.xml").await;
    let _answer = audited
        .discover(&responding(&server), &query(), None, PROMPT)
        .await;
    let sent = super::sent(&server).await;
    let recorded = kept.events()[0].query.expose_secret().to_owned();
    assert!(
        sent.contains(&format!("<controlActProcess classCode=\"CACT\" moodCode=\"EVN\"><code code=\"PRPA_TE201305UV02\" codeSystem=\"2.16.840.1.113883.1.6\"/>{recorded}</controlActProcess>")),
        "the audit records the bytes the request carried"
    );
}

impl Kept {
    fn events(&self) -> Vec<AuditEvent> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[tokio::test]
async fn every_exchange_records_one_event_with_its_outcome() {
    let kept = Arc::new(Kept::default());
    let audited = client().audited(kept.clone());
    let matched = answering("match.xml").await;
    let faulting = gateway(Templated::new(500, SOAP_XML, fixture("fault.xml"))).await;
    let unreachable = RespondingGateway::unencrypted_for_development(
        Url::parse("http://127.0.0.1:0/rg").expect("a URL"),
        oid(RECEIVER),
    )
    .expect("a gateway");
    let asked = query().sent_for(HomeCommunityId::new(oid("2.999.40")));
    for target in [responding(&matched), responding(&faulting), unreachable] {
        let _answer = audited.discover(&target, &asked, None, PROMPT).await;
    }

    let events = kept.events();
    let outcomes: Vec<EventOutcome> = events.iter().map(|event| event.outcome).collect();
    assert_eq!(
        vec![
            EventOutcome::Success,
            EventOutcome::MinorFailure,
            EventOutcome::SeriousFailure
        ],
        outcomes
    );
    let first = &events[0];
    assert_eq!(std::process::id(), first.process_id);
    assert_eq!(responding(&matched).endpoint(), &first.destination);
    assert_eq!(
        Some(NetworkAccessPoint::IpAddress(
            "127.0.0.1".parse().expect("an IP")
        )),
        first.destination_access_point
    );
    assert_eq!(
        "2",
        first
            .destination_access_point
            .as_ref()
            .map_or("", NetworkAccessPoint::type_code)
    );
    assert_eq!(
        Some("urn:oid:2.999.40".to_owned()),
        first.home_community.as_ref().map(ToString::to_string),
        "the homeCommunityID the request named (§3.55.4.1.2.4)"
    );
    let segment = first.query.expose_secret();
    assert!(segment.starts_with("<queryByParameter>") && segment.ends_with("</queryByParameter>"));
    assert!(
        segment.contains(PATIENT_VALUE),
        "the query parameters as sent"
    );
}

#[tokio::test]
async fn the_event_shows_no_identifier_and_no_userinfo() {
    let kept = Arc::new(Kept::default());
    let audited = client().audited(kept.clone());
    let server = answering("no-match.xml").await;
    let mut endpoint = Url::parse(&format!("{}{}", server.uri(), super::PATH)).expect("a URL");
    endpoint.set_username("svc").expect("a user");
    endpoint
        .set_password(Some("synthetic-secret"))
        .expect("a password");
    let target =
        RespondingGateway::unencrypted_for_development(endpoint, oid(RECEIVER)).expect("a gateway");
    let _answer = audited.discover(&target, &query(), None, PROMPT).await;

    let events = kept.events();
    assert_eq!(1, events.len());
    let rendered = format!("{:?}", events[0]);
    assert!(!rendered.contains(PATIENT_VALUE), "{rendered}");
    assert!(!rendered.contains("synthetic-secret"), "{rendered}");
    assert!(!events[0].destination.as_str().contains("synthetic-secret"));
    assert!(!events[0].destination.as_str().contains("svc@"));
}

#[test]
fn the_fixed_codes_are_the_table_s() {
    use ihe_iti::xcpd::audit::{EVENT_ACTION, EVENT_ID, EVENT_TYPE, HOME_COMMUNITY_DETAIL};
    assert_eq!(("110112", "DCM", "Query"), EVENT_ID);
    assert_eq!("E", EVENT_ACTION);
    assert_eq!(
        (
            "ITI-55",
            "IHE Transactions",
            "Cross Gateway Patient Discovery"
        ),
        EVENT_TYPE
    );
    assert_eq!("ihe:homeCommunityID", HOME_COMMUNITY_DETAIL);
    let codes: Vec<&str> = [
        EventOutcome::Success,
        EventOutcome::MinorFailure,
        EventOutcome::SeriousFailure,
    ]
    .into_iter()
    .map(EventOutcome::code)
    .collect();
    assert_eq!(vec!["0", "4", "8"], codes, "DICOM PS3.15 Annex A.5");
}

#[cfg(feature = "atna")]
#[tokio::test]
async fn the_dicom_message_fills_the_initiating_gateway_table() {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use ihe_iti::atna::message::{AccessPoint, AuditSource};

    let kept = Arc::new(Kept::default());
    let audited = client().audited(kept.clone());
    let server = answering("no-match.xml").await;
    let asked = query().sent_for(HomeCommunityId::new(oid("2.999.40")));
    let _answer = audited
        .discover(&responding(&server), &asked, None, PROMPT)
        .await;
    let event = kept.events().remove(0);
    let source = AuditSource {
        id: "gateway.example.org".to_owned(),
        enterprise_site: Some("2.999.40".to_owned()),
    };
    let host = AccessPoint {
        type_code: "1",
        id: "gateway.example.org".to_owned(),
    };
    let xml = event.message(&source, &host).to_xml().expect("written");
    let xml = String::from_utf8(xml.expose_secret().to_vec()).expect("UTF-8");

    let source_participant = format!(
        "<ActiveParticipant UserID=\"http://www.w3.org/2005/08/addressing/anonymous\" AlternativeUserID=\"{}\" UserIsRequestor=\"true\" NetworkAccessPointID=\"gateway.example.org\" NetworkAccessPointTypeCode=\"1\"><RoleIDCode csd-code=\"110153\" codeSystemName=\"DCM\" originalText=\"Source Role ID\"/>",
        std::process::id()
    );
    let detail = format!(
        "<ParticipantObjectDetail type=\"ihe:homeCommunityID\" value=\"{}\"/>",
        STANDARD.encode("urn:oid:2.999.40")
    );
    let query = format!(
        "<ParticipantObjectQuery>{}</ParticipantObjectQuery>",
        STANDARD.encode(event.query.expose_secret())
    );
    for expected in [
        "<EventIdentification EventActionCode=\"E\"",
        "EventOutcomeIndicator=\"0\"",
        "<EventID csd-code=\"110112\" codeSystemName=\"DCM\" originalText=\"Query\"/>",
        "<EventTypeCode csd-code=\"ITI-55\" codeSystemName=\"IHE Transactions\" originalText=\"Cross Gateway Patient Discovery\"/>",
        &source_participant,
        "UserIsRequestor=\"false\" NetworkAccessPointID=\"127.0.0.1\" NetworkAccessPointTypeCode=\"2\"><RoleIDCode csd-code=\"110152\" codeSystemName=\"DCM\" originalText=\"Destination Role ID\"/>",
        "<AuditSourceIdentification AuditEnterpriseSiteID=\"2.999.40\" AuditSourceID=\"gateway.example.org\"/>",
        "ParticipantObjectTypeCode=\"2\" ParticipantObjectTypeCodeRole=\"24\"><ParticipantObjectIDTypeCode csd-code=\"ITI-55\"",
        &detail,
        &query,
    ] {
        assert!(xml.contains(expected), "{expected} in {xml}");
    }
    assert!(
        !xml.contains(PATIENT_VALUE),
        "the identifier travels only base64-encoded inside the query"
    );
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?><AuditMessage>"));
}
