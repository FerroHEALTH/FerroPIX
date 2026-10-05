// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 and ITI-119 audit records of an audited PDQm client (PDQm 3.2.0
//! §2:3.78.5.1, §2:3.119.5.1.1): held to the Query and Match Consumer audit
//! profiles and their examples, the request as sent in the query entity, and
//! an exchange whose record is refused fails.

use std::sync::Arc;

use ihe_iti::balp::Outcome;
use ihe_iti::pdqm::error::PdqmError;
use ihe_iti::pdqm::input::MatchInput;
use ihe_iti::recording::Late;
use ihe_iti::user::OnBehalfOf;
use secrecy::SecretString;

use super::profile::{
    CLIENT, Kept, Refusing, SUBJECT, Stalled, base64_decoded, holds_to, like, names_no_user,
    names_the_user, user, vendored, written,
};
use crate::pdqm::{
    EXAMPLE_BUNDLE, FHIR_JSON, PROMPT, client, schmidt, supplier, unreachable_client,
};

#[tokio::test]
async fn a_search_is_recorded_as_the_consumer_audit_profile_fixes_it() {
    let server = supplier(200, FHIR_JSON, crate::pdqm::vendored(EXAMPLE_BUNDLE)).await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a page");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::Success);
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-pdqm",
            "package/StructureDefinition-IHE.PDQm.Query.Audit.Consumer.json",
        ),
    );
    like(
        &record,
        &vendored(
            "ihe-pdqm",
            "package/example/AuditEvent-ex-auditPdqmQuery-consumer.json",
        ),
        true,
    );
    let query = base64_decoded(record["entity"][0]["query"].as_str().expect("a query"));
    assert_eq!(
        query,
        format!(
            "POST {}/fhir/Patient/_search\nContent-Type: application/x-www-form-urlencoded\n\nfamily=Schmidt",
            server.uri()
        ),
        "the raw request, as BALP's POST search example writes it"
    );
}

#[tokio::test]
async fn an_unreachable_supplier_is_recorded_as_a_serious_failure() {
    let kept = Arc::new(Kept::default());
    let client = unreachable_client().audited(kept.clone());
    client
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("no Supplier answers");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::SeriousFailure);
}

#[tokio::test]
async fn a_search_whose_record_is_refused_fails() {
    let server = supplier(200, FHIR_JSON, crate::pdqm::vendored(EXAMPLE_BUNDLE)).await;
    let client = client(&server).audited(Arc::new(Refusing));
    let error = client
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("the search fails closed");
    assert!(matches!(error, PdqmError::Audit(_)), "{error:?}");
    assert!(
        !format!("{error:?} {error}").contains("Schmidt"),
        "the error names no demographic"
    );
}

/// A stub Supplier that answers every match with the IG's one-match example.
async fn matcher() -> wiremock::MockServer {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/fhir/Patient/$match"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(
            crate::pdqm::vendored("example/Bundle-ex-match-output.json").into_bytes(),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    server
}

fn match_input() -> MatchInput {
    MatchInput::new(crate::pdqm::DOMAIN, &SecretString::from("12345"))
        .expect("an input")
        .only_certain_matches(true)
}

#[tokio::test]
async fn a_match_is_recorded_as_the_match_consumer_audit_profile_fixes_it() {
    let server = matcher().await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .match_patient(&match_input(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a match");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::Success);
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-pdqm",
            "package/StructureDefinition-IHE.PDQm.Match.Audit.Consumer.json",
        ),
    );
    like(
        &record,
        &vendored(
            "ihe-pdqm",
            "package/example/AuditEvent-ex-auditPdqmMatch-consumer.json",
        ),
        false,
    );
    let query = base64_decoded(record["entity"][0]["query"].as_str().expect("a query"));
    let sent = format!(
        "POST {}/fhir/Patient/$match\nContent-Type: application/fhir+json\n\n",
        server.uri()
    );
    assert!(
        query.starts_with(&sent) && query.contains(r#""resourceType":"Parameters""#),
        "the raw request, as for the ITI-78 POST search: {query}"
    );
    assert_eq!(
        record["entity"][1]["what"]["identifier"]["value"], "12345",
        "the patient the input identifies (entity:patient)"
    );
}

#[tokio::test]
async fn a_match_whose_record_is_refused_fails() {
    let server = matcher().await;
    let client = client(&server).audited(Arc::new(Refusing));
    let error = client
        .match_patient(&match_input(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("the match fails closed");
    assert!(matches!(error, PdqmError::Audit(_)), "{error:?}");
    assert!(!format!("{error:?} {error}").contains("12345"));
}

/// The time the exchanges below are given.
const BUDGET: std::time::Duration = std::time::Duration::from_millis(300);

/// The time a loaded host may add to any wait a test makes.
const SLACK: std::time::Duration = std::time::Duration::from_secs(3);

#[tokio::test]
async fn a_search_whose_record_is_not_stored_within_the_exchange_s_time_fails() {
    let server = supplier(200, FHIR_JSON, crate::pdqm::vendored(EXAMPLE_BUNDLE)).await;
    let client = client(&server).audited(Arc::new(Stalled));
    let asked = std::time::Instant::now();
    let error = client
        .search(&schmidt(), &OnBehalfOf::System, BUDGET)
        .await
        .expect_err("the search fails closed");
    assert!(asked.elapsed() < BUDGET + SLACK, "{:?}", asked.elapsed());
    let PdqmError::Audit(audit) = &error else {
        panic!("an audit failure: {error:?}");
    };
    assert!(audit.0.is::<Late>(), "{audit:?}");
}

#[tokio::test]
async fn a_match_whose_record_is_not_stored_within_the_exchange_s_time_fails() {
    let server = matcher().await;
    let client = client(&server).audited(Arc::new(Stalled));
    let asked = std::time::Instant::now();
    let error = client
        .match_patient(&match_input(), &OnBehalfOf::System, BUDGET)
        .await
        .expect_err("the match fails closed");
    assert!(asked.elapsed() < BUDGET + SLACK, "{:?}", asked.elapsed());
    let PdqmError::Audit(audit) = &error else {
        panic!("an audit failure: {error:?}");
    };
    assert!(audit.0.is::<Late>(), "{audit:?}");
    assert!(!format!("{error:?} {error}").contains("12345"));
}

#[tokio::test]
async fn an_unreachable_supplier_of_a_match_is_recorded_as_a_serious_failure() {
    let kept = Arc::new(Kept::default());
    let client = unreachable_client().audited(kept.clone());
    client
        .match_patient(&match_input(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("no Supplier answers");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::SeriousFailure);
}

#[tokio::test]
async fn a_search_made_for_a_user_names_them_from_their_token() {
    let server = supplier(200, FHIR_JSON, crate::pdqm::vendored(EXAMPLE_BUNDLE)).await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .search(&schmidt(), &user(), PROMPT)
        .await
        .expect("a page");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-pdqm",
            "package/StructureDefinition-IHE.PDQm.Query.Audit.Consumer.json",
        ),
    );
    names_the_user(&record);
}

#[tokio::test]
async fn a_match_made_for_a_user_names_them_from_their_token() {
    let server = matcher().await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .match_patient(&match_input(), &user(), PROMPT)
        .await
        .expect("a match");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-pdqm",
            "package/StructureDefinition-IHE.PDQm.Match.Audit.Consumer.json",
        ),
    );
    names_the_user(&record);
    let shown = format!("{exchange:?}");
    for value in [SUBJECT, CLIENT] {
        assert!(!shown.contains(value), "Debug names no user: {shown}");
    }
}

#[tokio::test]
async fn a_search_and_a_match_the_system_makes_on_its_own_behalf_name_no_user() {
    let server = supplier(200, FHIR_JSON, crate::pdqm::vendored(EXAMPLE_BUNDLE)).await;
    let kept = Arc::new(Kept::default());
    client(&server)
        .audited(kept.clone())
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a page");
    let matching = matcher().await;
    client(&matching)
        .audited(kept.clone())
        .match_patient(&match_input(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a match");
    let exchanges = kept.taken();
    assert_eq!(2, exchanges.len(), "a search and a match");
    for exchange in &exchanges {
        names_no_user(&written(exchange));
    }
}
