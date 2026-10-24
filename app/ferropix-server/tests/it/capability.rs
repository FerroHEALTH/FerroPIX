// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `CapabilityStatement` of `GET /fhir/metadata` against the vendored
//! PIXm Manager actor statement and against the router (#69).
//!
//! Every interaction and operation the statement declares is a mounted row
//! of the permission table, and every row the statement can declare is
//! declared; each requirement of `CapabilityStatement-IHE.PIXm.Manager` is
//! met but `conditionalCreate`, which no `POST` serves. The statement
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
use ferropix_server::fhir::capability::{self, IHE_PIX, PIXM_MANAGER};
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
            other => panic!("an interaction the router does not know: {other:?}"),
        };
    }
    for operation in resource["operation"].as_array().into_iter().flatten() {
        let name = operation["name"].as_str().expect("a name");
        for method in [Method::GET, Method::POST] {
            routes.insert((method, format!("/fhir/{kind}/${name}")));
        }
    }
    routes
}

#[tokio::test]
async fn the_statement_meets_the_pixm_manager_actor_statement_and_names_only_mounted_routes() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let (status, media_type, text) = get(&app, "/fhir/metadata", None).await;
    assert_eq!(StatusCode::OK, status);
    assert_eq!("application/fhir+json", media_type);
    let statement: Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!("CapabilityStatement", statement["resourceType"]);
    assert_eq!("4.0.1", statement["fhirVersion"]);
    assert_eq!(serde_json::json!([PIXM_MANAGER]), statement["instantiates"]);
    let formats: BTreeSet<&str> = statement["format"]
        .as_array()
        .expect("formats")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let vendored = vendored_manager();
    for format in vendored["format"].as_array().expect("formats") {
        assert!(
            formats.contains(format.as_str().expect("a code")),
            "{format}: PIXm 3.1.0 §2:3.104.4.1.2"
        );
    }
    let resources = statement["rest"][0]["resource"]
        .as_array()
        .expect("resources");
    assert_eq!(1, resources.len(), "one entry per type");
    let patient = &resources[0];
    let wanted = &vendored["rest"][0]["resource"][0];
    assert_eq!(wanted["type"], patient["type"]);
    assert_eq!(wanted["supportedProfile"], patient["supportedProfile"]);
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
    assert_eq!(codes(wanted), codes(patient));
    let definitions = |resource: &Value| -> BTreeSet<(String, String)> {
        resource["operation"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|operation| {
                (
                    operation["name"]
                        .as_str()
                        .unwrap_or_default()
                        .trim_start_matches('$')
                        .to_owned(),
                    operation["definition"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .collect()
    };
    assert_eq!(definitions(wanted), definitions(patient));
    assert!(definitions(patient).contains(&(String::from("ihe-pix"), IHE_PIX.to_owned())));

    let declared = routes_declared(patient);
    for (method, path) in &declared {
        let route = table::find(method, path).unwrap_or_else(|| panic!("{method} {path}: no row"));
        assert!(Mounts::all().admits(route.mount), "{method} {path}");
    }
    let pixm: BTreeSet<(Method, String)> = table::ROUTES
        .iter()
        .filter(|route| matches!(route.mount, Mount::Pixm | Mount::PixmRemove))
        .map(|route| (route.method.clone(), route.path.to_owned()))
        .collect();
    assert_eq!(
        pixm, declared,
        "every PIXm route is declared, and only those"
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
    assert_eq!(0, without_pixm.instantiates.len());
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
