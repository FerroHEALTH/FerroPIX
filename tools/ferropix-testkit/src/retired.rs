// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The release a binary's list of retired configuration keys prepares, held
//! to the workspace version.
//!
//! Each binary declares `PREPARING`, the release its tree prepares, and
//! refuses a retired key by name for that one release only. The release pull
//! request moves the workspace version to it, and the first pull request
//! after the cut moves it to the next release. [`prepares`] holds it to the
//! workspace version or the release after it, so it cannot fall behind. No
//! specification governs this: our own design (the owner's one-release rule
//! of 2026-11-09).

/// A version that is not `major.minor.patch` in decimal numbers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{version}` is not a major.minor.patch version")]
pub struct NotAVersion {
    /// The text that was read.
    pub version: String,
}

/// Whether `preparing` names the release `workspace` is at, or the release
/// after it: the next patch, minor or major version.
///
/// # Errors
///
/// Returns [`NotAVersion`] when either is not `major.minor.patch`.
pub fn prepares(workspace: &str, preparing: &str) -> Result<bool, NotAVersion> {
    let [major, minor, patch] = numbers(workspace)?;
    let next = numbers(preparing)?;
    Ok(next == [major, minor, patch]
        || Some(next) == patch.checked_add(1).map(|patch| [major, minor, patch])
        || Some(next) == minor.checked_add(1).map(|minor| [major, minor, 0])
        || Some(next) == major.checked_add(1).map(|major| [major, 0, 0]))
}

/// The three numbers of the `major.minor.patch` `version`.
fn numbers(version: &str) -> Result<[u64; 3], NotAVersion> {
    let refused = || NotAVersion {
        version: version.to_owned(),
    };
    let parts = version
        .split('.')
        .map(|part| part.parse::<u64>().map_err(|_unparsed| refused()))
        .collect::<Result<Vec<_>, _>>()?;
    <[u64; 3]>::try_from(parts).map_err(|_parts| refused())
}

#[cfg(test)]
mod tests {
    use super::{NotAVersion, prepares};

    #[test]
    fn the_workspace_release_and_the_one_after_it_are_prepared() {
        assert_eq!(Ok(true), prepares("0.0.45", "0.0.45"));
        assert_eq!(Ok(true), prepares("0.0.45", "0.0.46"));
        assert_eq!(Ok(true), prepares("0.0.45", "0.1.0"));
        assert_eq!(Ok(true), prepares("0.0.45", "1.0.0"));
    }

    #[test]
    fn a_release_behind_or_two_ahead_is_not() {
        assert_eq!(Ok(false), prepares("0.0.45", "0.0.27"));
        assert_eq!(Ok(false), prepares("0.0.45", "0.0.47"));
        assert_eq!(Ok(false), prepares("0.0.45", "0.1.1"));
    }

    #[test]
    fn a_version_that_is_not_three_numbers_is_refused() {
        assert_eq!(
            Err(NotAVersion {
                version: String::from("0.0"),
            }),
            prepares("0.0.45", "0.0")
        );
        assert!(prepares("0.0.x", "0.0.45").is_err());
    }
}
