// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! FerroPIX behind FerroSMART: one `[auth.issuer.<name>]` table naming only
//! the issuer trusts it.
//!
//! A token's `typ` is `at+jwt` (RFC 9068 §2.1) and its `scope` is required
//! (SMART App Launch 2.2.0, Obtain access token, Response). FerroSMART's
//! registry is the one list of clients: any app whose token's `aud` names
//! FerroPIX reaches it (RFC 9068 §4). A Backend Services client's token,
//! whose `sub` is its `client_id` (RFC 9068 §2.2), reads as the system.

use http::{Method, StatusCode};

use crate::auth::{AUDIENCE, Issuer, application, bearer, send, settings_trusting};
use crate::support::Secrets;

/// A SMART app a person signs in through.
const APP: &str = "growth-chart";

/// The Backend Services client.
const BACKEND: &str = "lab-backend";

/// The person signed in at FerroSMART, by FerroSMART's pairwise `sub`.
const PERSON: &str = "pairwise-subject-1";

/// The steward's scope of the permission table.
const STEWARD: &str = "__ferropix.steward";

/// The `[auth.issuer.idp]` table of the FerroSMART issuer.
fn tables() -> String {
    format!("[auth.issuer.idp]\nissuer = \"{{issuer}}\"\naudience = [\"{AUDIENCE}\"]\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_through_an_app_audienced_to_ferropix_is_read_and_another_audience_is_refused() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _smart, _spool) =
        application(settings_trusting(&issuer, &secrets, &tables(), "")).await;

    let person = bearer(&issuer.token_for(APP, PERSON, STEWARD));
    let answer = send(&app, Method::GET, "/operator/dependencies", Some(&person)).await;
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);

    let elsewhere = bearer(&issuer.token_with(
        APP,
        PERSON,
        STEWARD,
        serde_json::Map::from_iter([(
            String::from("aud"),
            serde_json::json!("https://other.example.org/fhir"),
        )]),
    ));
    let answer = send(
        &app,
        Method::GET,
        "/operator/dependencies",
        Some(&elsewhere),
    )
    .await;
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        answer.status,
        "a token audienced to another server is no caller: {}",
        answer.body
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_services_client_reads_as_the_system() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _smart, _spool) =
        application(settings_trusting(&issuer, &secrets, &tables(), "")).await;
    let token = bearer(&issuer.token_for(BACKEND, BACKEND, "system/Patient.rs"));
    let answer = send(
        &app,
        Method::GET,
        "/fhir/Patient?identifier=urn%3Aoid%3A2.999.1.1%7CX",
        Some(&token),
    )
    .await;
    assert!(
        ![StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN].contains(&answer.status),
        "a Backend Services client with system/Patient.rs reaches the search: {} {}",
        answer.status,
        answer.body
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_without_its_scope_is_refused() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _smart, _spool) =
        application(settings_trusting(&issuer, &secrets, &tables(), "")).await;
    let token = bearer(&issuer.token_without(BACKEND, BACKEND, "scope"));
    let answer = send(&app, Method::GET, "/operator/dependencies", Some(&token)).await;
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        answer.status,
        "a token without `scope` is no caller: {}",
        answer.body
    );
}
