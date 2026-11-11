// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[auth]` section, which is `ferrohealth-smart`'s `AuthSection`, and
//! the `[trust]` section beside it.
//!
//! `[auth]` names the issuers whose tokens the server accepts, each an
//! `[[auth.issuer]]` table with its `kind`, its profile and its own token
//! rules, verified against the issuer's key set or by its RFC 7662
//! introspection endpoint, the trust roots and the Basic users of the
//! development profile; `ferrohealth-smart` resolves it, with the issuer URL
//! rule of RFC 8414 §2, the audience of RFC 9068 §4 and the asymmetric
//! algorithms of RFC 8725 §3.1. A Basic user is a person: an entry granting a `system/`
//! scope is refused, since a machine uses the client credentials grant
//! (RFC 6749 §4.4). Its `product_scope` is the scope of a data steward,
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
use ferrohealth_smart::idp::section::IdpSection;
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
}

impl AuthSettings {
    /// Returns whether a Source authenticated as `client` can be admitted:
    /// an issuer lists it in its `clients`. A Basic user never is one.
    #[must_use]
    pub fn admits(&self, client: &str) -> bool {
        self.clients
            .values()
            .any(|clients| clients.contains(client))
    }
}

impl Auth {
    /// Resolves the section under `profile`, with `trust` and the
    /// administrator scope `admin_scope`.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for `enabled = false`, an `[auth.dpop]`,
    /// `[[auth.party]]` or `[auth.idp]` table, a Basic user's `roles` or an
    /// issuer's `role_claims`,
    /// which FerroPIX would never apply, a
    /// product scope in no
    /// extension form, two equal product scopes, a `[[trust.issuer]]` that
    /// names an issuer `[auth]` does not or one named twice, or a client id
    /// outside RFC 6749 Appendix A.1; [`Error::DevelopmentOnly`] for Basic
    /// users under production; [`Error::BasicSystemScope`] for a Basic user
    /// granted a `system/` scope; and [`Error::Auth`] for every rule
    /// `ferrohealth-smart` checks: no issuer and no Basic
    /// user, an issuer without `kind`, no audience, an issuer outside the URL
    /// rule, a symmetric algorithm, a skew over the bound, a profile refused,
    /// an `[auth.issuer.introspection]` that breaks a rule, an `[auth.tls]` that
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
        unapplied(section)?;
        if section.basic.is_some() && profile == Profile::Production {
            return Err(Error::DevelopmentOnly {
                key: String::from("auth.basic"),
                rule: "a Basic user sends a reusable password on every request",
            });
        }
        persons_only(section)?;
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
        let resolved = section
            .resolve_with_tls(&ferrohealth_smart::config::ServerTls)
            .map_err(|source| Error::Auth { source })?;
        let clients = clients(section, trust)?;
        Ok(AuthSettings {
            resolved,
            steward_scope,
            admin_scope: admin_scope.to_owned(),
            clients,
        })
    }
}

/// Refuses each table of `section` FerroPIX would read and never apply, as
/// it refuses `server.caller_rate`.
///
/// `[auth.dpop]` sets the `DPoP` proof check (RFC 9449) a product builds with
/// its own replay store, and FerroPIX builds none: its callers are backend
/// services and persons holding bearer tokens, and no IHE profile it serves
/// asks for `DPoP`. `[[auth.party]]` binds a person's login to a party, and
/// `[auth.idp]` holds the identity provider adapters only that binding
/// reads; FerroPIX keeps no party. A Basic user's `roles` and an issuer's
/// `role_claims` are refused too, since the permission table reads scopes. `[[auth.federation]]` and
/// `[[auth.attributes]]` are applied: the first classifies the tokens of a
/// `federation` issuer, the second states the professional's attributes the
/// audit record carries ([`crate::audit::records::Call::of`]).
fn unapplied(section: &AuthSection) -> Result<(), Error> {
    // NOTE: no specification governs this: our own design; a key that would be read and
    // never applied is refused, so an operator never believes it is in force.
    for (set, key, rule) in [
        (
            section.dpop.is_some(),
            "auth.dpop",
            "FerroPIX checks no DPoP proof (RFC 9449), so the table would never apply",
        ),
        (
            !section.party.is_empty(),
            "auth.party",
            "FerroPIX binds no login to a party, so the table would never apply",
        ),
        (
            section
                .basic
                .iter()
                .flat_map(|basic| basic.users.iter())
                .any(|user| !user.roles.is_empty()),
            "auth.basic.users[].roles",
            "FerroPIX's permission table reads scopes, so a role would never apply",
        ),
        (
            section.idp != IdpSection::default(),
            "auth.idp",
            "FerroPIX binds no login to a party, the only reader of an identity provider adapter",
        ),
    ] {
        if set {
            return Err(value(key, rule));
        }
    }
    if let Some(index) = section
        .issuers()
        .iter()
        .position(|entry| entry.role_claims.is_some())
    {
        return Err(value(
            &format!("auth.issuer[{index}].role_claims"),
            "FerroPIX's permission table reads scopes, so a role would never apply",
        ));
    }
    Ok(())
}

/// Refuses a Basic user of `section` granted a `system/` scope.
///
/// A Basic credential is a person's, never a machine's, and
/// `ferrohealth-smart` classifies it as a person (its `Caller::basic`); a
/// `system/` scope (SMART App Launch 2.2.0, Scopes and Launch Context) on
/// it would grant a person what only a client credentials token (RFC 6749
/// §4.4) may hold.
fn persons_only(section: &AuthSection) -> Result<(), Error> {
    let users = section.basic.iter().flat_map(|basic| basic.users.iter());
    for (index, user) in users.enumerate() {
        if user
            .scopes
            .iter()
            .any(|scope| scope.split(' ').any(|token| token.starts_with("system/")))
        {
            return Err(Error::BasicSystemScope {
                index,
                user: user.user.clone(),
            });
        }
    }
    Ok(())
}

/// Returns the Source clients of each `[[trust.issuer]]`, by issuer, each
/// an issuer `section` names.
fn clients(
    section: &AuthSection,
    trust: &Trust,
) -> Result<BTreeMap<String, BTreeSet<String>>, Error> {
    let named: BTreeSet<String> = section
        .issuers()
        .iter()
        .map(|entry| entry.issuer.expose().to_owned())
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
    use ferrohealth_secret::secret_url::SecretUrl;
    use ferrohealth_smart::config::{
        AuthSection, BasicSection, BasicUser, ConfigError, IssuerEntry,
    };
    use ferrohealth_smart::kind::IssuerKind;
    use ferrohealth_smart::party::config::PartySection;
    use ferrohealth_smart::sender::proof::DpopSection;
    use secrecy::SecretString;

    use super::{Auth, STEWARD_SCOPE, Trust, TrustedIssuer, is_client_id};
    use crate::config::Profile;
    use crate::config::error::Error;

    const ADMIN: &str = "__ferropix.admin";

    fn auth(issuers: &[&str]) -> Auth {
        Auth(AuthSection {
            issuer: issuers
                .iter()
                .map(|issuer| IssuerEntry {
                    issuer: SecretUrl::new(*issuer),
                    kind: Some(IssuerKind::Machines),
                    audience: vec![String::from("https://pix.example.org/fhir")],
                    ..IssuerEntry::default()
                })
                .collect(),
            ..AuthSection::default()
        })
    }

    fn basic(user: &str, scopes: &[&str]) -> BasicSection {
        BasicSection {
            users: vec![BasicUser {
                user: user.to_owned(),
                password_hash: Some(SecretString::from(
                    "$argon2id$v=19$m=19456,t=2,p=1$c3ludGhldGljLXNhbHQtMTY$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                )),
                scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
                ..BasicUser::default()
            }],
            ..BasicSection::default()
        }
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
            Err(ConfigError::ProductScope { .. })
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
    fn a_basic_user_is_refused_under_production_and_is_no_source_under_development() {
        let mut person = auth(&[]);
        person.0.basic = Some(basic("dev-steward", &[STEWARD_SCOPE]));
        let error = person
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect_err("Basic is a development credential");
        assert!(
            matches!(&error, Error::DevelopmentOnly { key, .. } if key == "auth.basic"),
            "{error:?}"
        );
        let resolved = person
            .resolve(Profile::Development, &Trust::default(), ADMIN)
            .expect("development admits a Basic person");
        assert!(
            !resolved.admits("dev-steward"),
            "a Basic user is a person, never a Source"
        );
    }

    #[test]
    fn a_basic_user_granted_a_system_scope_is_refused_naming_the_user() {
        for scopes in [
            &["system/Patient.cud"][..],
            &[STEWARD_SCOPE, "system/Patient.rs"][..],
            &["__ferropix.steward system/Subscription.cruds"][..],
        ] {
            let mut machine = auth(&[]);
            machine.0.basic = Some(basic("east-lab", scopes));
            let error = machine
                .resolve(Profile::Development, &Trust::default(), ADMIN)
                .expect_err("a Basic credential is never a machine's");
            assert!(
                matches!(&error, Error::BasicSystemScope { index: 0, user } if user == "east-lab"),
                "{scopes:?}: {error:?}"
            );
            assert!(error.to_string().contains("east-lab"), "{error}");
            assert!(error.to_string().contains("client credentials"), "{error}");
        }
    }

    #[test]
    fn a_dpop_or_party_table_or_a_basic_role_the_server_never_applies_is_refused_naming_its_key() {
        let url = "https://idp.example.org";
        let mut dpop = auth(&[url]);
        dpop.0.dpop = Some(DpopSection::default());
        let mut party = auth(&[url]);
        party.0.party = vec![PartySection {
            issuer: url.to_owned(),
            ..PartySection::default()
        }];
        let mut roles = auth(&[url]);
        let mut users = basic("dev-steward", &[STEWARD_SCOPE]);
        users.users[0].roles = vec![String::from("steward")];
        roles.0.basic = Some(users);
        let mut role_claims = auth(&[url, "https://idp.example.org/second"]);
        role_claims.0.issuer[1].role_claims = Some(vec![String::from("groups")]);
        for (section, key) in [
            (dpop, "auth.dpop"),
            (party, "auth.party"),
            (roles, "auth.basic.users[].roles"),
            (role_claims, "auth.issuer[1].role_claims"),
        ] {
            let error = section
                .resolve(Profile::Production, &Trust::default(), ADMIN)
                .expect_err("a table read and never applied is refused");
            assert!(
                matches!(&error, Error::Value { key: named, .. } if named == key),
                "{key}: {error:?}"
            );
            assert!(error.to_string().contains("never apply"), "{error}");
        }
    }

    #[test]
    fn an_issuer_without_its_kind_is_refused() {
        let mut kindless = auth(&["https://idp.example.org"]);
        for entry in &mut kindless.0.issuer {
            entry.kind = None;
        }
        let error = kindless
            .resolve(Profile::Production, &Trust::default(), ADMIN)
            .expect_err("kind has no default");
        assert!(matches!(error, Error::Auth { .. }), "{error:?}");
    }
}
