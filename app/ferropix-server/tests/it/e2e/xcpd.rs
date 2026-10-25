// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The XCPD Responding Gateway over PostgreSQL 18 (#231), behind
//! `FERROPIX_E2E`: each case of ITI TF-2 §3.55.4.2.3 answered over the
//! store, a shared-identifier query sent by `ihe-iti`'s Initiating Gateway
//! client, the `NS250` refusal of a Deferred request, the fault of a request
//! that is no query, the audit record of every query, and no identifier or
//! name in a log line.
//!
//! Every name, date and identifier is synthetic.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use ferropix_server::audit::Audit;
use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::xcpd::{Xcpd, XcpdSettings};
use ferropix_server::outbox::{self, Channels, Wake};
use ferropix_server::xcpd::{Gateway, routes};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Request, StatusCode, header};
use ihe_atna::user::OnBehalfOf;
use ihe_iti::xcpd::XcpdClient;
use ihe_iti::xcpd::discovery::Discovery;
use ihe_iti::xcpd::error::{DetectedIssue, XcpdError};
use ihe_iti::xcpd::identifier::{HomeCommunityId, Oid, PatientIdentifier};
use ihe_iti::xcpd::request::{DiscoveryQuery, ProcessingCode, RespondingGateway};
use secrecy::{ExposeSecret as _, SecretString};
use tower::ServiceExt as _;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::decisions::{fed, patient};
use super::pixm::{Harness, synthetic_seeds};

/// The responding community.
const COMMUNITY: &str = "2.999.60.1";

/// The responding gateway's device.
const DEVICE: &str = "2.999.60.2";

/// The initiating community the gateway answers.
const INITIATING: &str = "2.999.70.1";

/// The initiating gateway's device.
const SENDER: &str = "2.999.70.2";

/// An initiating community the gateway does not answer.
const STRANGER: &str = "2.999.80.1";

/// The ITI-55 request action.
const ACTION: &str = "urn:hl7-org:v3:PRPA_IN201305UV02:CrossGatewayPatientDiscovery";

/// The SOAP 1.2 media type.
pub(super) const SOAP: &str = "application/soap+xml; charset=UTF-8";

/// The family names the store holds, which no log line may show.
const FAMILIES: [&str; 2] = ["Wolderink", "Brouwershaven"];

/// Returns the OID `text`.
fn oid(text: &str) -> Oid {
    Oid::new(text).expect("an OID")
}

/// The `[xcpd]` section of the gateway: the hospital's domain first,
/// delivering to `reply_endpoints` with a short wait between attempts.
fn settings(reply_endpoints: &[String]) -> XcpdSettings {
    section(reply_endpoints)
        .resolve()
        .expect("the section resolves")
        .expect("the gateway is on")
}

/// The `[xcpd]` section [`settings`] resolves, as written.
pub(super) fn section(reply_endpoints: &[String]) -> Xcpd {
    Xcpd {
        enabled: true,
        home_community_id: format!("urn:oid:{COMMUNITY}"),
        device: DEVICE.to_owned(),
        domains: vec![HOSPITAL_DOMAIN.to_owned(), LABORATORY_DOMAIN.to_owned()],
        initiating_communities: vec![format!("urn:oid:{INITIATING}")],
        reply_endpoints: reply_endpoints.to_vec(),
        delivery_attempts: 3,
        delivery_initial_backoff_ms: 10,
        ..Xcpd::default()
    }
}

/// Returns the gateway's router over the harness's store, scoring with
/// `matching`, recording to the harness's spool.
fn gateway(harness: &Harness, matching: &Matching) -> Router {
    gateway_with(harness, matching, settings(&[]))
}

/// Returns the gateway's router over the harness's store as `xcpd` sets it.
pub(super) fn gateway_with(harness: &Harness, matching: &Matching, xcpd: XcpdSettings) -> Router {
    let wake = Wake::default();
    let gateway = gateway_only(harness, matching, xcpd, &wake);
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let _sender = outbox::start(
        harness.store.clone(),
        Channels {
            xcpd: Some(gateway.channel()),
        },
        wake,
        stopped,
    );
    tokio::spawn(async move {
        let _held = stop;
        std::future::pending::<()>().await;
    });
    routes(gateway)
}

/// Returns the gateway over the harness's store as `xcpd` sets it, woken by
/// `wake`, with no outbox sender started.
pub(super) fn gateway_only(
    harness: &Harness,
    matching: &Matching,
    xcpd: XcpdSettings,
    wake: &Wake,
) -> Gateway {
    let mut audit_settings = harness.audit_settings.clone();
    audit_settings.spool_dir = harness.spool.path().join("spool");
    let audit = Audit::open(&audit_settings, &harness.public_base_url).expect("the spool opens");
    Gateway::new(
        &harness.store,
        matching,
        xcpd,
        &harness.public_base_url,
        &audit,
        wake.clone(),
    )
    .expect("the gateway starts")
}

/// Feeds the store: Sanne Wolderink in the hospital, and two records of
/// Ada Brouwershaven, alike enough to stay two persons under review.
pub(super) async fn seeded(harness: &Harness) {
    fed(
        harness,
        HOSPITAL_DOMAIN,
        "H-1",
        &patient(HOSPITAL_DOMAIN, "H-1", "Wolderink", "Sanne", "1966-08-17"),
    )
    .await;
    for (system, value) in [(HOSPITAL_DOMAIN, "H-2"), (LABORATORY_DOMAIN, "L-2")] {
        fed(
            harness,
            system,
            value,
            &patient(system, value, "Brouwershaven", "Ada", "1971-03-05"),
        )
        .await;
    }
}

/// A demographics query from `community` with `parameters`, at `priority`,
/// targeted at `target` when one is named.
fn request(community: &str, target: Option<&str>, priority: &str, parameters: &str) -> String {
    let receiver = target.map_or_else(String::new, |target| {
        format!(
            r#"<asAgent classCode="AGNT"><representedOrganization classCode="ORG" determinerCode="INSTANCE"><id root="{target}"/></representedOrganization></asAgent>"#
        )
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:wsa="http://www.w3.org/2005/08/addressing">
 <soap:Header>
  <wsa:Action soap:mustUnderstand="1">{ACTION}</wsa:Action>
  <wsa:MessageID>urn:uuid:7f6a1d2c-0000-4000-8000-000000000231</wsa:MessageID>
 </soap:Header>
 <soap:Body>
  <PRPA_IN201305UV02 xmlns="urn:hl7-org:v3" ITSVersion="XML_1.0">
   <id root="2.999.70.9" extension="M-1"/>
   <creationTime value="20261025120000"/>
   <interactionId root="2.16.840.1.113883.1.6" extension="PRPA_IN201305UV02"/>
   <processingCode code="P"/>
   <processingModeCode code="T"/>
   <acceptAckCode code="AL"/>
   <receiver typeCode="RCV"><device classCode="DEV" determinerCode="INSTANCE"><id root="{DEVICE}"/>{receiver}</device></receiver>
   <sender typeCode="SND"><device classCode="DEV" determinerCode="INSTANCE"><id root="{SENDER}"/>
    <asAgent classCode="AGNT"><representedOrganization classCode="ORG" determinerCode="INSTANCE"><id root="{community}"/></representedOrganization></asAgent>
   </device></sender>
   <controlActProcess classCode="CACT" moodCode="EVN">
    <code code="PRPA_TE201305UV02" codeSystem="2.16.840.1.113883.1.6"/>
    <queryByParameter>
     <queryId root="2.999.70.10" extension="Q-1"/>
     <statusCode code="new"/>
     <responseModalityCode code="R"/>
     <responsePriorityCode code="{priority}"/>
     <parameterList>{parameters}</parameterList>
    </queryByParameter>
   </controlActProcess>
  </PRPA_IN201305UV02>
 </soap:Body>
</soap:Envelope>"#
    )
}

/// The query of `request` sent asynchronously, its answer wanted at
/// `reply_to` (ITI TF-2 V.9.2.3).
pub(super) fn asynchronous(reply_to: &str, parameters: &str) -> String {
    request(INITIATING, None, "I", parameters).replacen(
        "</wsa:MessageID>",
        &format!(
            "</wsa:MessageID>\n  <wsa:ReplyTo><wsa:Address>{reply_to}</wsa:Address></wsa:ReplyTo>"
        ),
        1,
    )
}

/// The parameters naming `given` `family`, born on `born`, with `extra`.
pub(super) fn named(given: &str, family: &str, born: &str, extra: &str) -> String {
    format!(
        r#"<livingSubjectBirthTime><value value="{born}"/><semanticsText>LivingSubject.birthTime</semanticsText></livingSubjectBirthTime>
<livingSubjectName><value><given>{given}</given><family>{family}</family></value><semanticsText>LivingSubject.name</semanticsText></livingSubjectName>{extra}"#
    )
}

/// The female gender parameter.
pub(super) const FEMALE: &str = r#"<livingSubjectAdministrativeGender><value code="F"/><semanticsText>LivingSubject.administrativeGender</semanticsText></livingSubjectAdministrativeGender>"#;

/// Posts `body` as `media` and returns the status and the answer.
pub(super) async fn post(app: &Router, media: &str, body: String) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::post("/xcpd")
                .header(header::CONTENT_TYPE, media)
                .body(Body::from(body))
                .expect("a legal request"),
        )
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    (status, String::from_utf8(bytes.to_vec()).expect("UTF-8"))
}

/// Returns how many `registrationEvent`s `answer` carries.
pub(super) fn events(answer: &str) -> usize {
    answer.matches("<registrationEvent").count()
}

/// Serves `app` on a loopback port and returns its address.
async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("the gateway serves");
    });
    address
}

/// Asks the gateway at `address`, from `community`, about `value` in the
/// hospital's domain, with `ihe-iti`'s Initiating Gateway client.
async fn discover(
    address: SocketAddr,
    community: &str,
    value: &str,
) -> Result<Discovery, XcpdError> {
    let endpoint = url::Url::parse(&format!("http://{address}/xcpd")).expect("a URL");
    let target = RespondingGateway::unencrypted_for_development(endpoint, oid(DEVICE))
        .expect("a development gateway");
    let identifier = PatientIdentifier::new(
        oid(HOSPITAL_DOMAIN.trim_start_matches("urn:oid:")),
        SecretString::from(value.to_owned()),
    )
    .expect("an identifier");
    let query = DiscoveryQuery::new(oid(SENDER), identifier)
        .sent_for(HomeCommunityId::new(oid(community)))
        .processing(ProcessingCode::Production);
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    XcpdClient::new(http)
        .discover(
            &target,
            &query,
            None,
            &OnBehalfOf::System,
            Duration::from_secs(10),
        )
        .await
}

/// A log sink the case reads back.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_shared_identifier_from_the_initiating_client_is_case_one_and_audited() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let before = harness.records();
    let address = serve(gateway(&harness, &harness.matching)).await;
    let Ok(Discovery::Matched(matches)) = discover(address, INITIATING, "H-1").await else {
        panic!("Case 1");
    };
    assert_eq!(1, matches.len());
    assert_eq!(COMMUNITY, matches[0].community().oid().as_str());
    let ids = matches[0].patient_ids();
    assert_eq!("2.999.1.1", ids[0].root().expose_secret());
    assert_eq!(
        Some("H-1"),
        ids[0].extension().map(secrecy::ExposeSecret::expose_secret)
    );
    assert!(matches!(
        discover(address, INITIATING, "H-404").await,
        Ok(Discovery::NoMatch)
    ));
    let records = harness.audit_texts();
    assert_eq!(before + 2, records.len(), "one record per query");
    assert!(
        records
            .iter()
            .any(|record| record.contains("ITI-55") && record.contains("ihe:homeCommunityID")),
        "the Responding Gateway record"
    );
}

#[tokio::test]
async fn demographics_settle_cases_one_two_and_four() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let app = gateway(&harness, &harness.matching);
    let (status, one) = post(
        &app,
        SOAP,
        request(
            INITIATING,
            None,
            "I",
            &named("Sanne", "Wolderink", "19660817", FEMALE),
        ),
    )
    .await;
    assert_eq!(StatusCode::OK, status, "{one}");
    assert_eq!(1, events(&one), "Case 1: {one}");
    assert!(one.contains(r#"queryResponseCode code="OK""#), "{one}");
    assert!(one.contains(r#"root="2.999.1.1" extension="H-1""#), "{one}");
    let (_, two) = post(
        &app,
        SOAP,
        request(
            INITIATING,
            None,
            "I",
            &named("Ada", "Brouwershaven", "19710305", FEMALE),
        ),
    )
    .await;
    assert_eq!(2, events(&two), "Case 2: {two}");
    assert!(two.contains(r#"extension="H-2""#), "{two}");
    assert!(two.contains(r#"extension="L-2""#), "{two}");
    let (_, four) = post(
        &app,
        SOAP,
        request(
            INITIATING,
            None,
            "I",
            &named("Pim", "Zandvoortje", "20010101", ""),
        ),
    )
    .await;
    assert_eq!(0, events(&four), "Case 4: {four}");
    assert!(four.contains(r#"queryResponseCode code="NF""#), "{four}");
}

#[tokio::test]
async fn a_possible_match_alone_is_case_three_asking_for_the_attributes_left_out() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let mut demanding = harness.matching.clone();
    demanding.link_at = 2.0;
    demanding.review_at = 2.0;
    let app = gateway(&harness, &demanding);
    let (_, three) = post(
        &app,
        SOAP,
        request(
            INITIATING,
            None,
            "I",
            &named("Sanne", "Wolderink", "19660817", ""),
        ),
    )
    .await;
    assert_eq!(0, events(&three), "Case 3: {three}");
    assert!(three.contains(r#"queryResponseCode code="OK""#), "{three}");
    for code in [
        "LivingSubjectAdministrativeGenderRequested",
        "PatientAddressRequested",
        "PatientTelecomRequested",
        "MothersMaidenNameRequested",
    ] {
        assert!(three.contains(code), "{code}: {three}");
    }
}

#[tokio::test]
async fn an_unlisted_community_another_target_and_a_deferred_request_are_refused() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let app = gateway(&harness, &harness.matching);
    let address = serve(app.clone()).await;
    match discover(address, STRANGER, "H-1").await {
        Err(XcpdError::ApplicationError { issues }) => {
            assert_eq!(vec![DetectedIssue::AnswerNotAvailable], issues);
        }
        other => panic!("Case 5 for an unlisted community: {other:?}"),
    }
    let sanne = named("Sanne", "Wolderink", "19660817", FEMALE);
    let (_, elsewhere) = post(&app, SOAP, request(INITIATING, Some(STRANGER), "I", &sanne)).await;
    assert!(
        elsewhere.contains(r#"queryResponseCode code="AE""#),
        "{elsewhere}"
    );
    assert_eq!(0, events(&elsewhere));
    let (_, ours) = post(
        &app,
        SOAP,
        request(INITIATING, Some(COMMUNITY), "I", &sanne),
    )
    .await;
    assert_eq!(
        1,
        events(&ours),
        "a query targeted at this community: {ours}"
    );
    let (status, deferred) = post(&app, SOAP, request(INITIATING, None, "D", &sanne)).await;
    assert_eq!(StatusCode::OK, status);
    assert!(deferred.contains("MCCI_IN000002UV01"), "{deferred}");
    assert!(deferred.contains(r#"code="NS250""#), "{deferred}");
    let (status, fault) = post(&app, "text/plain", String::from("hello")).await;
    assert_eq!(StatusCode::BAD_REQUEST, status);
    assert!(fault.contains("Fault"), "{fault}");
}

#[tokio::test]
async fn no_identifier_or_name_reaches_a_log_line() {
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
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let app = gateway(&harness, &harness.matching);
    for (community, given, family) in [
        (INITIATING, "Sanne", "Wolderink"),
        (INITIATING, "Ada", "Brouwershaven"),
        (STRANGER, "Sanne", "Wolderink"),
    ] {
        let _answer = post(
            &app,
            SOAP,
            request(
                community,
                None,
                "I",
                &named(given, family, "19660817", FEMALE),
            ),
        )
        .await;
    }
    let _malformed = post(&app, SOAP, String::from("<Wolderink>H-1</Wolderink>")).await;
    let logged = String::from_utf8(capture.0.lock().unwrap().clone()).expect("UTF-8");
    assert!(logged.contains("ITI-55"), "the gateway logged: {logged}");
    for value in FAMILIES
        .iter()
        .chain(&["Sanne", "Ada", "H-1", "H-2", "L-2"])
    {
        assert!(!logged.contains(value), "{value} reached a log line");
    }
}

/// Waits until `stub` has received `count` requests and returns them.
pub(super) async fn received(stub: &MockServer, count: usize) -> Vec<wiremock::Request> {
    for _ in 0..200 {
        let requests = stub.received_requests().await.expect("recording is on");
        if requests.len() >= count {
            return requests;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the stub did not receive {count} requests");
}

#[tokio::test]
async fn an_asynchronous_query_is_accepted_and_its_answer_posted_to_its_reply_endpoint() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/replies"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&stub)
        .await;
    Mock::given(method("POST"))
        .and(path("/replies"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&stub)
        .await;
    let app = gateway_with(
        &harness,
        &harness.matching,
        settings(&[format!("{}/", stub.uri())]),
    );
    let reply_to = format!("{}/replies", stub.uri());
    let sanne = named("Sanne", "Wolderink", "19660817", FEMALE);
    let (status, body) = post(&app, SOAP, asynchronous(&reply_to, &sanne)).await;
    assert_eq!(StatusCode::ACCEPTED, status, "{body}");
    assert!(
        body.is_empty(),
        "no envelope on the request's exchange: {body}"
    );
    let requests = received(&stub, 2).await;
    assert_eq!(2, requests.len(), "retried once after the 500");
    let answer = String::from_utf8(requests[1].body.clone()).expect("UTF-8");
    assert_eq!(
        Some("application/soap+xml; charset=UTF-8"),
        requests[1]
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    );
    assert!(
        answer.contains(
            "<wsa:RelatesTo>urn:uuid:7f6a1d2c-0000-4000-8000-000000000231</wsa:RelatesTo>"
        ),
        "{answer}"
    );
    assert!(
        answer.contains(&format!(
            r#"<wsa:To soap:mustUnderstand="1">{reply_to}</wsa:To>"#
        )),
        "{answer}"
    );
    assert_eq!(1, events(&answer), "Case 1: {answer}");
    assert!(
        harness
            .audit_texts()
            .iter()
            .any(|record| record.contains("ITI-55") && record.contains(&reply_to)),
        "the record names the ReplyTo as the source"
    );
}

#[tokio::test]
async fn a_reply_endpoint_not_configured_is_refused_and_nothing_is_posted() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let stub = MockServer::start().await;
    let app = gateway_with(
        &harness,
        &harness.matching,
        settings(&[String::from("https://ig.example.org/")]),
    );
    let sanne = named("Sanne", "Wolderink", "19660817", FEMALE);
    let (status, fault) = post(
        &app,
        SOAP,
        asynchronous(&format!("{}/replies", stub.uri()), &sanne),
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, status);
    assert!(fault.contains("Fault"), "{fault}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        stub.received_requests()
            .await
            .expect("recording is on")
            .is_empty(),
        "no answer posted"
    );
}
