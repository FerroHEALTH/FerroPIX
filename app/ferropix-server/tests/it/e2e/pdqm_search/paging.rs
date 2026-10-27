// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-78's paging against PostgreSQL 18: the pages bounded and stable over
//! `GET` and `POST`, and the first page of a search over 100,000 records
//! within the request timeout.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON"
)]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::profiles::Deprecated;
use ferropix_server::pdqm::MAX_PAGE;
use ferropix_server::store::Store;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
use http::{Method, StatusCode};
use serde_json::Value;
use uuid::Uuid;

use super::super::supplier::{Fed, feed, request, search_both, serve};

/// Feeds `count` persons named `family` straight into the store, each
/// with one record of `domain`, rotating the synthetic family names when
/// `family` is `None`.
pub(crate) async fn bulk(store: &Store, count: i32, salt: &str, family: Option<&str>) {
    let domain = super::super::domain(store, HOSPITAL_DOMAIN).await;
    let mut transaction = store.pool().begin().await.expect("a transaction");
    let names: Vec<String> = FAMILY_NAMES.iter().map(|n| (*n).to_owned()).collect();
    let given: Vec<String> = GIVEN_NAMES.iter().map(|n| (*n).to_owned()).collect();
    sqlx::query(
        "INSERT INTO pix.person (id)
         SELECT md5('person-' || $2 || i)::uuid FROM generate_series(1, $1) i",
    )
    .bind(count)
    .bind(salt)
    .execute(&mut *transaction)
    .await
    .expect("the persons");
    sqlx::query(
        "INSERT INTO pix.source_record (id, domain_id, identifier_hmac, identifier_value, key_id)
         SELECT md5('record-' || $2 || i)::uuid, $3,
                sha256(convert_to('bulk-' || $2 || i, 'UTF8')), 'B-' || $2 || '-' || i, $4
         FROM generate_series(1, $1) i",
    )
    .bind(count)
    .bind(salt)
    .bind(domain.id.uuid())
    .bind(store.sealer().key_id().as_bytes().as_slice())
    .execute(&mut *transaction)
    .await
    .expect("the records");
    sqlx::query(
        "INSERT INTO pix.source_record_version (record_id, version, demographics, channel)
         SELECT md5('record-' || $2 || i)::uuid, 1,
                jsonb_build_object(
                    'name', jsonb_build_array(jsonb_build_object(
                        'family', coalesce($3, ($4::text[])[1 + i % 8]),
                        'given', jsonb_build_array(($5::text[])[1 + (i / 8) % 8]))),
                    'gender', CASE WHEN i % 2 = 0 THEN 'female' ELSE 'male' END,
                    'birthDate', to_char(date '1940-01-01' + (i % 20000), 'YYYY-MM-DD')),
                'fhir'
         FROM generate_series(1, $1) i",
    )
    .bind(count)
    .bind(salt)
    .bind(family)
    .bind(&names)
    .bind(&given)
    .execute(&mut *transaction)
    .await
    .expect("the versions");
    sqlx::query(
        "INSERT INTO pix.link (record_id, person_id, kind, valid, made_by)
         SELECT md5('record-' || $2 || i)::uuid, md5('person-' || $2 || i)::uuid,
                'feed', tstzrange(now(), NULL), 'e2e'
         FROM generate_series(1, $1) i",
    )
    .bind(count)
    .bind(salt)
    .execute(&mut *transaction)
    .await
    .expect("the links");
    transaction.commit().await.expect("the bulk commits");
}

/// Returns the request URI of an absolute paging link.
fn relative(link: &str) -> String {
    link.strip_prefix("http://localhost:8080")
        .expect("a link under the base")
        .to_owned()
}

/// Returns the URL of the link `relation` of `answer`.
fn link(answer: &Value, relation: &str) -> Option<String> {
    answer["link"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|link| link["relation"] == relation)
        .and_then(|link| link["url"].as_str())
        .map(str::to_owned)
}

#[tokio::test]
async fn paging_is_bounded_and_stable_over_get_and_post() {
    if !e2e_enabled() {
        return;
    }
    let served = serve(Deprecated::Inactive, &Matching::default()).await;
    let store = served.started.store.clone();
    let token = served.consumer();
    let family = FAMILY_NAMES[5];
    let feeder = super::super::feeds::feeder(&store);
    let fed = |index: usize| {
        let value = format!("S-{index}");
        let body = Fed::new(HOSPITAL_DOMAIN, &value, family, GIVEN_NAMES[index % 8]).body();
        (value, body)
    };
    for index in 0..7 {
        let (value, body) = fed(index);
        feed(&feeder, HOSPITAL_DOMAIN, &value, &body).await;
    }
    let query = format!("family={family}&_count=3");
    let first = search_both(&served.app, &token, &query).await;
    assert_eq!(StatusCode::OK, first.status, "{}", first.body);
    let json = first.json();
    assert_eq!(7, json["total"]);
    assert_eq!(3, first.ids("match").len());
    assert!(link(&json, "first").is_some(), "a first link");
    let mut seen: Vec<Uuid> = first.ids("match");
    let (value, body) = fed(7);
    let late = feed(&feeder, HOSPITAL_DOMAIN, &value, &body).await;
    let mut next = link(&json, "next");
    let mut pages = 1;
    while let Some(url) = next {
        assert!(
            url.contains("_count=3"),
            "the next link keeps the page size: {url}"
        );
        let page = request(
            &served.app,
            Method::GET,
            &relative(&url),
            &[("authorization", &token)],
            Vec::new(),
        )
        .await;
        assert_eq!(StatusCode::OK, page.status, "{}", page.body);
        assert!(page.ids("match").len() <= 3, "bounded by _count");
        seen.extend(page.ids("match"));
        next = link(&page.json(), "next");
        pages += 1;
    }
    let unique: BTreeSet<Uuid> = seen.iter().copied().collect();
    assert_eq!(seen.len(), unique.len(), "no person on two pages");
    assert_eq!(8, unique.len(), "every person once");
    assert_eq!(
        Some(&late),
        seen.last(),
        "the person created while paging comes last"
    );
    assert_eq!(3, pages);
    let count_only = search_both(&served.app, &token, &format!("family={family}&_count=0")).await;
    assert_eq!(8, count_only.json()["total"]);
    assert!(
        count_only.ids("match").is_empty(),
        "_count=0 answers the total alone"
    );
    bulk(&store, 105, "many", Some(FAMILY_NAMES[6])).await;
    let capped = search_both(
        &served.app,
        &token,
        &format!("family={}&_count=1000", FAMILY_NAMES[6]),
    )
    .await;
    assert_eq!(105, capped.json()["total"]);
    assert_eq!(
        usize::try_from(MAX_PAGE).unwrap(),
        capped.ids("match").len(),
        "a page holds at most MAX_PAGE"
    );
    let unpaged = search_both(&served.app, &token, &format!("family={}", FAMILY_NAMES[6])).await;
    assert_eq!(
        usize::try_from(ferropix_server::pdqm::DEFAULT_PAGE).unwrap(),
        unpaged.ids("match").len()
    );
    let malformed = search_both(&served.app, &token, "family=x&_page=not-a-page").await;
    assert_eq!(StatusCode::BAD_REQUEST, malformed.status);
}

#[tokio::test]
async fn the_first_page_of_a_search_over_100000_records_answers_within_the_request_timeout() {
    if !e2e_enabled() {
        return;
    }
    let served = serve(Deprecated::Inactive, &Matching::default()).await;
    bulk(&served.started.store, 100_000, "load", None).await;
    let token = served.consumer();
    let timeout = Duration::from_millis(
        ferrohealth_server::section::ServerSection::default().request_timeout_ms,
    );
    for query in [
        "family=Proefsma&birthdate=1961-04-12",
        "family=proef",
        "given=Ada&gender=female",
    ] {
        let started = Instant::now();
        let answer = request(
            &served.app,
            Method::GET,
            &format!("/fhir/Patient?{query}"),
            &[("authorization", &token)],
            Vec::new(),
        )
        .await;
        let elapsed = started.elapsed();
        assert_eq!(StatusCode::OK, answer.status, "{query}: {}", answer.body);
        assert!(
            elapsed < timeout,
            "{query}: the first page took {elapsed:?}, the timeout is {timeout:?}"
        );
        eprintln!(
            "{query}: first page in {elapsed:?}, total {}",
            answer.json()["total"]
        );
    }
}
