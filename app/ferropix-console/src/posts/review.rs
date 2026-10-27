// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review's posts: the queue's filters, paging and deferral, a pair's
//! decision, and a merge and its revert. No specification governs the
//! console: our own design.

use ferropix_operator_api::candidate::{Candidate, CandidateQuery, Decided, Decision, Verdict};
use ferropix_operator_api::merge::{NewMerge, Outcome as MergeOutcome, Preview, Revert};

use super::field;
use crate::model::{DecisionOutcome, Document, Read};
use crate::pix::AccessToken;
use crate::server::ConsoleState;

/// The queue's filter a form posts, every filter dropped by Clear.
fn queue_filter(pairs: &[(String, String)]) -> CandidateQuery {
    if field(pairs, "clear").is_some() {
        return CandidateQuery::default();
    }
    // NOTE: no specification governs this: our own design; a filter or page field that reads
    // as no number is absent, as an empty field is, and the queue is read without it.
    CandidateQuery {
        grade: field(pairs, "grade").and_then(|grade| crate::pages::queue::grade_of(&grade)),
        domain: field(pairs, "domain"),
        older_than_days: field(pairs, "older").and_then(|days| days.parse().ok()),
        deferred: field(pairs, "deferred").is_some(),
        offset: field(pairs, "offset")
            .and_then(|offset| offset.parse().ok())
            .unwrap_or_default(),
    }
}

/// A post of the queue: the filters or a page, or a deferral from a row's
/// side panel; and the queue's document under the filter.
pub(super) async fn queued(
    state: &ConsoleState,
    token: &AccessToken,
    pairs: &[(String, String)],
) -> Document {
    let pix = state.pix();
    let filter = queue_filter(pairs);
    let deferred =
        if field(pairs, crate::pages::queue::FORM).as_deref() == Some(crate::pages::queue::DEFER) {
            Some(deferral(state, token, pairs).await)
        } else {
            None
        };
    let query = candidate_query(&filter);
    let route = if query.is_empty() {
        String::from("operator/candidates")
    } else {
        format!("operator/candidates?{query}")
    };
    Document::Queue {
        candidates: pix.read(&route, token).await,
        filter,
        deferred,
    }
}

/// A decision posted on the pair `candidate`: the address of the next pair
/// in the queue's order (the queue when none follows) once the server took
/// it, or the pair's document with what it came to.
///
/// # Errors
///
/// The pair's document, re-read, when the decision names no verdict or
/// versions, carries no reason, or the server refused it.
pub(super) async fn decided(
    state: &ConsoleState,
    token: &AccessToken,
    candidate: &str,
    pairs: &[(String, String)],
) -> Result<String, Box<Document>> {
    let pix = state.pix();
    let route = format!("operator/candidates/{candidate}");
    let verdict = match field(pairs, "decision").as_deref() {
        Some("link") => Some(Verdict::Link),
        Some("not-a-match") => Some(Verdict::NotAMatch),
        Some("defer") => Some(Verdict::Defer),
        _ => None,
    };
    let version = |name: &str| field(pairs, name).and_then(|version| version.parse::<i32>().ok());
    let reason = field(pairs, "reason");
    let outcome = match (verdict, version("a_version"), version("b_version"), reason) {
        (Some(verdict), Some(a_version), Some(b_version), Some(reason)) => {
            let decision = Decision {
                verdict,
                until: (verdict == Verdict::Defer)
                    .then(|| field(pairs, "until"))
                    .flatten(),
                a_version,
                b_version,
                reason,
            };
            match pix
                .post::<_, Decided>(&format!("{route}/decide"), &decision, token)
                .await
            {
                Ok(decided) => {
                    return Ok(decided.next.map_or_else(
                        || String::from(crate::app::QUEUE),
                        |next| format!("{}/{next}", crate::app::QUEUE),
                    ));
                }
                Err(probe) => DecisionOutcome::Refused(probe),
            }
        }
        _ => DecisionOutcome::NoReason,
    };
    Err(Box::new(Document::Pair {
        candidate: pix.read(&route, token).await,
        outcome: Some(outcome),
    }))
}

/// The preview of merging `merged` into `survivor`.
async fn preview_of(
    state: &ConsoleState,
    token: &AccessToken,
    survivor: &str,
    merged: &str,
) -> Read<Preview> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("survivor", survivor)
        .append_pair("merged", merged)
        .finish();
    state
        .pix()
        .read(&format!("operator/merges/preview?{query}"), token)
        .await
}

/// A post of the merge page of `person`: a preview of merging the patient
/// it names, the older surviving unless the post names the survivor, or the
/// merge itself.
///
/// # Errors
///
/// The merge page's document with the preview, or with the refusal and a
/// fresh preview when the server refused the merge.
pub(super) async fn merged(
    state: &ConsoleState,
    token: &AccessToken,
    person: &str,
    pairs: &[(String, String)],
) -> Result<String, Box<Document>> {
    let page = |preview, refused| {
        Box::new(Document::Merge {
            person: person.to_owned(),
            preview,
            refused,
        })
    };
    let uuid = |name: &str| field(pairs, name).filter(|id| crate::pages::screens::is_uuid(id));
    if field(pairs, crate::pages::merge::FORM).as_deref() == Some(crate::pages::merge::MERGE) {
        let (Some(survivor), Some(merged)) = (uuid("survivor"), uuid("merged")) else {
            return Err(page(None, None));
        };
        let merge = NewMerge {
            survivor: survivor.clone(),
            survivor_version: field(pairs, "survivor_version").unwrap_or_default(),
            merged: merged.clone(),
            merged_version: field(pairs, "merged_version").unwrap_or_default(),
            reason: field(pairs, "reason").unwrap_or_default(),
        };
        return match state
            .pix()
            .post::<_, MergeOutcome>("operator/merges", &merge, token)
            .await
        {
            Ok(outcome) => Ok(format!("/patients/{}", outcome.survivor)),
            Err(probe) => Err(page(
                Some(preview_of(state, token, &survivor, &merged).await),
                Some(probe),
            )),
        };
    }
    let Some(other) = uuid("other").filter(|other| other != person) else {
        return Err(page(None, None));
    };
    if let Some(survivor) = uuid("survivor") {
        let merged = if survivor == other {
            person.to_owned()
        } else {
            other
        };
        return Err(page(
            Some(preview_of(state, token, &survivor, &merged).await),
            None,
        ));
    }
    let first = preview_of(state, token, person, &other).await;
    // NOTE: no specification governs this: our own design; the older patient survives
    // unless the steward chooses the other, as RFC 3339 UTC times compare as text.
    let preview = match &first {
        Ok(found) if found.merged.created_at < found.survivor.created_at => {
            preview_of(state, token, &other, person).await
        }
        _ => first,
    };
    Err(page(Some(preview), None))
}

/// A post of the unmerge page of `person`: the revert of the merge it
/// names, with the survivor's version the preview read.
///
/// # Errors
///
/// The unmerge page's document, read again, with the refusal.
pub(super) async fn unmerged(
    state: &ConsoleState,
    token: &AccessToken,
    person: &str,
    pairs: &[(String, String)],
) -> Result<String, Box<Document>> {
    let merge = field(pairs, "merge").filter(|id| crate::pages::screens::is_uuid(id));
    let refused = match merge {
        Some(merge) => {
            let revert = Revert {
                survivor_version: field(pairs, "survivor_version").unwrap_or_default(),
                reason: field(pairs, "reason").unwrap_or_default(),
            };
            match state
                .pix()
                .post::<_, MergeOutcome>(&format!("operator/merges/{merge}/revert"), &revert, token)
                .await
            {
                Ok(_) => return Ok(format!("/patients/{person}")),
                Err(probe) => Some(probe),
            }
        }
        None => None,
    };
    Err(Box::new(
        crate::document::unmerge(state.pix(), token, person, refused).await,
    ))
}

/// The query string of `GET /operator/candidates` for `filter`.
fn candidate_query(filter: &CandidateQuery) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if let Some(grade) = filter.grade {
        query.append_pair("grade", crate::pages::queue::grade_key(grade));
    }
    if let Some(domain) = filter.domain.as_deref().filter(|domain| !domain.is_empty()) {
        query.append_pair("domain", domain);
    }
    if let Some(days) = filter.older_than_days {
        query.append_pair("older_than_days", &days.to_string());
    }
    if filter.deferred {
        query.append_pair("deferred", "true");
    }
    if filter.offset > 0 {
        query.append_pair("offset", &filter.offset.to_string());
    }
    query.finish()
}

/// A deferral from the queue: the candidate read for the versions its
/// records stand at, then decided as deferred to the posted date with the
/// posted reason.
async fn deferral(
    state: &ConsoleState,
    token: &AccessToken,
    pairs: &[(String, String)],
) -> Read<()> {
    let pix = state.pix();
    let id = field(pairs, "candidate")
        .filter(|id| crate::pages::screens::is_uuid(id))
        .unwrap_or_default();
    // NOTE: no specification governs this: our own design; the queue's row shows no record
    // version, so a deferral from it carries the versions the candidate stands at now.
    let candidate: Candidate = pix
        .read(&format!("operator/candidates/{id}"), token)
        .await?;
    let decision = Decision {
        verdict: Verdict::Defer,
        until: field(pairs, "until"),
        a_version: candidate.a.version,
        b_version: candidate.b.version,
        reason: field(pairs, "reason").unwrap_or_default(),
    };
    pix.post::<_, Decided>(
        &format!("operator/candidates/{id}/decide"),
        &decision,
        token,
    )
    .await
    .map(|_| ())
}
