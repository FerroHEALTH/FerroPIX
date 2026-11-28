// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The keys a release removed or renamed, each refused by name for that one
//! release.
//!
//! A section keeps each old key as a `ferrohealth_config::removed::Removed`
//! field and declares it in [`ALL`] with the release that retired it. The
//! next release deletes it (the owner's one-release rule of 2026-11-09), and
//! the key then fails as an ordinary unknown key under
//! `deny_unknown_fields`; the test below holds that through [`PREPARING`].
//! No specification governs this: our own design.

use ferrohealth_config::removed::Retired;

/// The release this tree prepares.
///
/// The release pull request moves the workspace version to it, and the
/// first pull request after the cut moves it to the next release, which
/// fails the test below until every refusal of the release just cut is
/// deleted. A test holds it to the workspace version or the release after
/// it, so it cannot fall behind.
pub const PREPARING: &str = "0.0.48";

/// `trust.post_quantum_only`, which `ferrohealth-smart` 0.36's
/// `auth.post_quantum_only` replaces.
pub const TRUST_POST_QUANTUM_ONLY: Retired = Retired::replaced(
    "trust.post_quantum_only",
    "auth.post_quantum_only, the switch ferrohealth-smart reads in [auth]",
    "0.0.48",
);

/// Every key refused by name.
pub const ALL: [Retired; 1] = [TRUST_POST_QUANTUM_ONLY];

#[cfg(test)]
mod tests {
    use ferrohealth_config::removed::stale;

    use super::{ALL, PREPARING, Retired};

    /// Returns the three numbers of a `major.minor.patch` version.
    fn numbers(version: &str) -> [u64; 3] {
        let parts: Vec<u64> = version
            .split('.')
            .map(|part| part.parse().expect("a version number"))
            .collect();
        parts.try_into().expect("major.minor.patch")
    }

    /// Returns whether `next` is the release after `current`: the next
    /// patch, minor or major version.
    fn is_next_release(current: [u64; 3], next: [u64; 3]) -> bool {
        let [major, minor, patch] = current;
        next == [major, minor, patch + 1]
            || next == [major, minor + 1, 0]
            || next == [major + 1, 0, 0]
    }

    #[test]
    fn preparing_names_the_workspace_release_or_the_one_after_it() {
        let workspace = numbers(env!("CARGO_PKG_VERSION"));
        let preparing = numbers(PREPARING);
        assert!(
            preparing == workspace || is_next_release(workspace, preparing),
            "PREPARING is v{PREPARING} while the workspace is v{}: set it to the release this tree prepares",
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn a_preparing_behind_or_two_releases_ahead_is_refused() {
        assert!(is_next_release([0, 0, 45], [0, 0, 46]));
        assert!(is_next_release([0, 0, 45], [0, 1, 0]));
        assert!(is_next_release([0, 0, 45], [1, 0, 0]));
        assert!(!is_next_release([0, 0, 45], [0, 0, 27]));
        assert!(!is_next_release([0, 0, 45], [0, 0, 47]));
    }

    #[test]
    fn every_refusal_lives_for_the_one_release_that_renamed_its_key() {
        let past: Vec<&str> = stale(&ALL, PREPARING)
            .into_iter()
            .map(Retired::key)
            .collect();
        assert!(
            past.is_empty(),
            "v{PREPARING} is prepared, and these refusals belong to an earlier release: delete them from src/config/retired.rs: {past:?}"
        );
    }
}
