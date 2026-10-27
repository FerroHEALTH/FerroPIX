// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What FerroPIX puts into the usage report of `ferrohealth-usage-report`.
//!
//! The crate carries `FerroPULSE`'s report envelope v2, the reporter, the
//! instance id and the window. FerroPIX adds its `status` ([`Status`]: the
//! profile, the services switched on, the identifier domains as a range and
//! the readiness of the store and the audit spool), its request groups by
//! matched route template ([`Group`]), and the HL7 v2 messages
//! ([`record_message`]). A report carries no identifier other than the
//! instance id, no domain system or name, no host name and no request path.
//! No specification governs any of this: our own design.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ferrobridge_hl7v2::ack::Code;
use ferrohealth_health::document::State;
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use ferrohealth_server::health::{LIVENESS, READINESS, SUMMARY};
use ferrohealth_usage_report::envelope::{Key, LicenceGrant, Product, report_git_sha};
use ferrohealth_usage_report::reporter::{Identity, Notice, ProductSection};
use ferrohealth_usage_report::section::UsageReportSection;
use ferrohealth_usage_report::window::Window;
use http::Method;
use serde::Serialize;
use tokio::task::JoinHandle;

use crate::auth::table::{self, Mount};
use crate::config::Profile;
use crate::http::METRICS;

/// What the boot line says about the report.
pub const NOTICE: Notice = Notice {
    contents: "instance id, version, licence type, deployment, uptime, host size, profile, the \
               services switched on, the identifier domains as a range, the readiness of the \
               store and the audit spool, and coarse request latency per route group",
    switch: "FERROPIX__USAGE_REPORT__ENABLED=false",
    state_dir_variable: "FERROPIX__USAGE_REPORT__STATE_DIR",
    url: "https://ferropix.eu/docs/operate/configuration.html#usage_report",
};

/// The commit this binary was built from, as the build recorded it.
const COMMIT: &str = env!("FERROPIX_BUILD_COMMIT");

/// The readiness indicator the `db_ok` member reads.
pub const DATABASE_INDICATOR: &str = "database";

/// The readiness indicator the `audit_spool_ok` member reads.
pub const AUDIT_SPOOL_INDICATOR: &str = "audit_spool";

/// The counter of HL7 v2 messages answered `AE`.
pub const HL7V2_AE: &str = "hl7v2_ae";

/// The counter of HL7 v2 messages and frames answered `AR`.
pub const HL7V2_AR: &str = "hl7v2_ar";

/// The identity of this build.
///
/// The server installs no commercial licence, so it runs under what the
/// Business Source License grants everyone, the non-commercial grant.
#[must_use]
pub fn identity() -> Identity {
    Identity {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        git_sha: report_git_sha(COMMIT),
        licence: LicenceGrant::NonCommercial,
    }
}

/// Logs the boot line and, when the report runs, starts its detached task
/// over `product`, reading the daily metrics from `window`.
///
/// `ci` is the CI variable `ferrohealth_usage_report::section::ci_of` found
/// in the process environment, if any.
pub async fn start(
    section: &UsageReportSection,
    product: Pix,
    window: Arc<Window>,
    started: Instant,
    ci: Option<&'static str>,
) -> Option<JoinHandle<()>> {
    ferrohealth_usage_report::reporter::start(
        section,
        &NOTICE,
        identity(),
        Arc::new(product),
        window,
        started,
        ci,
    )
    .await
}

/// The window of one process whose routes are mounted under `base`, the
/// `[server] base_path` (empty for the root), grouping requests with
/// [`Group::of`].
#[must_use]
pub fn window(base: &str) -> Window {
    let base = base.to_owned();
    Window::new(move |method, template| {
        let under_base = template.map(|template| {
            template
                .strip_prefix(base.as_str())
                .filter(|rest| rest.starts_with('/'))
                .unwrap_or(template)
        });
        Group::of(method, under_base).and_then(|group| Key::new(group.key()))
    })
}

/// Records one HL7 v2 message that took `elapsed` and was answered with
/// `code`, `None` for one left unanswered.
///
/// Its latency goes in the `hl7v2` group and an `AE` or `AR` in its counter;
/// nothing of the message reaches the window.
pub fn record_message(window: &Window, elapsed: Duration, code: Option<Code>) {
    let counter = match code {
        Some(Code::Error) => Some(HL7V2_AE),
        Some(Code::Reject) => Some(HL7V2_AR),
        Some(Code::Accept) | None => None,
    };
    if let Some(key) = counter.and_then(Key::new) {
        window.count(key);
    }
    if let Some(key) = Key::new(Group::Hl7v2.key()) {
        window.record_work(key, elapsed);
    }
}

/// A group of requests, by the route template an HTTP request matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// A message of the HL7 v2 face, from its frame to its answer.
    Hl7v2,
    /// The capability statement and the SMART discovery document.
    Metadata,
    /// Every route of the operator API.
    Operator,
    /// Every other request outside the health and metrics routes, an
    /// unmatched one included.
    Other,
    /// ITI-119, `Patient/$match`.
    PdqMatch,
    /// ITI-78, the `Patient` search and read.
    PdqQuery,
    /// ITI-104, the Patient Identity Feed.
    PixFeed,
    /// ITI-83, `Patient/$ihe-pix`.
    PixQuery,
    /// ITI-93, the Mobile Patient Identity Feed.
    PmirFeed,
    /// ITI-94, the `Subscription` interactions.
    PmirSubscription,
}

impl Group {
    /// Returns the group of a request from its method and the route template
    /// it matched under `{base}`, or `None` for a request the report does not
    /// count.
    ///
    /// `template` is `None` for a request no route matched, which is `other`.
    /// The health routes are probes and the metrics route a scrape, and
    /// neither is counted. The group is the transaction the permission table
    /// names for the route, so a template's parameter segments are never
    /// read.
    #[must_use]
    pub fn of(method: &Method, template: Option<&str>) -> Option<Self> {
        let Some(template) = template else {
            return Some(Self::Other);
        };
        if [LIVENESS, READINESS, SUMMARY, METRICS].contains(&template) {
            return None;
        }
        let Some(route) = table::ROUTES
            .iter()
            .find(|route| route.method == *method && route.path == template)
        else {
            return Some(Self::Other);
        };
        let group = match (route.mount, route.interaction) {
            (Mount::Operator, _) => Self::Operator,
            (_, "capabilities" | "smart-configuration") => Self::Metadata,
            (_, "ITI-83") => Self::PixQuery,
            (_, "ITI-104") => Self::PixFeed,
            (_, "ITI-78") => Self::PdqQuery,
            (_, "ITI-119") => Self::PdqMatch,
            (_, "ITI-93") => Self::PmirFeed,
            (_, "ITI-94") => Self::PmirSubscription,
            _ => Self::Other,
        };
        Some(group)
    }

    /// Returns the group's name in the report.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Hl7v2 => "hl7v2",
            Self::Metadata => "metadata",
            Self::Operator => "operator",
            Self::Other => "other",
            Self::PdqMatch => "pdq_match",
            Self::PdqQuery => "pdq_query",
            Self::PixFeed => "pix_feed",
            Self::PixQuery => "pix_query",
            Self::PmirFeed => "pmir_feed",
            Self::PmirSubscription => "pmir_subscription",
        }
    }
}

/// The identifier domains the store holds, bucketed, so a report never states
/// how many a deployment cross-references exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DomainsBucket {
    /// None.
    #[serde(rename = "0")]
    None,
    /// One or two.
    #[serde(rename = "1-2")]
    UpTo2,
    /// Three to five.
    #[serde(rename = "3-5")]
    UpTo5,
    /// Six to ten.
    #[serde(rename = "6-10")]
    UpTo10,
    /// Eleven to fifty.
    #[serde(rename = "11-50")]
    UpTo50,
    /// More than fifty.
    #[serde(rename = "51+")]
    Above50,
}

impl DomainsBucket {
    /// Returns the bucket holding `domains`.
    #[must_use]
    pub const fn of(domains: usize) -> Self {
        match domains {
            0 => Self::None,
            1..=2 => Self::UpTo2,
            3..=5 => Self::UpTo5,
            6..=10 => Self::UpTo10,
            11..=50 => Self::UpTo50,
            _ => Self::Above50,
        }
    }
}

/// A service the configuration switches on, the only way a report names what
/// runs.
///
/// The declaration order is the order of the serialized names, so a sorted
/// list serializes sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    /// The HL7 v2 face (`[hl7v2] enabled`).
    Hl7v2,
    /// The operator API (`[operator] enabled`).
    Operator,
    /// The PDQm Supplier (`[pdqm] enabled`).
    Pdqm,
    /// The PIXm Manager (`[pixm] enabled`).
    Pixm,
    /// The PMIR Registry (`[pmir] enabled`).
    Pmir,
}

/// FerroPIX's `status` object, carried by both events.
///
/// An absent readiness member is an indicator this process does not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// The deployment profile.
    pub profile: Profile,
    /// The services the configuration switches on, unique and sorted.
    pub services: Vec<Service>,
    /// The identifier domains the store holds, bucketed.
    pub domains: DomainsBucket,
    /// Whether the store answered its readiness check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_ok: Option<bool>,
    /// Whether the audit spool had headroom at its readiness check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_spool_ok: Option<bool>,
}

/// The state facts the configuration and the store fix at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The deployment profile.
    pub profile: Profile,
    /// The services the configuration switches on, unique and sorted.
    pub services: Vec<Service>,
    /// The identifier domains the store holds, bucketed.
    pub domains: DomainsBucket,
}

impl Facts {
    /// Returns the facts of `profile` with `services` switched on and
    /// `domains` identifier domains in the store.
    #[must_use]
    pub fn new(profile: Profile, mut services: Vec<Service>, domains: usize) -> Self {
        services.sort_unstable();
        services.dedup();
        Self {
            profile,
            services,
            domains: DomainsBucket::of(domains),
        }
    }
}

/// What the report reads from the running server: the facts fixed at start
/// and the readiness of the store and the audit spool at each report.
#[derive(Debug, Clone)]
pub struct Pix {
    /// The facts fixed at start.
    pub facts: Facts,
    /// The readiness registry, read at each report.
    pub health: Registry,
    /// The server's lifecycle, which the readiness document names.
    pub lifecycle: Lifecycle,
}

impl ProductSection for Pix {
    type Status = Status;

    const PRODUCT: Product = Product::Pix;

    async fn status(&self) -> Status {
        let report = self.health.readiness(&self.lifecycle).await;
        let up = |name: &str| {
            report
                .indicators
                .iter()
                .find(|indicator| indicator.name == name)
                .map(|indicator| indicator.state == State::Up)
        };
        Status {
            profile: self.facts.profile,
            services: self.facts.services.clone(),
            domains: self.facts.domains,
            db_ok: up(DATABASE_INDICATOR),
            audit_spool_ok: up(AUDIT_SPOOL_INDICATOR),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ferrobridge_hl7v2::ack::Code;
    use ferrohealth_usage_report::envelope::Key;
    use http::{Method, StatusCode};

    use super::{DomainsBucket, Group, HL7V2_AE, HL7V2_AR, record_message, window};
    use crate::auth::table::ROUTES;

    #[test]
    fn every_route_of_the_permission_table_has_its_group() {
        let cases = [
            (Method::GET, "/fhir/metadata", Group::Metadata),
            (
                Method::GET,
                "/fhir/.well-known/smart-configuration",
                Group::Metadata,
            ),
            (Method::GET, "/fhir/Patient/$ihe-pix", Group::PixQuery),
            (Method::POST, "/fhir/Patient/$ihe-pix", Group::PixQuery),
            (Method::PUT, "/fhir/Patient", Group::PixFeed),
            (Method::DELETE, "/fhir/Patient", Group::PixFeed),
            (Method::GET, "/fhir/Patient", Group::PdqQuery),
            (Method::POST, "/fhir/Patient/_search", Group::PdqQuery),
            (Method::GET, "/fhir/Patient/{id}", Group::PdqQuery),
            (Method::POST, "/fhir/Patient/$match", Group::PdqMatch),
            (Method::POST, "/fhir", Group::PmirFeed),
            (Method::POST, "/fhir/Subscription", Group::PmirSubscription),
            (
                Method::DELETE,
                "/fhir/Subscription/{id}",
                Group::PmirSubscription,
            ),
            (Method::GET, "/operator/overview", Group::Operator),
            (
                Method::POST,
                "/operator/candidates/{id}/decide",
                Group::Operator,
            ),
            (Method::PATCH, "/fhir/Patient", Group::Other),
        ];
        for (method, template, group) in cases {
            assert_eq!(
                Group::of(&method, Some(template)),
                Some(group),
                "{method} {template}"
            );
        }
        assert_eq!(Group::of(&Method::GET, None), Some(Group::Other));
        for route in &ROUTES {
            assert_ne!(
                Group::of(&route.method, Some(route.path)),
                Some(Group::Other),
                "{} {} has a group of its own",
                route.method,
                route.path
            );
        }
    }

    #[test]
    fn the_probes_and_the_scrape_are_not_counted() {
        for template in [
            "/health",
            "/health/liveness",
            "/health/readiness",
            "/metrics",
        ] {
            assert_eq!(Group::of(&Method::GET, Some(template)), None, "{template}");
        }
        let window = window("/pix");
        window.record(
            &Method::GET,
            Some("/pix/health/readiness"),
            StatusCode::OK,
            Duration::ZERO,
        );
        assert_eq!(window.take().requests(), 0);
    }

    #[test]
    fn a_template_is_read_under_the_base_path() {
        let window = window("/pix");
        window.record(
            &Method::GET,
            Some("/pix/fhir/Patient/$ihe-pix"),
            StatusCode::OK,
            Duration::from_millis(2),
        );
        window.record(
            &Method::GET,
            Some("/pixel/fhir/metadata"),
            StatusCode::OK,
            Duration::from_millis(2),
        );
        let counts = window.take();
        assert_eq!(
            counts.groups.keys().map(Key::as_str).collect::<Vec<_>>(),
            ["other", "pix_query"]
        );
    }

    #[test]
    fn an_hl7v2_message_is_timed_and_a_refusal_counted() {
        let window = window("");
        record_message(&window, Duration::from_millis(3), Some(Code::Accept));
        record_message(&window, Duration::from_millis(3), Some(Code::Error));
        record_message(&window, Duration::from_millis(3), Some(Code::Reject));
        let counts = window.take();
        assert_eq!(
            Key::new(Group::Hl7v2.key())
                .and_then(|key| counts.groups.get(&key))
                .map(ferrohealth_usage_report::window::GroupCounts::total),
            Some(3)
        );
        for counter in [HL7V2_AE, HL7V2_AR] {
            assert_eq!(
                Key::new(counter).and_then(|key| counts.counters.get(&key).copied()),
                Some(1),
                "{counter}"
            );
        }
    }

    #[test]
    fn the_domains_are_a_range() {
        assert_eq!(DomainsBucket::of(0), DomainsBucket::None);
        assert_eq!(DomainsBucket::of(2), DomainsBucket::UpTo2);
        assert_eq!(DomainsBucket::of(3), DomainsBucket::UpTo5);
        assert_eq!(DomainsBucket::of(51), DomainsBucket::Above50);
    }
}
