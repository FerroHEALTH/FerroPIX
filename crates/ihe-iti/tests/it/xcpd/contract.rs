// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The request ITI-55 defines, as it leaves the client: the SOAP 1.2
//! envelope with its WS-Addressing headers (Appendix V.3.2.2, §3.55.6.1), the
//! `PRPA_IN201305UV02` wrappers (Appendix O, Table 3.55.4.1.2.3-1), and the
//! query by the shared identifier alone (§3.55.1, §3.55.4.1.2.1).

use ihe_iti::user::OnBehalfOf;
use ihe_iti::xcpd::error::InvalidInput;
use ihe_iti::xcpd::identifier::{HomeCommunityId, Oid};
use ihe_iti::xcpd::request::RespondingGateway;
use ihe_iti::xcpd::security::XuaAssertion;
use url::Url;

use super::{
    AUTHORITY, PATIENT_VALUE, PROMPT, RECEIVER, SENDER, answering, between, client, oid, query,
    responding, sent,
};

/// A synthetic, unsigned assertion: the client embeds it as written.
const ASSERTION: &str = r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion" ID="_synthetic" Version="2.0" IssueInstant="2026-10-03T12:00:00Z"><saml2:Issuer>urn:oid:2.999.7</saml2:Issuer></saml2:Assertion>"#;

#[tokio::test]
async fn the_envelope_carries_the_ws_addressing_headers_appendix_v_requires() {
    let server = answering("no-match.xml").await;
    let gateway = responding(&server);
    let _answer = client()
        .discover(&gateway, &query(), None, &OnBehalfOf::System, PROMPT)
        .await;
    let body = sent(&server).await;

    assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(
        body.contains(r#"<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:wsa="http://www.w3.org/2005/08/addressing">"#),
        "a SOAP 1.2 envelope (Appendix V.3.1.1): {body}"
    );
    assert!(
        body.contains(r#"<wsa:Action soap:mustUnderstand="1">urn:hl7-org:v3:PRPA_IN201305UV02:CrossGatewayPatientDiscovery</wsa:Action>"#),
        "the §3.55.6.1.1 action, which must be understood (IHE-WSA101)"
    );
    let message = between(&body, "<wsa:MessageID>", "</wsa:MessageID>").unwrap_or_default();
    assert!(message.starts_with("urn:uuid:"), "a MessageID: {message}");
    assert!(
        body.contains("<wsa:ReplyTo><wsa:Address>http://www.w3.org/2005/08/addressing/anonymous</wsa:Address></wsa:ReplyTo>"),
        "a ReplyTo (IHE-WSA102), anonymous for the synchronous exchange"
    );
    assert!(body.contains(&format!("<wsa:To>{}</wsa:To>", gateway.endpoint())));
    assert!(
        !body.contains("wsse:Security"),
        "no security header without an assertion"
    );
}

#[tokio::test]
async fn the_request_is_posted_as_soap_1_2_with_its_action() {
    let server = answering("no-match.xml").await;
    let _answer = client()
        .discover(
            &responding(&server),
            &query(),
            None,
            &OnBehalfOf::System,
            PROMPT,
        )
        .await;
    let requests = server.received_requests().await.expect("recording is on");
    let media = requests[0]
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(media.starts_with("application/soap+xml"), "{media}");
    assert!(
        media.contains("action=\"urn:hl7-org:v3:PRPA_IN201305UV02:CrossGatewayPatientDiscovery\"")
    );
}

#[tokio::test]
async fn the_wrappers_hold_to_table_3_55_4_1_2_3_1() {
    let server = answering("no-match.xml").await;
    let _answer = client()
        .discover(
            &responding(&server),
            &query(),
            None,
            &OnBehalfOf::System,
            PROMPT,
        )
        .await;
    let body = sent(&server).await;

    let message =
        between(&body, "<wsa:MessageID>urn:uuid:", "</wsa:MessageID>").unwrap_or_default();
    for expected in [
        r#"<PRPA_IN201305UV02 xmlns="urn:hl7-org:v3" ITSVersion="XML_1.0">"#.to_owned(),
        format!(r#"<id root="{message}"/>"#),
        r#"<interactionId root="2.16.840.1.113883.1.6" extension="PRPA_IN201305UV02"/>"#.to_owned(),
        r#"<processingCode code="P"/>"#.to_owned(),
        r#"<processingModeCode code="T"/>"#.to_owned(),
        r#"<acceptAckCode code="AL"/>"#.to_owned(),
        format!(r#"<receiver typeCode="RCV"><device classCode="DEV" determinerCode="INSTANCE"><id root="{RECEIVER}"/></device></receiver>"#),
        format!(r#"<sender typeCode="SND"><device classCode="DEV" determinerCode="INSTANCE"><id root="{SENDER}"/></device></sender>"#),
        r#"<controlActProcess classCode="CACT" moodCode="EVN"><code code="PRPA_TE201305UV02" codeSystem="2.16.840.1.113883.1.6"/>"#.to_owned(),
        r#"<statusCode code="new"/><responseModalityCode code="R"/><responsePriorityCode code="I"/>"#.to_owned(),
    ] {
        assert!(body.contains(&expected), "{expected} in {body}");
    }
    assert_eq!(1, body.matches("<receiver ").count(), "one receiver device");
    let created = between(&body, "<creationTime value=\"", "\"/>").unwrap_or_default();
    assert_eq!(
        19,
        created.len(),
        "a TS to the second with its offset: {created}"
    );
    assert!(
        !body.contains("authorOrPerformer"),
        "no reverse-query authority is named"
    );
}

#[tokio::test]
async fn the_query_names_the_shared_identifier_and_no_demographics() {
    let server = answering("no-match.xml").await;
    let _answer = client()
        .discover(
            &responding(&server),
            &query(),
            None,
            &OnBehalfOf::System,
            PROMPT,
        )
        .await;
    let body = sent(&server).await;

    assert!(body.contains(&format!(
        r#"<parameterList><livingSubjectId><value root="{AUTHORITY}" extension="{PATIENT_VALUE}"/><semanticsText>LivingSubject.id</semanticsText></livingSubjectId></parameterList>"#
    )));
    for demographic in [
        "livingSubjectName",
        "livingSubjectBirthTime",
        "livingSubjectAdministrativeGender",
        "patientAddress",
        "patientTelecom",
        "mothersMaidenName",
    ] {
        assert!(
            !body.contains(demographic),
            "no {demographic} is sent (§3.55.1)"
        );
    }
}

#[tokio::test]
async fn the_sender_and_target_communities_are_named_when_given() {
    let server = answering("no-match.xml").await;
    let gateway = responding(&server).targeting(HomeCommunityId::new(oid("2.999.50")));
    let asked = query().sent_for(HomeCommunityId::new(oid("2.999.40")));
    let _answer = client()
        .discover(&gateway, &asked, None, &OnBehalfOf::System, PROMPT)
        .await;
    let body = sent(&server).await;

    for community in ["2.999.50", "2.999.40"] {
        assert!(
            body.contains(&format!(
                r#"<asAgent classCode="AGNT"><representedOrganization classCode="ORG" determinerCode="INSTANCE"><id root="{community}"/></representedOrganization></asAgent>"#
            )),
            "the device's organization names community {community} (§3.55.4.1.2.4, §3.55.4.1.2.5)"
        );
    }
}

#[tokio::test]
async fn an_assertion_rides_in_a_ws_security_header_as_written() {
    let server = answering("no-match.xml").await;
    let assertion = XuaAssertion::new(ASSERTION).expect("an assertion");
    let _answer = client()
        .discover(
            &responding(&server),
            &query(),
            Some(&assertion),
            &OnBehalfOf::System,
            PROMPT,
        )
        .await;
    let body = sent(&server).await;
    let security = between(&body, "<wsse:Security ", "</wsse:Security>").unwrap_or_default();
    assert!(security.starts_with(r#"xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd" soap:mustUnderstand="1">"#));
    assert!(
        security.ends_with(ASSERTION),
        "the assertion's bytes are unchanged, so its signature still verifies"
    );
    assert!(
        body.find("<wsse:Security") < body.find("</soap:Header>"),
        "the security header is a SOAP header"
    );
}

#[test]
fn a_responding_gateway_is_reached_over_https_only() {
    let device = Oid::new(RECEIVER).expect("an OID");
    let https = Url::parse("https://xcpd.example.org/RespondingGateway").expect("a URL");
    let http = Url::parse("http://xcpd.example.org/RespondingGateway").expect("a URL");
    assert!(RespondingGateway::new(https, device.clone()).is_ok());
    assert_eq!(
        Some(InvalidInput::Endpoint),
        RespondingGateway::new(http.clone(), device.clone()).err(),
        "the identifier and the assertion never cross the network in clear text (ITI TF-1 §27.4.1)"
    );
    assert!(
        RespondingGateway::unencrypted_for_development(http, device.clone()).is_ok(),
        "the development path names its risk"
    );
    for refused in [
        "ftp://xcpd.example.org/",
        "https://xcpd.example.org/#fragment",
    ] {
        let url = Url::parse(refused).expect("a URL");
        assert!(RespondingGateway::unencrypted_for_development(url, device.clone()).is_err());
    }
}

#[test]
fn an_oid_is_two_or_more_numeric_arcs() {
    for valid in [
        "2.999",
        "1.3.6.1.4.1.19376.1.2.27.3",
        "urn:oid:2.999.1",
        "0.0",
    ] {
        assert!(Oid::new(valid).is_ok(), "{valid}");
    }
    for invalid in [
        "",
        "2",
        "3.1",
        "2.",
        "2..1",
        "2.01",
        "2.a",
        "urn:uuid:2.1",
        " 2.1",
    ] {
        assert_eq!(Err(InvalidInput::Oid), Oid::new(invalid), "{invalid:?}");
    }
    assert_eq!(
        "urn:oid:2.999.1",
        Oid::new("urn:oid:2.999.1").expect("an OID").urn()
    );
}
