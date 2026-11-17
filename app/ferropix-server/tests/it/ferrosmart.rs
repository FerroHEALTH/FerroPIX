// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! FerroPIX behind FerroSMART: FerroSMART-shaped tokens read under
//! `ferrohealth-smart`'s `ferrosmart` issuer preset.
//!
//! The preset keeps every `rfc9068` default but two: `typ` is `at+jwt` alone
//! (RFC 9068 §2.1) and `scope` is required (SMART App Launch 2.2.0, Obtain
//! access token, Response). A person reaches FerroPIX only through a SMART
//! app the `persons` table lists in `delegates`. A Backend Services client's
//! token, whose `sub` is its `client_id` (RFC 9068 §2.2), reads as the
//! system under the same table: a `persons` issuer also signs its machines'
//! app-only tokens. One table names the FerroSMART issuer: two tables of one
//! issuer refuse the start (FerroHEALTH/FerroAUTH#591).

use http::{Method, StatusCode};

use crate::auth::{AUDIENCE, Issuer, application, bearer, send, settings_trusting};
use crate::support::Secrets;

/// The SMART app the deployment admits for a person.
const APP: &str = "growth-chart";

/// A SMART app the deployment does not list.
const UNLISTED: &str = "unlisted-app";

/// The Backend Services client.
const BACKEND: &str = "lab-backend";

/// The person signed in at FerroSMART, by FerroSMART's pairwise `sub`.
const PERSON: &str = "pairwise-subject-1";

/// The steward's scope of the permission table.
const STEWARD: &str = "__ferropix.steward";

/// The `[[auth.issuer]]` table of the FerroSMART issuer.
fn tables() -> String {
    format!(
        "[[auth.issuer]]\nissuer = \"{{issuer}}\"\nkind = \"persons\"\nprofile = \"ferrosmart\"\naudience = [\"{AUDIENCE}\"]\ndelegates = [{{ client = \"{APP}\" }}]\n"
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_through_a_listed_app_is_read_and_an_unlisted_app_is_refused() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _smart, _spool) =
        application(settings_trusting(&issuer, &secrets, &tables(), "")).await;

    let listed = bearer(&issuer.token_for(APP, PERSON, STEWARD));
    let answer = send(&app, Method::GET, "/operator/dependencies", Some(&listed)).await;
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);

    let unlisted = bearer(&issuer.token_for(UNLISTED, PERSON, STEWARD));
    let answer = send(&app, Method::GET, "/operator/dependencies", Some(&unlisted)).await;
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        answer.status,
        "a person through an app the deployment does not list is no caller: {}",
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
        "the preset requires `scope`: {}",
        answer.body
    );
}
