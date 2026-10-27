// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! One pair to compare and decide, drawn from a fixed candidate in every
//! state: open, with a link that merges, with a record changed, decided by
//! someone else; and a decision posted with the versions and the reason
//! the page was drawn from, leading on to the next pair, refused as stale
//! (`409`) in the bar, and held back without a reason. Every value is
//! synthetic.

use std::error::Error;

use ferropix_operator_api::candidate::{Candidate, Status};
use http::StatusCode;
use http::header::LOCATION;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

use crate::filled::{page, server};
use crate::support::{console, offline, post_form, send, steward_session};

/// The candidate every case reads.
const CANDIDATE: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4b01";

/// The pair that follows it in the queue.
const NEXT: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4b02";

/// The synthetic candidate, changed by `change`.
fn candidate(change: impl FnOnce(&mut Candidate)) -> String {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/ferropix-testkit/fixtures/operator/candidate.json"
    ))
    .expect("the synthetic candidate");
    let mut value: Candidate = serde_json::from_str(&text).expect("a candidate");
    change(&mut value);
    serde_json::to_string(&value).expect("JSON")
}

/// The pair's address.
fn address() -> String {
    format!("/queue/{CANDIDATE}")
}

/// The route the console reads the pair from.
fn route() -> String {
    format!("/operator/candidates/{CANDIDATE}")
}

#[tokio::test]
async fn an_open_pair_draws_the_compare_the_breakdown_the_persons_and_the_bar()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[("GET", &route(), candidate(|_| {}))]).await;
    let drawn = page(&filled, &address()).await?;
    assert!(drawn.contains("The two records"), "the compare");
    assert!(drawn.contains("Synthetic laboratory"), "the second record");
    assert!(drawn.contains("Why the pair scored this"), "the breakdown");
    assert!(drawn.contains("family_birth"), "the blocking key");
    assert!(
        drawn.contains("given: close match"),
        "a field's contribution"
    );
    assert!(
        drawn.contains("/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5e"),
        "a person, linked"
    );
    assert!(
        !drawn.contains("4711\u{2022}") && !drawn.contains("SYNTH"),
        "identifiers masked"
    );
    assert!(
        drawn.contains("aria-keyshortcuts=\"N\""),
        "Not a match on n"
    );
    assert!(drawn.contains("aria-keyshortcuts=\"D\""), "Defer on d");
    assert!(drawn.contains("aria-keyshortcuts=\"R\""), "the reason on r");
    assert!(drawn.contains("aria-keyshortcuts=\"]\""), "Next on ]");
    assert!(
        drawn.contains("name=\"a_version\" value=\"2\""),
        "the versions read"
    );
    assert!(
        drawn.contains("Link and merge the two patients?"),
        "a link that merges asks first"
    );
    assert!(
        drawn.contains("commandfor=\"link-merges\""),
        "Link… opens the dialog"
    );
    let plain = server(&[("GET", &route(), candidate(|value| value.merges = false))]).await;
    let drawn = page(&plain, &address()).await?;
    assert!(
        !drawn.contains("link-merges"),
        "no dialog when nothing merges"
    );
    assert!(
        drawn.contains("value=\"link\""),
        "Link is a decision of the bar"
    );
    Ok(())
}

#[tokio::test]
async fn a_changed_record_and_a_pair_decided_elsewhere_say_so() -> Result<(), Box<dyn Error>> {
    let changed = server(&[("GET", &route(), candidate(|value| value.b.head_version = 2))]).await;
    assert!(
        page(&changed, &address())
            .await?
            .contains("changed after it was scored")
    );
    let decided = server(&[(
        "GET",
        &route(),
        candidate(|value| {
            value.status = Status::Linked;
            value.decided_by = Some(String::from("Synthetic Steward"));
            value.decided_at = Some(String::from("2026-10-25T09:00:00Z"));
        }),
    )])
    .await;
    let drawn = page(&decided, &address()).await?;
    assert!(drawn.contains("Linked by Synthetic Steward."));
    assert!(
        !drawn.contains("fui-dock"),
        "no decision bar on a decided pair"
    );
    Ok(())
}

#[tokio::test]
async fn a_decision_posts_the_versions_and_the_reason_and_leads_to_the_next_pair()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[("GET", &route(), candidate(|_| {}))]).await;
    Mock::given(method("POST"))
        .and(path(format!("{}/decide", route())))
        .and(body_string_contains(r#""verdict":"not-a-match""#))
        .and(body_string_contains(r#""a_version":2"#))
        .and(body_string_contains(r#""b_version":1"#))
        .and(body_string_contains("Synthetic ward call"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{"candidate":"{CANDIDATE}","status":"not-a-match","action":"action-4","next":"{NEXT}"}}"#
        )))
        .expect(1)
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, _) = send(
        &service,
        post_form(
            &address(),
            &cookie,
            "decision=not-a-match&a_version=2&b_version=1&reason=Synthetic+ward+call&until=",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    assert_eq!(
        Some(format!("/queue/{NEXT}").as_str()),
        response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
    );
    Ok(())
}

#[tokio::test]
async fn a_stale_decision_is_drawn_in_the_bar_and_one_without_a_reason_is_never_sent()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[("GET", &route(), candidate(|_| {}))]).await;
    Mock::given(method("POST"))
        .and(path(format!("{}/decide", route())))
        .and(body_string_contains("Synthetic"))
        .respond_with(ResponseTemplate::new(409).set_body_string(
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"conflict","diagnostics":"the candidate was decided"}]}"#,
        ))
        .expect(1)
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(
            &address(),
            &cookie,
            "decision=defer&a_version=2&b_version=1&reason=Synthetic&until=2099-12-01",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(drawn.contains("Someone decided this pair or changed a record first"));
    assert!(drawn.contains("Reload this pair"));
    assert!(
        drawn.contains(&format!("/queue/{NEXT}")),
        "the next pair is a way on"
    );
    let (response, drawn) = send(
        &service,
        post_form(
            &address(),
            &cookie,
            "decision=link&a_version=2&b_version=1&reason=&until=",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(
        drawn.contains("Give a reason to decide."),
        "the reason's error"
    );
    assert!(drawn.contains("autofocus"), "the reason takes the focus");
    Ok(())
}
