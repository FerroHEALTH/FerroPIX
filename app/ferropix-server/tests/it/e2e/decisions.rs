// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review queue and the steward's decisions over PostgreSQL 18 (#107),
//! behind `FERROPIX_E2E`: a link merges the two persons, a decision on a
//! stale read is `409`, a not-a-match pair stays out of the queue until a
//! record's demographics change, a deferral comes back on its day, and the
//! queue lists in its order under its filters.
//!
//! Every name, date and identifier is synthetic.

use ferropix_operator_api::candidate::{Candidate, Candidates, Decided, Status};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use fhir_types::codec::Value as Document;
use http::StatusCode;

use super::operator::{document, get, post};
use super::pixm::{Answer, Harness, synthetic_seeds};

/// The `Patient` body of `value` in `system` with the given demographics,
/// at a postal code of its birth date's day, so two pairs never share a
/// blocking key.
fn patient(system: &str, value: &str, family: &str, given: &str, birth: &str) -> Document {
    let day = birth.rsplit('-').next().unwrap_or("00");
    serde_json::from_str(&format!(
        r#"{{"resourceType":"Patient","identifier":[{{"system":"{system}","value":"{value}"}}],"name":[{{"family":"{family}","given":["{given}"]}}],"gender":"female","birthDate":"{birth}","address":[{{"postalCode":"99{day}ZZ"}}]}}"#
    ))
    .expect("a patient")
}

/// Feeds `body` as `value` in `system` and asserts it was taken.
async fn fed(harness: &Harness, system: &str, value: &str, body: &Document) {
    let answer = harness.feed(system, value, body).await;
    assert!(
        answer.status == StatusCode::CREATED || answer.status == StatusCode::OK,
        "{}",
        answer.body
    );
}

/// Feeds one pair, the hospital's `H-{n}` and the laboratory's `L-{n}`,
/// alike enough to be a candidate.
async fn pair(harness: &Harness, n: usize, family: &str) {
    let birth = format!("1971-03-{:02}", n + 1);
    fed(
        harness,
        HOSPITAL_DOMAIN,
        &format!("H-{n}"),
        &patient(HOSPITAL_DOMAIN, &format!("H-{n}"), family, "Ada", &birth),
    )
    .await;
    fed(
        harness,
        LABORATORY_DOMAIN,
        &format!("L-{n}"),
        &patient(LABORATORY_DOMAIN, &format!("L-{n}"), family, "Ada", &birth),
    )
    .await;
}

/// Reads the queue at `query`.
async fn queue(harness: &Harness, query: &str) -> Candidates {
    document(
        &get(harness, &format!("/operator/candidates{query}")).await,
        "the queue",
    )
}

/// Reads the candidate `id`.
async fn candidate(harness: &Harness, id: &str) -> Candidate {
    document(
        &get(harness, &format!("/operator/candidates/{id}")).await,
        "the candidate",
    )
}

/// Posts the decision `body` on the candidate `id`.
async fn decide(harness: &Harness, id: &str, body: &str) -> Answer {
    post(harness, &format!("/operator/candidates/{id}/decide"), body).await
}

/// The decision body of `verdict` on the versions of `read`, with `extra`
/// members.
fn body(verdict: &str, read: &Candidate, extra: &str) -> String {
    format!(
        r#"{{"verdict":"{verdict}","a_version":{},"b_version":{},"reason":"synthetic review"{extra}}}"#,
        read.a.version, read.b.version
    )
}

/// Counts the rows of `statement`.
async fn count(harness: &Harness, statement: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement.to_owned()))
        .fetch_one(harness.store.pool())
        .await
        .expect("the count reads")
}

#[tokio::test]
async fn a_link_merges_the_two_persons_with_one_action_and_one_record() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    pair(&harness, 1, "Vosterdijk").await;
    let listed = queue(&harness, "").await;
    assert_eq!(1, listed.total, "one candidate");
    let id = listed.rows[0].id.clone();
    let read = candidate(&harness, &id).await;
    assert!(read.merges, "the two records sit in two persons");
    assert_eq!(Status::Open, read.status);
    assert!(!read.evidence.fields.is_empty(), "the evidence is drawn");
    let before = harness.records();
    let decided: Decided = document(
        &decide(&harness, &id, &body("link", &read, "")).await,
        "link",
    );
    assert_eq!(Status::Linked, decided.status);
    assert!(decided.merge.is_some(), "the persons merged");
    assert_eq!(before + 1, harness.records(), "one BALP record");
    let answered = harness
        .pix_get(HOSPITAL_DOMAIN, "H-1", &[LABORATORY_DOMAIN])
        .await;
    assert!(
        answered
            .identifiers()
            .contains(&format!("{LABORATORY_DOMAIN}|L-1")),
        "the PIX query answers the laboratory's identifier: {}",
        answered.body
    );
    assert_eq!(
        1,
        count(
            &harness,
            &format!(
                "SELECT count(*) FROM pix.steward_action WHERE kind = 'link' AND candidate_id = '{id}' AND reason = 'synthetic review'"
            )
        )
        .await
    );
    assert_eq!(0, queue(&harness, "").await.total, "the queue is empty");
    let again = decide(&harness, &id, &body("link", &read, "")).await;
    assert_eq!(
        StatusCode::CONFLICT,
        again.status,
        "a decided candidate is stale"
    );
}

#[tokio::test]
async fn of_two_concurrent_decisions_one_commits_and_the_other_is_409() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    pair(&harness, 2, "Hollemink").await;
    let id = queue(&harness, "").await.rows[0].id.clone();
    let read = candidate(&harness, &id).await;
    let (link, apart) = (body("link", &read, ""), body("not-a-match", &read, ""));
    let (first, second) = tokio::join!(decide(&harness, &id, &link), decide(&harness, &id, &apart));
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!([StatusCode::OK, StatusCode::CONFLICT], statuses);
}

#[tokio::test]
async fn a_feed_between_the_read_and_the_decision_makes_it_409() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    pair(&harness, 3, "Terwolde").await;
    let id = queue(&harness, "").await.rows[0].id.clone();
    let read = candidate(&harness, &id).await;
    fed(
        &harness,
        HOSPITAL_DOMAIN,
        "H-3",
        &patient(HOSPITAL_DOMAIN, "H-3", "Terwolde", "Adah", "1971-03-04"),
    )
    .await;
    let answer = decide(&harness, &id, &body("link", &read, "")).await;
    assert_eq!(StatusCode::CONFLICT, answer.status, "{}", answer.body);
    assert!(answer.body.contains("reload"), "{}", answer.body);
}

#[tokio::test]
async fn a_not_a_match_stays_out_of_the_queue_until_a_record_s_demographics_change() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    pair(&harness, 4, "Brakelinga").await;
    let id = queue(&harness, "").await.rows[0].id.clone();
    let read = candidate(&harness, &id).await;
    let decided: Decided = document(
        &decide(&harness, &id, &body("not-a-match", &read, "")).await,
        "not a match",
    );
    assert_eq!(Status::NotAMatch, decided.status);
    let carrying: Document = serde_json::from_str(&format!(
        r#"{{"resourceType":"Patient","identifier":[{{"system":"{LABORATORY_DOMAIN}","value":"L-4"}},{{"system":"{PRACTICE_DOMAIN}","value":"P-4"}}],"name":[{{"family":"Brakelinga","given":["Ada"]}}],"gender":"female","birthDate":"1971-03-05","address":[{{"postalCode":"9905ZZ"}}]}}"#
    ))
    .expect("a patient");
    fed(&harness, LABORATORY_DOMAIN, "L-4", &carrying).await;
    assert_eq!(
        0,
        queue(&harness, "").await.total,
        "a feed that changes no demographics leaves the decision standing"
    );
    fed(
        &harness,
        LABORATORY_DOMAIN,
        "L-4",
        &patient(
            LABORATORY_DOMAIN,
            "L-4",
            "Brakelinga",
            "Adaline",
            "1971-03-08",
        ),
    )
    .await;
    assert_eq!(
        1,
        queue(&harness, "").await.total,
        "a change of the demographics queues the pair again"
    );
}

#[tokio::test]
async fn a_deferral_leaves_the_queue_until_its_day_and_a_malformed_decision_is_400() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    pair(&harness, 5, "Oldenzijl").await;
    let id = queue(&harness, "").await.rows[0].id.clone();
    let read = candidate(&harness, &id).await;
    for (verdict, extra) in [
        ("defer", ""),
        ("defer", r#","until":"2001-01-01""#),
        ("link", r#","until":"2999-01-01""#),
    ] {
        let refused = decide(&harness, &id, &body(verdict, &read, extra)).await;
        assert_eq!(StatusCode::BAD_REQUEST, refused.status, "{verdict}{extra}");
    }
    let blank = decide(
        &harness,
        &id,
        &format!(
            r#"{{"verdict":"link","a_version":{},"b_version":{},"reason":"  "}}"#,
            read.a.version, read.b.version
        ),
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, blank.status, "a blank reason");
    let deferred: Decided = document(
        &decide(
            &harness,
            &id,
            &body("defer", &read, r#","until":"2999-01-01""#),
        )
        .await,
        "defer",
    );
    assert_eq!(Status::Deferred, deferred.status);
    assert_eq!(0, queue(&harness, "").await.total, "out of the queue");
    assert_eq!(
        1,
        queue(&harness, "?deferred=true").await.total,
        "listed as deferred"
    );
    sqlx::query(
        "UPDATE pix.candidate SET deferred_until = now() - interval '1 day' WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(harness.store.pool())
    .await
    .expect("the day comes");
    let back = queue(&harness, "").await;
    assert_eq!(1, back.total, "back on its day");
    assert_eq!(Status::Deferred, back.rows[0].status);
    let decided: Decided = document(
        &decide(&harness, &id, &body("not-a-match", &read, "")).await,
        "a deferred candidate is decided",
    );
    assert_eq!(Status::NotAMatch, decided.status);
}

#[tokio::test]
async fn the_queue_lists_by_grade_score_and_age_under_its_filters() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    for (n, family) in [(6, "Zandvoorde"), (7, "Kerkhoving"), (8, "Middelbeek")] {
        pair(&harness, n, family).await;
    }
    let listed = queue(&harness, "").await;
    assert_eq!(3, listed.total);
    let ids: Vec<String> = listed.rows.iter().map(|row| row.id.clone()).collect();
    for (id, grade, score) in [
        (&ids[0], "possible", 0.65),
        (&ids[1], "certain", 0.99),
        (&ids[2], "probable", 0.85),
    ] {
        sqlx::query("UPDATE pix.candidate SET grade = $2, score = $3 WHERE id = $1::uuid")
            .bind(id)
            .bind(grade)
            .bind(score)
            .execute(harness.store.pool())
            .await
            .expect("the grade is set");
    }
    let ordered: Vec<String> = queue(&harness, "")
        .await
        .rows
        .iter()
        .map(|row| row.id.clone())
        .collect();
    assert_eq!(
        vec![ids[1].clone(), ids[2].clone(), ids[0].clone()],
        ordered,
        "certain, probable, possible"
    );
    let probable = queue(&harness, "?grade=probable").await;
    assert_eq!(vec![ids[2].clone()], vec![probable.rows[0].id.clone()]);
    assert_eq!(1, probable.total);
    let encoded: String =
        url::form_urlencoded::byte_serialize(LABORATORY_DOMAIN.as_bytes()).collect();
    assert_eq!(
        3,
        queue(&harness, &format!("?domain={encoded}")).await.total
    );
    let practice: String =
        url::form_urlencoded::byte_serialize(PRACTICE_DOMAIN.as_bytes()).collect();
    assert_eq!(
        0,
        queue(&harness, &format!("?domain={practice}")).await.total
    );
    assert_eq!(0, queue(&harness, "?older_than_days=1").await.total);
    sqlx::query(
        "UPDATE pix.candidate SET created_at = now() - interval '2 days' WHERE id = $1::uuid",
    )
    .bind(&ids[0])
    .execute(harness.store.pool())
    .await
    .expect("the candidate ages");
    assert_eq!(1, queue(&harness, "?older_than_days=1").await.total);
    let refused = get(&harness, "/operator/candidates?colour=red").await;
    assert_eq!(StatusCode::BAD_REQUEST, refused.status);
    let read = candidate(&harness, &ordered[1]).await;
    assert_eq!(Some(ordered[0].clone()), read.previous);
    assert_eq!(Some(ordered[2].clone()), read.next);
    let unknown = get(
        &harness,
        "/operator/candidates/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
    )
    .await;
    assert_eq!(StatusCode::NOT_FOUND, unknown.status);
}
