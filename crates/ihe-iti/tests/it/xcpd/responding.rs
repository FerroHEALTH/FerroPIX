// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Responding Gateway side of ITI-55 (ITI TF-2 §3.55): the query read in
//! each mode of §3.55.1, the five cases of §3.55.4.2.3 written and read back
//! by the crate's own Initiating Gateway, the `NS250` acknowledgement of a
//! Deferred request (§3.55.4.1.3), the SOAP fault, and the Responding Gateway
//! audit message (§3.55.5.1.2). Every identifier is synthetic, in the `2.999`
//! example arc.

use ihe_atna::user::OnBehalfOf;
use ihe_iti::xcpd::discovery::{Discovery, RequestedAttribute};
use ihe_iti::xcpd::error::{DetectedIssue, FaultCode, XcpdError};
use ihe_iti::xcpd::identifier::HomeCommunityId;
use ihe_iti::xcpd::responding::answer::{self, Blame, Candidate, MEDIA_TYPE, Outcome, Responder};
use ihe_iti::xcpd::responding::query::{
    self, BirthTime, PersonName, PostalAddress, Priority, Refusal,
};
use secrecy::{ExposeSecret, SecretString};
use wiremock::{Request, Respond, ResponseTemplate};

use super::{
    AUTHORITY, PATIENT_VALUE, PROMPT, RECEIVER, SENDER, client, gateway, oid, query, responding,
};

/// The responding community.
const COMMUNITY: &str = "2.999.60.1";

/// The initiating community.
const INITIATING: &str = "2.999.70.1";

/// A gateway that reads each request and answers `outcome`.
struct Answering(Outcome);

impl Respond for Answering {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let media = request
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok());
        let read = query::read(media, &request.body).expect("the request reads");
        let responder = Responder {
            device: oid(RECEIVER),
            community: HomeCommunityId::new(oid(COMMUNITY)),
        };
        let body = answer::envelope(&read, &responder, &self.0).expect("the answer writes");
        ResponseTemplate::new(200).set_body_raw(body, MEDIA_TYPE)
    }
}

/// A candidate in the responding community holding `value`.
fn candidate(value: &str) -> Candidate {
    Candidate {
        community: HomeCommunityId::new(oid(COMMUNITY)),
        patient_id: (oid("2.999.60.2"), SecretString::from(value.to_owned())),
        other_ids: vec![(oid(AUTHORITY), SecretString::from(PATIENT_VALUE))],
        names: vec![PersonName {
            given: vec![String::from("Synthia")],
            family: vec![String::from("Testa")],
        }],
        gender: Some(String::from("F")),
        birth_time: Some(String::from("19630804")),
        addresses: vec![PostalAddress {
            lines: vec![String::from("1 Example Lane")],
            city: Some(String::from("Exampleton")),
            postal_code: Some(String::from("9999 ZZ")),
            ..PostalAddress::default()
        }],
    }
}

/// Asks a gateway answering `outcome` about the synthetic patient.
async fn discover(outcome: Outcome) -> Result<Discovery, XcpdError> {
    let server = gateway_answering(outcome).await;
    client()
        .discover(
            &responding(&server),
            &query(),
            None,
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
}

/// A stub gateway answering every request with `outcome`.
async fn gateway_answering(outcome: Outcome) -> wiremock::MockServer {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(Answering(outcome))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn one_and_two_matches_are_read_back_as_cases_1_and_2() {
    let Ok(Discovery::Matched(one)) = discover(Outcome::Matched(vec![candidate("RG-1")])).await
    else {
        panic!("Case 1 reads as one match");
    };
    assert_eq!(1, one.len());
    assert_eq!(COMMUNITY, one[0].community().oid().as_str());
    let ids: Vec<(String, Option<String>)> = one[0]
        .patient_ids()
        .iter()
        .map(|id| {
            (
                id.root().expose_secret().to_owned(),
                id.extension().map(|value| value.expose_secret().to_owned()),
            )
        })
        .collect();
    assert_eq!(
        vec![(String::from("2.999.60.2"), Some(String::from("RG-1")))],
        ids
    );
    let Ok(Discovery::Matched(two)) =
        discover(Outcome::Matched(vec![candidate("RG-1"), candidate("RG-2")])).await
    else {
        panic!("Case 2 reads as two matches");
    };
    assert_eq!(2, two.len());
}

#[tokio::test]
async fn more_attributes_no_match_and_an_error_are_read_back_as_cases_3_to_5() {
    let requested = vec![
        RequestedAttribute::PatientAddress,
        RequestedAttribute::PatientTelecom,
    ];
    match discover(Outcome::MoreAttributes(requested.clone())).await {
        Ok(Discovery::MoreAttributesRequested(read)) => assert_eq!(requested, read),
        other => panic!("Case 3 reads as a request for attributes: {other:?}"),
    }
    assert!(matches!(
        discover(Outcome::NoMatch).await,
        Ok(Discovery::NoMatch)
    ));
    match discover(Outcome::Unable(vec![DetectedIssue::ResponderBusy])).await {
        Err(XcpdError::ApplicationError { issues }) => {
            assert_eq!(vec![DetectedIssue::ResponderBusy], issues);
        }
        other => panic!("Case 5 reads as an application error: {other:?}"),
    }
}

#[tokio::test]
async fn a_shared_identifier_query_reads_with_its_headers_and_wrappers() {
    let server = gateway(super::Templated::new(500, "text/plain", "")).await;
    let sent_for = query().sent_for(HomeCommunityId::new(oid(INITIATING)));
    let target = responding(&server).targeting(HomeCommunityId::new(oid(COMMUNITY)));
    let _refused = client()
        .discover(&target, &sent_for, None, &OnBehalfOf::System, PROMPT)
        .await;
    let requests = server.received_requests().await.expect("recording is on");
    let read = query::read(Some(MEDIA_TYPE), &requests[0].body).expect("the request reads");
    assert!(read.message_id.starts_with("urn:uuid:"));
    assert_eq!(None, read.reply_to, "the synchronous exchange");
    assert_eq!(Some(oid(SENDER)), read.sender_device);
    assert_eq!(Some(oid(RECEIVER)), read.receiver_device);
    assert_eq!(
        Some(INITIATING),
        read.sender_community.as_ref().map(|c| c.oid().as_str())
    );
    assert_eq!(
        Some(COMMUNITY),
        read.target_community.as_ref().map(|c| c.oid().as_str())
    );
    assert_eq!(Priority::Immediate, read.priority);
    assert_eq!(1, read.parameters.ids.len());
    assert_eq!(AUTHORITY, read.parameters.ids[0].authority().as_str());
    assert_eq!(
        PATIENT_VALUE,
        read.parameters.ids[0].value().expose_secret()
    );
    assert!(read.parameters.names.is_empty(), "no demographics");
    let segment = read.query_by_parameter.expose_secret();
    assert!(segment.starts_with("<queryByParameter"), "{segment}");
    assert!(segment.ends_with("</queryByParameter>"), "{segment}");
    assert!(
        !format!("{read:?}").contains(PATIENT_VALUE),
        "Debug shows no identifier"
    );
}

/// A demographics query and feed after the example of §3.55.4.1.2.4, every
/// value synthetic.
fn demographics_request(priority: &str, action: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:wsa="http://www.w3.org/2005/08/addressing">
 <soap:Header>
  <wsa:Action soap:mustUnderstand="1">{action}</wsa:Action>
  <wsa:MessageID>urn:uuid:7f6a1d2c-0000-4000-8000-000000000055</wsa:MessageID>
  <wsa:ReplyTo><wsa:Address>https://ig.example.org/replies</wsa:Address></wsa:ReplyTo>
  <xcpd:CorrelationTimeToLive xmlns:xcpd="urn:ihe:iti:xcpd:2009">P0Y0M7D</xcpd:CorrelationTimeToLive>
 </soap:Header>
 <soap:Body>
  <PRPA_IN201305UV02 xmlns="urn:hl7-org:v3" ITSVersion="XML_1.0">
   <id root="2.999.40.9" extension="M-1"/>
   <creationTime value="20261025120000"/>
   <interactionId root="2.16.840.1.113883.1.6" extension="PRPA_IN201305UV02"/>
   <processingCode code="T"/>
   <processingModeCode code="T"/>
   <acceptAckCode code="AL"/>
   <receiver typeCode="RCV"><device classCode="DEV" determinerCode="INSTANCE"><id root="{RECEIVER}"/></device></receiver>
   <sender typeCode="SND"><device classCode="DEV" determinerCode="INSTANCE"><id root="{SENDER}"/>
    <asAgent classCode="AGNT"><representedOrganization classCode="ORG" determinerCode="INSTANCE"><id root="{INITIATING}"/></representedOrganization></asAgent>
   </device></sender>
   <controlActProcess classCode="CACT" moodCode="EVN">
    <code code="PRPA_TE201305UV02" codeSystem="2.16.840.1.113883.1.6"/>
    <authorOrPerformer typeCode="AUT"><assignedDevice><id root="2.999.70.2"/></assignedDevice></authorOrPerformer>
    <queryByParameter>
     <queryId root="2.999.40.10" extension="Q-1"/>
     <statusCode code="new"/>
     <responseModalityCode code="R"/>
     <responsePriorityCode code="{priority}"/>
     <matchCriterionList><minimumDegreeMatch><value value="80"/><semanticsText>LivingSubject.MinimumDegreeMatch</semanticsText></minimumDegreeMatch></matchCriterionList>
     <parameterList>
      <livingSubjectAdministrativeGender><value code="F"/><semanticsText>LivingSubject.administrativeGender</semanticsText></livingSubjectAdministrativeGender>
      <livingSubjectBirthTime><value><low value="19590101"/><high value="19601231"/></value><semanticsText>LivingSubject.birthTime</semanticsText></livingSubjectBirthTime>
      <livingSubjectName><value><given>Synthia</given><given>Ann</given><family>Testa</family></value><semanticsText>LivingSubject.name</semanticsText></livingSubjectName>
      <LivingSubjectId><value root="2.999.70.2" extension="IG-1"/><semanticsText>LivingSubject.id</semanticsText></LivingSubjectId>
      <patientAddress><value><streetAddressLine>1 Example Lane</streetAddressLine><city>Exampleton</city><postalCode>9999 ZZ</postalCode></value><semanticsText>Patient.addr</semanticsText></patientAddress>
      <patientTelecom><value value="tel:+31-000-0000"/><semanticsText>Patient.telecom</semanticsText></patientTelecom>
      <mothersMaidenName><value><family>Exempla</family></value><semanticsText>Person.MothersMaidenName</semanticsText></mothersMaidenName>
     </parameterList>
    </queryByParameter>
   </controlActProcess>
  </PRPA_IN201305UV02>
 </soap:Body>
</soap:Envelope>"#
    )
}

/// The ITI-55 request action.
const ACTION: &str = "urn:hl7-org:v3:PRPA_IN201305UV02:CrossGatewayPatientDiscovery";

#[test]
fn a_demographics_query_and_feed_reads_every_parameter() {
    let body = demographics_request("I", ACTION);
    let read = query::read(Some(MEDIA_TYPE), body.as_bytes()).expect("the request reads");
    assert_eq!(
        Some("https://ig.example.org/replies"),
        read.reply_to.as_deref(),
        "asynchronous"
    );
    assert_eq!(Some("P0Y0M7D"), read.correlation_time_to_live.as_deref());
    assert_eq!(
        ("2.999.40.9".to_owned(), Some("M-1".to_owned())),
        read.transmission
    );
    assert_eq!(
        ("2.999.40.10".to_owned(), Some("Q-1".to_owned())),
        read.query_id
    );
    assert_eq!(Some(oid("2.999.70.2")), read.reverse_authority);
    let parameters = &read.parameters;
    assert_eq!(Some("F"), parameters.gender.as_deref());
    assert_eq!(
        Some(BirthTime::Between {
            low: Some(String::from("19590101")),
            high: Some(String::from("19601231"))
        }),
        parameters.birth_time
    );
    assert_eq!(
        vec![PersonName {
            given: vec![String::from("Synthia"), String::from("Ann")],
            family: vec![String::from("Testa")]
        }],
        parameters.names
    );
    assert_eq!(
        1,
        parameters.ids.len(),
        "the capitalised LivingSubjectId of the example"
    );
    assert_eq!(
        Some("9999 ZZ"),
        parameters.addresses[0].postal_code.as_deref()
    );
    assert_eq!(vec![String::from("tel:+31-000-0000")], parameters.telecom);
    assert_eq!(
        Some(vec![String::from("Exempla")]),
        parameters
            .mothers_maiden_name
            .as_ref()
            .map(|name| name.family.clone())
    );
    assert_eq!(Some(80), parameters.minimum_degree_match);
    let shown = format!("{read:?}");
    for value in ["Synthia", "Testa", "19590101", "IG-1", "Exampleton"] {
        assert!(!shown.contains(value), "Debug shows {value}");
    }
}

#[test]
fn a_request_that_is_no_iti_55_query_is_refused_without_quoting_it() {
    let good = demographics_request("I", ACTION);
    let cases = [
        (good.replace(ACTION, "urn:example:other"), Refusal::WrongAction),
        (
            good.replace("<wsa:MessageID>urn:uuid:7f6a1d2c-0000-4000-8000-000000000055</wsa:MessageID>", ""),
            Refusal::NoMessageId,
        ),
        (good.replace(r#"<queryId root="2.999.40.10" extension="Q-1"/>"#, ""), Refusal::NoQueryId),
        (
            good.replace(r#"<LivingSubjectId><value root="2.999.70.2" extension="IG-1"/><semanticsText>LivingSubject.id</semanticsText></LivingSubjectId>"#, "")
                .replace("<livingSubjectBirthTime>", "<other>")
                .replace("</livingSubjectBirthTime>", "</other>"),
            Refusal::TooFewParameters,
        ),
        (
            good.replace("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "<?xml version=\"1.0\"?><!DOCTYPE x>"),
            Refusal::DocumentType,
        ),
        (good.replace("PRPA_IN201305UV02\"/>", "PRPA_IN201309UV02\"/>"), Refusal::WrongInteraction),
    ];
    for (body, refusal) in cases {
        assert_eq!(
            Err(refusal),
            query::read(Some(MEDIA_TYPE), body.as_bytes()).map(|_| ())
        );
    }
    assert_eq!(
        Err(Refusal::NotSoap),
        query::read(Some("text/xml"), good.as_bytes()).map(|_| ())
    );
    let refusal = query::read(Some(MEDIA_TYPE), b"<soap:Envelope").expect_err("broken XML");
    assert!(!refusal.to_string().contains("Testa"));
}

#[test]
fn a_deferred_request_is_refused_with_ns250_and_a_fault_names_its_code() {
    let body = demographics_request("D", ACTION);
    let read = query::read(Some(MEDIA_TYPE), body.as_bytes()).expect("the request reads");
    assert_eq!(Priority::Deferred, read.priority);
    let responder = Responder {
        device: oid(RECEIVER),
        community: HomeCommunityId::new(oid(COMMUNITY)),
    };
    let refused = String::from_utf8(answer::deferred_refused(&read, &responder).expect("written"))
        .expect("UTF-8");
    for expected in [
        "MCCI_IN000002UV01",
        r#"<typeCode code="AE"/>"#,
        r#"code="NS250""#,
        "urn:uuid:7f6a1d2c-0000-4000-8000-000000000055",
        r#"<id root="2.999.40.9" extension="M-1"/>"#,
    ] {
        assert!(refused.contains(expected), "{expected} in {refused}");
    }
    let fault = String::from_utf8(
        answer::fault(Blame::Sender, "the request is not an ITI-55 query").expect("written"),
    )
    .expect("UTF-8");
    assert!(
        fault.contains("<soap:Value>soap:Sender</soap:Value>"),
        "{fault}"
    );
}

#[tokio::test]
async fn a_fault_the_gateway_writes_reads_back_as_a_sender_fault() {
    let body =
        String::from_utf8(answer::fault(Blame::Sender, "not an ITI-55 query").expect("written"))
            .expect("UTF-8");
    let server = gateway(super::Templated::new(400, MEDIA_TYPE, body)).await;
    match client()
        .discover(
            &responding(&server),
            &query(),
            None,
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
    {
        Err(XcpdError::Fault { code, .. }) => assert_eq!(FaultCode::Sender, code),
        other => panic!("a sender fault: {other:?}"),
    }
}

#[test]
fn the_answer_copies_the_query_and_names_the_request() {
    let body = demographics_request("I", ACTION);
    let read = query::read(Some(MEDIA_TYPE), body.as_bytes()).expect("the request reads");
    let responder = Responder {
        device: oid(RECEIVER),
        community: HomeCommunityId::new(oid(COMMUNITY)),
    };
    let answered = String::from_utf8(
        answer::envelope(
            &read,
            &responder,
            &Outcome::Matched(vec![candidate("RG-1")]),
        )
        .expect("written"),
    )
    .expect("UTF-8");
    let copy = read.query_by_parameter.expose_secret();
    assert!(
        answered.contains(copy),
        "the query copied after the queryAck"
    );
    let ack = answered.find("<queryAck>").expect("a queryAck");
    let copied = answered.find(copy).expect("the copy");
    assert!(ack < copied, "the copy follows the queryAck");
    for expected in [
        "<wsa:RelatesTo>urn:uuid:7f6a1d2c-0000-4000-8000-000000000055</wsa:RelatesTo>",
        "urn:hl7-org:v3:PRPA_IN201306UV02:CrossGatewayPatientDiscovery",
        r#"<acceptAckCode code="NE"/>"#,
        r#"<processingCode code="T"/>"#,
        r#"<queryId root="2.999.40.10" extension="Q-1"/>"#,
        r#"code="NotHealthDataLocator""#,
        r#"<id root="2.999.60.2" extension="RG-1"/>"#,
    ] {
        assert!(answered.contains(expected), "{expected} in {answered}");
    }
}

#[cfg(feature = "atna")]
#[test]
fn the_responding_gateway_audit_message_holds_to_its_table() {
    use ihe_atna::atna::message::{AccessPoint, AuditSource};
    use ihe_iti::xcpd::audit::{EventOutcome, NetworkAccessPoint};
    use ihe_iti::xcpd::identifier::Oid;
    use ihe_iti::xcpd::responding::audit::RespondingAuditEvent;

    let event = RespondingAuditEvent {
        date_time: jiff::Timestamp::now(),
        outcome: EventOutcome::Success,
        source_user: String::from("http://www.w3.org/2005/08/addressing/anonymous"),
        source_access_point: Some(NetworkAccessPoint::MachineName(String::from(
            "ig.example.org",
        ))),
        destination: url::Url::parse("https://rg.example.org/xcpd").expect("a URL"),
        process_id: 55,
        patients: vec![SecretString::from(String::from("RG-1^^^&2.999.60.2&ISO"))],
        query: SecretString::from(String::from("<queryByParameter/>")),
        home_community: Some(HomeCommunityId::new(Oid::new(INITIATING).expect("an OID"))),
    };
    assert!(
        !format!("{event:?}").contains("RG-1"),
        "Debug shows no patient"
    );
    let message = event.message(
        &AuditSource {
            id: String::from("rg.example.org"),
            enterprise_site: None,
            type_codes: Vec::new(),
        },
        &AccessPoint {
            type_code: "1",
            id: String::from("rg.example.org"),
        },
    );
    assert_eq!("E", message.event.action);
    assert_eq!(
        2,
        message.participants.len(),
        "the source and the destination"
    );
    assert!(message.participants[0].user_is_requestor);
    assert_eq!(
        Some(String::from("55")),
        message.participants[1].alternative_user_id
    );
    assert_eq!(2, message.objects.len(), "one patient and the query");
    assert_eq!(
        (Some("1"), Some("1")),
        (
            message.objects[0].type_code,
            message.objects[0].type_code_role
        )
    );
    assert_eq!(
        (Some("2"), Some("24")),
        (
            message.objects[1].type_code,
            message.objects[1].type_code_role
        )
    );
    assert_eq!("ihe:homeCommunityID", message.objects[1].details[0].0);
}
