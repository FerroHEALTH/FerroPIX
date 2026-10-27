// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! FerroPIX as a SMART App Launch 2.x resource server, over
//! `ferrohealth-smart`.
//!
//! The crate reads each `[auth]` issuer's RFC 8414 metadata and key set, or
//! under `mode = "introspection"` asks the issuer's RFC 7662 introspection
//! endpoint, trusting the `ca_file` roots beside the platform's, validates
//! every access token, checks the Basic users of the development
//! profile and builds the SMART discovery document. This module holds what
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

use ferrohealth_smart::guard::Gate;
use ferrohealth_smart::resource::configuration::{
    OpenIdConnect, PermissionCapability, SmartOptions,
};
use ferrohealth_smart::resource::introspection::{IntrospectionError, Introspector};
use ferrohealth_smart::resource::{IssuerSet, ResourceError, ResourceServer, Verifier};

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
    /// No issuer's metadata names a token endpoint, which the discovery
    /// document requires (SMART App Launch 2.2.0, Conformance, §Metadata,
    /// `token_endpoint`: REQUIRED).
    #[error("no [auth] issuer metadata names a token_endpoint")]
    NoTokenEndpoint,
}

/// The issuer the discovery document and the `CapabilityStatement` name.
#[derive(Debug)]
struct Primary {
    /// How that issuer's tokens are checked.
    checker: Checker,
    /// The token endpoint its metadata names.
    token_endpoint: String,
}

/// How the primary issuer's tokens are checked.
#[derive(Debug)]
enum Checker {
    /// Against its key set.
    Jwt(Arc<ResourceServer>),
    /// By its introspection endpoint.
    Introspection(Arc<Introspector>),
}

impl Checker {
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
    /// Reads the metadata and the key set of every issuer `auth` names, and
    /// takes its Basic users.
    ///
    /// Each issuer is read once here, so an authorization server that cannot
    /// be reached refuses the start rather than the first request.
    ///
    /// # Errors
    ///
    /// Returns [`SmartError::Issuer`] for an issuer whose metadata or key set
    /// could not be read or that breaks a rule, [`SmartError::Introspector`]
    /// for an introspecting issuer that cannot be set up, and
    /// [`SmartError::NoTokenEndpoint`] when issuers are configured and none
    /// names a token endpoint.
    pub async fn discover(auth: AuthSettings, domains: &[DomainSeed]) -> Result<Self, SmartError> {
        let AuthSettings {
            resolved,
            steward_scope,
            admin_scope,
            clients,
            basic_users,
        } = auth;
        let mut gate = Gate::open();
        let primary = if let Some(settings) = resolved.introspection {
            let issuer = settings.issuer.clone();
            let checker = Introspector::discover(settings).await.map_err(|source| {
                SmartError::Introspector {
                    issuer,
                    source: Box::new(source),
                }
            })?;
            let checker = Arc::new(checker);
            let bearer: Arc<dyn Verifier> = checker.clone();
            gate = gate.with_bearer(bearer);
            let token_endpoint = checker
                .metadata()
                .token_endpoint
                .clone()
                .ok_or(SmartError::NoTokenEndpoint)?;
            Some(Primary {
                checker: Checker::Introspection(checker),
                token_endpoint,
            })
        } else {
            let (issuers, primary) = discover_issuers(resolved.issuers).await?;
            if let Some(issuers) = issuers {
                gate = gate.with_bearer(Arc::new(issuers));
            }
            primary
        };
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
            sources: Sources::new(domains, clients, basic_users),
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
            ..SmartOptions::default()
        }))
    }
}

/// Reads the metadata and the key set of every issuer of `settings`, and
/// returns the set that checks their tokens, `None` for no issuer, with the
/// first issuer whose metadata names a token endpoint.
///
/// # Errors
///
/// Returns [`SmartError::Issuer`] for an issuer that cannot be read and
/// [`SmartError::NoTokenEndpoint`] when none names a token endpoint.
async fn discover_issuers(
    settings: Vec<ferrohealth_smart::resource::Settings>,
) -> Result<(Option<IssuerSet>, Option<Primary>), SmartError> {
    if settings.is_empty() {
        return Ok((None, None));
    }
    let mut issuers = IssuerSet::new();
    let mut primary = None;
    for one in settings {
        let name = one.issuer.clone();
        let server = ResourceServer::discover(one)
            .await
            .map_err(|source| SmartError::Issuer {
                issuer: name,
                source: Box::new(source),
            })?;
        let server = Arc::new(server);
        if primary.is_none()
            && let Some(token_endpoint) = server.metadata().token_endpoint.clone()
        {
            primary = Some(Primary {
                checker: Checker::Jwt(Arc::clone(&server)),
                token_endpoint,
            });
        }
        issuers = issuers.with(server);
    }
    let primary = primary.ok_or(SmartError::NoTokenEndpoint)?;
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
