// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's client of the FerroPIX server, the one place the console
//! reaches it.
//!
//! The console calls the server server to server, with the signed-in
//! steward's own access token, so the server records the steward as the
//! actor of every call (`docs/architecture.md` §12.1). Each call carries an
//! `X-Request-Id` the console mints, which a page shows beside a failure so
//! a steward can quote it. It reads the server's liveness route,
//! `GET {base}/health/liveness` (`docs/architecture.md` §9), its FHIR
//! `CapabilityStatement`, `GET {base}/fhir/metadata` (HL7 FHIR R4, the REST
//! interaction `capabilities`), and the operator API's documents, typed as
//! `ferropix-operator-api` defines them, so a field renamed on the server
//! fails this crate's build; a page whose document the server does not serve
//! draws its refusal from the [`Probe`] of the call. No specification
//! governs the client: our own design.

use std::fmt;
use std::time::Duration;

use http::Method;
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;

use ferropix_operator_api::overview::Overview;
use ferropix_operator_api::problem::{self, Problem};

use crate::model::{Capability, Failure, Probe, Reach, ServerIssue};

use crate::config::settings::PixSettings;

/// The header that carries the request id the console mints for a call.
pub const REQUEST_ID: &str = "x-request-id";

/// The liveness route of the server, under its base.
pub const HEALTH: &str = "health/liveness";

/// The FHIR capability route of the server, under its base.
pub const METADATA: &str = "fhir/metadata";

/// The operator API's dashboard route, under the server's base.
pub const OVERVIEW: &str = "operator/overview";

/// How many bytes of an answer the console reads at most: a
/// `CapabilityStatement` is a few kilobytes, so a larger answer is refused
/// rather than drawn.
pub const BODY_LIMIT: usize = 1 << 20;

/// The steward's access token, which the console sends to the server and
/// never renders.
#[derive(Clone)]
pub struct AccessToken(SecretString);

impl AccessToken {
    /// Wraps the token a session holds.
    #[must_use]
    pub const fn new(token: SecretString) -> Self {
        Self(token)
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(..)")
    }
}

impl Failure {
    /// The failure `error` names.
    fn of(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connect
        } else {
            Self::Other
        }
    }
}

/// The issues of an `OperationOutcome`, the only part a FHIR refusal's body is
/// read for.
#[derive(Debug, Default, serde::Deserialize)]
struct Outcome {
    /// `issue`.
    #[serde(default)]
    issue: Vec<ServerIssue>,
}

/// A client of the FerroPIX server could not be built.
#[derive(Debug, thiserror::Error)]
#[error("the client of the FerroPIX server could not be built")]
pub struct ClientError(#[from] reqwest::Error);

/// The client of one FerroPIX server.
#[derive(Debug, Clone)]
pub struct Pix {
    base: Url,
    http: reqwest::Client,
}

impl Pix {
    /// The client of the server `settings` names.
    ///
    /// # Errors
    /// Returns [`ClientError`] when the HTTP client cannot be built.
    pub fn new(settings: &PixSettings) -> Result<Self, ClientError> {
        // NOTE: no specification governs this: our own design; a call is never
        // redirected elsewhere, so the steward's token reaches only the base.
        let http = reqwest::Client::builder()
            .timeout(settings.timeout)
            .connect_timeout(settings.timeout.min(Duration::from_secs(5)))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(ClientError::from)?;
        Ok(Self {
            base: settings.base.clone(),
            http,
        })
    }

    /// The server's base URL.
    #[must_use]
    pub const fn base(&self) -> &Url {
        &self.base
    }

    /// Asks the server's liveness route whether it answers.
    pub async fn probe(&self, token: &AccessToken) -> Probe {
        self.get(HEALTH, token).await.0
    }

    /// Reads the server's `CapabilityStatement`.
    pub async fn capability_statement(&self, token: &AccessToken) -> Capability {
        match self.get(METADATA, token).await {
            (probe, Some(text)) if probe.is_up() => Capability::Declared(text),
            (probe, _) => Capability::Missing(probe),
        }
    }

    /// Reads the operator API's dashboard document, or the call that did not
    /// bring one back.
    ///
    /// # Errors
    /// Returns the [`Probe`] of a call that was refused, did not answer, or
    /// answered with a body that is not the document.
    pub async fn overview(&self, token: &AccessToken) -> Result<Overview, Probe> {
        self.read(OVERVIEW, token).await
    }

    /// Reads the operator API document at `route`, under the server's base,
    /// as `T`, or the call that did not bring one back.
    ///
    /// # Errors
    /// Returns the [`Probe`] of a call that was refused, did not answer, or
    /// answered with a body that is not the document.
    pub async fn read<T: serde::de::DeserializeOwned>(
        &self,
        route: &str,
        token: &AccessToken,
    ) -> Result<T, Probe> {
        match self.get(route, token).await {
            (probe, Some(text)) if probe.is_up() => document(&text, probe),
            (probe, _) => Err(probe),
        }
    }

    /// Posts `body` as JSON to the operator API at `route` and reads the
    /// answer as `T`, or the call that did not bring one back.
    ///
    /// # Errors
    /// Returns the [`Probe`] of a call that was refused, did not answer, or
    /// answered with a body that is not the document.
    pub async fn post<B: serde::Serialize + Sync, T: serde::de::DeserializeOwned>(
        &self,
        route: &str,
        body: &B,
        token: &AccessToken,
    ) -> Result<T, Probe> {
        match self.send(Method::POST, route, Some(body), token).await {
            (probe, Some(text)) if probe.is_up() => document(&text, probe),
            (probe, _) => Err(probe),
        }
    }

    /// Patches the operator API document at `route` with the JSON of `body`
    /// and reads the answer as `T`, or the call that did not bring one back.
    ///
    /// # Errors
    /// Returns the [`Probe`] of a call that was refused, did not answer, or
    /// answered with a body that is not the document.
    pub async fn patch<B: serde::Serialize + Sync, T: serde::de::DeserializeOwned>(
        &self,
        route: &str,
        body: &B,
        token: &AccessToken,
    ) -> Result<T, Probe> {
        match self.send(Method::PATCH, route, Some(body), token).await {
            (probe, Some(text)) if probe.is_up() => document(&text, probe),
            (probe, _) => Err(probe),
        }
    }

    /// Posts `body` as JSON to the operator API at `route` and returns the
    /// answer as text, the trail's CSV export.
    ///
    /// # Errors
    /// Returns the [`Probe`] of a call that was refused or did not answer.
    pub async fn post_text<B: serde::Serialize + Sync>(
        &self,
        route: &str,
        body: &B,
        token: &AccessToken,
    ) -> Result<String, Probe> {
        match self.send(Method::POST, route, Some(body), token).await {
            (probe, Some(text)) if probe.is_up() => Ok(text),
            (probe, _) => Err(probe),
        }
    }

    /// `GET {base}/{route}` with the steward's token: the call's [`Probe`]
    /// and, for a `2xx` answer within [`BODY_LIMIT`], its body as text.
    async fn get(&self, route: &str, token: &AccessToken) -> (Probe, Option<String>) {
        self.send::<()>(Method::GET, route, None, token).await
    }

    /// `method {base}/{route}` with the steward's token and, for a body, the
    /// JSON of `body`: the call's [`Probe`] and, for a `2xx` answer within
    /// [`BODY_LIMIT`], its body as text.
    async fn send<B: serde::Serialize + Sync>(
        &self,
        method: Method,
        route: &str,
        body: Option<&B>,
        token: &AccessToken,
    ) -> (Probe, Option<String>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let at = jiff::Timestamp::now()
            .round(jiff::Unit::Second)
            .map_or_else(|_| jiff::Timestamp::now().to_string(), |at| at.to_string());
        let shown = route.split('?').next().unwrap_or(route).to_owned();
        let probe = |reach| Probe {
            route: format!("{method} /{shown}"),
            reach,
            at: at.clone(),
            request_id: request_id.clone(),
            issues: Vec::new(),
            problem: None,
        };
        let Ok(url) = self.base.join(route) else {
            return (probe(Reach::Unreachable(Failure::Other)), None);
        };
        let mut request = self
            .http
            .request(method.clone(), url)
            .bearer_auth(token.0.expose_secret())
            .header(REQUEST_ID, &request_id);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(route = shown.as_str(), %request_id, failure = ?Failure::of(&error), "the FerroPIX server did not answer");
                return (probe(Reach::Unreachable(Failure::of(&error))), None);
            }
        };
        let status = response.status();
        if !status.is_success() {
            tracing::warn!(route = shown.as_str(), %request_id, %status, "the FerroPIX server refused a call");
            let is_problem = response
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(problem::MEDIA_TYPE));
            let text = response
                .text()
                .await
                .ok()
                .filter(|text| text.len() <= BODY_LIMIT);
            // NOTE: no specification governs this: our own design; a refusal body
            // that does not read is shown as the status alone, never as an error.
            let refused = probe(Reach::Refused(status));
            let refused = match text {
                Some(text) if is_problem => Probe {
                    problem: serde_json::from_str::<Problem>(&text).ok().map(Box::new),
                    ..refused
                },
                Some(text) => Probe {
                    issues: serde_json::from_str::<Outcome>(&text)
                        .unwrap_or_default()
                        .issue,
                    ..refused
                },
                None => refused,
            };
            return (refused, None);
        }
        let too_long = response.content_length().is_some_and(|length| {
            usize::try_from(length).map_or(true, |length| length > BODY_LIMIT)
        });
        if too_long {
            return (probe(Reach::Unreachable(Failure::Other)), None);
        }
        match response.text().await {
            Ok(text) if text.len() <= BODY_LIMIT => (probe(Reach::Up), Some(text)),
            Ok(_) => (probe(Reach::Unreachable(Failure::Other)), None),
            Err(error) => (probe(Reach::Unreachable(Failure::of(&error))), None),
        }
    }
}

/// Reads `text` as the document `T`; a body that is not one is the call's
/// failure.
fn document<T: serde::de::DeserializeOwned>(text: &str, probe: Probe) -> Result<T, Probe> {
    serde_json::from_str(text).map_err(|_unread| Probe {
        reach: Reach::Unreachable(Failure::Other),
        ..probe
    })
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::actions::{Action, ActionFilter, Actions};
    use ferropix_operator_api::domain::{
        Domain, DomainChange, DomainPage, DomainQuery, Domains, Feed, NewDomain,
    };
    use ferropix_operator_api::matching::{FieldWeights, Matching};
    use ferropix_operator_api::overview::{DayCount, HourFeeds, Overview, Queue, QueueEntry};
    use ferropix_operator_api::patient::{Address, Demographics, DomainRef, Name, Telecom};
    use ferropix_operator_api::person::{
        HistoryEntry, IdentifierRow, Link, Person, PseudonymRow, SourceRecord,
    };
    use ferropix_operator_api::reveal::{Reveal, Revealed};
    use ferropix_operator_api::search::{
        DemographicsSearch, Hit, IdentifierSearch, MaskedIdentifier, Results,
    };

    /// Names every field of every operator document without `..`, so a
    /// field the server renames, adds or drops fails this crate's build.
    #[expect(
        clippy::too_many_lines,
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "one exhaustive pattern per document, each taken whole, is the whole point of the guard"
    )]
    fn read_field_by_field(
        overview: Overview,
        person: Person,
        results: Results,
        domains: Domains,
        page: DomainPage,
        matching: Matching,
        actions: Actions,
        bodies: (
            IdentifierSearch,
            DemographicsSearch,
            ActionFilter,
            DomainQuery,
        ),
        writes: (NewDomain, DomainChange, Reveal, Revealed),
    ) -> usize {
        let Overview {
            queue,
            records_created_24h: _,
            records_linked_24h: _,
            versions_fed_today: _,
            subscriptions_in_error: _,
            queue_depth,
            feeds_per_hour,
            oldest,
        } = overview;
        let Queue {
            open: _,
            waiting_over_week: _,
            deferred: _,
        } = queue;
        for DayCount { day: _, open: _ } in queue_depth {}
        for HourFeeds {
            hour: _,
            fhir: _,
            v2: _,
            pmir: _,
        } in feeds_per_hour
        {}
        for QueueEntry {
            candidate: _,
            created_at: _,
            score: _,
            grade: _,
        } in oldest
        {}
        let Person {
            id: _,
            status: _,
            merged_into: _,
            created_at: _,
            leading,
            identifiers,
            records,
            history,
            history_truncated: _,
            pseudonyms,
        } = person;
        let mut seen = leading.map(demographics).unwrap_or_default();
        for IdentifierRow {
            handle: _,
            domain,
            masked: _,
            state: _,
            last_fed: _,
            link,
        } in identifiers
        {
            seen += domain_ref(&domain);
            if let Some(Link {
                kind: _,
                made_by: _,
                made_at: _,
            }) = link
            {
                seen += 1;
            }
        }
        for SourceRecord {
            id: _,
            domain,
            version: _,
            received_at: _,
            channel: _,
            state: _,
            demographics: stated,
        } in records
        {
            seen += domain_ref(&domain) + demographics(stated);
        }
        for HistoryEntry {
            at: _,
            actor: _,
            reason: _,
            event: _,
        } in history
        {
            seen += 1;
        }
        for PseudonymRow {
            domain: _,
            namespace: _,
            name: _,
            masked: _,
        } in pseudonyms
        {
            seen += 1;
        }
        let Results {
            total: _,
            offset: _,
            next: _,
            persons,
        } = results;
        for Hit {
            id: _,
            status: _,
            leading,
            identifiers,
            score: _,
            grade: _,
        } in persons
        {
            seen += leading.map(demographics).unwrap_or_default();
            for MaskedIdentifier { domain, masked: _ } in identifiers {
                seen += domain_ref(&domain);
            }
        }
        let Domains { domains, next: _ } = domains;
        let DomainPage { domain: one, feeds } = page;
        for Domain {
            id: _,
            system: _,
            name: _,
            kind: _,
            sensitive: _,
            source_client_id: _,
            created_at: _,
            retired_at: _,
            records: _,
            last_fed: _,
        } in domains.into_iter().chain([one])
        {
            seen += 1;
        }
        for Feed {
            at: _,
            channel: _,
            outcome: _,
        } in feeds
        {
            seen += 1;
        }
        let Matching {
            link_at: _,
            review_at: _,
            possible_at: _,
            auto_link: _,
            blocking_keys: _,
            fields,
        } = matching;
        for FieldWeights {
            field: _,
            m: _,
            u: _,
            configured: _,
        } in fields
        {
            seen += 1;
        }
        let Actions { actions, next: _ } = actions;
        for Action {
            id: _,
            kind: _,
            actor: _,
            at: _,
            person: _,
            record: _,
            candidate: _,
            merge: _,
            domain: _,
            pseudonym_domain: _,
            reason: _,
            request_id: _,
        } in actions
        {
            seen += 1;
        }
        let (
            IdentifierSearch {
                system: _,
                value: _,
                offset: _,
                limit: _,
            },
            DemographicsSearch {
                family: _,
                given: _,
                birth_date: _,
                gender: _,
                postal_code: _,
                offset: _,
                limit: _,
            },
            ActionFilter {
                actor: _,
                kind: _,
                person: _,
                from: _,
                to: _,
                after: _,
                limit: _,
            },
            DomainQuery { after: _, limit: _ },
        ) = bodies;
        let (
            NewDomain {
                system: _,
                name: _,
                kind: _,
                source_client_id: _,
                sensitive: _,
                reason: _,
            },
            DomainChange {
                name: _,
                source_client_id: _,
                retire: _,
                reason: _,
            },
            Reveal {
                target: _,
                reason: _,
                seconds: _,
            },
            Revealed {
                value: _,
                system: _,
                seconds: _,
                action: _,
            },
        ) = writes;
        seen
    }

    /// Names every field of a domain reference.
    fn domain_ref(domain: &DomainRef) -> usize {
        let DomainRef {
            id: _,
            system: _,
            name: _,
            kind: _,
        } = domain;
        1
    }

    /// Names every field of a demographics document.
    fn demographics(demographics: Demographics) -> usize {
        let Demographics {
            names,
            birth_date: _,
            gender: _,
            addresses,
            telecom,
        } = demographics;
        let mut seen = 1;
        for Name {
            family: _,
            given: _,
        } in names
        {
            seen += 1;
        }
        for Address {
            lines: _,
            city: _,
            postal_code: _,
            country: _,
        } in addresses
        {
            seen += 1;
        }
        for Telecom {
            system: _,
            value: _,
        } in telecom
        {
            seen += 1;
        }
        seen
    }

    #[test]
    fn the_console_reads_every_operator_document_field_by_field() {
        let overview: Overview = serde_json::from_str(
            r#"{"queue":{"open":1,"waiting_over_week":0,"deferred":0},
                "records_created_24h":2,"records_linked_24h":1,"versions_fed_today":3,
                "subscriptions_in_error":0,
                "queue_depth":[{"day":"2026-10-24","open":1}],
                "feeds_per_hour":[{"hour":"2026-10-24T10:00:00Z","fhir":1,"v2":0,"pmir":0}],
                "oldest":[{"candidate":"c-1","created_at":"2026-10-24T10:00:00Z","score":0.9,"grade":"probable"}]}"#,
        )
        .expect("the dashboard document reads");
        let person: Person = serde_json::from_str(
            r#"{"id":"p-1","status":"active","created_at":"2026-10-24T10:00:00Z",
                "leading":{"names":[{"family":"Synthfamily"}]},
                "identifiers":[{"handle":{"record":"r-1"},
                    "domain":{"id":"d-1","system":"urn:oid:2.999.1.1","name":"Synthetic","kind":"local"},
                    "masked":"•••123","state":"active",
                    "link":{"kind":"feed","made_by":"east-lab","made_at":"2026-10-24T10:00:00Z"}}],
                "records":[],"history":[],"history_truncated":false,"pseudonyms":[]}"#,
        )
        .expect("the person document reads");
        let empty = |text: &str| text.to_owned();
        let seen = read_field_by_field(
            overview,
            person,
            serde_json::from_str(&empty(r#"{"total":0,"offset":0,"persons":[]}"#))
                .expect("the results read"),
            serde_json::from_str(r#"{"domains":[]}"#).expect("the domains read"),
            serde_json::from_str(
                r#"{"domain":{"id":"d-1","system":"urn:oid:2.999.1.1","name":"Synthetic",
                    "kind":"local","sensitive":false,"source_client_id":"east-lab",
                    "created_at":"2026-10-24T10:00:00Z","records":0},"feeds":[]}"#,
            )
            .expect("the domain page reads"),
            serde_json::from_str(
                r#"{"link_at":0.95,"review_at":0.8,"possible_at":0.6,"auto_link":false,
                    "blocking_keys":[],"fields":[]}"#,
            )
            .expect("the matching document reads"),
            serde_json::from_str(r#"{"actions":[]}"#).expect("the trail reads"),
            (
                serde_json::from_str(r#"{"system":"urn:oid:2.999.1.1","value":"x"}"#)
                    .expect("the identifier search reads"),
                DemographicsSearch::default(),
                ActionFilter::default(),
                DomainQuery::default(),
            ),
            (
                serde_json::from_str(
                    r#"{"system":"urn:oid:2.999.1.9","name":"Synthetic","kind":"local","source_client_id":"east-lab"}"#,
                )
                .expect("a new domain reads"),
                serde_json::from_str(r#"{"retire":true,"reason":"closed"}"#)
                    .expect("a domain change reads"),
                serde_json::from_str(r#"{"target":{"record":"r-1"},"reason":"asked"}"#)
                    .expect("a reveal reads"),
                serde_json::from_str(
                    r#"{"value":"SYNTHETIC-1","system":"urn:oid:2.999.1.1","seconds":15,"action":"a-1"}"#,
                )
                .expect("a revealed value reads"),
            ),
        );
        assert!(seen >= 4, "every document was walked: {seen}");
    }
}
