// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Who may feed which identifier domain.
//!
//! "The Patient Identifier Cross-reference Manager should check that the
//! Patient Identity Source is only making changes to Patient Identifier
//! Domain for which it is authorized" (PIXm 3.1.0 §2:3.104.5).
//!
//! A domain has one Source, the client its `source_client_id` names. A
//! bearer caller is that client when its token's `client_id` equals it and
//! the issuer that signed the token lists it in its `clients`; a Basic user
//! of the development profile is that client when its user-id equals it.
//! The issuer binding is FerroPIX's own design: two authorization servers
//! may each mint a token for a client of the same name. An administrator's
//! write of a domain replaces the map at once ([`Sources::replace`]), so a
//! new Source feeds and a replaced one stops without a restart.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

use ferrohealth_smart::resource::{Access, Via};

use crate::config::section::domain::{DomainKind, DomainSeed};

/// The Sources of the domains and the clients each issuer may vouch for.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// The `source_client_id` of each unretired domain, by system, shared by
    /// every clone so a replacement reaches the guard.
    domains: Arc<RwLock<BTreeMap<String, String>>>,
    /// The system of the master domain, shared the same way.
    master: Arc<RwLock<Option<String>>>,
    /// The Source clients each issuer may vouch for, by issuer.
    clients: BTreeMap<String, BTreeSet<String>>,
    /// The user-ids of the Basic users.
    basic_users: BTreeSet<String>,
}

/// Whether a caller may write to the domain a request names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The caller is the domain's Source.
    Admitted,
    /// The request names no domain FerroPIX knows, which the transaction's
    /// handler answers.
    Unknown,
    /// The caller is not the domain's Source.
    Forbidden,
}

impl Sources {
    /// Returns the Sources of `domains`, the ones the store holds after the
    /// seed, with the `clients` each issuer lists and the Basic users.
    #[must_use]
    pub fn new(
        domains: &[DomainSeed],
        clients: BTreeMap<String, BTreeSet<String>>,
        basic_users: BTreeSet<String>,
    ) -> Self {
        Self {
            domains: Arc::new(RwLock::new(map(domains))),
            master: Arc::new(RwLock::new(master(domains))),
            clients,
            basic_users,
        }
    }

    /// Replaces the domains with `domains`, the unretired ones the store
    /// holds after an administrator's write.
    pub fn replace(&self, domains: &[DomainSeed]) {
        // NOTE: no specification governs this: our own design; the map is
        // replaced whole, so a writer that panicked left no half-written map.
        let mut held = self.domains.write().unwrap_or_else(PoisonError::into_inner);
        let mut kept = self.master.write().unwrap_or_else(PoisonError::into_inner);
        *held = map(domains);
        *kept = master(domains);
    }

    /// Returns the system of the master domain, the PMIR Master Patient
    /// Identification Domain, when the store holds one.
    #[must_use]
    pub fn master(&self) -> Option<String> {
        self.master
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Returns whether `access` may feed the master domain: only its Source
    /// sends ITI-93 (PMIR 1.6.0 §2:3.93.4.1.3), and with no master domain
    /// no caller may.
    #[must_use]
    pub fn verdict_master(&self, access: &Access) -> Verdict {
        match self.master() {
            Some(system) if self.verdict(access, &system) == Verdict::Admitted => Verdict::Admitted,
            _ => Verdict::Forbidden,
        }
    }

    /// Returns the Source clients each issuer may vouch for, by issuer.
    #[must_use]
    pub fn clients(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.clients
    }

    /// Returns the user-ids of the Basic users of the development profile.
    #[must_use]
    pub fn basic_users(&self) -> &BTreeSet<String> {
        &self.basic_users
    }

    /// Returns the `source_client_id` of each unretired domain, by system.
    #[must_use]
    pub fn domains(&self) -> BTreeMap<String, String> {
        self.domains
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Returns whether `client` may be a Source: an issuer lists it in its
    /// `clients`, or it is a Basic user.
    #[must_use]
    pub fn admits(&self, client: &str) -> bool {
        self.clients.values().any(|listed| listed.contains(client))
            || self.basic_users.contains(client)
    }

    /// Returns whether `access` may write to the domain `system` names.
    #[must_use]
    pub fn verdict(&self, access: &Access, system: &str) -> Verdict {
        let held = self.domains.read().unwrap_or_else(PoisonError::into_inner);
        let Some(source) = held.get(system) else {
            return Verdict::Unknown;
        };
        if self.client(access) == Some(source.as_str()) {
            Verdict::Admitted
        } else {
            Verdict::Forbidden
        }
    }

    /// Returns the Source client `access` authenticates as, or `None` when
    /// it is no client a Source may be.
    #[must_use]
    pub fn client<'a>(&self, access: &'a Access) -> Option<&'a str> {
        let caller = access.caller()?;
        match caller.via() {
            Via::Bearer => {
                let client = caller.client_id()?;
                let issuer = caller.issuer()?;
                self.clients
                    .get(issuer)
                    .is_some_and(|listed| listed.contains(client))
                    .then_some(client)
            }
            Via::Basic => {
                let user = caller.subject();
                self.basic_users.contains(user).then_some(user)
            }
        }
    }
}

/// Returns the system of the first master domain of `domains`.
fn master(domains: &[DomainSeed]) -> Option<String> {
    domains
        .iter()
        .find(|domain| domain.kind == DomainKind::Master)
        .map(|domain| domain.system.clone())
}

/// Returns the `source_client_id` of each domain of `domains`, by system.
fn map(domains: &[DomainSeed]) -> BTreeMap<String, String> {
    domains
        .iter()
        .map(|domain| (domain.system.clone(), domain.source_client_id.clone()))
        .collect()
}

/// Returns the system of every `identifier` search parameter of the query
/// `query`, the domains an ITI-104 conditional update or delete names (PIXm
/// 3.1.0 §2:3.104.4.1.2), in the order written.
///
/// Only the part before `|` is returned; the value is never read. A token
/// without a system names no domain, which the transaction's handler
/// answers.
#[must_use]
pub fn domains_of(query: Option<&str>) -> Vec<String> {
    let Some(query) = query else {
        return Vec::new();
    };
    url::form_urlencoded::parse(query.as_bytes())
        .filter(|(name, _)| name == "identifier")
        .filter_map(|(_, token)| {
            token
                .split_once('|')
                .map(|(system, _)| system.to_owned())
                .filter(|system| !system.is_empty())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use ferrohealth_smart::resource::{Access, Caller};

    use super::{Sources, Verdict, domains_of};
    use crate::config::section::domain::{DomainKind, DomainSeed};

    const ISSUER: &str = "https://idp.example.org";

    fn sources() -> Sources {
        let domain = DomainSeed {
            system: String::from("urn:oid:2.999.1.1"),
            name: String::from("East"),
            kind: DomainKind::Local,
            source_client_id: String::from("east-lab"),
            sensitive: true,
        };
        Sources::new(
            &[domain],
            BTreeMap::from([(
                String::from(ISSUER),
                BTreeSet::from([String::from("east-lab"), String::from("west-lab")]),
            )]),
            BTreeSet::from([String::from("dev-source")]),
        )
    }

    fn bearer(client: &str, issuer: &str) -> Access {
        Access::Caller(Arc::new(
            Caller::new(
                String::from("synthetic-sub"),
                Some(client.to_owned()),
                "system/Patient.cud",
                None,
                false,
            )
            .with_issuer(issuer.to_owned()),
        ))
    }

    #[test]
    fn only_the_domains_source_from_an_issuer_that_lists_it_may_write() {
        let sources = sources();
        let system = "urn:oid:2.999.1.1";
        assert_eq!(
            Verdict::Admitted,
            sources.verdict(&bearer("east-lab", ISSUER), system)
        );
        assert_eq!(
            Verdict::Forbidden,
            sources.verdict(&bearer("west-lab", ISSUER), system),
            "another Source"
        );
        assert_eq!(
            Verdict::Forbidden,
            sources.verdict(&bearer("east-lab", "https://other.example.org"), system),
            "an issuer that does not list the client"
        );
        assert_eq!(
            Verdict::Unknown,
            sources.verdict(&bearer("east-lab", ISSUER), "urn:oid:2.999.9")
        );
        let basic = Access::Caller(Arc::new(Caller::basic(String::from("dev-source"), "")));
        assert_eq!(Some("dev-source"), sources.client(&basic));
        assert_eq!(None, sources.client(&Access::Open));
    }

    #[test]
    fn only_the_master_domains_source_feeds_the_master_domain() {
        let sources = sources();
        let east = bearer("east-lab", ISSUER);
        assert_eq!(None, sources.master());
        assert_eq!(
            Verdict::Forbidden,
            sources.verdict_master(&east),
            "no master domain, no Source of it"
        );
        let guard = sources.clone();
        sources.replace(&[DomainSeed {
            system: String::from("https://pix.example.org"),
            name: String::from("Master"),
            kind: DomainKind::Master,
            source_client_id: String::from("east-lab"),
            sensitive: false,
        }]);
        assert_eq!(Some("https://pix.example.org"), guard.master().as_deref());
        assert_eq!(Verdict::Admitted, guard.verdict_master(&east));
        assert_eq!(
            Verdict::Forbidden,
            guard.verdict_master(&bearer("west-lab", ISSUER))
        );
    }

    #[test]
    fn a_replaced_map_reaches_every_clone_and_admits_only_a_listed_client() {
        let sources = sources();
        let guard = sources.clone();
        let system = "urn:oid:2.999.1.1";
        sources.replace(&[DomainSeed {
            system: system.to_owned(),
            name: String::from("East"),
            kind: DomainKind::Local,
            source_client_id: String::from("west-lab"),
            sensitive: true,
        }]);
        assert_eq!(
            Verdict::Admitted,
            guard.verdict(&bearer("west-lab", ISSUER), system),
            "the new Source"
        );
        assert_eq!(
            Verdict::Forbidden,
            guard.verdict(&bearer("east-lab", ISSUER), system),
            "the replaced one"
        );
        sources.replace(&[]);
        assert_eq!(
            Verdict::Unknown,
            guard.verdict(&bearer("west-lab", ISSUER), system)
        );
        assert!(sources.admits("east-lab"));
        assert!(sources.admits("dev-source"));
        assert!(!sources.admits("north-lab"));
    }

    #[test]
    fn the_domains_are_the_systems_of_every_identifier_parameter() {
        assert_eq!(
            vec![String::from("urn:oid:2.999.1.1")],
            domains_of(Some("identifier=urn:oid:2.999.1.1%7C123"))
        );
        assert_eq!(
            vec![String::from("https://example.org/sid")],
            domains_of(Some("_format=json&identifier=https://example.org/sid|9"))
        );
        assert_eq!(
            vec![String::from("a"), String::from("b")],
            domains_of(Some("identifier=a|1&identifier=b|2"))
        );
        for query in [
            None,
            Some(""),
            Some("identifier=no-bar"),
            Some("identifier=%7C123"),
        ] {
            assert!(domains_of(query).is_empty(), "{query:?}");
        }
    }
}
