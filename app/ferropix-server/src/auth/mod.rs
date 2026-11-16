// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! FerroPIX as a SMART App Launch 2.x resource server, over
//! `ferrohealth-smart`.
//!
//! The crate reads each `[auth]` issuer's RFC 8414 metadata and key set, or
//! under `mode = "introspection"` asks the issuer's RFC 7662 introspection
//! endpoint, trusting the `[auth.tls]` roots beside the platform's, validates
//! every access token, checks the Basic users of the development
//! profile, picks the issuer the SMART discovery document describes
//! (`resource::described::describe`) and builds that document. This module holds what
//! is FerroPIX's own: the permission table of every route ([`table`]), the
//! layer that applies it with its `OperationOutcome` refusals ([`guard`]),
//! and the rule that a Patient Identity Source feeds only its own domain
//! ([`source`], PIXm 3.1.0 §2:3.104.5).
//!
//! FerroPIX never forwards a caller's token: an access token names the
//! resource server it is for (RFC 9068 §4), and the audit repository and
//! the PMIR subscribers are called with FerroPIX's own credential.

pub mod guard;
pub mod source;
pub mod table;

use std::collections::BTreeSet;
use std::sync::Arc;

use ferrohealth_secret::secret_url::SecretUrl;
use ferrohealth_smart::guard::Gate;
use ferrohealth_smart::resource::configuration::{
    OpenIdConnect, PermissionCapability, Purpose, SmartOptions,
};
use ferrohealth_smart::resource::described::{Candidate, DescribeError, describe};
use ferrohealth_smart::resource::introspection::{
    IntrospectionError, IntrospectionSettings, Introspector,
};
use ferrohealth_smart::resource::{IssuerSet, ResourceError, ResourceServer};

use crate::auth::source::Sources;
use crate::config::section::auth::AuthSettings;
use crate::config::section::domain::DomainSeed;

/// A resource server FerroPIX cannot start with.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SmartError {
    /// An issuer's metadata or key set could not be read, or breaks a rule.
    #[error("the authorization server {issuer} could not be read")]
    Issuer {
        /// The issuer identifier.
        issuer: String,
        /// What the resource server reported.
        #[source]
        source: Box<ResourceError>,
    },
    /// The introspecting issuer's metadata or endpoint could not be read,
    /// or breaks a rule.
    #[error("the introspection endpoint of {issuer} could not be set up")]
    Introspector {
        /// The issuer identifier.
        issuer: String,
        /// What the introspection check reported.
        #[source]
        source: Box<IntrospectionError>,
    },
    /// No issuer can be described: `[auth] described_issuer` names an issuer
    /// the rule refuses, or without it no single issuer is eligible.
    #[error("the SMART discovery document can describe no issuer (auth.described_issuer)")]
    Described {
        /// Why `describe` refused.
        #[source]
        source: DescribeError,
    },
    /// The described issuer's metadata names no token endpoint, which the
    /// discovery document requires (SMART App Launch 2.2.0, Conformance,
    /// §Metadata, `token_endpoint`: REQUIRED).
    #[error("the authorization server metadata of {issuer} names no token_endpoint")]
    NoTokenEndpoint {
        /// The issuer identifier.
        issuer: String,
    },
}

/// The issuer the discovery document and the `CapabilityStatement` name.
#[derive(Debug)]
struct Primary {
    /// How that issuer's tokens are checked.
    checker: Checker,
    /// The token endpoint its metadata names.
    token_endpoint: String,
    /// What the two documents describe that issuer for.
    purpose: Purpose,
}

/// How the described issuer's tokens are checked.
#[derive(Debug)]
enum Checker {
    /// Against its key set.
    Jwt(Arc<ResourceServer>),
    /// By its introspection endpoint.
    Introspection(Arc<Introspector>),
}

impl Checker {
    /// The token endpoint the issuer's metadata names.
    fn token_endpoint(&self) -> Option<&String> {
        match self {
            Self::Jwt(server) => server.metadata().token_endpoint.as_ref(),
            Self::Introspection(checker) => checker.metadata().token_endpoint.as_ref(),
        }
    }

    /// The issuer identifier.
    fn issuer(&self) -> &str {
        match self {
            Self::Jwt(server) => &server.settings().issuer,
            Self::Introspection(checker) => &checker.settings().issuer,
        }
    }

    /// The SMART discovery document over the issuer's metadata.
    fn smart_configuration(&self, options: &SmartOptions) -> Discovery {
        Discovery(match self {
            Self::Jwt(server) => server.smart_configuration(options),
            Self::Introspection(checker) => checker.smart_configuration(options),
        })
    }
}

/// The started resource server: the gate in front of every guarded route,
/// the two product scopes, the Sources of each domain and what the two
/// discovery documents name.
#[derive(Debug)]
pub struct Smart {
    /// The credentials every guarded route admits.
    gate: Gate,
    /// The issuer the discovery documents name; `None` on a development
    /// surface of Basic users alone.
    primary: Option<Primary>,
    /// The scope of a data steward.
    steward_scope: String,
    /// The scope of an administrator.
    admin_scope: String,
    /// The scopes `[auth] scopes_supported` lists; empty for the table's.
    scopes_listed: Vec<String>,
    /// The capabilities `[auth] capabilities` adds to the discovery document.
    capabilities: Vec<String>,
    /// Who may feed which domain.
    sources: Sources,
}

impl Smart {
    /// Reads the metadata and the key set of every issuer `auth` names, sets
    /// up every introspected one, and takes its Basic users.
    ///
    /// Each issuer is read once here, so an authorization server that cannot
    /// be reached refuses the start rather than the first request.
    ///
    /// # Errors
    ///
    /// Returns [`SmartError::Issuer`] for an issuer whose metadata or key set
    /// could not be read or that breaks a rule, [`SmartError::Introspector`]
    /// for an introspecting issuer that cannot be set up,
    /// [`SmartError::Described`] when issuers are configured and none can be
    /// described, and [`SmartError::NoTokenEndpoint`] when the described
    /// issuer names no token endpoint.
    pub async fn discover(auth: AuthSettings, domains: &[DomainSeed]) -> Result<Self, SmartError> {
        let AuthSettings {
            resolved,
            steward_scope,
            admin_scope,
            clients,
        } = auth;
        let mut gate = Gate::open();
        let (issuers, primary) = discover_issuers(
            resolved.issuers,
            resolved.introspection,
            resolved.described_issuer.as_ref(),
        )
        .await?;
        if let Some(issuers) = issuers {
            gate = gate.with_bearer(Arc::new(issuers));
        }
        if let Some(store) = resolved.basic {
            gate = gate.with_basic(Arc::new(store));
        }
        if let Some(realm) = resolved.realm {
            gate = gate.with_realm(realm);
        }
        Ok(Self {
            gate,
            primary,
            steward_scope,
            admin_scope,
            scopes_listed: resolved.scopes_supported,
            capabilities: resolved.capabilities,
            sources: Sources::new(domains, clients),
        })
    }

    /// Returns the credentials every guarded route admits.
    #[must_use]
    pub const fn gate(&self) -> &Gate {
        &self.gate
    }

    /// Returns the scope of a data steward.
    #[must_use]
    pub fn steward_scope(&self) -> &str {
        &self.steward_scope
    }

    /// Returns the scope of an administrator.
    #[must_use]
    pub fn admin_scope(&self) -> &str {
        &self.admin_scope
    }

    /// Returns who may feed which domain.
    #[must_use]
    pub const fn sources(&self) -> &Sources {
        &self.sources
    }

    /// Returns the token endpoint the discovery documents name, or `None`
    /// on a surface of Basic users alone.
    #[must_use]
    pub fn token_endpoint(&self) -> Option<&str> {
        self.primary
            .as_ref()
            .map(|primary| primary.token_endpoint.as_str())
    }

    /// Returns the scopes `scopes_supported` lists, which "the server SHALL
    /// support" (SMART App Launch 2.2.0, Conformance, §Metadata): the ones
    /// `[auth] scopes_supported` names, or every scope the permission table
    /// enforces when it names none.
    #[must_use]
    pub fn scopes_supported(&self) -> Vec<String> {
        if self.scopes_listed.is_empty() {
            table::scopes(&self.steward_scope, &self.admin_scope)
        } else {
            self.scopes_listed.clone()
        }
    }

    /// Returns the SMART discovery document, or `None` on a surface of Basic
    /// users alone, which no authorization server stands behind.
    ///
    /// FerroPIX admits `system/` scopes and the product scopes alone, so the
    /// document claims no `permission-patient` or `permission-user`; it
    /// evaluates no granular constraint, so it claims no `permission-v2`
    /// (Conformance, §Capabilities).
    #[must_use]
    pub fn configuration(&self) -> Option<Discovery> {
        let primary = self.primary.as_ref()?;
        Some(primary.checker.smart_configuration(&SmartOptions {
            scopes_supported: self.scopes_supported(),
            capabilities: self.capabilities.clone(),
            permissions: BTreeSet::from([PermissionCapability::V1]),
            openid_connect: OpenIdConnect::Withheld,
            purpose: primary.purpose,
            ..SmartOptions::default()
        }))
    }
}

/// Reads the metadata and the key set of every JWT issuer of `settings`
/// and sets up every introspected issuer of `introspection`, and returns
/// the one set that checks all their tokens, `None` for no issuer, with the
/// issuer the discovery documents name: `named`, else the one issuer
/// `describe` allows, never by the order the issuers are written in.
///
/// # Errors
///
/// Returns [`SmartError::Issuer`] for an issuer that cannot be read,
/// [`SmartError::Introspector`] for an introspected issuer that cannot be
/// set up, [`SmartError::Described`] when no issuer can be described, and
/// [`SmartError::NoTokenEndpoint`] when the described issuer names no token
/// endpoint.
async fn discover_issuers(
    settings: Vec<ferrohealth_smart::resource::Settings>,
    introspection: Vec<IntrospectionSettings>,
    named: Option<&SecretUrl>,
) -> Result<(Option<IssuerSet>, Option<Primary>), SmartError> {
    if settings.is_empty() && introspection.is_empty() {
        return Ok((None, None));
    }
    let mut issuers = IssuerSet::new();
    let mut candidates = Vec::new();
    for one in settings {
        let name = one.issuer.clone();
        let server = ResourceServer::discover(one)
            .await
            .map_err(|source| SmartError::Issuer {
                issuer: name,
                source: Box::new(source),
            })?;
        let server = Arc::new(server);
        candidates.push(Candidate::of_server(
            &server,
            Checker::Jwt(Arc::clone(&server)),
        ));
        issuers = issuers.with(server);
    }
    for one in introspection {
        let name = one.issuer.clone();
        let checker =
            Introspector::discover(one)
                .await
                .map_err(|source| SmartError::Introspector {
                    issuer: name,
                    source: Box::new(source),
                })?;
        let checker = Arc::new(checker);
        candidates.push(Candidate::of_introspector(
            &checker,
            Checker::Introspection(Arc::clone(&checker)),
        ));
        issuers = issuers.with_introspector(checker);
    }
    let described =
        describe(candidates, named).map_err(|source| SmartError::Described { source })?;
    let purpose = described.purpose();
    let checker = described.into_check();
    let token_endpoint =
        checker
            .token_endpoint()
            .cloned()
            .ok_or_else(|| SmartError::NoTokenEndpoint {
                issuer: checker.issuer().to_owned(),
            })?;
    let primary = Primary {
        checker,
        token_endpoint,
        purpose,
    };
    Ok((Some(issuers), Some(primary)))
}

/// The SMART discovery document as `ferrohealth-smart` writes it, a JSON
/// object handed to the wire as it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(transparent)]
#[expect(
    clippy::disallowed_types,
    reason = "the wire seam: ferrohealth-smart builds the discovery document as JSON, and it is served unchanged"
)]
pub struct Discovery(pub serde_json::Value);
