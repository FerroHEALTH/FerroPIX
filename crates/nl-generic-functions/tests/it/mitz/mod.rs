// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The closed authorization question against a stub Mitz, its answers shaped
//! after the XACML 3.0 example of the Implementatiehandleiding Open en
//! gesloten autorisatievraag 3.8.2 (§3.2.5.4), with synthetic values only: a
//! BSN, URAs and a professional number that are visibly no real identifier.

mod answers;
mod contract;
mod hygiene;
mod request;

use std::time::Duration;

use nl_generic_functions::identification::Ura;
use nl_generic_functions::mitz::MitzClient;
use nl_generic_functions::mitz::question::{
    Bsn, CareProviderType, ClosedQuestion, DataCategory, DataHolder, DataUser, ProfessionalId,
    Purpose, RoleCode,
};
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The BSN every question asks about.
pub(crate) const PATIENT: &str = "bsn-synthetic-0001";

/// The data holder's URA.
pub(crate) const HOLDER: &str = "ura-test-0001";

/// The data user's URA.
pub(crate) const USER: &str = "ura-test-0100";

/// The path the stub answers on.
pub(crate) const ENDPOINT: &str = "/mitz/geslotenautorisatievraag";

/// The media type of every SOAP 1.2 answer.
pub(crate) const SOAP_XML: &str = "application/soap+xml; charset=utf-8";

/// A timeout no stub answer comes near.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// The two data categories a question asks about.
pub(crate) const CATEGORIES: [&str; 2] = ["GGC002", "GGC013"];

/// The question about `patient`'s data of `categories` at the data holder.
pub(crate) fn question_about(patient: &str, categories: &[&str]) -> ClosedQuestion {
    let kind = CareProviderType::new("V6").expect("a category");
    let user = DataUser::new(
        Ura::new(USER).expect("a URA"),
        kind.clone(),
        ProfessionalId::new("2.999.10", "professional0001").expect("a professional"),
        RoleCode::new("01.015").expect("a role"),
    );
    let holder = DataHolder::new(Ura::new(HOLDER).expect("a URA"), kind);
    let categories = categories
        .iter()
        .map(|code| DataCategory::new(*code).expect("a data category"))
        .collect();
    ClosedQuestion::new(
        Bsn::new(SecretString::from(patient)).expect("a BSN"),
        holder,
        user,
        categories,
        Purpose::Treatment,
    )
    .expect("a question")
}

/// The question about [`PATIENT`]'s data of [`CATEGORIES`].
pub(crate) fn question() -> ClosedQuestion {
    question_about(PATIENT, &CATEGORIES)
}

/// A client of the stub, on its development path over `http`.
pub(crate) fn client(server: &MockServer) -> MitzClient {
    let endpoint = Url::parse(&format!("{}{ENDPOINT}", server.uri())).expect("an endpoint");
    MitzClient::unencrypted_for_development(endpoint, reqwest::Client::builder()).expect("a client")
}

/// A stub that answers every question with `status`, media type `media` and
/// `body`.
pub(crate) async fn mitz(status: u16, media: &str, body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(ENDPOINT))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), media))
        .mount(&server)
        .await;
    server
}

/// One `Result` of an answer: its decision, about `category`, `patient`
/// and the data holder `holder`, as the example of §3.2.5.4 writes it.
pub(crate) fn result(decision: &str, category: &str, patient: &str, holder: &str) -> String {
    format!(
        r#"<Result>
  <Decision>{decision}</Decision>
  <Attributes Category="urn:oasis:names:tc:xacml:3.0:attribute-category:resource" xml:id="resource">
    <Attribute AttributeId="urn:oasis:names:tc:xacml:2.0:resource:resource-id" IncludeInResult="true">
      <AttributeValue DataType="urn:hl7-org:v3#II">
        <ns9:InstanceIdentifier root="2.16.840.1.113883.2.4.6.3" extension="{patient}" xmlns:ns9="urn:hl7-org:v3"/>
      </AttributeValue>
    </Attribute>
    <Attribute AttributeId="urn:ihe:iti:appc:2016:document-entry:healthcare-facility-type-code" IncludeInResult="true">
      <AttributeValue DataType="urn:hl7-org:v3#CV">
        <ns9:CodedValue code="V6" codeSystem="2.16.840.1.113883.2.4.15.1060" xmlns:ns9="urn:hl7-org:v3"/>
      </AttributeValue>
    </Attribute>
    <Attribute AttributeId="urn:ihe:iti:appc:2016:author-institution:id" IncludeInResult="true">
      <AttributeValue DataType="urn:hl7-org:v3#II">
        <ns9:InstanceIdentifier root="2.16.528.1.1007.3.3" extension="{holder}" xmlns:ns9="urn:hl7-org:v3"/>
      </AttributeValue>
    </Attribute>
  </Attributes>
  <Attributes Category="urn:oasis:names:tc:xacml:3.0:attribute-category:action" xml:id="action0">
    <Attribute AttributeId="urn:ihe:iti:appc:2016:document-entry:event-code" IncludeInResult="true">
      <AttributeValue DataType="urn:hl7-org:v3#CV">
        <ns9:CodedValue code="{category}" codeSystem="2.16.840.1.113883.2.4.3.111.5.10.1" xmlns:ns9="urn:hl7-org:v3"/>
      </AttributeValue>
    </Attribute>
  </Attributes>
  <Attributes Category="urn:oasis:names:tc:xacml:1.0:subject-category:access-subject" xml:id="subject">
    <Attribute AttributeId="urn:oasis:names:tc:xacml:2.0:subject:role" IncludeInResult="true">
      <AttributeValue DataType="urn:hl7-org:v3#CV">
        <ns9:CodedValue code="01.015" codeSystem="2.16.840.1.113883.2.4.15.111" xmlns:ns9="urn:hl7-org:v3"/>
      </AttributeValue>
    </Attribute>
  </Attributes>
</Result>"#
    )
}

/// A `Result` about [`PATIENT`] and [`HOLDER`].
pub(crate) fn decided(decision: &str, category: &str) -> String {
    result(decision, category, PATIENT, HOLDER)
}

/// The SOAP 1.2 answer whose XACML 3.0 `Response` holds `results`.
pub(crate) fn answer(results: &[String]) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://www.w3.org/2005/08/addressing">
  <s:Header>
    <a:Action s:mustUnderstand="1">urn:example:XACMLAuthzDecisionQueryResponse</a:Action>
  </s:Header>
  <s:Body>
    <Response xmlns="urn:oasis:names:tc:xacml:3.0:core:schema:wd-17">
      {}
    </Response>
  </s:Body>
</s:Envelope>"#,
        results.concat()
    )
}

/// A SOAP 1.2 fault with code `code` and reason `reason`.
pub(crate) fn fault(code: &str, reason: &str) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <s:Fault>
      <s:Code><s:Value>{code}</s:Value></s:Code>
      <s:Reason><s:Text xml:lang="en">{reason}</s:Text></s:Reason>
    </s:Fault>
  </s:Body>
</s:Envelope>"#
    )
}
