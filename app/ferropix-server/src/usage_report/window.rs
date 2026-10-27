// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The usage report's metrics window.
//!
//! It holds a latency histogram and the server errors per request group and
//! the client refusals, recorded by the request log. A recording carries the
//! method, the route TEMPLATE the request matched (never the path a client
//! sent), the status and the latency; nothing of a request's query, body,
//! caller or domain reaches it, so nothing of them can reach a report. No
//! specification governs this: our own design.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use ferrobridge_hl7v2::ack::Code;
use ferrohealth_server::health::{LIVENESS, READINESS, SUMMARY};
use ferrohealth_telemetry::request_log::{Completed, Recorder};
use http::{Method, StatusCode};

use crate::auth::table::{self, Mount};
use crate::http::METRICS;
use crate::usage_report::payload::{
    BUCKET_BOUNDS_MS, BUCKETS, Counters, Group, GroupStats, MAX_COUNT, MAX_MS, Metrics,
    RequestsBucket, Window24h,
};

impl Group {
    /// Returns the group of a request from its method and the route template
    /// it matched under `{base}`, or `None` for a request the report does not
    /// count.
    ///
    /// `template` is `None` for a request no route matched, which is `other`.
    /// The health routes are probes and the metrics route a scrape, and
    /// neither is counted. The group is the
    /// transaction the permission table names for the route, so a template's
    /// parameter segments are never read.
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
}

/// One group's counts over a window.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GroupCounts {
    /// The count per latency bucket.
    pub buckets: [u64; BUCKETS],
    /// Responses with a 5xx status.
    pub errors_5xx: u64,
    /// The largest latency recorded, in milliseconds.
    pub max_ms: f64,
}

impl GroupCounts {
    /// Returns the number of recordings.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.buckets
            .iter()
            .fold(0_u64, |sum, count| sum.saturating_add(*count))
    }

    /// Returns the `q` quantile in milliseconds, interpolated linearly inside
    /// its bucket; the `+Inf` bucket reaches up to the largest recording.
    #[must_use]
    pub fn quantile_ms(&self, q: f64) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        let rank = q.clamp(0.0, 1.0) * to_f64(total);
        let mut cumulative = 0_u64;
        for (index, count) in self.buckets.iter().copied().enumerate() {
            if count == 0 {
                continue;
            }
            let before = cumulative;
            cumulative = cumulative.saturating_add(count);
            if to_f64(cumulative) >= rank {
                let lower = lower_bound_ms(index);
                let upper = upper_bound_ms(index, self.max_ms).max(lower);
                let fraction = ((rank - to_f64(before)) / to_f64(count)).clamp(0.0, 1.0);
                return report_ms((upper - lower).mul_add(fraction, lower));
            }
        }
        report_ms(self.max_ms)
    }

    /// Returns the group as the report carries it, or `None` without
    /// requests.
    #[must_use]
    pub fn stats(&self) -> Option<GroupStats> {
        (self.total() > 0).then(|| GroupStats {
            p50_ms: self.quantile_ms(0.50),
            p95_ms: self.quantile_ms(0.95),
            p99_ms: self.quantile_ms(0.99),
            errors_5xx: self.errors_5xx.min(MAX_COUNT),
            histogram: self.buckets.map(|count| count.min(MAX_COUNT)),
        })
    }

    /// Counts one recording that took `elapsed`.
    fn time(&mut self, elapsed: Duration) {
        let ms = elapsed.as_secs_f64() * 1000.0;
        if let Some(slot) = self.buckets.get_mut(bucket_of(ms)) {
            *slot = slot.saturating_add(1);
        }
        self.max_ms = self.max_ms.max(ms);
    }

    /// Adds `other` to these counts.
    fn absorb(&mut self, other: &Self) {
        for (mine, theirs) in self.buckets.iter_mut().zip(other.buckets) {
            *mine = mine.saturating_add(theirs);
        }
        self.errors_5xx = self.errors_5xx.saturating_add(other.errors_5xx);
        self.max_ms = self.max_ms.max(other.max_ms);
    }
}

/// A window's counts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Counts {
    /// The counts of each group with a recording.
    pub groups: BTreeMap<Group, GroupCounts>,
    /// Responses with a 4xx status, over every counted group.
    pub responses_4xx: u64,
    /// HL7 v2 messages answered `AE`.
    pub hl7v2_ae: u64,
    /// HL7 v2 messages and frames answered `AR`.
    pub hl7v2_ar: u64,
}

impl Counts {
    /// Returns the number of counted requests.
    #[must_use]
    pub fn requests(&self) -> u64 {
        self.groups
            .values()
            .fold(0_u64, |sum, group| sum.saturating_add(group.total()))
    }

    /// Renders the report's `metrics` object.
    #[must_use]
    pub fn to_metrics(&self) -> Metrics {
        Metrics {
            window_24h: Window24h {
                requests_bucket: RequestsBucket::of(self.requests()),
                groups: self
                    .groups
                    .iter()
                    .filter_map(|(group, counts)| counts.stats().map(|stats| (*group, stats)))
                    .collect(),
                counters: Counters {
                    responses_4xx: self.responses_4xx.min(MAX_COUNT),
                    hl7v2_ae: self.hl7v2_ae.min(MAX_COUNT),
                    hl7v2_ar: self.hl7v2_ar.min(MAX_COUNT),
                },
            },
        }
    }

    /// Adds `other` to these counts.
    fn absorb(&mut self, other: &Self) {
        for (group, counts) in &other.groups {
            self.groups.entry(*group).or_default().absorb(counts);
        }
        self.responses_4xx = self.responses_4xx.saturating_add(other.responses_4xx);
        self.hl7v2_ae = self.hl7v2_ae.saturating_add(other.hl7v2_ae);
        self.hl7v2_ar = self.hl7v2_ar.saturating_add(other.hl7v2_ar);
    }
}

/// The in-process window the request log records into and the daily report
/// takes.
///
/// The request log finds it as a request extension, so a router built
/// without one records nothing.
#[derive(Debug)]
pub struct Window {
    base: String,
    counts: Mutex<Counts>,
}

/// The request log of `ferrohealth-telemetry` records every finished request
/// into the window.
impl Recorder for Window {
    fn record(&self, completed: &Completed<'_>) {
        Self::record(
            self,
            completed.method,
            completed.route,
            completed.status,
            completed.latency,
        );
    }
}

impl Window {
    /// Returns an empty window for routes mounted under `base`, the
    /// `[server] base_path` (empty for the root).
    #[must_use]
    pub fn new(base: &str) -> Self {
        Self {
            base: base.to_owned(),
            counts: Mutex::new(Counts::default()),
        }
    }

    /// Records one request; a request [`Group::of`] does not count is
    /// dropped.
    ///
    /// `template` is the matched route template as the router reports it,
    /// under the base path, or `None` for a request no route matched.
    pub fn record(
        &self,
        method: &Method,
        template: Option<&str>,
        status: StatusCode,
        elapsed: Duration,
    ) {
        let under_base = template.map(|template| {
            template
                .strip_prefix(self.base.as_str())
                .filter(|rest| rest.starts_with('/'))
                .unwrap_or(template)
        });
        let Some(group) = Group::of(method, under_base) else {
            return;
        };
        let mut counts = self.lock();
        if status.is_client_error() {
            counts.responses_4xx = counts.responses_4xx.saturating_add(1);
        }
        let entry = counts.groups.entry(group).or_default();
        entry.time(elapsed);
        if status.is_server_error() {
            entry.errors_5xx = entry.errors_5xx.saturating_add(1);
        }
    }

    /// Records one HL7 v2 message that took `elapsed` and was answered with
    /// `code`, `None` for one left unanswered: its latency in the `hl7v2`
    /// group, and an `AE` or `AR` in its counter. Nothing of the message
    /// reaches the window.
    pub fn record_message(&self, elapsed: Duration, code: Option<Code>) {
        let mut counts = self.lock();
        match code {
            Some(Code::Error) => counts.hl7v2_ae = counts.hl7v2_ae.saturating_add(1),
            Some(Code::Reject) => counts.hl7v2_ar = counts.hl7v2_ar.saturating_add(1),
            Some(Code::Accept) | None => {}
        }
        counts.groups.entry(Group::Hl7v2).or_default().time(elapsed);
    }

    /// Takes the counts recorded so far and starts a fresh window.
    #[must_use]
    pub fn take(&self) -> Counts {
        std::mem::take(&mut *self.lock())
    }

    /// Puts `counts` back into the window, for a daily report that did not
    /// reach the collector.
    pub fn restore(&self, counts: &Counts) {
        self.lock().absorb(counts);
    }

    /// Returns the guard of the counts.
    fn lock(&self) -> std::sync::MutexGuard<'_, Counts> {
        // NOTE: no specification governs this: our own design; every update
        // leaves the counts consistent, so a poisoned lock still holds them.
        self.counts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Returns the index of the bucket holding `ms`; an upper bound includes its
/// own value.
fn bucket_of(ms: f64) -> usize {
    BUCKET_BOUNDS_MS
        .iter()
        .position(|bound| ms <= f64::from(*bound))
        .unwrap_or(BUCKETS - 1)
}

/// Returns the lower bound of bucket `index`, in milliseconds.
fn lower_bound_ms(index: usize) -> f64 {
    index
        .checked_sub(1)
        .and_then(|previous| BUCKET_BOUNDS_MS.get(previous))
        .map_or(0.0, |bound| f64::from(*bound))
}

/// Returns the upper bound of bucket `index`, in milliseconds; the `+Inf`
/// bucket reaches up to `max_ms`.
fn upper_bound_ms(index: usize, max_ms: f64) -> f64 {
    BUCKET_BOUNDS_MS
        .get(index)
        .map_or(max_ms, |bound| f64::from(*bound))
}

/// Returns `ms` within the contract's bounds, rounded to a microsecond.
fn report_ms(ms: f64) -> f64 {
    (ms.clamp(0.0, MAX_MS) * 1000.0).round() / 1000.0
}

/// Returns `count` as a float, saturating at `u32::MAX`, which a report's
/// counts (at most [`MAX_COUNT`]) never reach.
fn to_f64(count: u64) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::{Method, StatusCode};

    use super::{Group, GroupCounts, Window, bucket_of};
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
    fn the_probes_are_not_counted() {
        for template in ["/health", "/health/readiness"] {
            assert_eq!(Group::of(&Method::GET, Some(template)), None, "{template}");
        }
        let window = Window::new("/pix");
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
        let window = Window::new("/pix");
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
            counts.groups.keys().copied().collect::<Vec<_>>(),
            [Group::Other, Group::PixQuery]
        );
    }

    #[test]
    fn an_upper_bound_includes_its_own_value() {
        assert_eq!(bucket_of(0.0), 0);
        assert_eq!(bucket_of(5.0), 0);
        assert_eq!(bucket_of(5.001), 1);
        assert_eq!(bucket_of(5000.0), 9);
        assert_eq!(bucket_of(5000.5), 10);
    }

    #[test]
    fn a_quantile_interpolates_inside_its_bucket() {
        let counts = GroupCounts {
            buckets: [0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            errors_5xx: 0,
            max_ms: 9.0,
        };
        assert!((counts.quantile_ms(0.5) - 7.5).abs() < 1e-9);
        let tail = GroupCounts {
            buckets: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            errors_5xx: 0,
            max_ms: 8000.0,
        };
        assert!((tail.quantile_ms(0.99) - 7970.0).abs() < 1e-6);
        assert!(GroupCounts::default().stats().is_none());
    }

    #[test]
    fn a_restored_window_carries_its_counts_into_the_next() {
        let window = Window::new("");
        let read = Some("/fhir/Patient/{id}");
        window.record(&Method::GET, read, StatusCode::OK, Duration::from_millis(3));
        window.record(
            &Method::GET,
            read,
            StatusCode::SERVICE_UNAVAILABLE,
            Duration::from_millis(30),
        );
        window.record(&Method::GET, None, StatusCode::NOT_FOUND, Duration::ZERO);
        let taken = window.take();
        assert_eq!(taken.requests(), 3);
        assert_eq!(taken.responses_4xx, 1);
        assert_eq!(window.take().requests(), 0, "taking starts a fresh window");
        window.restore(&taken);
        window.record(&Method::GET, read, StatusCode::OK, Duration::from_millis(3));
        let next = window.take();
        let group = next.groups.get(&Group::PdqQuery).expect("recorded");
        assert_eq!(group.total(), 3);
        assert_eq!(group.errors_5xx, 1);
        assert_eq!(next.responses_4xx, 1);
        assert_eq!(next.to_metrics().window_24h.counters.responses_4xx, 1);
    }
}
