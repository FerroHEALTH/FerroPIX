// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PIXm Manager over the store (`docs/architecture.md` §6.1).
//!
//! - ITI-83, `$ihe-pix` by `GET` (PIXm 3.1.0 §2:3.83.4.1.2) and by `POST`
//!   of a `Parameters` body (FHIR R4 Operations, "Executing an Operation"),
//!   read by `ihe-iti`'s [`PixQuery`], answered from [`xref::lookup`] by
//!   [`answer::cross_references`] or the refusal the transaction names
//!   (§2:3.83.4.2.2), and recorded as the PIXm Query Manager audit profile
//!   before the answer leaves (§2:3.83.5.1.2).
//! - ITI-104, the conditional update and delete of `Patient` (§2:3.104.4),
//!   read by `ihe-iti`'s [`Feed`] and [`Removal`], applied by the feed
//!   transaction ([`Feeder`]) and recorded as the PIXm Feed Manager audit
//!   profiles after the commit (§2:3.104.5.1.2).
//!
//! The guard in front has admitted the caller by the permission table and,
//! for ITI-104, as the Source of the condition's domain (§2:3.104.5). The
//! audit layer has taken the record's place in the spool. No identifier,
//! value or name reaches a log line, a span or an error: the handlers log
//! the outcome's case alone (#52).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::response::Response;
use ferrohealth_smart::resource::Access;
use http::request::Parts;
use http::{HeaderValue, Method, header};
use ihe_atna::balp::Outcome;
use ihe_iti::pixm::identifier::{PatientReference, SourceIdentifier, TargetIdentifier};
use ihe_iti::pixm::server::answer;
use ihe_iti::pixm::server::feed::{Feed, Removal};
use ihe_iti::pixm::server::query::PixQuery;
use ihe_iti::server::answer::Answer;
use ihe_iti::server::format::Format;
use secrecy::{ExposeSecret as _, SecretString};

use crate::audit::Audit;
use crate::audit::records::{self, Call, Identifier};
use crate::audit::reserve::Pending;
use crate::auth::Smart;
use crate::config::section::matching::Matching;
use crate::feed::{self, Action, Applied, Context, FeedError, Feeder};
use crate::fhir::status::Case;
use crate::fhir::wire;
use crate::store::Store;
use crate::store::pseudonym;
use crate::store::record::{self, Channel};
use crate::store::xref::{self, Lookup};
use ferrohealth_telemetry::request_log::RequestId;

/// An ITI-83 answer with the pseudonyms it minted, as the person's
/// `Patient/{id}` and the domain's namespace.
type Looked = (
    Answer<fhir_types::r4::parameters::Parameters>,
    Vec<(String, String)>,
);

/// The PIXm Manager: the store it reads, the feed transaction it writes
/// through, and the audit trail it records to.
#[derive(Debug)]
pub struct Manager {
    /// The cross-reference store.
    store: Store,
    /// The feed transaction over the store.
    feeder: Feeder,
    /// The audit trail.
    audit: Audit,
    /// The resource server, with the Source of each domain.
    smart: Arc<Smart>,
    /// The base path the routes are mounted under.
    base: String,
    /// `public_base_url` without its trailing slash.
    public: String,
    /// The FHIR base, `{public_base_url}/fhir`.
    endpoint: url::Url,
}

impl Manager {
    /// Returns the Manager over `store` with the `[matching]` section
    /// `matching`, recording to `audit`, for routes under `base` reached at
    /// `public_base_url`.
    #[must_use]
    pub fn new(
        store: &Store,
        matching: &Matching,
        audit: &Audit,
        smart: &Arc<Smart>,
        base: &str,
        public_base_url: &url::Url,
    ) -> Self {
        let public = public_base_url.as_str().trim_end_matches('/').to_owned();
        // NOTE: no specification governs this: our own design; the configuration
        // admitted the public base, so the FHIR base under it parses too.
        let endpoint = url::Url::parse(&format!("{public}/fhir"))
            .unwrap_or_else(|_unparsed| public_base_url.clone());
        Self {
            store: store.clone(),
            feeder: Feeder::new(store.clone(), matching),
            audit: audit.clone(),
            smart: Arc::clone(smart),
            base: base.to_owned(),
            public,
            endpoint,
        }
    }

    /// ITI-83 by `GET` or `POST`.
    pub async fn query(&self, request: Request) -> Response {
        let (parts, body) = request.into_parts();
        let query = parts.uri.query();
        let mut format = match wire::negotiate(query, &parts.headers) {
            Ok(format) => format,
            Err(refused) => return *refused,
        };
        let read = if parts.method == Method::POST {
            let read = match wire::body_format(&parts.headers) {
                Ok(body_format) => bytes(body).await.map(|bytes| (body_format, bytes)),
                Err(case) => Err(case),
            };
            let (body_format, bytes) = match read {
                Ok(read) => read,
                Err(case) => return wire::send(&case.answer(), format),
            };
            PixQuery::from_body(body_format, &bytes).map(|pix| {
                let text = SecretString::from(String::from_utf8_lossy(&bytes).into_owned());
                (pix, Some((body_format, text)))
            })
        } else {
            PixQuery::from_query(query.unwrap_or_default()).map(|pix| (pix, None))
        };
        let (pix, posted) = match read {
            Ok(read) => read,
            Err(error) => {
                tracing::debug!(%error, "an ITI-83 request is refused");
                return wire::send(&error.refusal(), format);
            }
        };
        if let Some(asked) = pix.format() {
            let accept = parts
                .headers
                .get(header::ACCEPT)
                .and_then(|value| value.to_str().ok());
            format = match Format::negotiate(Some(asked), accept) {
                Ok(asked) => asked,
                Err(_unacceptable) => return wire::send(&Case::NotAcceptable.answer(), format),
            };
        }
        let client = parts
            .extensions
            .get::<Access>()
            .and_then(|access| access.caller())
            .and_then(|caller| caller.client_id().map(str::to_owned));
        let answered = self.lookup(&pix, client.as_deref()).await;
        let outcome = match &answered {
            Ok(_) => Outcome::Success,
            Err(case) => failure(*case),
        };
        if let Ok((_, minted)) = &answered
            && !minted.is_empty()
        {
            let call = self.call(&parts);
            let exchanges = minted
                .iter()
                .map(|(person, namespace)| {
                    records::pseudonym_minted(&call, person, namespace, Outcome::Success)
                })
                .collect();
            if let Err(error) = self.audit.record(exchanges).await {
                tracing::error!(error = %crate::chain(&error), "an ITI-83 answer is withheld: the record of a minted pseudonym could not be stored");
                return wire::send(&Case::CommittedUnaudited.answer(), format);
            }
        }
        let url = self.request_url(&parts);
        let raw = records::raw_request(
            &url,
            posted
                .as_ref()
                .map(|(body_format, text)| (body_format.media_type(), text)),
        );
        let exchange =
            records::pix_query(&self.call(&parts), raw, &identifier(pix.source()), outcome);
        if let Err(error) = record(&self.audit, &parts, &exchange).await {
            tracing::error!(error = %crate::chain(&error), "an ITI-83 answer is withheld: its audit record could not be stored");
            return wire::send(&Case::Unaudited.answer(), format);
        }
        match answered {
            Ok((found, _minted)) => wire::send(&found, format),
            Err(case) => wire::send(&case.answer(), format),
        }
    }

    /// Reads the cross-reference `pix` asks for, for the caller of client id
    /// `client`, and returns it with each pseudonym it minted, as the
    /// person's `Patient/{id}` and the domain's namespace.
    ///
    /// Only a pseudonymisation domain's consumer reads its pseudonyms. A
    /// domain it names as `targetSystem` answers the person's pseudonym,
    /// minted on this ask when the person has none; with no `targetSystem`,
    /// the answer carries the pseudonyms already minted in the caller's own
    /// domains and mints none. Another Consumer gets no pseudonym (PIXm
    /// 3.1.0 §2:3.83.4.1.2.2 and §2:3.83.4.1.3 leave the selection to the
    /// Manager).
    async fn lookup(&self, pix: &PixQuery, client: Option<&str>) -> Result<Looked, Case> {
        let source = pix.source();
        let mut connection = self.store.pool().acquire().await.map_err(unavailable)?;
        let found = xref::lookup(
            &mut connection,
            self.store.sealer(),
            source.system(),
            source.value().expose_secret(),
        )
        .await
        .map_err(|error| store_failure(&error))?;
        if found == Lookup::UnknownDomain {
            return Err(Case::SourceDomainNotRecognized);
        }
        let pseudonymised = pseudonym::domains(&mut connection)
            .await
            .map_err(|error| store_failure(&error))?;
        let asked: Vec<&pseudonym::PseudonymDomain> = pseudonymised
            .iter()
            .filter(|domain| {
                !domain.retired
                    && pix
                        .targets()
                        .iter()
                        .any(|target| target.as_str() == domain.namespace)
            })
            .collect();
        if !pix.targets().is_empty() {
            let domains = record::domains(&mut *connection)
                .await
                .map_err(|error| store_failure(&error))?;
            let known = |system: &str| {
                domains.iter().any(|d| d.system == system && !d.retired)
                    || asked.iter().any(|domain| domain.namespace == system)
            };
            if !pix.targets().iter().all(|target| known(target.as_str())) {
                return Err(Case::TargetDomainNotRecognized);
            }
        }
        let cross_reference = match found {
            Lookup::Found(cross_reference) => cross_reference,
            Lookup::Ambiguous => return Err(Case::SourceIdentifierAmbiguous),
            Lookup::UnknownDomain | Lookup::NotFound => {
                return Err(Case::SourceIdentifierNotFound);
            }
        };
        let person = cross_reference.person;
        let mut held: Vec<(String, String)> = cross_reference
            .identifiers
            .iter()
            .map(|(system, value)| (system.clone(), value.expose().to_owned()))
            .collect();
        let mut minted = Vec::new();
        if pix.targets().is_empty() {
            held.extend(own_pseudonyms(&mut connection, person, &pseudonymised, client).await?);
        }
        for domain in asked.iter().filter(|domain| domain.serves(client)) {
            let resolved = pseudonym::resolve(&mut connection, person, domain.id, true)
                .await
                .map_err(|error| store_failure(&error))?;
            if let Some(resolved) = resolved {
                if resolved.minted {
                    minted.push((
                        format!("Patient/{}", person.uuid()),
                        domain.namespace.clone(),
                    ));
                }
                held.push((domain.namespace.clone(), resolved.value.to_string()));
            }
        }
        if !minted.is_empty() {
            tracing::info!(minted = minted.len(), "ITI-83 minted pseudonyms");
        }
        let mut identifiers = Vec::with_capacity(held.len());
        for (system, value) in held {
            let identifier = TargetIdentifier::new(system, SecretString::from(value))
                .map_err(|_invalid| internal("a stored identifier is no target identifier"))?;
            identifiers.push(identifier);
        }
        let patient = PatientReference::new(SecretString::from(format!(
            "Patient/{}",
            cross_reference.person.uuid()
        )))
        .map_err(|_invalid| internal("a person id is no Patient reference"))?;
        Ok((
            answer::cross_references(pix, &identifiers, &[patient]),
            minted,
        ))
    }

    /// ITI-104's conditional update: Add or Revise Patient, or Resolve
    /// Duplicate Patient.
    pub async fn update(&self, request: Request) -> Response {
        let (parts, body) = request.into_parts();
        let query = parts.uri.query();
        let format = match wire::negotiate(query, &parts.headers) {
            Ok(format) => format,
            Err(refused) => return *refused,
        };
        let read = match wire::body_format(&parts.headers) {
            Ok(body_format) => bytes(body).await.map(|bytes| (body_format, bytes)),
            Err(case) => Err(case),
        };
        let (body_format, bytes) = match read {
            Ok(read) => read,
            Err(case) => return wire::send(&case.answer(), format),
        };
        let message = match Feed::from_update(query.unwrap_or_default(), body_format, &bytes) {
            Ok(message) => message,
            Err(error) => {
                tracing::debug!(%error, "an ITI-104 message is refused");
                return wire::send(&error.refusal(), format);
            }
        };
        let context = self.context(&parts);
        let (condition, applied) = match &message {
            Feed::Revise(revision) => (
                revision.identifier(),
                self.feeder.revise(revision, &context).await,
            ),
            Feed::ResolveDuplicate(merge) => (
                merge.subsumed(),
                self.feeder.resolve_duplicate(merge, &context).await,
            ),
            _ => return wire::send(&Case::Internal.answer(), format),
        };
        self.answer_feed(&parts, condition, false, applied, format)
            .await
    }

    /// ITI-104's conditional delete: Remove Patient (§2:3.104.4.3).
    pub async fn remove(&self, request: Request) -> Response {
        let (parts, _body) = request.into_parts();
        let query = parts.uri.query();
        let format = match wire::negotiate(query, &parts.headers) {
            Ok(format) => format,
            Err(refused) => return *refused,
        };
        let removal = match Removal::from_delete(query.unwrap_or_default()) {
            Ok(removal) => removal,
            Err(error) => {
                tracing::debug!(%error, "an ITI-104 removal is refused");
                return wire::send(&error.refusal(), format);
            }
        };
        let applied = self.feeder.remove(&removal, &self.context(&parts)).await;
        self.answer_feed(&parts, removal.identifier(), true, applied, format)
            .await
    }

    /// Records the feed of `condition` and answers its outcome.
    async fn answer_feed(
        &self,
        parts: &Parts,
        condition: &SourceIdentifier,
        removal: bool,
        applied: Result<Applied, FeedError>,
        format: Format,
    ) -> Response {
        let pending = parts.extensions.get::<Pending>();
        let recorded = feed::record(
            &self.audit,
            pending,
            &self.call(parts),
            condition,
            removal,
            applied.as_ref(),
        )
        .await;
        if let Err(error) = recorded {
            tracing::error!(error = %crate::chain(&error), "the audit record of an ITI-104 message could not be stored");
            let case = if applied.is_ok() {
                Case::CommittedUnaudited
            } else {
                Case::Unaudited
            };
            return wire::send(&case.answer(), format);
        }
        let applied = match applied {
            Ok(applied) => applied,
            Err(error) => {
                let case = feed_case(&error, removal);
                if matches!(case, Case::StoreUnavailable | Case::Internal) {
                    tracing::error!(error = %crate::chain(&error), "an ITI-104 message could not be applied");
                } else {
                    tracing::debug!(error = %crate::chain(&error), "an ITI-104 message is refused");
                }
                return wire::send(&case.answer(), format);
            }
        };
        let case = match applied.action {
            Action::Create => Case::FeedCreated,
            Action::Update | Action::ResolveDuplicate => Case::FeedUpdated,
            Action::Remove => Case::FeedRemoved,
        };
        let mut response = wire::send(&case.answer(), format);
        // NOTE: FHIR R4 http.html §3.1.0.4, an update "SHOULD return a Location
        // header"; it names the person, the Patient FerroPIX serves (§6, our own design).
        if let Some(person) = applied.person
            && let Ok(location) =
                HeaderValue::from_str(&format!("{}/fhir/Patient/{}", self.public, person.uuid()))
        {
            response.headers_mut().insert(header::LOCATION, location);
        }
        response
    }

    /// Returns the context the feed transaction records for the request.
    fn context(&self, parts: &Parts) -> Context {
        let actor = parts
            .extensions
            .get::<Access>()
            .and_then(|access| {
                self.smart
                    .sources()
                    .client(access)
                    .map(str::to_owned)
                    .or_else(|| {
                        access
                            .caller()
                            .map(|caller| caller.client_id().unwrap_or(caller.subject()).to_owned())
                    })
            })
            .unwrap_or_else(|| String::from("-"));
        Context {
            actor,
            channel: Channel::Fhir,
            request_id: parts
                .extensions
                .get::<RequestId>()
                .map(RequestId::id)
                .map(str::to_owned),
        }
    }

    /// Returns who called and where, as the request's audit record names
    /// them.
    fn call(&self, parts: &Parts) -> Call {
        let access = parts
            .extensions
            .get::<Access>()
            .cloned()
            .unwrap_or(Access::Open);
        let remote = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        Call::of(
            &access,
            remote,
            &self.endpoint,
            crate::http::exchanged_request_id(parts),
        )
    }

    /// Returns the URL the request was made to, under the public base, as
    /// BALP's query entity records it (BALP 1.1.4 §1:52.4.1.4).
    fn request_url(&self, parts: &Parts) -> url::Url {
        let path = parts.uri.path();
        let relative = path.strip_prefix(self.base.as_str()).unwrap_or(path);
        let query = parts
            .uri
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        url::Url::parse(&format!("{}{relative}{query}", self.public))
            .unwrap_or_else(|_unparsed| self.endpoint.clone())
    }
}

/// Returns the audit record's place in the spool, filled with `exchange`.
async fn record(
    audit: &Audit,
    parts: &Parts,
    exchange: &ihe_atna::balp::Exchange,
) -> Result<(), crate::audit::Unrecorded> {
    match parts.extensions.get::<Pending>() {
        Some(pending) => pending.record(audit, exchange).await,
        None => audit.record(vec![exchange.clone()]).await,
    }
}

/// Reads a request body under the server's ceiling.
async fn bytes(body: Body) -> Result<axum::body::Bytes, Case> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_too_large| Case::PayloadTooLarge)
}

/// Returns the audit record's identifier of `source`.
fn identifier(source: &SourceIdentifier) -> Identifier {
    Identifier {
        system: source.system().to_owned(),
        value: source.value().clone(),
    }
}

/// Returns the BALP outcome of a refusal or a failure (FHIR R4
/// `audit-event-outcome`): a refusal of the request is minor, a server
/// fault serious.
fn failure(case: Case) -> Outcome {
    match case {
        Case::StoreUnavailable | Case::Internal | Case::Contention => Outcome::SeriousFailure,
        _ => Outcome::MinorFailure,
    }
}

/// Returns the row of the status table a feed failure answers.
fn feed_case(error: &FeedError, removal: bool) -> Case {
    match error {
        FeedError::UnknownDomain => Case::FeedDomainNotRecognized,
        FeedError::Identifier { .. } => Case::FeedIdentifierRefused,
        FeedError::Subsumed => Case::FeedSubsumed,
        // NOTE: FHIR R4 http.html §3.1.0.7, a delete of what does not exist answers 200.
        FeedError::NotFound if removal => Case::FeedNothingToRemove,
        FeedError::NotFound | FeedError::SurvivorNotFound => Case::FeedSurvivorNotFound,
        FeedError::Contention => Case::Contention,
        FeedError::Store(_) => Case::StoreUnavailable,
        FeedError::Demographics(_) | FeedError::Evidence | FeedError::Notice => Case::Internal,
    }
}

/// The case of a pool that could not lend a connection.
fn unavailable(error: sqlx::Error) -> Case {
    store_failure(&crate::store::StoreError::from(error))
}

/// Logs a store failure and returns its case.
fn store_failure(error: &crate::store::StoreError) -> Case {
    tracing::error!(error = %crate::chain(error), "an ITI-83 query could not read the store");
    Case::StoreUnavailable
}

/// Logs a fault of the server and returns its case.
fn internal(what: &'static str) -> Case {
    tracing::error!(fault = what, "an ITI-83 answer could not be written");
    Case::Internal
}

/// The pseudonyms `person` already holds in the domains of `pseudonymised`
/// the caller of client id `client` is the consumer of, as `(namespace,
/// value)`: a query naming no `targetSystem` carries no other domain's.
async fn own_pseudonyms(
    connection: &mut sqlx::PgConnection,
    person: record::PersonId,
    pseudonymised: &[pseudonym::PseudonymDomain],
    client: Option<&str>,
) -> Result<Vec<(String, String)>, Case> {
    let pseudonyms = pseudonym::held(connection, person)
        .await
        .map_err(|error| store_failure(&error))?;
    Ok(pseudonyms
        .into_iter()
        .filter(|(namespace, _value)| {
            pseudonymised
                .iter()
                .any(|domain| &domain.namespace == namespace && domain.serves(client))
        })
        .map(|(namespace, value)| (namespace, value.to_string()))
        .collect())
}
