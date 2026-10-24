// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The one permission table: every route of the FHIR face and the operator
//! API, the transaction it is and what it requires of the caller.
//!
//! The scopes are `docs/architecture.md` §9's: a Consumer reads with
//! `system/Patient.rs`, a Patient Identity Source writes with
//! `system/Patient.cud` (SMART App Launch 2.2.0, Scopes and Launch Context),
//! a PMIR subscriber holds `system/Subscription.cruds`, and the operator API
//! takes the two product scopes. Each IHE transaction also admits the scope
//! its profile defines for a grouping with IUA (PIXm 3.1.0 §2:3.83.5.2 and
//! §2:3.104.5.2, PDQm 3.2.0 §2:3.78.5.2 and §2:3.119.5.2). The router mounts
//! exactly these routes, so no route can be reached without a row.

use ferrohealth_smart::guard::Requirement;
use ferrohealth_smart::scope::Permission;
use http::Method;

/// The FHIR face's path under `{base}`.
pub const FHIR: &str = "/fhir";

/// The operator API's path under `{base}`.
pub const OPERATOR: &str = "/operator";

/// Which switch of the configuration mounts a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mount {
    /// Always mounted: the capability statement and the discovery document.
    Always,
    /// `[pixm] enabled`.
    Pixm,
    /// `[pixm] enabled` and `[pixm] remove_patient` (PIXm 3.1.0 §1:41.2.1).
    PixmRemove,
    /// `[pdqm] enabled`.
    Pdqm,
    /// `[pmir] enabled`.
    Pmir,
    /// `[operator] enabled`.
    Operator,
}

/// What a route requires of the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// No credential: a client reads it before it holds a token.
    Public,
    /// A `system/` grant of one of `permissions` on `resource_type`, or the
    /// IUA scope `transaction` when one is named.
    Fhir {
        /// The permissions, any one of which admits the route.
        permissions: &'static [Permission],
        /// The FHIR resource type the route addresses.
        resource_type: &'static str,
        /// The IUA scope of the transaction, when its profile defines one.
        transaction: Option<&'static str>,
        /// Whether the caller must be the Source of the domain the request
        /// writes to (PIXm 3.1.0 §2:3.104.5).
        source: bool,
    },
    /// `[auth] steward_scope`.
    Steward,
    /// `[auth] admin_scope`.
    Admin,
    /// Either product scope: a read an administrator needs beside a
    /// steward.
    StewardOrAdmin,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The method.
    pub method: Method,
    /// The route template under `{base}`, in `axum`'s syntax.
    pub path: &'static str,
    /// The interaction the audit line records.
    pub interaction: &'static str,
    /// The switch that mounts it.
    pub mount: Mount,
    /// What it requires of the caller.
    pub need: Need,
}

/// The read of a Consumer: `s`, the search interaction (SMART App Launch
/// 2.2.0, Scopes and Launch Context), which `system/Patient.rs` grants.
const SEARCH: &[Permission] = &[Permission::Search];

/// The instance read of a Consumer.
const READ: &[Permission] = &[Permission::Read];

/// A consumer route on `Patient` admitting the IUA scope `transaction`.
const fn consumer(permissions: &'static [Permission], transaction: &'static str) -> Need {
    Need::Fhir {
        permissions,
        resource_type: "Patient",
        transaction: Some(transaction),
        source: false,
    }
}

/// A Source route on `Patient` needing `permission`.
const fn source(permissions: &'static [Permission], transaction: &'static str) -> Need {
    Need::Fhir {
        permissions,
        resource_type: "Patient",
        transaction: Some(transaction),
        source: true,
    }
}

/// A subscriber route on `Subscription` needing `permission`.
const fn subscriber(permissions: &'static [Permission]) -> Need {
    Need::Fhir {
        permissions,
        resource_type: "Subscription",
        transaction: None,
        source: false,
    }
}

/// Returns one row.
const fn row(
    method: Method,
    path: &'static str,
    interaction: &'static str,
    mount: Mount,
    need: Need,
) -> Route {
    Route {
        method,
        path,
        interaction,
        mount,
        need,
    }
}

/// Every route of the FHIR face (`docs/architecture.md` §6) and of the
/// operator API (§12.4), with what it requires.
// NOTE: SMART App Launch 2.2.0, Scopes and Launch Context: "u" admits an
// update that creates, so ITI-104's conditional update needs `u` alone.
pub static ROUTES: [Route; 37] = [
    row(
        Method::GET,
        "/fhir/metadata",
        "capabilities",
        Mount::Always,
        Need::Public,
    ),
    row(
        Method::GET,
        "/fhir/.well-known/smart-configuration",
        "smart-configuration",
        Mount::Always,
        Need::Public,
    ),
    row(
        Method::GET,
        "/fhir/Patient/$ihe-pix",
        "ITI-83",
        Mount::Pixm,
        consumer(SEARCH, "ITI-83"),
    ),
    row(
        Method::POST,
        "/fhir/Patient/$ihe-pix",
        "ITI-83",
        Mount::Pixm,
        consumer(SEARCH, "ITI-83"),
    ),
    row(
        Method::PUT,
        "/fhir/Patient",
        "ITI-104",
        Mount::Pixm,
        source(&[Permission::Update], "ITI-104"),
    ),
    row(
        Method::DELETE,
        "/fhir/Patient",
        "ITI-104",
        Mount::PixmRemove,
        source(&[Permission::Delete], "ITI-104"),
    ),
    row(
        Method::GET,
        "/fhir/Patient",
        "ITI-78",
        Mount::Pdqm,
        consumer(SEARCH, "ITI-78"),
    ),
    row(
        Method::POST,
        "/fhir/Patient/_search",
        "ITI-78",
        Mount::Pdqm,
        consumer(SEARCH, "ITI-78"),
    ),
    row(
        Method::GET,
        "/fhir/Patient/{id}",
        "ITI-78",
        Mount::Pdqm,
        consumer(READ, "ITI-78"),
    ),
    row(
        Method::POST,
        "/fhir/Patient/$match",
        "ITI-119",
        Mount::Pdqm,
        consumer(SEARCH, "ITI-119"),
    ),
    // TODO(#103): hold an ITI-93 feed to the domains its sender is the Source of.
    row(
        Method::POST,
        "/fhir",
        "ITI-93",
        Mount::Pmir,
        Need::Fhir {
            permissions: &[Permission::Create, Permission::Update],
            resource_type: "Patient",
            transaction: None,
            source: false,
        },
    ),
    row(
        Method::POST,
        "/fhir/Subscription",
        "ITI-94",
        Mount::Pmir,
        subscriber(&[Permission::Create]),
    ),
    row(
        Method::GET,
        "/fhir/Subscription/{id}",
        "ITI-94",
        Mount::Pmir,
        subscriber(READ),
    ),
    row(
        Method::PUT,
        "/fhir/Subscription/{id}",
        "ITI-94",
        Mount::Pmir,
        subscriber(&[Permission::Update]),
    ),
    row(
        Method::DELETE,
        "/fhir/Subscription/{id}",
        "ITI-94",
        Mount::Pmir,
        subscriber(&[Permission::Delete]),
    ),
    row(
        Method::GET,
        "/operator/overview",
        "operator-overview",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/dependencies",
        "operator-dependencies",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/search/identifier",
        "operator-search",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/search/demographics",
        "operator-search",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/persons/{id}",
        "operator-person",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/persons/{id}/reveal",
        "operator-reveal",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/candidates",
        "operator-candidates",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/candidates/{id}",
        "operator-candidate",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/candidates/{id}/decide",
        "operator-decide",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/merges/preview",
        "operator-merge-preview",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/merges",
        "operator-merge",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/merges/{id}/revert",
        "operator-unmerge",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/domains",
        "operator-domains",
        Mount::Operator,
        Need::StewardOrAdmin,
    ),
    row(
        Method::POST,
        "/operator/domains",
        "operator-domain-create",
        Mount::Operator,
        Need::Admin,
    ),
    row(
        Method::GET,
        "/operator/domains/{id}",
        "operator-domain",
        Mount::Operator,
        Need::StewardOrAdmin,
    ),
    row(
        Method::PATCH,
        "/operator/domains/{id}",
        "operator-domain-update",
        Mount::Operator,
        Need::Admin,
    ),
    row(
        Method::GET,
        "/operator/pseudonym-domains",
        "operator-pseudonym-domains",
        Mount::Operator,
        Need::StewardOrAdmin,
    ),
    row(
        Method::POST,
        "/operator/pseudonym-domains",
        "operator-pseudonym-domain-create",
        Mount::Operator,
        Need::Admin,
    ),
    row(
        Method::POST,
        "/operator/pseudonyms/resolve",
        "operator-pseudonym-resolve",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::GET,
        "/operator/matching",
        "operator-matching",
        Mount::Operator,
        Need::StewardOrAdmin,
    ),
    row(
        Method::GET,
        "/operator/actions",
        "operator-actions",
        Mount::Operator,
        Need::Steward,
    ),
    row(
        Method::POST,
        "/operator/actions/export",
        "operator-actions-export",
        Mount::Operator,
        Need::Steward,
    ),
];

/// Returns the row of `method` on the route `template` under `{base}`.
#[must_use]
pub fn find(method: &Method, template: &str) -> Option<&'static Route> {
    ROUTES
        .iter()
        .find(|route| route.method == *method && route.path == template)
}

/// Returns whether `path` under `{base}` is on the guarded surface: the FHIR
/// face or the operator API.
#[must_use]
pub fn is_guarded(path: &str) -> bool {
    [FHIR, OPERATOR].iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

impl Need {
    /// Returns what the gate checks for this need, with the product scopes
    /// `steward` and `admin`.
    ///
    /// The `Patient` compartment a `patient/` scope reaches and the domain a
    /// Source writes to are checked behind the gate (`crate::auth::guard`).
    #[must_use]
    pub fn requirement(self, steward: &str, admin: &str) -> Requirement {
        match self {
            Self::Public => Requirement::Public,
            Self::Fhir {
                permissions,
                resource_type,
                transaction,
                ..
            } => {
                let granted = permissions.iter().map(|permission| Requirement::Fhir {
                    permission: *permission,
                    resource_type: resource_type.to_owned(),
                });
                let iua = transaction.map(|scope| Requirement::Scope(scope.to_owned()));
                granted
                    .chain(iua)
                    .reduce(|left, right| Requirement::Either(Box::new(left), Box::new(right)))
                    .unwrap_or(Requirement::Refused)
            }
            Self::Steward => Requirement::Scope(steward.to_owned()),
            Self::Admin => Requirement::Scope(admin.to_owned()),
            Self::StewardOrAdmin => Requirement::Either(
                Box::new(Requirement::Scope(steward.to_owned())),
                Box::new(Requirement::Scope(admin.to_owned())),
            ),
        }
    }
}

/// Returns every scope the table enforces, in the order of first use, with
/// the product scopes `steward` and `admin`.
#[must_use]
pub fn scopes(steward: &str, admin: &str) -> Vec<String> {
    let mut scopes: Vec<String> = Vec::new();
    let mut add = |scope: String| {
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    };
    add(String::from("system/Patient.rs"));
    add(String::from("system/Patient.cud"));
    add(String::from("system/Subscription.cruds"));
    for route in &ROUTES {
        if let Need::Fhir {
            transaction: Some(transaction),
            ..
        } = route.need
        {
            add(transaction.to_owned());
        }
    }
    add(steward.to_owned());
    add(admin.to_owned());
    scopes
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use ferrohealth_smart::resource::{Access, Caller, Honoured};
    use http::Method;

    use super::{Need, ROUTES, find, is_guarded, scopes};

    fn access(scope: &str) -> Access {
        Access::Caller(Arc::new(Caller::new(
            String::from("synthetic-sub"),
            Some(String::from("synthetic-client")),
            scope,
            None,
            false,
        )))
    }

    fn admits(need: Need, scope: &str) -> bool {
        need.requirement("__ferropix.steward", "__ferropix.admin")
            .met_by(&access(scope), Honoured::default())
    }

    #[test]
    fn every_row_is_one_method_on_one_template_under_a_guarded_prefix() {
        let mut seen = BTreeSet::new();
        for route in &ROUTES {
            assert!(
                seen.insert((route.method.as_str(), route.path)),
                "{} {} is listed twice",
                route.method,
                route.path
            );
            assert!(is_guarded(route.path), "{}", route.path);
            assert_eq!(Some(route), find(&route.method, route.path));
        }
        assert!(!is_guarded("/health/readiness"));
        assert!(!is_guarded("/fhirx"));
        assert!(is_guarded("/fhir"));
    }

    #[test]
    fn a_consumer_reads_with_system_patient_rs_or_the_iua_scope_of_its_transaction() {
        let pix = find(&Method::POST, "/fhir/Patient/$ihe-pix")
            .expect("ITI-83")
            .need;
        assert!(admits(pix, "system/Patient.rs"));
        assert!(admits(pix, "system/*.read"));
        assert!(admits(pix, "ITI-83"));
        assert!(!admits(pix, "ITI-78"));
        assert!(!admits(pix, "system/Patient.cud"));
        assert!(!admits(pix, "__ferropix.steward"));
        let read = find(&Method::GET, "/fhir/Patient/{id}")
            .expect("ITI-78 read")
            .need;
        assert!(admits(read, "system/Patient.r"));
        assert!(!admits(read, "system/Patient.s"));
    }

    #[test]
    fn a_source_writes_with_system_patient_cud_and_a_subscriber_with_subscription() {
        let feed = find(&Method::PUT, "/fhir/Patient").expect("ITI-104").need;
        assert!(admits(feed, "system/Patient.cud"));
        assert!(admits(feed, "system/Patient.u"));
        assert!(admits(feed, "ITI-104"));
        assert!(!admits(feed, "system/Patient.rs"));
        let remove = find(&Method::DELETE, "/fhir/Patient")
            .expect("ITI-104 remove")
            .need;
        assert!(admits(remove, "system/Patient.d"));
        assert!(!admits(remove, "system/Patient.u"));
        let subscribe = find(&Method::POST, "/fhir/Subscription")
            .expect("ITI-94")
            .need;
        assert!(admits(subscribe, "system/Subscription.cruds"));
        assert!(!admits(subscribe, "system/Patient.cruds"));
    }

    #[test]
    fn the_operator_api_takes_the_steward_scope_and_its_writes_of_domains_the_admin_scope() {
        let person = find(&Method::GET, "/operator/persons/{id}")
            .expect("person")
            .need;
        assert!(admits(person, "__ferropix.steward"));
        assert!(!admits(person, "__ferropix.admin"));
        assert!(!admits(person, "system/*.cruds"));
        let write = find(&Method::POST, "/operator/domains")
            .expect("domain write")
            .need;
        assert!(admits(write, "__ferropix.admin"));
        assert!(!admits(write, "__ferropix.steward"));
        let list = find(&Method::GET, "/operator/domains")
            .expect("domains")
            .need;
        assert!(admits(list, "__ferropix.admin"));
        assert!(admits(list, "__ferropix.steward"));
    }

    #[test]
    fn the_supported_scopes_name_every_scope_the_table_checks_once() {
        let listed = scopes("__ferropix.steward", "__ferropix.admin");
        for scope in [
            "system/Patient.rs",
            "system/Patient.cud",
            "system/Subscription.cruds",
            "ITI-83",
            "ITI-104",
            "ITI-78",
            "ITI-119",
            "__ferropix.steward",
            "__ferropix.admin",
        ] {
            assert!(listed.iter().any(|listed| listed == scope), "{scope}");
        }
        let unique: BTreeSet<&String> = listed.iter().collect();
        assert_eq!(unique.len(), listed.len(), "{listed:?}");
    }
}
