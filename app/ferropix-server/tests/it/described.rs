// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The issuer the SMART discovery document and the `CapabilityStatement`
//! name, chosen by `ferrohealth-smart`'s `describe`: `[auth]
//! described_issuer`, else the one issuer a user launch starts at, else, with
//! only `machines` issuers trusted, the one `machines` issuer for SMART
//! Backend Services; never by the order of the `[[auth.issuer]]` tables.
//! SMART App Launch 2.2.0, Conformance, §Metadata, and Backend Services,
//! §Retrieve `.well-known/smart-configuration`; which of several issuers is
//! described is our own design.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the discovery document is read as a JSON value"
)]

use ferrohealth_secret::secret_url::SecretUrl;
use ferrohealth_smart::kind::IssuerKind;
use ferrohealth_smart::resource::described::DescribeError;
use ferropix_server::auth::{Smart, SmartError};
use ferropix_server::config::settings::Settings;
use http::{Method, StatusCode};

use crate::auth::{Issuer, application, send, settings};
use crate::support::Secrets;

/// The settings of a server trusting `issuers` in this order, each of its
/// kind, describing `named` when set.
fn trusting(
    issuers: &[(&Issuer, IssuerKind)],
    named: Option<&Issuer>,
    secrets: &Secrets,
) -> Settings {
    let (first, _) = issuers.first().expect("an issuer");
    let mut settings = settings(first, secrets);
    let template = settings
        .auth
        .resolved
        .issuers
        .first()
        .expect("the issuer table")
        .clone();
    settings.auth.resolved.issuers = issuers
        .iter()
        .map(|(issuer, kind)| {
            let mut one = template.clone();
            one.issuer = issuer.uri();
            one.kind = *kind;
            one
        })
        .collect();
    settings.auth.resolved.described_issuer = named.map(|issuer| SecretUrl::new(issuer.uri()));
    settings
}

/// Returns why `settings` refuse the start.
async fn refusal(settings: Settings) -> DescribeError {
    match Smart::discover(settings.auth, &settings.domains).await {
        Err(SmartError::Described { source }) => source,
        other => panic!("the start is refused by describe: {other:?}"),
    }
}

/// The token endpoint the started server's documents name.
async fn described(settings: Settings) -> Option<String> {
    let smart = Smart::discover(settings.auth, &settings.domains)
        .await
        .expect("the server starts");
    smart.token_endpoint().map(str::to_owned)
}

#[tokio::test]
async fn a_machines_issuer_written_first_is_never_described() {
    let machines = Issuer::start().await;
    let persons = Issuer::start().await;
    let secrets = Secrets::new();
    for order in [
        [
            (&machines, IssuerKind::Machines),
            (&persons, IssuerKind::Persons),
        ],
        [
            (&persons, IssuerKind::Persons),
            (&machines, IssuerKind::Machines),
        ],
    ] {
        assert_eq!(
            Some(format!("{}/token", persons.uri())),
            described(trusting(&order, None, &secrets)).await
        );
    }
}

#[tokio::test]
async fn several_user_issuers_without_the_key_refuse_the_start() {
    let persons = Issuer::start().await;
    let patients = Issuer::start().await;
    let secrets = Secrets::new();
    let mut sorted = vec![persons.uri(), patients.uri()];
    sorted.sort();
    for order in [
        [
            (&persons, IssuerKind::Persons),
            (&patients, IssuerKind::Patients),
        ],
        [
            (&patients, IssuerKind::Patients),
            (&persons, IssuerKind::Persons),
        ],
    ] {
        let DescribeError::SeveralEligible { issuers } =
            refusal(trusting(&order, None, &secrets)).await
        else {
            panic!("several user issuers are refused as such");
        };
        let named: Vec<&str> = issuers.iter().map(SecretUrl::expose).collect();
        assert_eq!(named, sorted, "the table order never shows");
    }
}

#[tokio::test]
async fn the_named_issuer_is_described_among_several() {
    let persons = Issuer::start().await;
    let patients = Issuer::start().await;
    let secrets = Secrets::new();
    let issuers = [
        (&persons, IssuerKind::Persons),
        (&patients, IssuerKind::Patients),
    ];
    for named in [&persons, &patients] {
        assert_eq!(
            Some(format!("{}/token", named.uri())),
            described(trusting(&issuers, Some(named), &secrets)).await
        );
    }
}

#[tokio::test]
async fn a_named_machines_issuer_beside_a_user_issuer_refuses_the_start() {
    let machines = Issuer::start().await;
    let persons = Issuer::start().await;
    let secrets = Secrets::new();
    let issuers = [
        (&persons, IssuerKind::Persons),
        (&machines, IssuerKind::Machines),
    ];
    let DescribeError::MachinesIssuer { issuer } =
        refusal(trusting(&issuers, Some(&machines), &secrets)).await
    else {
        panic!("a named machines issuer beside a user issuer is refused");
    };
    assert_eq!(issuer.expose(), machines.uri());
}

#[tokio::test]
async fn a_named_unknown_issuer_refuses_the_start() {
    let persons = Issuer::start().await;
    let elsewhere = Issuer::start().await;
    let secrets = Secrets::new();
    let refused = refusal(trusting(
        &[(&persons, IssuerKind::Persons)],
        Some(&elsewhere),
        &secrets,
    ))
    .await;
    assert!(
        matches!(&refused, DescribeError::UnknownIssuer { issuer } if issuer.expose() == elsewhere.uri()),
        "{refused:?}"
    );
}

// SMART App Launch 2.2.0, Backend Services, Retrieve
// `.well-known/smart-configuration`: a backend client reads the token endpoint here.
#[tokio::test(flavor = "multi_thread")]
async fn a_machines_only_server_is_described_for_backend_services() {
    let machines = Issuer::start().await;
    let secrets = Secrets::new();
    let settings = trusting(&[(&machines, IssuerKind::Machines)], None, &secrets);
    let (app, _smart, _spool) = application(settings).await;
    let discovery = send(
        &app,
        Method::GET,
        "/fhir/.well-known/smart-configuration",
        None,
    )
    .await;
    assert_eq!(StatusCode::OK, discovery.status);
    let document = discovery.json();
    assert_eq!(
        serde_json::json!(format!("{}/token", machines.uri())),
        document["token_endpoint"]
    );
    assert_eq!(
        serde_json::json!(["client_credentials"]),
        document["grant_types_supported"]
    );
    assert_eq!(
        serde_json::json!(["client-confidential-asymmetric", "permission-v1"]),
        document["capabilities"]
    );
    let listed = document["scopes_supported"]
        .as_array()
        .expect("scopes_supported is listed");
    assert!(
        listed.contains(&serde_json::json!("system/Patient.rs")),
        "{listed:?}"
    );
    assert!(
        listed
            .iter()
            .filter_map(serde_json::Value::as_str)
            .all(|scope| scope.starts_with("system/")),
        "a Backend Services client requests system/ scopes: {listed:?}"
    );
    assert!(
        document.get("authorization_endpoint").is_none(),
        "{document}"
    );
}
