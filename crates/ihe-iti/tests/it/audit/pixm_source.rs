// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-104 audit record of an audited Patient Identity Source (PIXm
//! 3.1.0 §2:3.104.5.1.1): an update or a delete, held to the Feed Update and
//! Feed Delete Source audit profiles and their examples, naming the patient
//! by the identifier the message names, recorded whatever the outcome, and
//! failing the exchange when the recorder refuses it.

use std::sync::Arc;

use ihe_atna::balp::Outcome;
use ihe_atna::user::OnBehalfOf;
use ihe_iti::pixm::source::error::SourceError;
use wiremock::ResponseTemplate;

use super::profile::{
    CLIENT, Kept, Refusing, SUBJECT, holds_to, like, names_no_user, names_the_user, user, vendored,
    written,
};
use crate::pixm::source::{alissa, maiden, manager, red, source, unreachable_source};
use crate::pixm::{PROMPT, RED, RED_VALUE};

/// The Source's Feed Update audit profile and its example.
const UPDATE: (&str, &str) = (
    "package/StructureDefinition-IHE.PIXm.Feed.Update.Audit.Source.json",
    "package/example/AuditEvent-ex-auditPixmFeed-update-source.json",
);

/// The Source's Feed Delete audit profile and its example.
const DELETE: (&str, &str) = (
    "package/StructureDefinition-IHE.PIXm.Feed.Delete.Audit.Source.json",
    "package/example/AuditEvent-ex-auditPixmFeed-delete-source.json",
);

/// Holds `record` to the profile and the example of `(profile, example)`.
#[expect(
    clippy::disallowed_types,
    reason = "the test seam: the records are read as JSON values"
)]
fn holds_to_source(record: &serde_json::Value, (profile, example): (&str, &str)) {
    holds_to(record, &vendored("ihe-pixm", profile));
    like(record, &vendored("ihe-pixm", example), true);
}

/// The entity of `record` whose role code is `role`.
#[expect(
    clippy::disallowed_types,
    reason = "the test seam: the records are read as JSON values"
)]
fn entity<'a>(record: &'a serde_json::Value, role: &str) -> &'a serde_json::Value {
    record["entity"]
        .as_array()
        .expect("entities")
        .iter()
        .find(|entity| entity["role"]["code"] == role)
        .unwrap_or_else(|| panic!("an entity of role {role}"))
}

#[tokio::test]
async fn a_feed_is_recorded_as_the_update_source_profile_fixes_it() {
    let server = manager(
        "PUT",
        ResponseTemplate::new(201).insert_header("Location", "Patient/p-1"),
    )
    .await;
    let kept = Arc::new(Kept::default());
    source(&server)
        .audited(kept.clone())
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("applied");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::Success);
    let record = written(&exchange);
    holds_to_source(&record, UPDATE);
    let patient = entity(&record, "1");
    assert_eq!(patient["what"]["identifier"]["system"], RED);
    assert_eq!(patient["what"]["identifier"]["value"], RED_VALUE);
    assert!(
        patient["what"]["reference"].is_null(),
        "patient.what.reference is 0..0"
    );
    assert_eq!(
        entity(&record, "4")["what"]["reference"],
        "Patient/p-1",
        "the data entity is the Patient the Manager named"
    );
    assert_eq!(
        record["agent"][1]["who"]["display"],
        format!("{}/fhir/", server.uri()),
        "the Manager is named by its base"
    );
    assert!(
        !format!("{exchange:?}").contains(RED_VALUE),
        "Debug shows no identifier"
    );
    names_no_user(&record);
}

#[tokio::test]
async fn a_resolve_duplicate_is_recorded_as_an_update_of_the_subsumed_identifier() {
    let server = manager("PUT", ResponseTemplate::new(200)).await;
    let kept = Arc::new(Kept::default());
    source(&server)
        .audited(kept.clone())
        .resolve_duplicate(
            &red("IHERED-m94"),
            &red(RED_VALUE),
            &maiden(),
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
        .expect("applied");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    holds_to_source(&record, UPDATE);
    assert_eq!(
        entity(&record, "1")["what"]["identifier"]["value"],
        "IHERED-m94"
    );
    assert!(
        entity(&record, "4")["what"]["reference"].is_null(),
        "no Location, no reference"
    );
}

#[tokio::test]
async fn a_remove_is_recorded_as_the_delete_source_profile_fixes_it() {
    let server = manager("DELETE", ResponseTemplate::new(204)).await;
    let kept = Arc::new(Kept::default());
    source(&server)
        .audited(kept.clone())
        .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
        .await
        .expect("applied");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::Success);
    let record = written(&exchange);
    holds_to_source(&record, DELETE);
    assert_eq!(
        entity(&record, "1")["what"]["identifier"]["value"],
        RED_VALUE
    );
    names_no_user(&record);
}

#[tokio::test]
async fn a_rejection_is_a_minor_failure_and_no_answer_a_serious_one() {
    let server = manager("PUT", ResponseTemplate::new(422)).await;
    let kept = Arc::new(Kept::default());
    source(&server)
        .audited(kept.clone())
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("rejected");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::MinorFailure);
    let kept = Arc::new(Kept::default());
    unreachable_source()
        .audited(kept.clone())
        .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("no Manager");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::SeriousFailure);
}

#[tokio::test]
async fn a_message_refused_before_sending_is_not_recorded() {
    let kept = Arc::new(Kept::default());
    let error = unreachable_source()
        .audited(kept.clone())
        .feed(&red(RED_VALUE), &maiden(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("refused");
    assert!(matches!(error, SourceError::Invalid(_)), "{error:?}");
    assert!(kept.taken().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn an_outcome_whose_record_is_refused_is_not_used() {
    let server = manager("PUT", ResponseTemplate::new(201)).await;
    let error = source(&server)
        .audited(Arc::new(Refusing))
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("the exchange fails closed");
    assert!(matches!(error, SourceError::Audit(_)), "{error:?}");
    assert!(!format!("{error:?} {error}").contains(RED_VALUE));
}

#[tokio::test]
async fn a_feed_made_for_a_user_names_them_from_their_token() {
    let server = manager("PUT", ResponseTemplate::new(200)).await;
    let kept = Arc::new(Kept::default());
    source(&server)
        .audited(kept.clone())
        .feed(&red(RED_VALUE), &alissa(), &user(), PROMPT)
        .await
        .expect("applied");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    holds_to(&record, &vendored("ihe-pixm", UPDATE.0));
    // NOTE: PIXm §2:3.104.5.2.1 augments the record "following IHE-BALP" with the agent
    // details of the OAuth token, which BALP 1.1.4 §3:5.7.5.4 maps.
    names_the_user(&record);
    let shown = format!("{exchange:?}");
    for value in [SUBJECT, CLIENT] {
        assert!(!shown.contains(value), "Debug names no user: {shown}");
    }
}
