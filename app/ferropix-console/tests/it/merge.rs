// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The merge and the unmerge drawn from fixed previews: the merge asks for
//! the other patient, previews with the older surviving, posts both
//! versions only from its dialog and draws a `409` in place; the unmerge
//! shows the latest merge with its reason and the revert's preview, names
//! the merge to revert first, says when there is none, and draws a refused
//! revert in place. Every value is synthetic.

use std::error::Error;

use http::StatusCode;
use http::header::LOCATION;
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use crate::filled::{page, server};
use crate::support::{console, offline, post_form, send, steward_session};

/// The patient the pages are about.
const PERSON: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

/// The other patient.
const OTHER: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5e";

/// The merge into the patient.
const MERGE: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4d01";

/// A synthetic operator API document of the testkit.
fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../../tools/ferropix-testkit/fixtures/operator/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the synthetic document")
}

/// A refusal of the server with `status`, as the RFC 9457 problem `slug`
/// with `detail`.
fn refusal(status: u16, slug: &str, detail: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(
        format!(
            r#"{{"type":"https://ferropix.eu/docs/operate/problems.html#{slug}","status":{status},"detail":"{detail}"}}"#
        ),
        "application/problem+json",
    )
}

#[tokio::test]
async fn a_merge_asks_for_the_other_patient_and_previews_with_the_older_surviving()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[]).await;
    Mock::given(method("GET"))
        .and(path("/operator/merges/preview"))
        .and(query_param("survivor", PERSON))
        .and(query_param("merged", OTHER))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("merge-preview")))
        .expect(1)
        .mount(&filled)
        .await;
    let address = format!("/patients/{PERSON}/merge");
    let drawn = page(&filled, &address).await?;
    assert!(
        drawn.contains("name=\"other\""),
        "the other patient is asked for"
    );
    assert!(
        !drawn.contains("merge-confirm"),
        "nothing to confirm before a preview"
    );
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(&address, &cookie, &format!("form=preview&other={OTHER}"))?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status(), "drawn in place");
    assert!(drawn.contains("What the merge does"));
    assert!(
        drawn.contains("Synthetic laboratory"),
        "the identifier that moves"
    );
    assert!(
        drawn.contains("Synthetic clinical domain"),
        "the pseudonym that resolves"
    );
    assert!(
        drawn.contains("1 identifiers move"),
        "the dialog names the count"
    );
    assert!(
        drawn.contains("name=\"survivor_version\" value=\"3\""),
        "both versions"
    );
    assert!(drawn.contains("name=\"merged_version\" value=\"1\""));
    assert!(
        drawn.contains("commandfor=\"merge-confirm\""),
        "Merge… opens the dialog"
    );
    Ok(())
}

#[tokio::test]
async fn a_merge_posts_both_versions_and_leads_to_the_survivor_and_a_409_is_drawn_in_place()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[("GET", "/operator/merges/preview", fixture("merge-preview"))]).await;
    Mock::given(method("POST"))
        .and(path("/operator/merges"))
        .and(body_string_contains(r#""survivor_version":"3""#))
        .and(body_string_contains(r#""merged_version":"1""#))
        .and(body_string_contains("Synthetic ward call"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("merge-outcome")))
        .up_to_n_times(1)
        .mount(&filled)
        .await;
    Mock::given(method("POST"))
        .and(path("/operator/merges"))
        .respond_with(refusal(409, "stale", "a person changed since it was read"))
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let address = format!("/patients/{PERSON}/merge");
    let body = format!(
        "form=merge&survivor={PERSON}&survivor_version=3&merged={OTHER}&merged_version=1&reason=Synthetic+ward+call"
    );
    let (response, _) = send(&service, post_form(&address, &cookie, &body)?).await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    assert_eq!(
        Some(format!("/patients/{PERSON}").as_str()),
        response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
    );
    let (response, drawn) = send(&service, post_form(&address, &cookie, &body)?).await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(drawn.contains("Nothing was changed"), "the stale notice");
    assert!(
        drawn.contains("What the merge does"),
        "the preview read again"
    );
    Ok(())
}

#[tokio::test]
async fn the_unmerge_shows_the_merge_its_reason_and_its_revert_or_the_merge_to_revert_first()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[
        (
            "GET",
            &format!("/operator/persons/{PERSON}"),
            fixture("person"),
        ),
        ("GET", "/operator/merges/preview", fixture("revert-preview")),
    ])
    .await;
    let address = format!("/patients/{PERSON}/unmerge");
    let drawn = page(&filled, &address).await?;
    assert!(drawn.contains("What the revert does"));
    assert!(drawn.contains("Synthetic ward call"), "the merge's reason");
    assert!(
        drawn.contains(&format!("name=\"merge\" value=\"{MERGE}\"")),
        "the merge reverted"
    );
    assert!(
        drawn.contains("commandfor=\"revert-confirm\""),
        "Unmerge… opens the dialog"
    );
    let later = fixture("revert-preview").replace(
        r#""reason""#,
        r#""first_revert": "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4d02", "reason""#,
    );
    let blocked = server(&[
        (
            "GET",
            &format!("/operator/persons/{PERSON}"),
            fixture("person"),
        ),
        ("GET", "/operator/merges/preview", later),
    ])
    .await;
    let drawn = page(&blocked, &address).await?;
    assert!(
        drawn.contains("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4d02"),
        "the merge to revert first"
    );
    assert!(!drawn.contains("revert-confirm"), "no revert out of order");
    let unmerged = fixture("person").replace("\"merged\": {", "\"unmerged\": {");
    let none = server(&[("GET", &format!("/operator/persons/{PERSON}"), unmerged)]).await;
    assert!(
        page(&none, &address)
            .await?
            .contains("No merge into this patient")
    );
    Ok(())
}

#[tokio::test]
async fn a_revert_posts_the_survivor_s_version_and_a_refusal_is_drawn_in_place()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[
        (
            "GET",
            &format!("/operator/persons/{PERSON}"),
            fixture("person"),
        ),
        ("GET", "/operator/merges/preview", fixture("revert-preview")),
    ])
    .await;
    Mock::given(method("POST"))
        .and(path(format!("/operator/merges/{MERGE}/revert")))
        .and(body_string_contains(r#""survivor_version":"3""#))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("merge-outcome")))
        .up_to_n_times(1)
        .mount(&filled)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/operator/merges/{MERGE}/revert")))
        .respond_with(refusal(
            409,
            "later-merge",
            "a later merge of the survivor has to be reverted first",
        ))
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let address = format!("/patients/{PERSON}/unmerge");
    let body = format!("form=revert&merge={MERGE}&survivor_version=3&reason=Synthetic+mistake");
    let (response, _) = send(&service, post_form(&address, &cookie, &body)?).await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    let (response, drawn) = send(&service, post_form(&address, &cookie, &body)?).await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(drawn.contains("Nothing was changed"));
    Ok(())
}
