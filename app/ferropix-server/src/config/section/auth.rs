// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[auth]` section, which is `ferrohealth-smart`'s `AuthSection`, and
//! the `[trust]` section beside it.
//!
//! `[auth]` names the issuers whose tokens the server accepts, verified
//! against each issuer's key set or by its RFC 7662 introspection endpoint,
//! the token rules, the trust roots and the Basic users of the development
//! profile; `ferrohealth-smart` resolves it, with the issuer URL rule of
//! RFC 8414 §2, the audience of RFC 9068 §4 and the asymmetric algorithms
//! of RFC 8725 §3.1. Its `product_scope` is the scope of a data steward,
//! `__ferropix.steward` unless set; the administrator's is `[operator]
//! admin_scope`. Both take an extension form of SMART App Launch 2.2.0,
//! Scopes and Launch Context, §Extensibility.
//!
//! `[[trust.issuer]]` lists, for each issuer, the client ids a token of that
//! issuer may carry to feed a domain (PIXm 3.1.0 §2:3.104.5). The binding of
//! a client to its issuer is FerroPIX's own design: two authorization
//! servers may each mint a token for a client of the same name.

use std::collections::{BTreeMap, BTreeSet};

use ferrohealth_smart::config::{AuthSection, Resolved};
use ferrohealth_smart::scope::{is_extension_scope, is_scope_token};
use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;

/// The steward scope when `[auth] product_scope` is empty.
pub const STEWARD_SCOPE: &str = "__ferropix.steward";

/// The `[auth]` section as written: `ferrohealth-smart`'s.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Auth(pub AuthSection);

/// The `[trust]` section as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Trust {
    /// What each issuer's tokens are honoured for (`[[trust.issuer]]`).
    pub issuer: Vec<TrustedIssuer>,
}

/// One `[[trust.issuer]]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrustedIssuer {
    /// The issuer, as `[auth]` names it.
    pub issuer: String,
    /// The client ids this issuer may vouch for as a Patient Identity Source.
    pub clients: Vec<String>,
}

/// The `[auth]` and `[trust]` sections, resolved.
#[derive(Debug)]
pub struct AuthSettings {
    /// What `ferrohealth-smart` resolved: the issuers' settings or the
    /// introspection check, and the Basic store.
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
    /// Resolves the section under `profile`, with `trust` and the
    /// administrator scope `admin_scope`.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for `enabled = false`, a product scope in no
    /// extension form, two equal product scopes, a `[[trust.issuer]]` that
    /// names an issuer `[auth]` does not or one named twice, or a client id
    /// outside RFC 6749 Appendix A.1; [`Error::DevelopmentOnly`] for Basic
    /// users under production; and [`Error::Auth`] for every rule
    /// `ferrohealth-smart` checks: no issuer and no Basic user, no audience,
    /// an issuer outside the URL rule, a symmetric algorithm, a skew over the
    /// bound, an `[auth.introspection]` that breaks a rule, a `ca_file` that
    /// is no PEM of certificates, and a Basic user it refuses.
    pub fn resolve(
        &self,
        profile: Profile,
        trust: &Trust,
        admin_scope: &str,
    ) -> Result<AuthSettings, Error> {
        let section = &self.0;
        // NOTE: no specification governs this: our own design; every route but
        // health carries identity data, so the surface is never open.
        if !section.enabled {
            return Err(value(
                "auth.enabled",
                "FerroPIX always authenticates its callers",
            ));
        }
        if section.basic.is_some() && profile == Profile::Production {
            return Err(Error::DevelopmentOnly {
                key: String::from("auth.basic"),
                rule: "a Basic user sends a reusable password on every request",
            });
        }
        let steward_scope = if section.product_scope.is_empty() {
            STEWARD_SCOPE.to_owned()
        } else {
            section.product_scope.clone()
        };
        for (key, scope) in [
            ("auth.product_scope", steward_scope.as_str()),
            ("operator.admin_scope", admin_scope),
        ] {
            if !(is_scope_token(scope) && is_extension_scope(scope)) {
                return Err(value(
                    key,
                    "a scope token that is a full URI or starts with __ (SMART App Launch 2.2.0)",
                ));
            }
        }
        if steward_scope == admin_scope {
            return Err(value(
                "operator.admin_scope",
                "it must differ from the steward scope, auth.product_scope",
            ));
        }
        let clients = clients(section, trust)?;
        let resolved = section.resolve().map_err(|source| Error::Auth { source })?;
        let basic_users = section
            .basic
            .iter()
            .flat_map(|basic| basic.users.iter().map(|user| user.user.clone()))
            .collect();
        Ok(AuthSettings {
            resolved,
            steward_scope,
            admin_scope: admin_scope.to_owned(),
            clients,
            basic_users,
        })
    }
}

/// Returns the Source clients of each `[[trust.issuer]]`, by issuer, each
/// an issuer `section` names.
fn clients(
    section: &AuthSection,
    trust: &Trust,
) -> Result<BTreeMap<String, BTreeSet<String>>, Error> {
    let named: BTreeSet<String> = section
        .issuers()
        .into_iter()
        .map(|entry| entry.issuer)
        .filter(|issuer| !issuer.is_empty())
        .collect();
    let mut clients = BTreeMap::new();
    for (index, entry) in trust.issuer.iter().enumerate() {
        if !named.contains(&entry.issuer) {
            return Err(value(
                &format!("trust.issuer[{index}].issuer"),
                "an issuer [auth] names",
            ));
        }
        if !entry.clients.iter().all(|client| is_client_id(client)) {
            return Err(value(
                &format!("trust.issuer[{index}].clients"),
                "a client id of the RFC 6749 Appendix A.1 characters, not empty",
            ));
        }
        let listed: BTreeSet<String> = entry.clients.iter().cloned().collect();
        if clients.insert(entry.issuer.clone(), listed).is_some() {
            return Err(value(
                &format!("trust.issuer[{index}].issuer"),
                "an earlier entry names this issuer",
            ));
        }
    }
    Ok(clients)
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
    use ferrohealth_smart::config::{AuthSection, BasicSection, BasicUser, IssuerEntry, IssuerKey};
    use secrecy::SecretString;

    use super::{Auth, STEWARD_SCOPE, Trust, TrustedIssuer, is_client_id};
    use crate::config::Profile;
    use crate::config::error::Error;

    const ADMIN: &str = "__ferropix.admin";

    fn auth(issuers: &[&str]) -> Auth {
        Auth(AuthSection {
            issuer: if issuers.is_empty() {
                IssuerKey::Flat(String::new())
            } else {
                IssuerKey::List(
                    issuers
                        .iter()
                        .map(|issuer| IssuerEntry {
                            issuer: (*issuer).to_owned(),
                            ..IssuerEntry::default()
                        })
                        .collect(),
                )
            },
            audience: vec![String::from("https://pix.example.org/fhir")],
            ..AuthSection::default()
        })
    }

    fn trust(entries: &[(&str, &[&str])]) -> Trust {
        Trust {
            issuer: entries
                .iter()
                .map(|(issuer, clients)| TrustedIssuer {
                    issuer: (*issuer).to_owned(),
                    clients: clients.iter().map(|client| (*client).to_owned()).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_issuer_resolves_and_admits_exactly_the_clients_its_trust_entry_lists() {
        let url = "https://idp.example.org/realms/pix";
        let resolved = auth(&[url])
            .resolve(
                Profile::Production,
                &trust(&[(url, &["lab-source"])]),
                ADMIN,
            )
            .expect("one issuer resolves");
        assert_eq!(1, resolved.resolved.issuers.len());
        assert!(resolved.admits("lab-source"));
        assert!(!resolved.admits("other-source"));
        assert_eq!(STEWARD_SCOPE, resolved.steward_scope);
        assert_eq!(ADMIN, resolved.admin_scope);
    }

    #[test]
    fn a_section_with_no_issuer_and_no_basic_user_and_a_disabled_one_are_refused() {
        let error = auth(&[])
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect_err("a surface that admits no credential fails closed");
        assert!(matches!(error, Error::Auth { .. }), "{error:?}");
        let mut off = auth(&["https://idp.example.org"]);
        off.0.enabled = false;
        let error = off
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect_err("FerroPIX is never open");
        assert!(
            matches!(&error, Error::Value { key, .. } if key == "auth.enabled"),
            "{error:?}"
        );
    }

    #[test]
    fn an_issuer_the_family_crate_refuses_is_refused() {
        let error = auth(&["http://idp.example.org"])
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect_err("RFC 8414 §2 wants https off loopback");
        assert!(matches!(error, Error::Auth { .. }), "{error:?}");
    }

    #[test]
    fn a_trust_entry_of_an_unknown_issuer_named_twice_or_with_a_bad_client_id_is_refused() {
        let url = "https://idp.example.org";
        for (bad, key) in [
            (
                trust(&[("https://other.example.org", &[])]),
                "trust.issuer[0].issuer",
            ),
            (trust(&[(url, &[]), (url, &[])]), "trust.issuer[1].issuer"),
            (trust(&[(url, &["caf\u{e9}"])]), "trust.issuer[0].clients"),
        ] {
            let error = auth(&[url])
                .resolve(Profile::Production, &bad, ADMIN)
                .expect_err("refused");
            assert!(
                matches!(&error, Error::Value { key: named, .. } if named == key),
                "{key}: {error:?}"
            );
        }
        assert!(is_client_id("lab source 1"));
        assert!(!is_client_id(""));
        assert!(!is_client_id("tab\there"));
    }

    #[test]
    fn a_product_scope_in_no_extension_form_and_two_equal_scopes_are_refused() {
        let url = "https://idp.example.org";
        let mut bad = auth(&[url]);
        bad.0.product_scope = String::from("steward");
        assert!(matches!(
            bad.0.resolve(),
            Err(ferrohealth_smart::config::ConfigError::ProductScope { .. })
        ));
        assert!(matches!(
            auth(&[url]).resolve(Profile::Production, &Trust::default(), "admin"),
            Err(Error::Value { .. })
        ));
        assert!(matches!(
            auth(&[url]).resolve(Profile::Production, &Trust::default(), STEWARD_SCOPE),
            Err(Error::Value { .. })
        ));
        let mut uri = auth(&[url]);
        uri.0.product_scope = String::from("https://pix.example.org/scope/steward");
        let resolved = uri
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect("a full URI is an extension scope");
        assert_eq!(
            "https://pix.example.org/scope/steward",
            resolved.steward_scope
        );
    }

    #[test]
    fn a_basic_user_is_refused_under_production_and_admits_its_client_under_development() {
        let mut basic = auth(&[]);
        basic.0.basic = Some(BasicSection {
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
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect_err("Basic is a development credential");
        assert!(
            matches!(&error, Error::DevelopmentOnly { key, .. } if key == "auth.basic"),
            "{error:?}"
        );
        let resolved = basic
            .resolve(Profile::Development, &Trust::default(), ADMIN)
            .expect("development admits a Basic user");
        assert!(resolved.admits("dev-source"));
    }
}
