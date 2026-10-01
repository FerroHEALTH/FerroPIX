// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The client held to the vendored `$ihe-pix` `OperationDefinition` and to the
//! IG's own ITI-83 request example (PIXm 3.1.0, §2:3.83.4.1.2).

use std::collections::BTreeSet;

use serde::Deserialize;

use super::{
    BLUE, FHIR_JSON, GREEN, OPERATION, PROMPT, client, manager, red_source, target, vendored,
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
        .cross_reference(&red_source(), &[target(BLUE), target(GREEN)], PROMPT)
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
        .cross_reference(&red_source(), &[target(BLUE), target(GREEN)], PROMPT)
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
