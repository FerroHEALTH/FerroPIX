// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[auth]` section: the issuers whose tokens the server accepts, with
//! the Source clients each may vouch for, the token rules, the two product
//! scopes and the Basic users of the development profile.
//!
//! The section is resolved by `ferrohealth-smart`: the issuer URL rule of
//! RFC 8414 §2, the audience of RFC 9068 §4 and the asymmetric algorithms of
//! RFC 8725 §3.1. The product scopes take an extension form of SMART App
//! Launch 2.2.0, Scopes and Launch Context, §Extensibility. The `clients` of
//! an issuer are FerroPIX's own design: the client ids a token of that issuer
//! may carry to feed a domain (PIXm 3.1.0 §2:3.104.5).

use std::collections::{BTreeMap, BTreeSet};

use ferrohealth_smart::config::{AuthSection, BasicSection, IssuerEntry, IssuerKey, Resolved};
use ferrohealth_smart::resource::TokenProfile;
use ferrohealth_smart::scope::{is_extension_scope, is_scope_token};
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;

/// The `[auth]` section as written.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Auth {
    /// The trusted issuers (`[[auth.issuer]]`).
    pub issuer: Vec<Issuer>,
    /// The audiences, of which a token's `aud` must contain one.
    pub audience: Vec<String>,
    /// The signature algorithms admitted.
    pub algorithms: Vec<Algorithm>,
    /// The claims a token must carry.
    pub token_profile: TokenProfile,
    /// The clock skew, at most 300.
    pub clock_skew_seconds: u64,
    /// How long a held key set is used.
    pub jwks_max_age_seconds: u64,
    /// The shortest time between two reads of the key set.
    pub jwks_refresh_interval_seconds: u64,
    /// A key set file in place of the issuer's `jwks_uri`; empty for none.
    pub jwks_file: String,
    /// How long one call to an authorization server may take.
    pub timeout_ms: u64,
    /// The realm the challenges name; empty for the server's own.
    pub realm: String,
    /// The scope of a data steward.
    pub steward_scope: String,
    /// The scope of an administrator.
    pub admin_scope: String,
    /// The Basic users, admitted under `profile = "development"` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basic: Option<BasicSection>,
}

impl Default for Auth {
    fn default() -> Self {
        let family = AuthSection::default();
        Self {
            issuer: Vec::new(),
            audience: Vec::new(),
            algorithms: family.algorithms,
            token_profile: family.token_profile,
            clock_skew_seconds: family.clock_skew_seconds,
            jwks_max_age_seconds: family.jwks_max_age_seconds,
            jwks_refresh_interval_seconds: family.jwks_refresh_interval_seconds,
            jwks_file: String::new(),
            timeout_ms: family.timeout_ms,
            realm: String::new(),
            steward_scope: String::from("__ferropix.steward"),
            admin_scope: String::from("__ferropix.admin"),
            basic: None,
        }
    }
}

/// One `[[auth.issuer]]` entry; a token field it leaves out is the section's.
#[expect(
    clippy::struct_field_names,
    reason = "the key is `issuer`, as the entry of ferrohealth-smart names it"
)]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Issuer {
    /// The issuer identifier every token's `iss` equals.
    pub issuer: String,
    /// The client ids this issuer may vouch for as a Patient Identity Source.
    pub clients: Vec<String>,
    /// The audiences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// The signature algorithms admitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithms: Option<Vec<Algorithm>>,
    /// The claims a token must carry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_profile: Option<TokenProfile>,
    /// The clock skew.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_skew_seconds: Option<u64>,
    /// How long a held key set is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_max_age_seconds: Option<u64>,
    /// The shortest time between two reads of the key set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_refresh_interval_seconds: Option<u64>,
    /// A key set file in place of the issuer's `jwks_uri`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_file: Option<String>,
}

/// The `[auth]` section, resolved.
#[derive(Debug)]
pub struct AuthSettings {
    /// What `ferrohealth-smart` resolved: the issuers' settings and the Basic
    /// store.
    pub resolved: Resolved,
    /// The scope of a data steward.
    pub steward_scope: String,
    /// The scope of an administrator.
    pub admin_scope: String,
    /// The Source clients each issuer may vouch for, by issuer.
    pub clients: BTreeMap<String, BTreeSet<String>>,
    /// The user-ids of the Basic users.
    pub basic_users: BTreeSet<String>,
}

impl AuthSettings {
    /// Returns whether a Source authenticated as `client` can be admitted:
    /// an issuer lists it in its `clients`, or a Basic user is named so.
    #[must_use]
    pub fn admits(&self, client: &str) -> bool {
        self.clients
            .values()
            .any(|clients| clients.contains(client))
            || self.basic_users.contains(client)
    }
}

impl Auth {
    /// Resolves the section under `profile`.
    ///
    /// # Errors
    /// Returns [`Error::DevelopmentOnly`] for Basic users under production,
    /// [`Error::Value`] for a product scope in no extension form, two equal
    /// product scopes, an issuer named twice or a client id outside RFC 6749
    /// Appendix A.1, and [`Error::Auth`] for every rule `ferrohealth-smart`
    /// checks: no issuer and no Basic user, no audience, an issuer outside the
    /// URL rule, a symmetric algorithm, a skew over the bound and a Basic user
    /// it refuses.
    pub fn resolve(&self, profile: Profile) -> Result<AuthSettings, Error> {
        if self.basic.is_some() && profile == Profile::Production {
            return Err(Error::DevelopmentOnly {
                key: String::from("auth.basic"),
                rule: "a Basic user sends a reusable password on every request",
            });
        }
        for (key, scope) in [
            ("auth.steward_scope", &self.steward_scope),
            ("auth.admin_scope", &self.admin_scope),
        ] {
            if !(is_scope_token(scope) && is_extension_scope(scope)) {
                return Err(value(
                    key,
                    "a scope token that is a full URI or starts with __ (SMART App Launch 2.2.0)",
                ));
            }
        }
        if self.steward_scope == self.admin_scope {
            return Err(value(
                "auth.admin_scope",
                "it must differ from auth.steward_scope",
            ));
        }
        let mut clients = BTreeMap::new();
        for (index, issuer) in self.issuer.iter().enumerate() {
            if !issuer.clients.iter().all(|client| is_client_id(client)) {
                return Err(value(
                    &format!("auth.issuer[{index}].clients"),
                    "a client id of the RFC 6749 Appendix A.1 characters, not empty",
                ));
            }
            let listed: BTreeSet<String> = issuer.clients.iter().cloned().collect();
            if clients.insert(issuer.issuer.clone(), listed).is_some() {
                return Err(value(
                    &format!("auth.issuer[{index}].issuer"),
                    "an earlier entry names this issuer",
                ));
            }
        }
        let resolved = self
            .family_section()
            .resolve()
            .map_err(|source| Error::Auth { source })?;
        let basic_users = self
            .basic
            .iter()
            .flat_map(|basic| basic.users.iter().map(|user| user.user.clone()))
            .collect();
        Ok(AuthSettings {
            resolved,
            steward_scope: self.steward_scope.clone(),
            admin_scope: self.admin_scope.clone(),
            clients,
            basic_users,
        })
    }

    /// Returns the section `ferrohealth-smart` resolves.
    ///
    /// The flat empty issuer stands for no issuer, which the crate reads as a
    /// surface of Basic users alone.
    fn family_section(&self) -> AuthSection {
        let issuer = if self.issuer.is_empty() {
            IssuerKey::Flat(String::new())
        } else {
            IssuerKey::List(
                self.issuer
                    .iter()
                    .map(|issuer| IssuerEntry {
                        issuer: issuer.issuer.clone(),
                        audience: issuer.audience.clone(),
                        algorithms: issuer.algorithms.clone(),
                        token_profile: issuer.token_profile,
                        clock_skew_seconds: issuer.clock_skew_seconds,
                        jwks_max_age_seconds: issuer.jwks_max_age_seconds,
                        jwks_refresh_interval_seconds: issuer.jwks_refresh_interval_seconds,
                        jwks_file: issuer.jwks_file.clone(),
                    })
                    .collect(),
            )
        };
        AuthSection {
            enabled: true,
            issuer,
            audience: self.audience.clone(),
            algorithms: self.algorithms.clone(),
            token_profile: self.token_profile,
            clock_skew_seconds: self.clock_skew_seconds,
            jwks_max_age_seconds: self.jwks_max_age_seconds,
            jwks_refresh_interval_seconds: self.jwks_refresh_interval_seconds,
            jwks_file: self.jwks_file.clone(),
            timeout_ms: self.timeout_ms,
            realm: self.realm.clone(),
            basic: self.basic.clone(),
            ..AuthSection::default()
        }
    }
}

/// Returns an [`Error::Value`] under `key`.
fn value(key: &str, rule: &'static str) -> Error {
    Error::Value {
        key: key.to_owned(),
        rule,
    }
}

/// Returns whether `client` is a client id of RFC 6749 Appendix A.1:
/// `*VSCHAR`, `%x20-7E`, here not empty.
fn is_client_id(client: &str) -> bool {
    !client.is_empty() && client.chars().all(|c| matches!(c, ' '..='~'))
}

#[cfg(test)]
mod tests {
    use ferrohealth_smart::config::{BasicSection, BasicUser};
    use secrecy::SecretString;

    use super::{Auth, Issuer, is_client_id};
    use crate::config::Profile;
    use crate::config::error::Error;

    fn issuer(url: &str, clients: &[&str]) -> Issuer {
        Issuer {
            issuer: url.to_owned(),
            clients: clients.iter().map(|client| (*client).to_owned()).collect(),
            ..Issuer::default()
        }
    }

    fn auth(issuers: Vec<Issuer>) -> Auth {
        Auth {
            issuer: issuers,
            audience: vec![String::from("https://pix.example.org/fhir")],
            ..Auth::default()
        }
    }

    #[test]
    fn an_issuer_resolves_and_admits_exactly_the_clients_it_lists() {
        let resolved = auth(vec![issuer(
            "https://idp.example.org/realms/pix",
            &["lab-source"],
        )])
        .resolve(Profile::Production)
        .expect("one issuer resolves");
        assert_eq!(1, resolved.resolved.issuers.len());
        assert!(resolved.admits("lab-source"));
        assert!(!resolved.admits("other-source"));
        assert_eq!("__ferropix.steward", resolved.steward_scope);
        assert_eq!("__ferropix.admin", resolved.admin_scope);
    }

    #[test]
    fn a_section_with_no_issuer_and_no_basic_user_is_refused() {
        let error = auth(Vec::new())
            .resolve(Profile::Production)
            .expect_err("a surface that admits no credential fails closed");
        assert!(matches!(error, Error::Auth { .. }), "{error:?}");
    }

    #[test]
    fn an_issuer_the_family_crate_refuses_is_refused() {
        let error = auth(vec![issuer("http://idp.example.org", &[])])
            .resolve(Profile::Production)
            .expect_err("RFC 8414 §2 wants https off loopback");
        assert!(matches!(error, Error::Auth { .. }), "{error:?}");
    }

    #[test]
    fn an_issuer_named_twice_and_a_client_id_outside_rfc_6749_are_refused() {
        let error = auth(vec![
            issuer("https://idp.example.org", &[]),
            issuer("https://idp.example.org", &[]),
        ])
        .resolve(Profile::Production)
        .expect_err("one entry per issuer");
        assert!(matches!(error, Error::Value { .. }), "{error:?}");
        let error = auth(vec![issuer("https://idp.example.org", &["caf\u{e9}"])])
            .resolve(Profile::Production)
            .expect_err("a client id is VSCHAR");
        assert!(matches!(error, Error::Value { .. }), "{error:?}");
        assert!(is_client_id("lab source 1"));
        assert!(!is_client_id(""));
        assert!(!is_client_id("tab\there"));
    }

    #[test]
    fn a_product_scope_in_no_extension_form_and_two_equal_scopes_are_refused() {
        let mut bad = auth(vec![issuer("https://idp.example.org", &[])]);
        bad.steward_scope = String::from("steward");
        assert!(matches!(
            bad.resolve(Profile::Production),
            Err(Error::Value { .. })
        ));
        let mut bad = auth(vec![issuer("https://idp.example.org", &[])]);
        bad.admin_scope = String::from("__ferropix.steward");
        assert!(matches!(
            bad.resolve(Profile::Production),
            Err(Error::Value { .. })
        ));
        let mut uri = auth(vec![issuer("https://idp.example.org", &[])]);
        uri.admin_scope = String::from("https://pix.example.org/scope/admin");
        uri.resolve(Profile::Production)
            .expect("a full URI is an extension scope");
    }

    #[test]
    fn a_basic_user_is_refused_under_production_and_admits_its_client_under_development() {
        let mut basic = auth(Vec::new());
        basic.basic = Some(BasicSection {
            users: vec![BasicUser {
                user: String::from("dev-source"),
                password_hash: Some(SecretString::from(
                    "$argon2id$v=19$m=19456,t=2,p=1$c3ludGhldGljLXNhbHQtMTY$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                )),
                ..BasicUser::default()
            }],
            ..BasicSection::default()
        });
        let error = basic
            .resolve(Profile::Production)
            .expect_err("Basic is a development credential");
        assert!(
            matches!(&error, Error::DevelopmentOnly { key, .. } if key == "auth.basic"),
            "{error:?}"
        );
        let resolved = basic
            .resolve(Profile::Development)
            .expect("development admits a Basic user");
        assert!(resolved.admits("dev-source"));
    }
}
