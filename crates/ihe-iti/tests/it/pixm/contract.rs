// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The client held to the vendored `$ihe-pix` `OperationDefinition` and to the
//! IG's own ITI-83 request example (PIXm 3.1.0, §2:3.83.4.1.2), asked by
//! `GET` and, for a posting client, by `POST` (FHIR R4 Operations §3.2.0.1).

use std::collections::BTreeSet;

use ihe_iti::pixm::{Invocation, PixmClient};
use ihe_iti::user::OnBehalfOf;
use serde::Deserialize;
use url::Url;
use wiremock::{MockServer, Request};

use super::{
    BLUE, FHIR_JSON, GREEN, OPERATION, PROMPT, client, manager, posting_client, posting_manager,
    red_source, target, vendored,
};

/// The members of the `OperationDefinition` the client's shape answers to.
#[derive(Debug, Deserialize)]
struct OperationDefinition {
    code: String,
    resource: Vec<String>,
    system: bool,
    #[serde(rename = "type")]
    type_level: bool,
    instance: bool,
    parameter: Vec<Parameter>,
}

#[derive(Debug, Deserialize)]
struct Parameter {
    name: String,
    #[serde(rename = "use")]
    direction: String,
    min: u32,
    max: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// The IG's request example: the same query as `Parameters`.
#[derive(Debug, Deserialize)]
struct RequestExample {
    parameter: Vec<RequestParameter>,
}

#[derive(Debug, Deserialize)]
struct RequestParameter {
    name: String,
    #[serde(rename = "valueString")]
    value: String,
}

fn definition() -> OperationDefinition {
    serde_json::from_str(&vendored("OperationDefinition-IHE.PIXm.pix.json"))
        .expect("the vendored $ihe-pix OperationDefinition")
}

fn names(definition: &OperationDefinition, direction: &str) -> BTreeSet<String> {
    definition
        .parameter
        .iter()
        .filter(|parameter| parameter.direction == direction)
        .map(|parameter| parameter.name.clone())
        .collect()
}

#[test]
fn the_definition_is_the_type_level_operation_the_client_calls() {
    let definition = definition();
    assert_eq!(definition.code, "ihe-pix", "the operation code");
    assert_eq!(definition.resource, ["Patient"], "on the Patient type");
    assert!(
        definition.type_level && !definition.instance && !definition.system,
        "a type-level operation: [base]/Patient/$ihe-pix"
    );
    assert!(
        OPERATION.ends_with("/Patient/$ihe-pix"),
        "the path the stub serves is the one the definition names"
    );
}

#[test]
fn the_out_parameters_are_the_two_the_client_reads() {
    let definition = definition();
    assert_eq!(
        names(&definition, "out"),
        BTreeSet::from(["targetId".to_owned(), "targetIdentifier".to_owned()]),
        "the client reads exactly these and refuses any other"
    );
    for parameter in definition
        .parameter
        .iter()
        .filter(|parameter| parameter.direction == "out")
    {
        let expected = if parameter.name == "targetIdentifier" {
            "Identifier"
        } else {
            "Reference"
        };
        assert_eq!(
            parameter.kind.as_deref(),
            Some(expected),
            "{}",
            parameter.name
        );
        assert_eq!(
            (parameter.min, parameter.max.as_str()),
            (0, "*"),
            "{}",
            parameter.name
        );
    }
}

#[tokio::test]
async fn the_request_carries_only_in_parameters_with_their_cardinality() {
    let definition = definition();
    let inputs = names(&definition, "in");
    let server = manager(
        200,
        FHIR_JSON,
        vendored("example/Parameters-pixm-response-mohralice-red-all.json"),
    )
    .await;
    client(&server)
        .cross_reference(
            &red_source(),
            &[target(BLUE), target(GREEN)],
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    assert_eq!(
        request.method.as_str(),
        "GET",
        "ITI-83 is an HTTP GET (§2:3.83.4.1.2)"
    );
    assert_eq!(
        request
            .headers
            .get("accept")
            .and_then(|value| value.to_str().ok()),
        Some(FHIR_JSON),
        "the response format (ITI TF-2 Appendix Z.6)"
    );
    let pairs: Vec<(String, String)> = request.url.query_pairs().into_owned().collect();
    for (name, _) in &pairs {
        assert!(
            inputs.contains(name),
            "{name} is an in parameter of $ihe-pix"
        );
    }
    for parameter in definition
        .parameter
        .iter()
        .filter(|parameter| parameter.direction == "in")
    {
        let count = pairs
            .iter()
            .filter(|(name, _)| *name == parameter.name)
            .count();
        let max = parameter.max.parse::<usize>().unwrap_or(usize::MAX);
        assert!(
            count <= max,
            "{} appears {count} times, max {}",
            parameter.name,
            parameter.max
        );
        if parameter.name == "sourceIdentifier" {
            assert_eq!(count, 1, "sourceIdentifier is 1..1");
        }
    }
}

#[tokio::test]
async fn the_request_is_the_igs_own_example_query() {
    let example: RequestExample = serde_json::from_str(&vendored(
        "example/Parameters-pixm-request-mohralice-red-to-blue.json",
    ))
    .expect("the vendored ITI-83 request example");
    let expected: Vec<(String, String)> = example
        .parameter
        .into_iter()
        .filter(|parameter| parameter.name != "_format")
        .map(|parameter| (parameter.name, parameter.value))
        .collect();
    let server = manager(
        200,
        FHIR_JSON,
        vendored("example/Parameters-pixm-response-mohralice-red-all.json"),
    )
    .await;
    client(&server)
        .cross_reference(
            &red_source(),
            &[target(BLUE), target(GREEN)],
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let pairs: Vec<(String, String)> = requests
        .first()
        .expect("one request")
        .url
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(
        pairs, expected,
        "the query parameters of the IG's example, without the _format the Accept header replaces"
    );
}

/// A posted `Parameters` body, read strictly: a member the Query Parameters
/// In profile does not give a request is refused.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostedParameters {
    #[serde(rename = "resourceType")]
    resource_type: String,
    parameter: Vec<PostedParameter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostedParameter {
    name: String,
    #[serde(rename = "valueString")]
    value: String,
}

/// The one request a posting client made to `server`, with its body read.
async fn posted(server: &MockServer) -> (Request, PostedParameters) {
    let requests = server.received_requests().await.expect("recorded requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    let body = serde_json::from_slice(&request.body).expect("a Parameters body");
    (request.clone(), body)
}

#[test]
fn a_client_asks_with_the_get_iti_83_prescribes_unless_told_otherwise() {
    let base = Url::parse("https://pix.example.org/fhir/").expect("a base");
    let client = PixmClient::new(base, reqwest::Client::new()).expect("a client");
    assert_eq!(
        client.invocation(),
        Invocation::Get,
        "§2:3.83.4.1.2: the HTTP GET operation shall be used"
    );
    assert_eq!(
        client.invoked_by(Invocation::Post).invocation(),
        Invocation::Post
    );
}

#[tokio::test]
async fn a_posted_request_carries_the_in_parameters_in_its_body_and_none_in_its_url() {
    let definition = definition();
    let inputs = names(&definition, "in");
    let server = posting_manager(
        200,
        FHIR_JSON,
        vendored("example/Parameters-pixm-response-mohralice-red-all.json"),
    )
    .await;
    posting_client(&server)
        .cross_reference(
            &red_source(),
            &[target(BLUE), target(GREEN)],
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
        .expect("an answer");
    let (request, body) = posted(&server).await;
    assert_eq!(
        request.method.as_str(),
        "POST",
        "an operation invoked by POST (FHIR R4 Operations §3.2.0.1)"
    );
    assert_eq!(request.url.path(), OPERATION, "the operation's endpoint");
    assert_eq!(request.url.query(), None, "the URL holds no parameter");
    for (header, expected) in [("content-type", FHIR_JSON), ("accept", FHIR_JSON)] {
        assert_eq!(
            request
                .headers
                .get(header)
                .and_then(|value| value.to_str().ok()),
            Some(expected),
            "{header} (ITI TF-2 Appendix Z.6)"
        );
    }
    assert_eq!(body.resource_type, "Parameters");
    for parameter in &body.parameter {
        assert!(
            inputs.contains(&parameter.name),
            "{} is an in parameter of $ihe-pix",
            parameter.name
        );
    }
    for parameter in definition
        .parameter
        .iter()
        .filter(|parameter| parameter.direction == "in")
    {
        let count = body
            .parameter
            .iter()
            .filter(|posted| posted.name == parameter.name)
            .count();
        let max = parameter.max.parse::<usize>().unwrap_or(usize::MAX);
        assert!(
            count <= max,
            "{} appears {count} times, max {}",
            parameter.name,
            parameter.max
        );
        if parameter.name == "sourceIdentifier" {
            assert_eq!(count, 1, "sourceIdentifier is 1..1");
        }
    }
}

#[tokio::test]
async fn the_posted_body_is_the_igs_own_example_request() {
    let example: RequestExample = serde_json::from_str(&vendored(
        "example/Parameters-pixm-request-mohralice-red-to-blue.json",
    ))
    .expect("the vendored ITI-83 request example");
    let expected: Vec<(String, String)> = example
        .parameter
        .into_iter()
        .filter(|parameter| parameter.name != "_format")
        .map(|parameter| (parameter.name, parameter.value))
        .collect();
    let server = posting_manager(
        200,
        FHIR_JSON,
        vendored("example/Parameters-pixm-response-mohralice-red-all.json"),
    )
    .await;
    posting_client(&server)
        .cross_reference(
            &red_source(),
            &[target(BLUE), target(GREEN)],
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
        .expect("an answer");
    let (_, body) = posted(&server).await;
    let sent: Vec<(String, String)> = body
        .parameter
        .into_iter()
        .map(|parameter| (parameter.name, parameter.value))
        .collect();
    assert_eq!(
        sent, expected,
        "the parameters of the IG's example, without the _format the Accept header replaces"
    );
}
