// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-90 and ITI-91 audit records of an audited mCSD client (mCSD 4.0.0
//! §2:3.90.5.1, §2:3.91.5.1): one record per search and per history, held to
//! the Query and Updates audit profiles and their examples, and a
//! transaction whose record is refused fails.

use std::sync::Arc;

use ihe_iti::balp::Outcome;
use ihe_iti::mcsd::client::CareService;
use ihe_iti::mcsd::error::McsdError;
use ihe_iti::user::OnBehalfOf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::profile::{
    Kept, Refusing, base64_decoded, holds_to, like, names_no_user, vendored, written,
};
use crate::mcsd::{BASE, FHIR_JSON, budget, bundle, client, full_url, matched, organization};

/// A directory that answers `GET {BASE}{at}` with one page of `kind`.
async fn directory(at: &str, kind: &str) -> MockServer {
    let server = MockServer::start().await;
    let entries = if kind == "searchset" {
        vec![matched(
            &full_url(&server, "Organization", "org-a"),
            &organization("org-a", "urn:oid:2.999.10", &[]),
        )]
    } else {
        Vec::new()
    };
    Mock::given(method("GET"))
        .and(path(format!("{BASE}{at}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(bundle(kind, &entries, &[]).into_bytes(), FHIR_JSON),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_directory_search_is_the_system_s_own_and_names_no_user() {
    let server = directory("Organization", "searchset").await;
    let kept = Arc::new(Kept::default());
    client(&server)
        .audited(kept.clone())
        .find(CareService::Organization, &[], &mut budget())
        .await
        .expect("a search");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.on_behalf, OnBehalfOf::System);
    names_no_user(&written(&exchange));
}

#[tokio::test]
async fn a_search_is_recorded_as_the_query_audit_profile_fixes_it() {
    let server = directory("Organization", "searchset").await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .find(
            CareService::Organization,
            &[("active", "true")],
            &mut budget(),
        )
        .await
        .expect("a search");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::Success);
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-mcsd",
            "package/StructureDefinition-IHE.mCSD.Audit.CareServices.Query.json",
        ),
    );
    like(
        &record,
        &vendored(
            "ihe-mcsd",
            "package/example/AuditEvent-ex-AuditMcsdCareServicesQuery.json",
        ),
        true,
    );
    let query = base64_decoded(record["entity"][0]["query"].as_str().expect("a query"));
    assert_eq!(
        query,
        format!("{}{BASE}Organization?active=true", server.uri())
    );
    assert!(
        record["entity"]
            .as_array()
            .expect("entities")
            .iter()
            .all(|entity| entity["role"]["code"] != "1"),
        "a care services search names no patient"
    );
}

#[tokio::test]
async fn a_history_is_recorded_as_the_updates_audit_profile_fixes_it() {
    let server = directory("Organization/_history", "history").await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .updates(
            CareService::Organization,
            "2026-10-01T00:00:00Z",
            &mut budget(),
        )
        .await
        .expect("a history");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-mcsd",
            "package/StructureDefinition-IHE.mCSD.Audit.CareServices.Updates.json",
        ),
    );
    like(
        &record,
        &vendored(
            "ihe-mcsd",
            "package/example/AuditEvent-ex-AuditMcsdCareServicesUpdates.json",
        ),
        true,
    );
}

#[tokio::test]
async fn a_refused_search_is_recorded_as_a_minor_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .find(CareService::Endpoint, &[], &mut budget())
        .await
        .expect_err("a refusal");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::MinorFailure);
}

#[tokio::test]
async fn a_search_whose_record_is_refused_fails() {
    let server = directory("Organization", "searchset").await;
    let client = client(&server).audited(Arc::new(Refusing));
    let error = client
        .find(CareService::Organization, &[], &mut budget())
        .await
        .expect_err("the search fails closed");
    assert!(matches!(error, McsdError::Audit(_)), "{error:?}");
    assert!(!error.answered(), "an answer set aside is no answer");
}
