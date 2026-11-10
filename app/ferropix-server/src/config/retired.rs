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
//! No key is refused by name in this release. No specification governs
//! this: our own design.

use ferrohealth_config::removed::Retired;

/// The release this tree prepares.
///
/// The release pull request moves the workspace version to it, and the
/// first pull request after the cut moves it to the next release, which
/// fails the test below until every refusal of the release just cut is
/// deleted.
pub const PREPARING: &str = "0.0.26";

/// Every key refused by name.
pub const ALL: [Retired; 0] = [];

#[cfg(test)]
mod tests {
    use ferrohealth_config::removed::stale;

    use super::{ALL, PREPARING, Retired};

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
