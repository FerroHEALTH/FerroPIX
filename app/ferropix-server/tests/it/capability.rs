// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `CapabilityStatement` of `GET /fhir/metadata` against the vendored
//! PIXm Manager and PDQm Supplier actor statements and against the router
//! (#69, #70).
//!
//! Every interaction and operation the statement declares is a mounted row
//! of the permission table, and every row the statement can declare is
//! declared; each requirement of `CapabilityStatement-IHE.PIXm.Manager` is
//! met but `conditionalCreate`, which no `POST` serves, and each of
//! `CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch` but the
//! `_lastUpdated` parameter, which the search does not read. The statement
//! renders in JSON and XML and refuses any other format with `406`.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the statements are read as JSON values"
)]

use std::collections::BTreeSet;

use axum::Router;
use axum::body::Body;
use ferropix_server::auth::table::{self, Mount};
use ferropix_server::fhir::Mounts;
use ferropix_server::fhir::capability::{
    self, IHE_PIX, PDQM_MATCH, PDQM_SUPPLIER, PDQM_SUPPLIER_MATCH, PIXM_MANAGER,
};
use http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt as _;

use crate::auth::{Issuer, application, settings};
use crate::support::Secrets;

/// The vendored PIXm Manager actor statement (PIXm 3.1.0).
fn vendored_manager() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/specs/ihe-pixm/package/CapabilityStatement-IHE.PIXm.Manager.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("the vendored statement reads"))
        .expect("the vendored statement is JSON")
}

/// The vendored PDQm Supplier statement with the Match Operation Option
/// (PDQm 3.2.0).
fn vendored_supplier() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/specs/ihe-pdqm/package/CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("the vendored statement reads"))
        .expect("the vendored statement is JSON")
}

/// Returns the strings of the array `values`, as a set.
fn strings(values: &Value) -> BTreeSet<String> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

/// The `(name, definition)` of every operation of `resource`, the name
/// without its `$`.
fn definitions(resource: &Value) -> BTreeSet<(String, String)> {
    resource["operation"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|operation| {
            let text = |key: &str| operation[key].as_str().unwrap_or_default().to_owned();
            (
                text("name").trim_start_matches('$').to_owned(),
                text("definition"),
            )
        })
        .collect()
}

/// Sends `GET uri` with `accept`, and returns the status, the media type
/// and the body.
async fn get(app: &Router, uri: &str, accept: Option<&str>) -> (StatusCode, String, String) {
    let mut request = Request::builder().method(Method::GET).uri(uri);
    if let Some(accept) = accept {
        request = request.header(header::ACCEPT, accept);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).expect("a legal request"))
        .await
        .expect("the router answers");
    let status = response.status();
    let media_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        media_type,
        String::from_utf8(bytes.to_vec()).expect("UTF-8"),
    )
}

/// The `(method, route)` a statement's entry for `resource_type` asks the
/// router for: an interaction on the type, or an operation on it.
fn routes_declared(resource: &Value) -> BTreeSet<(Method, String)> {
    let kind = resource["type"].as_str().expect("a type");
    let mut routes = BTreeSet::new();
    for interaction in resource["interaction"].as_array().into_iter().flatten() {
        match interaction["code"].as_str() {
            // NOTE: PIXm 3.1.0 §2:3.104.4.1.2, update and delete are conditional,
            // on the type with an identifier query.
            Some("update") => routes.insert((Method::PUT, format!("/fhir/{kind}"))),
            Some("delete") => routes.insert((Method::DELETE, format!("/fhir/{kind}"))),
            // NOTE: PDQm 3.2.0 §2:3.78.4.1.2, the Supplier searches by GET and
            // POST, and §2:3.78.4.3 reads one Patient by its id.
            Some("search-type") => {
                routes.insert((Method::GET, format!("/fhir/{kind}")));
                routes.insert((Method::POST, format!("/fhir/{kind}/_search")))
            }
            Some("read") => routes.insert((Method::GET, format!("/fhir/{kind}/{{id}}"))),
            other => panic!("an interaction the router does not know: {other:?}"),
        };
    }
    for operation in resource["operation"].as_array().into_iter().flatten() {
        let name = operation["name"].as_str().expect("a name");
        // NOTE: PDQm 3.2.0 §2:3.119.4.1.2, `$match` is invoked by POST alone.
        let methods = if name == "match" {
            vec![Method::POST]
        } else {
            vec![Method::GET, Method::POST]
        };
        for method in methods {
            routes.insert((method, format!("/fhir/{kind}/${name}")));
        }
    }
    routes
}

#[tokio::test]
async fn the_statement_meets_the_pixm_and_pdqm_actor_statements_and_names_only_mounted_routes() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let (status, media_type, text) = get(&app, "/fhir/metadata", None).await;
    assert_eq!(StatusCode::OK, status);
    assert_eq!("application/fhir+json", media_type);
    let statement: Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!("CapabilityStatement", statement["resourceType"]);
    assert_eq!("4.0.1", statement["fhirVersion"]);
    assert_eq!(
        serde_json::json!([PIXM_MANAGER, PDQM_SUPPLIER, PDQM_SUPPLIER_MATCH]),
        statement["instantiates"]
    );
    let formats: BTreeSet<&str> = statement["format"]
        .as_array()
        .expect("formats")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let vendored = vendored_manager();
    let supplier = vendored_supplier();
    for format in vendored["format"]
        .as_array()
        .expect("formats")
        .iter()
        .chain(supplier["format"].as_array().expect("formats"))
    {
        assert!(
            formats.contains(format.as_str().expect("a code")),
            "{format}: PIXm 3.1.0 §2:3.104.4.1.2, PDQm 3.2.0 §2:3.78.4.1.2.4"
        );
    }
    let resources = statement["rest"][0]["resource"]
        .as_array()
        .expect("resources");
    assert_eq!(1, resources.len(), "one entry per type");
    let patient = &resources[0];
    let wanted = &vendored["rest"][0]["resource"][0];
    let pdqm = &supplier["rest"][0]["resource"][0];
    assert_eq!(wanted["type"], patient["type"]);
    assert_eq!(pdqm["type"], patient["type"]);
    let profiles: BTreeSet<String> = strings(&wanted["supportedProfile"])
        .union(&strings(&pdqm["supportedProfile"]))
        .cloned()
        .collect();
    assert_eq!(profiles, strings(&patient["supportedProfile"]));
    assert_eq!(wanted["conditionalUpdate"], patient["conditionalUpdate"]);
    assert_eq!(wanted["conditionalDelete"], patient["conditionalDelete"]);
    assert_eq!(
        Value::Null,
        patient["conditionalCreate"],
        "no POST creates a Patient"
    );
    let codes = |resource: &Value| -> BTreeSet<String> {
        resource["interaction"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["code"].as_str().map(str::to_owned))
            .collect()
    };
    let union: BTreeSet<String> = codes(wanted).union(&codes(pdqm)).cloned().collect();
    assert_eq!(union, codes(patient));
    let union: BTreeSet<(String, String)> = definitions(wanted)
        .union(&definitions(pdqm))
        .cloned()
        .collect();
    assert_eq!(union, definitions(patient));
    assert!(definitions(patient).contains(&(String::from("ihe-pix"), IHE_PIX.to_owned())));
    assert!(definitions(patient).contains(&(String::from("match"), PDQM_MATCH.to_owned())));

    let declared = routes_declared(patient);
    for (method, path) in &declared {
        let route = table::find(method, path).unwrap_or_else(|| panic!("{method} {path}: no row"));
        assert!(Mounts::all().admits(route.mount), "{method} {path}");
    }
    let pixm: BTreeSet<(Method, String)> = table::ROUTES
        .iter()
        .filter(|route| matches!(route.mount, Mount::Pixm | Mount::PixmRemove | Mount::Pdqm))
        .map(|route| (route.method.clone(), route.path.to_owned()))
        .collect();
    assert_eq!(
        pixm, declared,
        "every PIXm and PDQm route is declared, and only those"
    );
}

#[tokio::test]
async fn the_statement_declares_every_pdqm_search_parameter_the_search_reads() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let (status, _, text) = get(&app, "/fhir/metadata", None).await;
    assert_eq!(StatusCode::OK, status);
    let statement: Value = serde_json::from_str(&text).expect("JSON");
    let parameters = |resource: &Value| -> BTreeSet<(String, String, String)> {
        resource["searchParam"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|parameter| {
                let text = |key: &str| parameter[key].as_str().unwrap_or_default().to_owned();
                (text("name"), text("type"), text("definition"))
            })
            .collect()
    };
    let mut read = parameters(&vendored_supplier()["rest"][0]["resource"][0]);
    assert!(
        read.remove(&(
            String::from("_lastUpdated"),
            String::from("date"),
            String::new()
        )),
        "the one parameter the search does not read"
    );
    assert_eq!(
        read,
        parameters(&statement["rest"][0]["resource"][0]),
        "PDQm 3.2.0 §2:3.78.4.1.2.1"
    );
}

#[tokio::test]
async fn the_statement_follows_the_switches() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (_, smart, _spool) = application(settings(&issuer, &secrets)).await;
    let base = url::Url::parse("https://pix.example.org").expect("a URL");
    let without_remove = capability::statement(
        &base,
        &smart,
        Mounts {
            remove_patient: false,
            pdqm: false,
            ..Mounts::all()
        },
    );
    let patient = &without_remove.rest[0].resource[0];
    let codes: Vec<&str> = patient
        .interaction
        .iter()
        .filter_map(|i| i.code.value.as_deref())
        .collect();
    assert_eq!(vec!["update"], codes, "PIXm 3.1.0 §2:3.104.4.3.3");
    assert_eq!(None, patient.conditional_delete);
    let without_pixm = capability::statement(
        &base,
        &smart,
        Mounts {
            pixm: false,
            ..Mounts::all()
        },
    );
    let instantiated: Vec<&str> = without_pixm
        .instantiates
        .iter()
        .filter_map(|canonical| canonical.value.as_deref())
        .collect();
    assert_eq!(
        vec![PDQM_SUPPLIER, PDQM_SUPPLIER_MATCH],
        instantiated,
        "the Supplier alone"
    );
    assert!(
        without_pixm.rest[0]
            .resource
            .iter()
            .all(|resource| resource
                .operation
                .iter()
                .all(|o| o.definition.value.as_deref() != Some(IHE_PIX))),
        "no $ihe-pix without the Manager"
    );
    let without_pdqm = capability::statement(
        &base,
        &smart,
        Mounts {
            pdqm: false,
            ..Mounts::all()
        },
    );
    assert_eq!(1, without_pdqm.instantiates.len(), "the PIXm Manager alone");
    let patient = &without_pdqm.rest[0].resource[0];
    assert!(
        patient.search_param.is_empty(),
        "no search without the Supplier"
    );
    assert!(
        patient
            .operation
            .iter()
            .all(|o| o.definition.value.as_deref() != Some(PDQM_MATCH)),
        "no $match without the Supplier"
    );
}

#[tokio::test]
async fn the_statement_renders_in_json_and_xml_and_refuses_any_other_format_406() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    for (uri, accept) in [
        ("/fhir/metadata?_format=xml", None),
        ("/fhir/metadata?_format=application/fhir%2Bxml", None),
        ("/fhir/metadata", Some("application/fhir+xml")),
    ] {
        let (status, media_type, text) = get(&app, uri, accept).await;
        assert_eq!(StatusCode::OK, status, "{uri}");
        assert_eq!("application/fhir+xml", media_type, "{uri}");
        assert!(
            text.contains("<CapabilityStatement xmlns=\"http://hl7.org/fhir\">"),
            "{uri}: {text}"
        );
        assert!(
            text.contains("<conditionalUpdate value=\"true\"/>"),
            "{uri}"
        );
    }
    for (uri, accept) in [
        ("/fhir/metadata?_format=text/turtle", None),
        ("/fhir/metadata", Some("text/html")),
    ] {
        let (status, media_type, text) = get(&app, uri, accept).await;
        assert_eq!(StatusCode::NOT_ACCEPTABLE, status, "{uri}");
        assert_eq!("application/fhir+json", media_type, "{uri}");
        let outcome: Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!("not-supported", outcome["issue"][0]["code"], "{uri}");
        assert_eq!("error", outcome["issue"][0]["severity"], "{uri}");
    }
}
