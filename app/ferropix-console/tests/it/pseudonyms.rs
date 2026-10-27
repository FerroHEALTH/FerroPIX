// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudonymisation domains page drawn from fixed operator API
//! documents: with domains, with none and refused; the resolve form posted
//! and answered with the person, never echoing the pseudonym or carrying it
//! into an address; a refused resolve; and the administrator's forms drawn
//! only for the administrator scope. Every value is synthetic.

use std::error::Error;

use ferropix_operator_api::pseudonym::{PseudonymDomain, PseudonymDomains, Resolution};
use http::StatusCode;
use http::header::LOCATION;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

use crate::filled::{page, server};
use crate::support::{console, get_with, offline, post_form, send, session_with, steward_session};

/// The person a pseudonym resolves to.
const PERSON: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

/// A synthetic pseudonym, which no page may draw back.
const PSEUDONYM: &str = "5e0c7a91-0000-4000-8000-00000000c0de";

/// The page's address.
const PAGE: &str = "/pseudonym-domains";

/// Two synthetic domains, one of them retired.
fn domains() -> PseudonymDomains {
    PseudonymDomains {
        domains: vec![
            PseudonymDomain {
                id: String::from("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a60"),
                namespace: String::from("urn:oid:2.999.7.1"),
                name: String::from("Synthetic clinical domain"),
                consumer_client_id: Some(String::from("synthetic-ehr")),
                created_at: String::from("2026-10-20T08:00:00Z"),
                retired_at: None,
                pseudonyms: 42,
            },
            PseudonymDomain {
                id: String::from("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a61"),
                namespace: String::from("urn:oid:2.999.7.2"),
                name: String::from("Synthetic research domain"),
                consumer_client_id: None,
                created_at: String::from("2026-10-21T08:00:00Z"),
                retired_at: Some(String::from("2026-10-24T08:00:00Z")),
                pseudonyms: 3,
            },
        ],
    }
}

#[tokio::test]
async fn the_domains_are_drawn_with_their_counts_with_none_and_refused()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[(
        "GET",
        "/operator/pseudonym-domains",
        serde_json::to_string(&domains())?,
    )])
    .await;
    let drawn = page(&filled, PAGE).await?;
    assert!(drawn.contains("Synthetic clinical domain"));
    assert!(drawn.contains("urn:oid:2.999.7.1"));
    assert!(drawn.contains("synthetic-ehr"), "the consumer");
    assert!(drawn.contains(">42<"), "the count of pseudonyms");
    assert!(drawn.contains("Retired"), "the retired domain's state");
    assert!(drawn.contains("Resolve a pseudonym"), "the resolve form");
    assert!(
        drawn.contains("name=\"pseudonym\""),
        "the pseudonym is a form field"
    );
    assert!(
        !drawn.contains("<option value=\"urn:oid:2.999.7.2\""),
        "a retired domain resolves nothing"
    );
    let empty = server(&[(
        "GET",
        "/operator/pseudonym-domains",
        serde_json::to_string(&PseudonymDomains {
            domains: Vec::new(),
        })?,
    )])
    .await;
    let drawn = page(&empty, PAGE).await?;
    assert!(drawn.contains("No pseudonymisation domain is configured"));
    assert!(
        !drawn.contains("name=\"pseudonym\""),
        "no domain, no resolve form"
    );
    let refusing = server(&[]).await;
    let drawn = page(&refusing, PAGE).await?;
    assert!(
        drawn.contains("GET /operator/pseudonym-domains"),
        "the refused read names its route"
    );
    Ok(())
}

#[tokio::test]
async fn a_resolve_answers_the_person_and_never_draws_the_pseudonym_or_moves_it_into_an_address()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[(
        "GET",
        "/operator/pseudonym-domains",
        serde_json::to_string(&domains())?,
    )])
    .await;
    Mock::given(method("POST"))
        .and(path("/operator/pseudonyms/resolve"))
        .and(body_string_contains(PSEUDONYM))
        .and(body_string_contains("Synthetic phone call"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&Resolution {
                person: String::from(PERSON),
                action: String::from("action-resolve-1"),
            })?),
        )
        .expect(1)
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(
            PAGE,
            &cookie,
            &format!(
                "form=resolve&namespace=urn%3Aoid%3A2.999.7.1&pseudonym={PSEUDONYM}&reason=Synthetic+phone+call"
            ),
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status(), "drawn in place");
    assert!(
        response.headers().get(LOCATION).is_none(),
        "no redirect carries the pseudonym"
    );
    assert!(drawn.contains("Pseudonym resolved"));
    assert!(
        drawn.contains(&format!("href=\"/patients/{PERSON}\"")),
        "the person, linked"
    );
    assert!(drawn.contains("disclosure"), "says it is recorded");
    assert!(
        !drawn.contains(PSEUDONYM),
        "the pseudonym is never drawn back"
    );
    Ok(())
}

#[tokio::test]
async fn a_refused_resolve_shows_the_server_s_issue_in_place() -> Result<(), Box<dyn Error>> {
    let filled = server(&[(
        "GET",
        "/operator/pseudonym-domains",
        serde_json::to_string(&domains())?,
    )])
    .await;
    Mock::given(method("POST"))
        .and(path("/operator/pseudonyms/resolve"))
        .respond_with(ResponseTemplate::new(404).set_body_string(
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"not-found","diagnostics":"no unretired pseudonymisation domain holds this pseudonym"}]}"#,
        ))
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(
            PAGE,
            &cookie,
            &format!(
                "form=resolve&namespace=urn%3Aoid%3A2.999.7.1&pseudonym={PSEUDONYM}&reason=Synthetic"
            ),
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(drawn.contains("no unretired pseudonymisation domain holds this pseudonym"));
    assert!(drawn.contains("404"), "the status");
    assert!(
        !drawn.contains(PSEUDONYM),
        "the pseudonym is never drawn back"
    );
    Ok(())
}

#[tokio::test]
async fn the_domain_forms_are_drawn_only_for_the_administrator_scope_and_a_change_patches_its_domain()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[(
        "GET",
        "/operator/pseudonym-domains",
        serde_json::to_string(&domains())?,
    )])
    .await;
    Mock::given(method("PATCH"))
        .and(path(
            "/operator/pseudonym-domains/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a60",
        ))
        .and(body_string_contains("\"retire\":true"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .expect(1)
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let admin = steward_session(&state);
    let (_, drawn) = send(&service, get_with(PAGE, &admin)?).await?;
    assert!(drawn.contains("Add a domain"), "the create form");
    assert!(drawn.contains("Change a domain"), "the change form");
    let (response, drawn) = send(
        &service,
        post_form(
            PAGE,
            &admin,
            "form=change&domain=0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a60&retire=on&reason=Synthetic",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(drawn.contains("The domain is saved"));
    let (response, _) = send(
        &service,
        post_form(
            PAGE,
            &admin,
            "form=change&domain=123456789&reason=Synthetic",
        )?,
    )
    .await?;
    assert_eq!(
        StatusCode::SEE_OTHER,
        response.status(),
        "a change naming no domain id goes back"
    );
    let steward = session_with(&state, &["openid", "__ferropix.steward"]);
    let (_, drawn) = send(&service, get_with(PAGE, &steward)?).await?;
    assert!(!drawn.contains("Add a domain"));
    assert!(drawn.contains("needs the administrator scope"));
    assert!(
        drawn.contains("Resolve a pseudonym"),
        "every steward resolves"
    );
    Ok(())
}
