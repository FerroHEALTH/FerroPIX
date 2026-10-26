// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! IHE's own ITI-55 example messages, the implementation material ITI TF-2
//! Appendix W names, vendored under `docs/specs/ihe-xcpd-examples/`: the
//! Responding Gateway reads IHE's request, every element its answers write
//! is one IHE's example answers carry, and the Initiating Gateway reads
//! IHE's answers.
//!
//! Five of the twelve files are not well-formed XML upstream and one spells
//! a code otherwise than ITI TF-2 Table 3.55.4.2.2.6-1 does; the tests read
//! the files as published and say where they work around them.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ihe_atna::user::OnBehalfOf;
use ihe_iti::xcpd::discovery::{Discovery, RequestedAttribute};
use ihe_iti::xcpd::error::{DetectedIssue, XcpdError};
use ihe_iti::xcpd::identifier::{HomeCommunityId, Oid};
use ihe_iti::xcpd::responding::answer::{self, Candidate, MEDIA_TYPE, Outcome, Responder};
use ihe_iti::xcpd::responding::query::{self, BirthTime, PersonName, PostalAddress, Priority};
use quick_xml::Reader;
use quick_xml::events::Event;
use secrecy::{ExposeSecret as _, SecretString};

use super::{PROMPT, SOAP_XML, Templated, client, gateway, query as discovery_query, responding};

/// The text of IHE's example `name`.
fn example(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-xcpd-examples",
        name,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).expect("an example under docs/specs/ihe-xcpd-examples")
}

/// Every element path under the interaction of `xml`, by local name, the
/// namespace prefixes dropped and the request's interaction named as the
/// answer's, `PRPA_IN201306UV02`, so the two directions compare.
///
/// The reader resolves no namespace and does not match end tags to start
/// tags, so it reads the upstream files whose prefixes are unbound or whose
/// end tags differ from their start tags.
fn paths(xml: &str) -> BTreeSet<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    let mut open: Vec<String> = Vec::new();
    let mut found = BTreeSet::new();
    loop {
        let (name, empty) = match reader.read_event().expect("well-formed enough to tokenise") {
            Event::Start(element) => (element.local_name().as_ref().to_owned(), false),
            Event::Empty(element) => (element.local_name().as_ref().to_owned(), true),
            Event::End(_) => {
                open.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let root = open
            .iter()
            .position(|step| step == "PRPA_IN201306UV02" || step == "MCCI_IN000002UV01");
        if let Some(at) = root {
            found.insert(format!("{}/{name}", open[at..].join("/")));
        }
        if !empty {
            open.push(if name == "PRPA_IN201305UV02" {
                String::from("PRPA_IN201306UV02")
            } else {
                name
            });
        }
    }
    found
}

/// The element paths an answer to IHE's request may carry: those of IHE's
/// example answer `name`, the transmission wrapper's devices as IHE's request
/// writes them (the same `MCCI` device in either direction, ITI TF-2
/// Appendix O), and the request's own `queryByParameter`, which the answer
/// copies (Table 3.55.4.2.2.3-1).
fn allowed(name: &str) -> BTreeSet<String> {
    let theirs = paths(&example(name));
    let root = theirs
        .iter()
        .next()
        .and_then(|path| path.split('/').next())
        .unwrap_or("PRPA_IN201306UV02")
        .to_owned();
    let request = paths(&example("XCPDCrossGatewayPatientDiscoveryRequest.xml"));
    let mut allowed = theirs;
    for path in request {
        let Some(rest) = path.strip_prefix("PRPA_IN201306UV02/") else {
            continue;
        };
        let wrapper = rest.starts_with("sender/") || rest.starts_with("receiver/");
        let copy = rest.starts_with("controlActProcess/queryByParameter");
        if wrapper || (copy && root == "PRPA_IN201306UV02") {
            allowed.insert(format!("{root}/{rest}"));
        }
    }
    allowed
}

/// Asserts every element path of `ours` is one of `theirs`.
fn within(ours: &BTreeSet<String>, theirs: &BTreeSet<String>, case: &str) {
    let extra: Vec<&String> = ours.difference(theirs).collect();
    assert!(
        extra.is_empty(),
        "{case}: elements no IHE example carries: {extra:#?}"
    );
}

/// The responder IHE's examples name.
fn responder() -> Responder {
    Responder {
        device: oid("1.2.840.114350.1.13.999.234"),
        community: HomeCommunityId::new(oid("1.2.840.114350.1.13.99998.8734")),
    }
}

/// Returns the OID `text`.
fn oid(text: &str) -> Oid {
    Oid::new(text).expect("an OID")
}

/// IHE's request, read.
fn ihe_request() -> query::ReceivedQuery {
    query::read(
        Some(MEDIA_TYPE),
        example("XCPDCrossGatewayPatientDiscoveryRequest.xml").as_bytes(),
    )
    .expect("IHE's request reads")
}

/// `body`, an `PRPA_IN201306UV02` as some examples publish it alone, in the
/// SOAP 1.2 envelope ITI-55 sends it in (ITI TF-2 §3.55.6), answering
/// `{relates_to}`.
fn enveloped(body: &str) -> String {
    let body = body
        .trim_start()
        .strip_prefix(r#"<?xml version="1.0" encoding="UTF-8"?>"#)
        .unwrap_or(body);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:wsa="http://www.w3.org/2005/08/addressing">
<soap:Header>
<wsa:Action soap:mustUnderstand="1">urn:hl7-org:v3:PRPA_IN201306UV02:CrossGatewayPatientDiscovery</wsa:Action>
<wsa:RelatesTo>{{relates_to}}</wsa:RelatesTo>
</soap:Header>
<soap:Body>{body}</soap:Body>
</soap:Envelope>"#
    )
}

/// Asks a stub answering `body` with the crate's Initiating Gateway client.
async fn discovered(body: String) -> Result<Discovery, XcpdError> {
    let server = gateway(Templated::new(200, SOAP_XML, body)).await;
    client()
        .discover(
            &responding(&server),
            &discovery_query(),
            None,
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
}

#[test]
fn the_responding_gateway_reads_ihes_request() {
    let read = ihe_request();
    assert_eq!(
        "urn:uuid:a02ca8cd-86fa-4afc-a27c-16c183b2055",
        read.message_id
    );
    assert_eq!(None, read.reply_to, "the anonymous address: synchronous");
    assert_eq!(Some("P0Y0M7D"), read.correlation_time_to_live.as_deref());
    assert_eq!(
        (
            "1.2.840.114350.1.13.0.1.7.1.1".to_owned(),
            Some("35423".to_owned())
        ),
        read.transmission
    );
    assert_eq!(Some(oid("1.2.840.114350.1.13.999.567")), read.sender_device);
    assert_eq!(
        Some("1.2.3.928.955"),
        read.sender_community.as_ref().map(|c| c.oid().as_str())
    );
    assert_eq!(
        Some(oid("1.2.840.114350.1.13.999.234")),
        read.receiver_device
    );
    assert_eq!(None, read.target_community);
    assert_eq!(
        Some(oid("1.2.840.114350.1.13.99997.2.3412")),
        read.reverse_authority
    );
    assert_eq!(
        (
            "1.2.840.114350.1.13.28.1.18.5.999".to_owned(),
            Some("18204".to_owned())
        ),
        read.query_id
    );
    assert_eq!(Priority::Immediate, read.priority);
    let parameters = &read.parameters;
    assert_eq!(Some(75), parameters.minimum_degree_match);
    assert_eq!(Some("M"), parameters.gender.as_deref());
    assert_eq!(
        Some(BirthTime::At(String::from("19630804"))),
        parameters.birth_time
    );
    assert_eq!(
        vec![PersonName {
            given: vec![String::from("Jimmy")],
            family: vec![String::from("Jones")],
        }],
        parameters.names
    );
    let ids: Vec<(&str, &str)> = parameters
        .ids
        .iter()
        .map(|id| (id.authority().as_str(), id.value().expose_secret()))
        .collect();
    assert_eq!(
        vec![
            ("1.2.840.114350.1.13.99997.2.3412", "1234"),
            ("2.16.840.1.113883.4.1", "58910")
        ],
        ids
    );
}

#[test]
fn every_element_of_a_matched_answer_is_one_ihes_example_answer_carries() {
    let candidate = Candidate {
        community: responder().community,
        patient_id: (
            oid("1.2.840.114350.1.13.99998.8734"),
            SecretString::from("34827K410"),
        ),
        other_ids: vec![(
            oid("1.2.840.114350.1.13.99997.2.3412"),
            SecretString::from("38273D433"),
        )],
        names: vec![PersonName {
            given: vec![String::from("James")],
            family: vec![String::from("Jones")],
        }],
        gender: Some(String::from("M")),
        birth_time: Some(String::from("19630804")),
        addresses: vec![PostalAddress {
            lines: vec![String::from("3443 North Arctic Avenue")],
            city: Some(String::from("Some City")),
            state: Some(String::from("IL")),
            ..PostalAddress::default()
        }],
    };
    let ours = String::from_utf8(
        answer::envelope(
            &ihe_request(),
            &responder(),
            &Outcome::Matched(vec![candidate]),
        )
        .expect("written"),
    )
    .expect("UTF-8");
    within(
        &paths(&ours),
        &allowed("XCPDCrossGatewayPatientDiscoveryResponse.xml"),
        "Case 1",
    );
}

#[test]
fn every_element_of_cases_three_and_five_is_one_ihes_example_answers_carry() {
    let request = ihe_request();
    let three = String::from_utf8(
        answer::envelope(
            &request,
            &responder(),
            &Outcome::MoreAttributes(vec![
                RequestedAttribute::PatientAddress,
                RequestedAttribute::PatientTelecom,
            ]),
        )
        .expect("written"),
    )
    .expect("UTF-8");
    within(
        &paths(&three),
        &allowed("XCPDCrossGatewayPatientDiscoveryResponseForMoreDemographics.xml"),
        "Case 3",
    );
    let five = String::from_utf8(
        answer::envelope(
            &request,
            &responder(),
            &Outcome::Unable(vec![DetectedIssue::ResponderBusy]),
        )
        .expect("written"),
    )
    .expect("UTF-8");
    within(
        &paths(&five),
        &allowed("XCPDCrossGatewayPatientDiscoveryResponseAEError.xml"),
        "Case 5",
    );
}

#[test]
fn every_element_of_the_ns250_refusal_is_one_ihes_example_nack_carries() {
    let ours =
        String::from_utf8(answer::deferred_refused(&ihe_request(), &responder()).expect("written"))
            .expect("UTF-8");
    within(
        &paths(&ours),
        &allowed("XCPDCrossGatewayPatientDiscovery_Deferred_RequestNack.xml"),
        "NS250",
    );
}

#[tokio::test]
async fn the_initiating_gateway_reads_ihes_answers() {
    let response = example("XCPDCrossGatewayPatientDiscoveryResponse.xml").replace(
        "urn:uuid:1ec52e14-4aad-4ba1-b7d3-fc9812a21340",
        "{relates_to}",
    );
    let Ok(Discovery::Matched(matches)) = discovered(response).await else {
        panic!("IHE's answer reads as Case 2");
    };
    assert_eq!(2, matches.len());
    assert_eq!(
        "1.2.840.114350.1.13.99998.8734",
        matches[0].community().oid().as_str()
    );
    assert_eq!(
        "34827K410",
        matches[0].patient_ids()[0]
            .extension()
            .map(secrecy::ExposeSecret::expose_secret)
            .unwrap_or_default()
    );
    match discovered(enveloped(&example(
        "XCPDCrossGatewayPatientDiscoveryResponseForMoreDemographics.xml",
    )))
    .await
    {
        // NOTE: ITI TF-2 Table 3.55.4.2.2.6-1 names PatientAddressRequested; IHE's example spells it PatientAdressRequested.
        Ok(Discovery::MoreAttributesRequested(asked)) => assert_eq!(
            vec![
                RequestedAttribute::Other,
                RequestedAttribute::PatientTelecom
            ],
            asked
        ),
        other => panic!("IHE's answer reads as Case 3: {other:?}"),
    }
    match discovered(enveloped(&example(
        "XCPDCrossGatewayPatientDiscoveryResponseAEError.xml",
    )))
    .await
    {
        Err(XcpdError::ApplicationError { issues }) => {
            assert_eq!(vec![DetectedIssue::ResponderBusy], issues);
        }
        other => panic!("IHE's answer reads as Case 5: {other:?}"),
    }
}
