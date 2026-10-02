// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why a Bundle does not read as directory content.

use fhir_types::codec::DecodeError;

/// A Bundle that does not read as `Organization` and `Endpoint` resources.
///
/// An entry is named by its position in the Bundle's `entry` list.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DirectoryError {
    /// The text is not JSON.
    #[error("the directory content is not JSON (line {line}, column {column})")]
    NotJson {
        /// The line of the syntax error.
        line: usize,
        /// The column of the syntax error.
        column: usize,
    },
    /// The JSON is not a FHIR resource of type `Bundle`.
    #[error("the directory content is not a FHIR Bundle")]
    NotABundle,
    /// The Bundle does not decode as FHIR R4.
    #[error("the Bundle does not decode as FHIR R4")]
    Decode(#[source] DecodeError),
    /// The Bundle's `type` is neither `collection` nor `searchset`.
    #[error("the Bundle's type {found:?} is neither collection nor searchset")]
    BundleType {
        /// The `type` the Bundle carries.
        found: Option<String>,
    },
    /// An entry has no `resource`.
    #[error("entry {index} has no resource")]
    NoResource {
        /// The entry's position.
        index: usize,
    },
    /// An entry holds a resource other than an `Organization` or an
    /// `Endpoint`.
    #[error("entry {index} holds a resource that is neither an Organization nor an Endpoint")]
    UnexpectedEntry {
        /// The entry's position.
        index: usize,
    },
    /// An entry's resource carries a `modifierExtension`, which changes its
    /// meaning in a way this reader does not know.
    #[error("entry {index} carries a modifierExtension")]
    ModifierExtension {
        /// The entry's position.
        index: usize,
    },
    /// Two entries share a `fullUrl`.
    #[error("entry {index} repeats the fullUrl of an earlier entry")]
    DuplicateFullUrl {
        /// The position of the later entry.
        index: usize,
    },
    /// Two resources of one type share a logical id.
    #[error("entry {index} repeats the logical id of an earlier resource of its type")]
    DuplicateId {
        /// The position of the later entry.
        index: usize,
    },
}
