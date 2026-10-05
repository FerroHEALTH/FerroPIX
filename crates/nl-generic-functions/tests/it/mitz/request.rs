// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The question as Mitz receives it: a SOAP 1.2 `POST` with the
//! WS-Addressing headers and the XACML 3.0 attributes of the
//! Implementatiehandleiding Open en gesloten autorisatievraag 3.8.2
//! (§3.2.4.2, §3.2.4.4), and the `X-Request-Id` of §6.

use nl_generic_functions::identification::Ura;
use nl_generic_functions::mitz::question::{
    Bsn, CareProviderType, ClosedQuestion, DataCategory, DataHolder, DataUser, ProfessionalId,
    Purpose, RoleCode,
};
use nl_generic_functions::mitz::{ACTION, REQUEST_ID};
use secrecy::SecretString;
use wiremock::MockServer;

use super::{
    CATEGORIES, ENDPOINT, HOLDER, PATIENT, PROMPT, SOAP_XML, USER, answer, client, decided, mitz,
    question,
};

/// The one request the stub received: its headers and its body.
async fn received(server: &MockServer) -> (http::HeaderMap, String) {
    let requests = server.received_requests().await.expect("recorded requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    let body = String::from_utf8(request.body.clone()).expect("a UTF-8 body");
    (request.headers.clone(), body)
}

async fn asked() -> (http::HeaderMap, String, String) {
    let body = answer(&[
        decided("Permit", CATEGORIES[0]),
        decided("Deny", CATEGORIES[1]),
    ]);
    let server = mitz(200, SOAP_XML, &body).await;
    client(&server)
        .ask(&question(), PROMPT)
        .await
        .expect("a decision per category");
    let (headers, body) = received(&server).await;
    (headers, body, format!("{}{ENDPOINT}", server.uri()))
}

/// The attribute `id` in `body`, returned in the result when `included`,
/// followed by `value`.
fn holds(body: &str, id: &str, included: bool, value: &str) -> bool {
    let attribute = format!(
        r#"<xacml:Attribute AttributeId="{id}" IncludeInResult="{included}"><xacml:AttributeValue DataType="#
    );
    body.split(&attribute).skip(1).any(|rest| {
        rest.split("</xacml:Attribute>")
            .next()
            .is_some_and(|inner| inner.contains(value))
    })
}

#[tokio::test]
async fn the_question_is_a_soap_post_with_its_action_and_a_request_id() {
    let (headers, body, endpoint) = asked().await;
    let media = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .expect("a content type");
    assert!(media.starts_with("application/soap+xml"), "{media}");
    assert!(
        media.contains(ACTION),
        "SOAP 1.2 HTTP binding action: {media}"
    );
    let id = headers
        .get(REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .expect("§6: X-Request-Id is required");
    assert!(uuid::Uuid::parse_str(id).is_ok(), "{id}");
    assert!(
        body.contains(&format!("<wsa:MessageID>urn:uuid:{id}</wsa:MessageID>")),
        "§6: the MessageID is the X-Request-Id"
    );
    assert!(body.contains(&format!(
        r#"<wsa:Action soap:mustUnderstand="1">{ACTION}</wsa:Action>"#
    )));
    assert!(body.contains(&format!("<wsa:To>{endpoint}</wsa:To>")));
    assert!(body.contains(
        "<wsa:ReplyTo><wsa:Address>http://www.w3.org/2005/08/addressing/anonymous</wsa:Address></wsa:ReplyTo>"
    ));
    assert!(body.contains(
        r#"<xacml-samlp:XACMLAuthzDecisionQuery xmlns:xacml-samlp="urn:oasis:names:tc:xacml:3.0:profile:saml2.0:v2:schema:protocol:wd-14" xmlns:xacml="urn:oasis:names:tc:xacml:3.0:core:schema:wd-17" xmlns:hl7="urn:hl7-org:v3">"#
    ));
    assert!(
        body.contains(r#"<xacml:Request ReturnPolicyIdList="false" CombinedDecision="false">"#)
    );
}

#[tokio::test]
async fn the_resource_names_the_patient_by_bsn_and_the_holder_by_ura_and_category() {
    let (_, body, _) = asked().await;
    assert!(body.contains(
        r#"<xacml:Attributes Category="urn:oasis:names:tc:xacml:3.0:attribute-category:resource" xml:id="resource">"#
    ));
    assert!(holds(
        &body,
        "urn:oasis:names:tc:xacml:2.0:resource:resource-id",
        true,
        &format!(
            r#"<hl7:InstanceIdentifier root="2.16.840.1.113883.2.4.6.3" extension="{PATIENT}"/>"#
        ),
    ));
    assert!(holds(
        &body,
        "urn:ihe:iti:appc:2016:document-entry:healthcare-facility-type-code",
        true,
        r#"<hl7:CodedValue code="V6" codeSystem="2.16.840.1.113883.2.4.15.1060"/>"#,
    ));
    assert!(holds(
        &body,
        "urn:ihe:iti:appc:2016:author-institution:id",
        true,
        r#"<hl7:InstanceIdentifier root="2.16.528.1.1007.3.3" extension="ura-test-0001"/>"#,
    ));
}

#[tokio::test]
async fn each_data_category_is_its_own_action_returned_in_the_result() {
    let (_, body, _) = asked().await;
    for (index, category) in CATEGORIES.iter().enumerate() {
        assert!(body.contains(&format!(
            r#"<xacml:Attributes Category="urn:oasis:names:tc:xacml:3.0:attribute-category:action" xml:id="action{index}">"#
        )));
        assert!(holds(
            &body,
            "urn:ihe:iti:appc:2016:document-entry:event-code",
            true,
            &format!(
                r#"<hl7:CodedValue code="{category}" codeSystem="2.16.840.1.113883.2.4.3.111.5.10.1"/>"#
            ),
        ));
    }
}

#[tokio::test]
async fn a_person_consulting_under_mandate_is_named_in_the_subject() {
    let kind = CareProviderType::new("V6").expect("a category");
    let user = DataUser::new(
        Ura::new(USER).expect("a URA"),
        kind.clone(),
        ProfessionalId::new("2.999.10", "professional0001").expect("a professional"),
        RoleCode::new("01.015").expect("a role"),
    )
    .mandating(ProfessionalId::new("2.999.10", "assistant0001").expect("a professional"));
    let question = ClosedQuestion::new(
        Bsn::new(SecretString::from(PATIENT)).expect("a BSN"),
        DataHolder::new(Ura::new(HOLDER).expect("a URA"), kind),
        user,
        vec![DataCategory::new(CATEGORIES[0]).expect("a code")],
        Purpose::ContinuityOfCare,
    )
    .expect("a question");
    let server = mitz(200, SOAP_XML, &answer(&[decided("Permit", CATEGORIES[0])])).await;
    client(&server)
        .ask(&question, PROMPT)
        .await
        .expect("a decision");
    let (_, body) = received(&server).await;
    assert!(holds(
        &body,
        "urn:nl:otv:names:tc:1.0:subject:mandated",
        false,
        r#"<hl7:InstanceIdentifier root="2.999.10" extension="assistant0001"/>"#,
    ));
    assert!(holds(
        &body,
        "urn:oasis:names:tc:xspa:1.0:subject:purposeofuse",
        false,
        r#"<hl7:CodedValue code="COC" codeSystem="2.16.840.1.113883.1.11.20448"/>"#,
    ));
}

#[tokio::test]
async fn the_subject_names_the_professional_and_the_user_and_the_environment_the_purpose() {
    let (_, body, _) = asked().await;
    assert!(holds(
        &body,
        "urn:oasis:names:tc:xacml:2.0:subject:role",
        true,
        r#"<hl7:CodedValue code="01.015" codeSystem="2.16.840.1.113883.2.4.15.111"/>"#,
    ));
    assert!(holds(
        &body,
        "urn:ihe:iti:xua:2017:subject:provider-identifier",
        true,
        r#"<hl7:InstanceIdentifier root="2.999.10" extension="professional0001"/>"#,
    ));
    assert!(holds(
        &body,
        "urn:nl:otv:names:tc:1.0:subject:consulting-healthcare-facility-type-code",
        false,
        r#"<hl7:CodedValue code="V6" codeSystem="2.16.840.1.113883.2.4.15.1060"/>"#,
    ));
    assert!(holds(
        &body,
        "urn:nl:otv:names:tc:1.0:subject:provider-institution",
        false,
        r#"<hl7:InstanceIdentifier root="2.16.528.1.1007.3.3" extension="ura-test-0100"/>"#,
    ));
    assert!(
        !body.contains("urn:nl:otv:names:tc:1.0:subject:mandated"),
        "no one consults under mandate"
    );
    assert!(body.contains(
        r#"<xacml:Attributes Category="urn:oasis:names:tc:xacml:3.0:attribute-category:environment" xml:id="environment">"#
    ));
    assert!(holds(
        &body,
        "urn:oasis:names:tc:xspa:1.0:subject:purposeofuse",
        false,
        r#"<hl7:CodedValue code="TREAT" codeSystem="2.16.840.1.113883.1.11.20448"/>"#,
    ));
}
