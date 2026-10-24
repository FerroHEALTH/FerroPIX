// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `FerroHEALTH` installation report to `FerroPULSE`.
//!
//! One `start` report at each `serve` start and one `daily` report a day,
//! carrying the installation id, the version, the licence grant type, the
//! deployment kind, coarse host size, the profile, the services switched on,
//! the identifier domains as a range, the readiness of the store and the
//! audit spool, and latency aggregates per route group. The wire contract is
//! `FerroPULSE`'s report envelope v2 (`docs/report-v2.md`), and every report
//! the server builds is validated against its vendored schema in the tests.
//! A report carries no identifier other than the instance id, no domain
//! system or name, no host name and no request path: the payload types have
//! no member that could hold one, and [`window`] classifies a request by its
//! matched route template alone. It is on by default; `[usage_report]
//! enabled = false` stops it, a CI environment stops it, and so does the lack
//! of a writable state directory. One boot line says which
//! ([`reporter::announce`]). No specification governs any of this: our own
//! design, after FerroBRIDGE's report.
//!
//! - [`payload`]: the typed report.
//! - [`window`]: the in-process metrics window the request log fills.
//! - [`state`]: the instance id and the last start time on disk.
//! - [`reporter`]: the decision, assembly, sending and scheduling.

pub mod payload;
pub mod reporter;
pub mod state;
pub mod window;

/// The process environment variables that mark a CI runner, read in this
/// order.
pub const CI_VARIABLES: [&str; 2] = ["GITHUB_ACTIONS", "CI"];

/// Returns the first CI variable `lookup` answers `true` or `1` for, in any
/// case.
///
/// A CI runner sets one of them for every step it runs, so a server it starts
/// would otherwise count as an installation.
#[must_use]
pub fn ci_of(lookup: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    CI_VARIABLES.into_iter().find(|name| {
        lookup(name).is_some_and(|value| {
            let value = value.trim();
            value.eq_ignore_ascii_case("true") || value == "1"
        })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::ci_of;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn ci_is_detected_from_either_variable_and_true_or_one_only() {
        assert_eq!(ci_of(lookup(&[])), None);
        assert_eq!(
            ci_of(lookup(&[("GITHUB_ACTIONS", "TRUE")])),
            Some("GITHUB_ACTIONS")
        );
        assert_eq!(ci_of(lookup(&[("CI", "1")])), Some("CI"));
        assert_eq!(
            ci_of(lookup(&[("CI", "true"), ("GITHUB_ACTIONS", "true")])),
            Some("GITHUB_ACTIONS"),
            "GITHUB_ACTIONS is read first"
        );
        for no in ["false", "0", "", "yes"] {
            assert_eq!(ci_of(lookup(&[("CI", no), ("GITHUB_ACTIONS", no)])), None);
        }
    }
}
