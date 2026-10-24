// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The report payload: `FerroPULSE`'s envelope v2 with FerroPIX's `status`
//! and `metrics` objects.
//!
//! The field set, the value sets and the bounds are `FerroPULSE`'s report v2
//! contract (`docs/report-v2.md` §Envelope, §`status`, §`metrics`), and
//! FerroPIX's keys are its §FerroPIX. Every type here is closed: a member
//! that is not declared cannot be serialized, a key is a variant name and
//! never a string from a request or the store, and the `start` and `daily`
//! variants carry exactly the members their event admits. No specification
//! governs the report: our own design.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};

use crate::config::Profile;
use crate::config::section::usage_report::Deployment;

/// The number of latency buckets in a histogram.
pub const BUCKETS: usize = 11;

/// The upper bounds of the finite latency buckets, in milliseconds; the
/// eleventh bucket is `+Inf`.
pub const BUCKET_BOUNDS_MS: [u32; BUCKETS - 1] = [5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000];

/// The largest count a report may carry.
pub const MAX_COUNT: u64 = 10_000_000;

/// The largest latency a report may carry, in milliseconds.
pub const MAX_MS: f64 = 600_000.0;

/// The largest uptime a report may carry: ten years of seconds.
pub const MAX_UPTIME_S: u64 = 315_576_000;

/// The payload schema version; serializes as the integer `2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaV2;

impl Serialize for SchemaV2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(2)
    }
}

/// The reporting product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Product {
    /// This Master Patient Index.
    #[serde(rename = "ferropix")]
    FerroPix,
}

/// The licence grant in force, as its type alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenceGrant {
    /// What the Business Source License grants everyone.
    NonCommercial,
    /// A commercial licence.
    Commercial,
}

/// The CPUs available to the process, bucketed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CpuBucket {
    /// One or two.
    #[serde(rename = "1-2")]
    UpTo2,
    /// Three or four.
    #[serde(rename = "3-4")]
    UpTo4,
    /// Five to eight.
    #[serde(rename = "5-8")]
    UpTo8,
    /// Nine to sixteen.
    #[serde(rename = "9-16")]
    UpTo16,
    /// Seventeen or more.
    #[serde(rename = "17+")]
    Above16,
}

impl CpuBucket {
    /// Returns the bucket holding `cpus`.
    #[must_use]
    pub const fn of(cpus: usize) -> Self {
        match cpus {
            0..=2 => Self::UpTo2,
            3..=4 => Self::UpTo4,
            5..=8 => Self::UpTo8,
            9..=16 => Self::UpTo16,
            _ => Self::Above16,
        }
    }
}

/// The memory available to the process in GiB, bucketed by half-open ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MemoryBucket {
    /// Under 4 GiB.
    #[serde(rename = "<4")]
    Below4,
    /// 4 GiB to under 8.
    #[serde(rename = "4-8")]
    Below8,
    /// 8 GiB to under 16.
    #[serde(rename = "8-16")]
    Below16,
    /// 16 GiB to under 32.
    #[serde(rename = "16-32")]
    Below32,
    /// 32 GiB or more.
    #[serde(rename = "32+")]
    AtLeast32,
}

impl MemoryBucket {
    /// Returns the bucket holding `bytes`.
    #[must_use]
    pub const fn of(bytes: u64) -> Self {
        match bytes >> 30 {
            0..=3 => Self::Below4,
            4..=7 => Self::Below8,
            8..=15 => Self::Below16,
            16..=31 => Self::Below32,
            _ => Self::AtLeast32,
        }
    }
}

/// The requests counted in the window, bucketed by half-open ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RequestsBucket {
    /// None.
    #[serde(rename = "0")]
    None,
    /// 1 to 99.
    #[serde(rename = "1-100")]
    Below100,
    /// 100 to 999.
    #[serde(rename = "100-1k")]
    Below1k,
    /// 1,000 to 9,999.
    #[serde(rename = "1k-10k")]
    Below10k,
    /// 10,000 to 99,999.
    #[serde(rename = "10k-100k")]
    Below100k,
    /// 100,000 or more.
    #[serde(rename = "100k+")]
    AtLeast100k,
}

impl RequestsBucket {
    /// Returns the bucket holding `requests`.
    #[must_use]
    pub const fn of(requests: u64) -> Self {
        match requests {
            0 => Self::None,
            1..=99 => Self::Below100,
            100..=999 => Self::Below1k,
            1_000..=9_999 => Self::Below10k,
            10_000..=99_999 => Self::Below100k,
            _ => Self::AtLeast100k,
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

/// A group of requests, by the route template an HTTP request matched.
///
/// The declaration order is the order of the serialized names, so the
/// report's `groups` object is written in name order.
// TODO(#85): add the HL7 v2 message group and the acknowledgment counters once the MLLP listener lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// The capability statement and the SMART discovery document.
    Metadata,
    /// Every route of the operator API.
    Operator,
    /// Every other request outside the health routes, an unmatched one
    /// included.
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

/// One group's latency and server errors over the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupStats {
    /// The median latency, in milliseconds.
    pub p50_ms: f64,
    /// The 95th percentile latency, in milliseconds.
    pub p95_ms: f64,
    /// The 99th percentile latency, in milliseconds.
    pub p99_ms: f64,
    /// Responses with a 5xx status.
    pub errors_5xx: u64,
    /// The request count per latency bucket.
    pub histogram: [u64; BUCKETS],
}

/// The counters of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counters {
    /// Responses with a 4xx status, over every counted group.
    pub responses_4xx: u64,
}

/// The window a daily report covers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window24h {
    /// The counted requests, bucketed.
    pub requests_bucket: RequestsBucket,
    /// Per group with requests in the window.
    pub groups: BTreeMap<Group, GroupStats>,
    /// The event counts.
    pub counters: Counters,
}

/// FerroPIX's `metrics` object, carried by the daily report only.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Metrics {
    /// The window since the previous daily report of this process.
    pub window_24h: Window24h,
}

/// The event a report records, with the members only that event carries.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Event {
    /// A process start.
    Start {
        /// Whether this start created the instance id.
        first_start: bool,
    },
    /// The daily report.
    Daily {
        /// The window's metrics.
        metrics: Box<Metrics>,
    },
}

/// One report, exactly as it is sent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Always `2`.
    pub schema: SchemaV2,
    /// The event and its event-specific members.
    #[serde(flatten)]
    pub event: Event,
    /// Always `ferropix`.
    pub product: Product,
    /// The random installation id, hyphenated.
    pub instance_id: String,
    /// The server version.
    pub version: String,
    /// The commit the binary was built from.
    pub git_sha: String,
    /// The licence grant type.
    pub licence: LicenceGrant,
    /// How the server was deployed.
    pub deployment: Deployment,
    /// Seconds since this process started.
    pub uptime_s: u64,
    /// The CPUs available, bucketed.
    pub cpu_bucket: CpuBucket,
    /// The memory available, bucketed.
    pub memory_bucket: MemoryBucket,
    /// The server's state facts.
    pub status: Status,
}

/// The `git_sha` a build reports: the commit when the build recorded one in
/// the contract's form (7 to 40 lower-case hex digits), else seven zeros.
///
/// The contract has no spelling for an unknown commit, so a build outside a
/// checkout reports the all-zero commit.
#[must_use]
pub fn report_git_sha(revision: &str) -> String {
    let valid = (7..=40).contains(&revision.len())
        && revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if valid {
        revision.to_owned()
    } else {
        String::from("0000000")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CpuBucket, DomainsBucket, Group, MemoryBucket, RequestsBucket, Service, report_git_sha,
    };

    #[test]
    fn buckets_are_half_open() {
        assert_eq!(RequestsBucket::of(0), RequestsBucket::None);
        assert_eq!(RequestsBucket::of(99), RequestsBucket::Below100);
        assert_eq!(RequestsBucket::of(100), RequestsBucket::Below1k);
        assert_eq!(RequestsBucket::of(100_000), RequestsBucket::AtLeast100k);
        assert_eq!(CpuBucket::of(1), CpuBucket::UpTo2);
        assert_eq!(CpuBucket::of(16), CpuBucket::UpTo16);
        assert_eq!(CpuBucket::of(17), CpuBucket::Above16);
        let gib = 1_u64 << 30;
        assert_eq!(MemoryBucket::of(4 * gib - 1), MemoryBucket::Below4);
        assert_eq!(MemoryBucket::of(4 * gib), MemoryBucket::Below8);
        assert_eq!(MemoryBucket::of(32 * gib), MemoryBucket::AtLeast32);
        assert_eq!(DomainsBucket::of(0), DomainsBucket::None);
        assert_eq!(DomainsBucket::of(2), DomainsBucket::UpTo2);
        assert_eq!(DomainsBucket::of(3), DomainsBucket::UpTo5);
        assert_eq!(DomainsBucket::of(10), DomainsBucket::UpTo10);
        assert_eq!(DomainsBucket::of(50), DomainsBucket::UpTo50);
        assert_eq!(DomainsBucket::of(51), DomainsBucket::Above50);
    }

    #[test]
    fn a_git_sha_outside_the_contract_reports_zeros() {
        assert_eq!(report_git_sha("1a2b3c4d5e6f"), "1a2b3c4d5e6f");
        assert_eq!(report_git_sha("unknown"), "0000000");
        assert_eq!(report_git_sha("1A2B3C4"), "0000000");
        assert_eq!(report_git_sha("abc"), "0000000");
    }

    #[test]
    fn the_declaration_order_of_every_key_enum_is_its_name_order() {
        fn names<T: serde::Serialize>(values: &[T]) -> Vec<String> {
            values
                .iter()
                .map(|value| {
                    serde_json::to_string(value)
                        .expect("serializes")
                        .trim_matches('"')
                        .to_owned()
                })
                .collect()
        }
        let services = names(&[
            Service::Hl7v2,
            Service::Operator,
            Service::Pdqm,
            Service::Pixm,
            Service::Pmir,
        ]);
        let groups = names(&[
            Group::Metadata,
            Group::Operator,
            Group::Other,
            Group::PdqMatch,
            Group::PdqQuery,
            Group::PixFeed,
            Group::PixQuery,
            Group::PmirFeed,
            Group::PmirSubscription,
        ]);
        for list in [services, groups] {
            let mut sorted = list.clone();
            sorted.sort();
            assert_eq!(list, sorted);
        }
    }
}
