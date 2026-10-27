// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review queue drawn from fixed operator API documents: with
//! candidates, empty, with nothing matching its filters and refused; the
//! filters and the paging posted, never in an address; and a deferral from
//! a row's side panel posted with the candidate's versions, its refusal
//! drawn in place. Every value is synthetic.

use std::error::Error;

use http::StatusCode;
use http::header::LOCATION;
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use crate::filled::{page, server};
use crate::support::{console, offline, post_form, send, steward_session};

/// The first candidate of the queue.
const CANDIDATE: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4b01";

/// A page of the queue with one candidate, as the server writes it.
fn candidates(next: Option<u32>) -> String {
    let next = next.map_or_else(String::new, |next| format!(r#","next":{next}"#));
    format!(
        r#"{{"total":51,"offset":0{next},"rows":[{{"id":"{CANDIDATE}","a":{{"domain":{{"id":"d-1","system":"urn:oid:2.999.1.1","name":"Synthetic hospital","kind":"local"}},"name":"Proefsma, Ada","birth_date":"1980-02-29"}},"b":{{"domain":{{"id":"d-2","system":"urn:oid:2.999.1.2","name":"Synthetic laboratory","kind":"local"}},"name":"Proefsma, A.","birth_date":"1980-02-29"}},"score":0.91,"grade":"probable","status":"open","created_at":"2026-10-18T08:00:00Z"}}]}}"#
    )
}

/// The candidate's evidence, its records at versions 2 and 1.
fn candidate() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/ferropix-testkit/fixtures/operator/candidate.json"
    ))
    .expect("the synthetic candidate")
}

#[tokio::test]
async fn the_queue_draws_its_rows_its_empty_states_and_its_refusal() -> Result<(), Box<dyn Error>> {
    let filled = server(&[("GET", "/operator/candidates", candidates(Some(50)))]).await;
    let drawn = page(&filled, "/queue").await?;
    assert!(
        drawn.contains("Proefsma, Ada and Proefsma, A."),
        "both names"
    );
    assert!(drawn.contains("Synthetic laboratory"), "the domains");
    assert!(drawn.contains("0.91"), "the score's figure");
    assert!(drawn.contains("Probable"), "the grade");
    assert!(
        drawn.contains(&format!("href=\"/queue/{CANDIDATE}\"")),
        "the row opens its pair"
    );
    assert!(
        drawn.contains(&format!("glance-{CANDIDATE}")),
        "the side panel"
    );
    assert!(drawn.contains("1–1 of 51"), "the range");
    assert!(
        drawn.contains("name=\"offset\" value=\"50\""),
        "the next page posts"
    );
    assert!(
        drawn.contains("aria-current=\"true\""),
        "the first row is focused"
    );
    let empty = server(&[(
        "GET",
        "/operator/candidates",
        String::from(
            r#"{"total":0,"offset":0,"rows":[],"last_decision_at":"2026-10-25T09:00:00Z"}"#,
        ),
    )])
    .await;
    let drawn = page(&empty, "/queue").await?;
    assert!(drawn.contains("The queue is empty"));
    assert!(drawn.contains("Last decision"), "the last decision's time");
    let refusing = server(&[]).await;
    assert!(
        page(&refusing, "/queue")
            .await?
            .contains("GET /operator/candidates")
    );
    Ok(())
}

#[tokio::test]
async fn the_filters_post_their_fields_to_the_server_and_keep_them_out_of_the_address()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[]).await;
    Mock::given(method("GET"))
        .and(path("/operator/candidates"))
        .and(query_param("grade", "probable"))
        .and(query_param("older_than_days", "7"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"total":0,"offset":0,"rows":[]}"#),
        )
        .expect(1)
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(
            "/queue",
            &cookie,
            "form=filter&grade=probable&domain=&older=7",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status(), "drawn in place");
    assert!(response.headers().get(LOCATION).is_none());
    assert!(drawn.contains("No pair matches these filters"));
    assert!(
        drawn.contains("value=\"7\""),
        "the filter is kept in its field"
    );
    Ok(())
}

#[tokio::test]
async fn a_deferral_posts_the_candidate_s_versions_and_a_refusal_is_drawn_in_place()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[
        ("GET", "/operator/candidates", candidates(None)),
        (
            "GET",
            &format!("/operator/candidates/{CANDIDATE}"),
            candidate(),
        ),
    ])
    .await;
    Mock::given(method("POST"))
        .and(path(format!("/operator/candidates/{CANDIDATE}/decide")))
        .and(body_string_contains(r#""verdict":"defer""#))
        .and(body_string_contains(r#""until":"2099-12-01""#))
        .and(body_string_contains(r#""a_version":2"#))
        .and(body_string_contains(r#""b_version":1"#))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{"candidate":"{CANDIDATE}","status":"deferred","action":"action-3"}}"#
        )))
        .up_to_n_times(1)
        .mount(&filled)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/operator/candidates/{CANDIDATE}/decide")))
        .respond_with(ResponseTemplate::new(409).set_body_string(
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"conflict","diagnostics":"a record changed after the candidate was read"}]}"#,
        ))
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let body = format!(
        "form=defer&candidate={CANDIDATE}&until=2099-12-01&reason=Synthetic+ward+call&grade=&domain=&older="
    );
    let (response, drawn) = send(&service, post_form("/queue", &cookie, &body)?).await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(drawn.contains("The pair is deferred"), "{drawn}");
    let (_, drawn) = send(&service, post_form("/queue", &cookie, &body)?).await?;
    assert!(drawn.contains("a record changed after the candidate was read"));
    assert!(drawn.contains("409"));
    Ok(())
}
