// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The forms a page posts, each answered with the page it came from, drawn
//! again with the server's answer.
//!
//! A search, a trail filter and a reveal are posts, so nothing a steward
//! typed reaches an address, the history or a log line (`docs/architecture.md`
//! §12.1, #52). Each handler checks the session, asks the FerroPIX server
//! with the steward's own token, and draws the page at the same address
//! with the answer in its context. A post with no live session goes back to
//! the page by `GET`, which then draws the sign-in. The trail's export
//! answers the server's CSV as a file. No specification governs the
//! console: our own design.

use axum::Extension;
use axum::extract::{FromRequest as _, Request};
use axum::response::{IntoResponse as _, Redirect, Response};
use ferropix_operator_api::actions::ActionFilter;
use ferropix_operator_api::domain::{Domain, DomainChange, NewDomain};
use ferropix_operator_api::patient::DomainKind;
use ferropix_operator_api::reveal::{MAX_SECONDS, Reveal, Target};
use ferropix_operator_api::search::{DemographicsSearch, IdentifierSearch, Results};
use http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use http::{HeaderMap, StatusCode};

use crate::app::PageContext;
use crate::document::{Document, Prefill, Read, Segment};
use crate::pix::AccessToken;
use crate::server::ConsoleState;
use crate::session::request::{Held, held};

/// The request's parts and its body read as a form of name and value pairs,
/// or the answer to a body that is no such form.
async fn form(
    request: Request,
) -> Result<(http::request::Parts, Vec<(String, String)>), Box<Response>> {
    let (parts, body) = request.into_parts();
    let rebuilt = Request::from_parts(parts.clone(), body);
    match axum::extract::Form::<Vec<(String, String)>>::from_request(rebuilt, &()).await {
        Ok(axum::extract::Form(pairs)) => Ok((parts, pairs)),
        Err(rejection) => Err(Box::new(rejection.into_response())),
    }
}

/// The value of the field `name` of `pairs`, `None` when it is absent or
/// blank.
fn field(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Who a live session holds: the steward's name and whether the provider
/// granted the server's administrator scope.
#[derive(Debug)]
struct Steward {
    /// The steward's name as the provider gave it.
    name: Option<String>,
    /// Whether the administrator scope was granted.
    admin: bool,
}

/// The steward's token and who they are when the request holds a live
/// session.
fn signed_in(state: &ConsoleState, headers: &HeaderMap) -> Option<(AccessToken, Steward)> {
    match held(state, headers) {
        Ok(Held::SignedIn {
            token,
            operator,
            scopes,
        }) => {
            let admin = scopes.contains(&state.settings().pix.admin_scope);
            Some((
                token,
                Steward {
                    name: operator,
                    admin,
                },
            ))
        }
        _ => None,
    }
}

/// Draws the page at the address of `parts` from `document`, beside the
/// call to the server's liveness route a page's state is drawn from.
async fn drawn(
    state: ConsoleState,
    mut parts: http::request::Parts,
    token: &AccessToken,
    steward: Steward,
    document: Document,
) -> Response {
    let probe = state.pix().probe(token).await;
    parts.extensions.insert(PageContext {
        operator: steward.name,
        admin: steward.admin,
        probe: Some(probe),
        capability: None,
        prefix: state.sessions().preference_prefix(),
        document: Some(document),
    });
    crate::server::render(state, parts).await
}

/// The shape of what the top bar's field carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// A birth date, as `YYYY-MM-DD`.
    Date(String),
    /// A name: letters, spaces, hyphens and apostrophes.
    Name(String),
    /// Anything else: an identifier.
    Identifier(String),
}

/// Reads the shape of `typed`: a date written `YYYY-MM-DD`, `DD-MM-YYYY`
/// or `DD/MM/YYYY`, a name, or an identifier.
#[must_use]
pub fn shape(typed: &str) -> Shape {
    let typed = typed.trim();
    let parts: Vec<&str> = typed.split(['-', '/']).collect();
    let digits = |part: &str, length: usize| {
        part.len() == length && part.chars().all(|c| c.is_ascii_digit())
    };
    if let [first, second, third] = parts.as_slice() {
        if digits(first, 4) && digits(second, 2) && digits(third, 2) {
            return Shape::Date(format!("{first}-{second}-{third}"));
        }
        if digits(first, 2) && digits(second, 2) && digits(third, 4) {
            return Shape::Date(format!("{third}-{second}-{first}"));
        }
    }
    let named = typed.chars().any(char::is_alphabetic)
        && typed
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\''));
    if named {
        Shape::Name(typed.to_owned())
    } else {
        Shape::Identifier(typed.to_owned())
    }
}

/// Serves a search post: the identifier or the demographics forwarded to
/// the operator API, and the search page drawn with the results.
///
/// A post of the top bar's one field is read by its shape: a date or a name
/// searches by demographics with the field filled, and anything else is an
/// identifier, searched in every domain the server holds.
pub async fn search(Extension(state): Extension<ConsoleState>, request: Request) -> Response {
    let (parts, pairs) = match form(request).await {
        Ok(read) => read,
        Err(answer) => return *answer,
    };
    let Some((token, steward)) = signed_in(&state, &parts.headers) else {
        return Redirect::to(parts.uri.path()).into_response();
    };
    let pix = state.pix();
    let domains: Read<ferropix_operator_api::domain::Domains> =
        pix.read("operator/domains?limit=200", &token).await;
    let typed = field(&pairs, "q").map(|typed| shape(&typed));
    let mut prefill = Prefill::default();
    let (segment, results) = match (field(&pairs, "segment").as_deref(), typed) {
        (Some("demographics"), _) => (
            Segment::Demographics,
            demographics(pix, &token, &pairs, &Prefill::default()).await,
        ),
        (None, Some(Shape::Date(born))) => {
            prefill.born = Some(born);
            (
                Segment::Demographics,
                demographics(pix, &token, &[], &prefill).await,
            )
        }
        (None, Some(Shape::Name(family))) => {
            prefill.family = Some(family);
            (
                Segment::Demographics,
                demographics(pix, &token, &[], &prefill).await,
            )
        }
        (None, Some(Shape::Identifier(value))) => (
            Segment::Identifier,
            everywhere(pix, &token, domains.as_ref().ok(), &value).await,
        ),
        _ => {
            let search = IdentifierSearch {
                system: field(&pairs, "domain").unwrap_or_default(),
                value: field(&pairs, "identifier").unwrap_or_default(),
                offset: None,
                limit: None,
            };
            (
                Segment::Identifier,
                pix.post("operator/search/identifier", &search, &token)
                    .await,
            )
        }
    };
    let document = Document::Search {
        domains,
        results: Some((segment, results)),
        prefill,
    };
    drawn(state, parts, &token, steward, document).await
}

/// Searches by the demographics `pairs` posts, or those `prefill` carries.
async fn demographics(
    pix: &crate::pix::Pix,
    token: &AccessToken,
    pairs: &[(String, String)],
    prefill: &Prefill,
) -> Read<Results> {
    let search = DemographicsSearch {
        family: field(pairs, "family").or_else(|| prefill.family.clone()),
        given: field(pairs, "given"),
        birth_date: field(pairs, "born").or_else(|| prefill.born.clone()),
        gender: field(pairs, "gender"),
        postal_code: field(pairs, "postal"),
        offset: None,
        limit: None,
    };
    pix.post("operator/search/demographics", &search, token)
        .await
}

/// Searches `value` in every unretired domain of `domains`, each person
/// once, in the order found.
async fn everywhere(
    pix: &crate::pix::Pix,
    token: &AccessToken,
    domains: Option<&ferropix_operator_api::domain::Domains>,
    value: &str,
) -> Read<Results> {
    let mut found = Results {
        total: 0,
        offset: 0,
        next: None,
        persons: Vec::new(),
    };
    let systems = domains
        .map(|domains| {
            domains
                .domains
                .iter()
                .filter(|domain| domain.retired_at.is_none())
                .map(|domain| domain.system.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for system in systems {
        let search = IdentifierSearch {
            system,
            value: value.to_owned(),
            offset: None,
            limit: None,
        };
        let answered: Results = pix
            .post("operator/search/identifier", &search, token)
            .await?;
        for hit in answered.persons {
            if !found.persons.iter().any(|held| held.id == hit.id) {
                found.persons.push(hit);
            }
        }
    }
    found.total = u32::try_from(found.persons.len()).unwrap_or(u32::MAX);
    Ok(found)
}

/// The trail's filter a form posts.
fn filter_of(pairs: &[(String, String)]) -> ActionFilter {
    ActionFilter {
        actor: field(pairs, "actor"),
        kind: field(pairs, "kind"),
        person: field(pairs, "person"),
        from: field(pairs, "from"),
        to: field(pairs, "to"),
        after: field(pairs, "after"),
        limit: None,
    }
}

/// Serves a trail post: the filter and the page drawn with the actions it
/// names.
pub async fn trail(Extension(state): Extension<ConsoleState>, request: Request) -> Response {
    let (parts, pairs) = match form(request).await {
        Ok(read) => read,
        Err(answer) => return *answer,
    };
    let Some((token, steward)) = signed_in(&state, &parts.headers) else {
        return Redirect::to(parts.uri.path()).into_response();
    };
    let document = crate::document::trail(state.pix(), &token, filter_of(&pairs)).await;
    drawn(state, parts, &token, steward, document).await
}

/// Serves the trail's export: the filter posted to the operator API's
/// export, its CSV answered as a file, or `502` when the server sends none.
pub async fn export(Extension(state): Extension<ConsoleState>, request: Request) -> Response {
    let (parts, pairs) = match form(request).await {
        Ok(read) => read,
        Err(answer) => return *answer,
    };
    let Some((token, _)) = signed_in(&state, &parts.headers) else {
        return Redirect::to(crate::app::AUDIT).into_response();
    };
    let mut filter = filter_of(&pairs);
    filter.after = None;
    match state
        .pix()
        .post_text("operator/actions/export", &filter, &token)
        .await
    {
        Ok(csv) => (
            [
                (CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    CONTENT_DISPOSITION,
                    "attachment; filename=\"ferropix-audit-trail.csv\"",
                ),
                (CACHE_CONTROL, "no-store"),
            ],
            csv,
        )
            .into_response(),
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

/// The seconds a reveal shows its value, the steward's preference read from
/// its cookie, the console's default when none is set.
fn reveal_seconds(state: &ConsoleState, headers: &HeaderMap) -> u32 {
    let name = format!(
        "{}{}",
        state.sessions().preference_prefix(),
        crate::preferences::REVEAL
    );
    headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(cookie, _)| *cookie == name)
        .and_then(|(_, value)| crate::preferences::kept_value(crate::preferences::REVEAL, value))
        .unwrap_or_else(|| String::from(crate::preferences::REVEAL_DEFAULT))
        .parse::<u32>()
        .map_or(MAX_SECONDS, |seconds| seconds.min(MAX_SECONDS))
}

/// Serves a reveal post: the target and the reason posted to the operator
/// API, and the patient drawn again with the value or the refusal.
pub async fn reveal(Extension(state): Extension<ConsoleState>, request: Request) -> Response {
    let (mut parts, pairs) = match form(request).await {
        Ok(read) => read,
        Err(answer) => return *answer,
    };
    let path = parts.uri.path().to_owned();
    let person = path
        .trim_end_matches('/')
        .strip_suffix(&format!("/{}", crate::pages::patient::REVEAL))
        .and_then(|rest| rest.strip_prefix("/patients/"))
        .filter(|person| crate::pages::screens::is_uuid(person))
        .map(str::to_owned);
    let Some(person) = person else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let page = format!("/patients/{person}");
    let Some((token, steward)) = signed_in(&state, &parts.headers) else {
        return Redirect::to(&page).into_response();
    };
    let target = match (field(&pairs, "target").as_deref(), field(&pairs, "id")) {
        (Some("record"), Some(id)) => Some(Target::Record(id)),
        (Some("carried"), Some(id)) => Some(Target::Carried(id)),
        (Some("pseudonym"), Some(id)) => Some(Target::Pseudonym(id)),
        _ => None,
    };
    let (Some(target), Some(reason)) = (target, field(&pairs, "reason")) else {
        return Redirect::to(&page).into_response();
    };
    let reveal = Reveal {
        target,
        reason,
        seconds: Some(reveal_seconds(&state, &parts.headers)),
    };
    let revealed = state
        .pix()
        .post(
            &format!("operator/persons/{person}/reveal"),
            &reveal,
            &token,
        )
        .await;
    let person_read = state
        .pix()
        .read(&format!("operator/persons/{person}"), &token)
        .await;
    if let Ok(uri) = page.parse() {
        parts.uri = uri;
    }
    let document = Document::Person {
        person: person_read,
        revealed: Some(revealed),
    };
    drawn(state, parts, &token, steward, document).await
}

/// Serves a domain create: the administrator's form posted to the
/// operator API, and the domains drawn with the outcome.
pub async fn domain_create(
    Extension(state): Extension<ConsoleState>,
    request: Request,
) -> Response {
    let (parts, pairs) = match form(request).await {
        Ok(read) => read,
        Err(answer) => return *answer,
    };
    let Some((token, steward)) = signed_in(&state, &parts.headers) else {
        return Redirect::to(parts.uri.path()).into_response();
    };
    let kind = match field(&pairs, "kind").as_deref() {
        Some("authoritative") => DomainKind::Authoritative,
        _ => DomainKind::Local,
    };
    let new = NewDomain {
        system: field(&pairs, "system").unwrap_or_default(),
        name: field(&pairs, "name").unwrap_or_default(),
        kind,
        source_client_id: field(&pairs, "source").unwrap_or_default(),
        sensitive: Some(field(&pairs, "sensitive").is_some()),
        reason: field(&pairs, "reason"),
    };
    let pix = state.pix();
    let written: Read<Domain> = pix.post("operator/domains", &new, &token).await;
    let document = Document::Domains {
        domains: pix.read("operator/domains?limit=200", &token).await,
        written: Some(written.map(|_| ())),
    };
    drawn(state, parts, &token, steward, document).await
}

/// Serves a domain change: the administrator's form patched to the
/// operator API, and the domain drawn with the outcome.
pub async fn domain_change(
    Extension(state): Extension<ConsoleState>,
    request: Request,
) -> Response {
    let (parts, pairs) = match form(request).await {
        Ok(read) => read,
        Err(answer) => return *answer,
    };
    let Some(domain) = parts
        .uri
        .path()
        .strip_prefix("/domains/")
        .filter(|id| crate::pages::screens::is_uuid(id))
        .map(str::to_owned)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((token, steward)) = signed_in(&state, &parts.headers) else {
        return Redirect::to(parts.uri.path()).into_response();
    };
    let change = DomainChange {
        name: field(&pairs, "name"),
        source_client_id: field(&pairs, "source"),
        retire: field(&pairs, "retire").is_some(),
        reason: field(&pairs, "reason").unwrap_or_default(),
    };
    let pix = state.pix();
    let route = format!("operator/domains/{domain}");
    let written: Read<serde::de::IgnoredAny> = pix.patch(&route, &change, &token).await;
    let document = Document::Domain {
        page: pix.read(&route, &token).await,
        written: Some(written.map(|_| ())),
    };
    drawn(state, parts, &token, steward, document).await
}

#[cfg(test)]
mod tests {
    use super::{Shape, shape};

    #[test]
    fn the_top_bar_reads_a_date_a_name_and_an_identifier_by_their_shape() {
        assert_eq!(shape("1980-02-29"), Shape::Date(String::from("1980-02-29")));
        assert_eq!(shape("29-02-1980"), Shape::Date(String::from("1980-02-29")));
        assert_eq!(shape("29/02/1980"), Shape::Date(String::from("1980-02-29")));
        assert_eq!(
            shape("van der Proef"),
            Shape::Name(String::from("van der Proef"))
        );
        assert_eq!(
            shape("O'Testerink"),
            Shape::Name(String::from("O'Testerink"))
        );
        assert_eq!(
            shape(" 999999991 "),
            Shape::Identifier(String::from("999999991"))
        );
        assert_eq!(
            shape("MRN-0042"),
            Shape::Identifier(String::from("MRN-0042"))
        );
    }
}
