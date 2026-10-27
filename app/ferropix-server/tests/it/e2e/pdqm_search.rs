// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-78 against PostgreSQL 18 through the guarded routes: every parameter
//! of PDQm 3.2.0 §2:3.78.4.1.2.1 alone, the two required combinations
//! (§2:3.78.4.1.2.1.1), AND and OR, `:exact` (§2:3.78.4.1.2.2), the domain
//! filter (§2:3.78.4.1.2.3) and the paging, each over `GET` and `POST`; the
//! six cases of §2:3.78.4.1.3 with the merged person under both
//! `[pdqm] deprecated` policies; the retrieve (§2:3.78.4.3); JSON and XML;
//! the guard; the BALP record of each search; and the first page of a
//! search over 100,000 records within the request timeout.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers and the audit records are read as JSON"
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

use super::supplier::{
    ACCENTED, BASE, FIXTURE_BSN, Fed, Population, Served, bsn_token, carried_bsn, feed, holds_to,
    over, patients_of, populate, profile, query_of, request, search_both, serve,
};

/// The hospital domain, written as a query value.
const HOSPITAL: &str = "urn%3Aoid%3A2.999.1.1";

/// Returns `ids` sorted.
fn sorted(ids: &[Uuid]) -> Vec<Uuid> {
    let mut ids = ids.to_vec();
    ids.sort();
    ids
}

/// Starts the store with the shared population, served under `deprecated`.
async fn populated(deprecated: Deprecated) -> (Served, Population) {
    let served = serve(deprecated, &Matching::default()).await;
    let population = populate(&served.started.store).await;
    (served, population)
}

#[tokio::test]
async fn every_parameter_alone_matches_over_get_and_post() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    let ada = [p.ada_hospital, p.ada_laboratory];
    let cases: Vec<(String, Vec<Uuid>)> = vec![
        (format!("_id={}", p.bram), vec![p.bram]),
        (
            String::from("active=true"),
            vec![p.ada_hospital, p.ada_laboratory, p.bram, p.cato],
        ),
        (String::from("active=false"), vec![p.merged]),
        (
            String::from("family=proefsma"),
            vec![ada[0], ada[1], p.cato],
        ),
        (String::from("family=Proef"), vec![ada[0], ada[1], p.cato]),
        (String::from("given=daan"), vec![p.bram]),
        (String::from("given=Ad"), ada.to_vec()),
        (format!("identifier={HOSPITAL}%7CH-300001"), vec![p.bram]),
        (String::from("identifier=H-300001"), vec![p.bram]),
        (
            format!("identifier={}", bsn_token(FIXTURE_BSN)),
            vec![p.ada_hospital],
        ),
        (
            format!("identifier={}", bsn_token(&carried_bsn())),
            vec![p.bram],
        ),
        (
            format!("identifier=http%3A%2F%2Flocalhost%3A8080%7C{}", p.bram),
            vec![p.bram],
        ),
        (format!("identifier={HOSPITAL}%7CH-100099"), Vec::new()),
        (String::from("telecom=phone%7C%2B31600000001"), vec![p.bram]),
        (String::from("telecom=%2B31600000001"), vec![p.bram]),
        (String::from("telecom=email%7C%2B31600000001"), Vec::new()),
        (String::from("birthdate=1970-01-01"), vec![p.bram]),
        (String::from("birthdate=1985"), vec![p.cato]),
        (String::from("birthdate=1985-07-15"), Vec::new()),
        (String::from("birthdate=ge1980"), vec![p.cato]),
        (String::from("birthdate=lt1965"), ada.to_vec()),
        (String::from("birthdate=sa1980-12-31"), vec![p.cato]),
        (String::from("birthdate=eb1962"), ada.to_vec()),
        (
            String::from("birthdate=ne1970-01-01"),
            vec![ada[0], ada[1], p.cato],
        ),
        (String::from("address=monster"), vec![p.bram]),
        (String::from("address=9999"), vec![p.bram]),
        (String::from("address=proeflaan"), vec![p.bram]),
        (String::from("address=Teststad"), ada.to_vec()),
        (String::from("address-city=Teststad"), ada.to_vec()),
        (
            String::from("address-country=NL"),
            vec![ada[0], ada[1], p.bram],
        ),
        (String::from("address-postalcode=9999%20ZZ"), vec![p.bram]),
        (
            String::from("address-state=Proefprovincie"),
            vec![p.ada_hospital, p.bram],
        ),
        (String::from("gender=male"), vec![p.bram]),
        (String::from("gender=female"), vec![ada[0], ada[1], p.cato]),
        (
            format!("mothersMaidenName={}", FAMILY_NAMES[4]),
            vec![p.bram],
        ),
    ];
    for (query, expected) in cases {
        let answer = search_both(&served.app, &token, &query).await;
        assert_eq!(StatusCode::OK, answer.status, "{query}: {}", answer.body);
        assert_eq!(sorted(&expected), sorted(&answer.ids("match")), "{query}");
        assert_eq!(
            Value::from(expected.len()),
            answer.json()["total"],
            "{query}: the total"
        );
    }
}

#[tokio::test]
async fn the_combinations_and_or_and_exact_match_over_get_and_post() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    let ada = [p.ada_hospital, p.ada_laboratory];
    let cases: Vec<(String, Vec<Uuid>)> = vec![
        (
            String::from("family=Proefsma&gender=female"),
            vec![ada[0], ada[1], p.cato],
        ),
        (
            format!("family={}&gender=female", FAMILY_NAMES[1]),
            Vec::new(),
        ),
        (
            String::from("birthdate=1961-04-12&family=Proefsma"),
            ada.to_vec(),
        ),
        (
            format!("family=Proefsma&given={}", GIVEN_NAMES[2]),
            vec![p.cato],
        ),
        (
            format!("given={},{}", GIVEN_NAMES[1], GIVEN_NAMES[2]),
            vec![p.bram, p.cato],
        ),
        (format!("given={}&given=Daan", GIVEN_NAMES[1]), vec![p.bram]),
        (String::from("family:exact=Proefsma"), ada.to_vec()),
        (String::from("family:exact=proefsma"), Vec::new()),
        (
            format!("family:exact={}", ACCENTED.replace('ó', "%C3%B3")),
            vec![p.cato],
        ),
        (
            format!("mothersMaidenName:exact={}", FAMILY_NAMES[4].to_lowercase()),
            Vec::new(),
        ),
        (String::from("address-city:exact=Monsterdam"), vec![p.bram]),
        (String::from("given:exact=Ad"), Vec::new()),
    ];
    for (query, expected) in cases {
        let answer = search_both(&served.app, &token, &query).await;
        assert_eq!(StatusCode::OK, answer.status, "{query}: {}", answer.body);
        assert_eq!(sorted(&expected), sorted(&answer.ids("match")), "{query}");
    }
}

#[tokio::test]
async fn the_domain_filter_keeps_the_named_domains_and_an_unknown_one_is_case_four() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    let answer = search_both(
        &served.app,
        &token,
        &format!("family=Proefsma&identifier={HOSPITAL}%7C"),
    )
    .await;
    assert_eq!(StatusCode::OK, answer.status, "§2:3.78.4.1.3 Case 2");
    let json = answer.json();
    for entry in json["entry"].as_array().expect("entries") {
        for identifier in entry["resource"]["identifier"]
            .as_array()
            .into_iter()
            .flatten()
        {
            assert_eq!(
                HOSPITAL_DOMAIN, identifier["system"],
                "only the named domain"
            );
        }
    }
    let ada = json["entry"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["resource"]["id"] == p.ada_hospital.to_string())
        .expect("Ada's hospital person");
    assert_eq!(
        1,
        ada["resource"]["identifier"].as_array().map_or(0, Vec::len)
    );
    let self_link = json["link"][0]["url"].as_str().expect("a self link");
    assert!(self_link.contains("identifier="), "{self_link}");
    let unknown = search_both(
        &served.app,
        &token,
        "family=Proefsma&identifier=urn%3Aoid%3A2.999.9.9%7C",
    )
    .await;
    assert_eq!(
        StatusCode::NOT_FOUND,
        unknown.status,
        "§2:3.78.4.1.3 Case 4"
    );
    assert_eq!(
        (
            String::from("warning"),
            String::from("not-found"),
            String::from("targetSystem not found")
        ),
        unknown.issue()
    );
}

#[tokio::test]
async fn the_six_cases_answer_their_codes_issues_and_bundles() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    let one = search_both(&served.app, &token, &format!("family={}", FAMILY_NAMES[1])).await;
    assert_eq!(StatusCode::OK, one.status, "Case 1");
    let json = one.json();
    assert_eq!(1, json["total"], "Case 1: the total");
    assert_eq!(
        format!("{BASE}/Patient/{}", p.bram),
        json["entry"][0]["fullUrl"]
    );
    holds_to(&json, &profile("QueryPatientResourceResponseMessage"));
    let none = search_both(&served.app, &token, "family=Nergens").await;
    assert_eq!(StatusCode::OK, none.status, "Case 3");
    assert_eq!(0, none.json()["total"]);
    assert!(none.json().get("entry").is_none(), "Case 3: no entry");
    let unacceptable = search_both(&served.app, &token, "family=Proefsma&_format=text/html").await;
    assert_eq!(StatusCode::NOT_ACCEPTABLE, unacceptable.status, "Case 5");
    assert_eq!("not-supported", unacceptable.issue().1);
    let accept = request(
        &served.app,
        Method::GET,
        "/fhir/Patient?family=Proefsma",
        &[("authorization", &token), ("accept", "text/html")],
        Vec::new(),
    )
    .await;
    assert_eq!(
        StatusCode::NOT_ACCEPTABLE,
        accept.status,
        "Case 5 by Accept"
    );

    let inactive = search_both(&served.app, &token, &format!("_id={}", p.merged)).await;
    assert_eq!(StatusCode::OK, inactive.status, "Case 6");
    assert_eq!(vec![p.merged], inactive.ids("match"));
    assert_eq!(
        vec![p.ada_hospital],
        inactive.ids("include"),
        "the survivor"
    );
    let json = inactive.json();
    let deprecated = &json["entry"][0]["resource"];
    assert_eq!(false, deprecated["active"]);
    assert_eq!("replaced-by", deprecated["link"][0]["type"]);
    assert_eq!(
        format!("Patient/{}", p.ada_hospital),
        deprecated["link"][0]["other"]["reference"]
    );
    assert_eq!(1, json["total"], "the survivor is no match");
    holds_to(&json, &profile("QueryPatientResourceResponseMessage"));

    let (omitting, issuer, _spool, _secrets) = over(
        &served.started.store,
        Deprecated::Omit,
        &Matching::default(),
    )
    .await;
    let token = crate::auth::bearer(&issuer.token("east-lab", "system/Patient.rs"));
    for query in [format!("_id={}", p.merged), String::from("active=false")] {
        let omitted = search_both(&omitting, &token, &query).await;
        assert_eq!(StatusCode::OK, omitted.status, "Case 6, omit");
        assert_eq!(0, omitted.json()["total"], "{query}");
    }
    let survivor = search_both(&omitting, &token, &format!("_id={}", p.ada_hospital)).await;
    assert_eq!(vec![p.ada_hospital], survivor.ids("match"));
}

#[tokio::test]
async fn the_retrieve_answers_one_patient_or_not_found_and_a_merged_one_by_policy() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    let get = |path: String, accept: Option<&'static str>| {
        let (app, token) = (served.app.clone(), token.clone());
        async move {
            let mut headers = vec![("authorization", token.as_str())];
            if let Some(accept) = accept {
                headers.push(("accept", accept));
            }
            request(&app, Method::GET, &path, &headers, Vec::new()).await
        }
    };
    let found = get(format!("/fhir/Patient/{}", p.ada_hospital), None).await;
    assert_eq!(StatusCode::OK, found.status, "§2:3.78.4.3.3 Case 1");
    let patient = found.json();
    assert_eq!(p.ada_hospital.to_string(), patient["id"]);
    holds_to(&patient, &profile("Patient"));
    let systems: Vec<&str> = patient["identifier"]
        .as_array()
        .expect("identifiers")
        .iter()
        .filter_map(|identifier| identifier["system"].as_str())
        .collect();
    assert_eq!(
        vec![
            "http://localhost:8080",
            "http://fhir.nl/fhir/NamingSystem/bsn",
            HOSPITAL_DOMAIN
        ],
        systems,
        "the master identifier, then each record's by system"
    );
    assert!(
        found.body.contains(FIXTURE_BSN),
        "the BSN opened for the answer"
    );
    for missing in [
        format!("/fhir/Patient/{}", Uuid::max()),
        String::from("/fhir/Patient/x-1"),
    ] {
        let answer = get(missing, None).await;
        assert_eq!(StatusCode::NOT_FOUND, answer.status, "Case 2");
        assert_eq!(
            ("error", "not-found"),
            (answer.issue().0.as_str(), answer.issue().1.as_str())
        );
    }
    let merged = get(format!("/fhir/Patient/{}", p.merged), None).await;
    assert_eq!(StatusCode::OK, merged.status, "Case 3, inactive");
    assert_eq!(false, merged.json()["active"]);
    let xml = get(
        format!("/fhir/Patient/{}", p.bram),
        Some("application/fhir+xml"),
    )
    .await;
    assert_eq!(StatusCode::OK, xml.status);
    assert!(
        xml.media_type.starts_with("application/fhir+xml"),
        "{}",
        xml.media_type
    );
    assert!(xml.body.contains("<Patient"), "{}", xml.body);
    let refused = get(format!("/fhir/Patient/{}?_format=text/html", p.bram), None).await;
    assert_eq!(StatusCode::BAD_REQUEST, refused.status, "§2:3.78.4.4.2");

    let (omitting, issuer, _spool, _secrets) = over(
        &served.started.store,
        Deprecated::Omit,
        &Matching::default(),
    )
    .await;
    let token = crate::auth::bearer(&issuer.token("east-lab", "system/Patient.r"));
    let omitted = request(
        &omitting,
        Method::GET,
        &format!("/fhir/Patient/{}", p.merged),
        &[("authorization", &token)],
        Vec::new(),
    )
    .await;
    assert_eq!(StatusCode::NOT_FOUND, omitted.status, "Case 3, omit");
}

#[tokio::test]
async fn a_search_answers_json_or_xml_as_asked() {
    if !e2e_enabled() {
        return;
    }
    let (served, _) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    for (query, accept, media, start) in [
        (
            "family=Testerink&_format=xml",
            None,
            "application/fhir+xml",
            "<Bundle",
        ),
        (
            "family=Testerink&_format=json",
            None,
            "application/fhir+json",
            "{",
        ),
        (
            "family=Testerink",
            Some("application/fhir+xml"),
            "application/fhir+xml",
            "<Bundle",
        ),
        ("family=Testerink", None, "application/fhir+json", "{"),
    ] {
        let mut headers = vec![("authorization", token.as_str())];
        if let Some(accept) = accept {
            headers.push(("accept", accept));
        }
        let answer = request(
            &served.app,
            Method::GET,
            &format!("/fhir/Patient?{query}"),
            &headers,
            Vec::new(),
        )
        .await;
        assert_eq!(StatusCode::OK, answer.status, "{query}");
        assert!(
            answer.media_type.starts_with(media),
            "{query}: {}",
            answer.media_type
        );
        assert!(
            answer.body.trim_start().starts_with(start),
            "{query}: {}",
            answer.body
        );
    }
}

/// Feeds `count` persons named `family` straight into the store, each
/// with one record of `domain`, rotating the synthetic family names when
/// `family` is `None`.
pub(crate) async fn bulk(store: &Store, count: i32, salt: &str, family: Option<&str>) {
    let domain = super::domain(store, HOSPITAL_DOMAIN).await;
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
    let feeder = super::feeds::feeder(&store);
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
async fn the_guard_admits_a_consumer_or_the_iua_scope_and_refuses_the_rest() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = |scope: &str| crate::auth::bearer(&served.issuer.token("east-lab", scope));
    for (path, scope, status) in [
        (
            String::from("/fhir/Patient?family=Proefsma"),
            None,
            StatusCode::UNAUTHORIZED,
        ),
        (
            String::from("/fhir/Patient?family=Proefsma"),
            Some("system/Practitioner.rs"),
            StatusCode::FORBIDDEN,
        ),
        (
            String::from("/fhir/Patient?family=Proefsma"),
            Some("ITI-78"),
            StatusCode::OK,
        ),
        (
            String::from("/fhir/Patient?family=Proefsma"),
            Some("ITI-119"),
            StatusCode::FORBIDDEN,
        ),
        (
            format!("/fhir/Patient/{}", p.bram),
            Some("system/Patient.s"),
            StatusCode::FORBIDDEN,
        ),
        (
            format!("/fhir/Patient/{}", p.bram),
            Some("system/Patient.r"),
            StatusCode::OK,
        ),
    ] {
        let mut headers = Vec::new();
        let bearer = scope.map(token);
        if let Some(bearer) = &bearer {
            headers.push(("authorization", bearer.as_str()));
        }
        let answer = request(&served.app, Method::GET, &path, &headers, Vec::new()).await;
        assert_eq!(
            status, answer.status,
            "{path} with {scope:?}: {}",
            answer.body
        );
    }
}

#[tokio::test]
async fn every_search_and_retrieve_is_one_balp_record_per_person_found() {
    if !e2e_enabled() {
        return;
    }
    let (served, p) = populated(Deprecated::Inactive).await;
    let token = served.consumer();
    let before = served.records().len();
    let answer = request(
        &served.app,
        Method::GET,
        "/fhir/Patient?family=Proefsma",
        &[("authorization", &token)],
        Vec::new(),
    )
    .await;
    assert_eq!(StatusCode::OK, answer.status);
    let records = served.records();
    let fresh = &records[before..];
    assert_eq!(3, fresh.len(), "one record per person found");
    let named: BTreeSet<Option<String>> = patients_of(fresh).into_iter().collect();
    let expected: BTreeSet<Option<String>> = [p.ada_hospital, p.ada_laboratory, p.cato]
        .iter()
        .map(|id| Some(format!("Patient/{id}")))
        .collect();
    assert_eq!(expected, named);
    for record in fresh {
        assert!(
            query_of(record).contains("family=Proefsma"),
            "the raw request"
        );
        assert_eq!(
            "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Query.Audit.Supplier",
            record["meta"]["profile"][0]
        );
    }
    let before = served.records().len();
    let posted = request(
        &served.app,
        Method::POST,
        "/fhir/Patient/_search",
        &[
            ("authorization", &token),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        b"family=Nergens".to_vec(),
    )
    .await;
    assert_eq!(StatusCode::OK, posted.status);
    let records = served.records();
    assert_eq!(1, records.len() - before, "one record when none was found");
    assert_eq!(vec![None], patients_of(&records[before..]));
    let raw = query_of(&records[before]);
    assert!(
        raw.starts_with("POST ") && raw.ends_with("family=Nergens"),
        "{raw}"
    );
    let before = served.records().len();
    let retrieved = request(
        &served.app,
        Method::GET,
        &format!("/fhir/Patient/{}", p.bram),
        &[("authorization", &token)],
        Vec::new(),
    )
    .await;
    assert_eq!(StatusCode::OK, retrieved.status);
    let records = served.records();
    assert_eq!(
        vec![Some(format!("Patient/{}", p.bram))],
        patients_of(&records[before..])
    );
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
