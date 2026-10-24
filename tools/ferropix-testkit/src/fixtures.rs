// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The synthetic FHIR R4 documents of each transaction, committed under
//! `fixtures/fhir/` and decoded into the `fhir-types` R4 model.
//!
//! The patients follow the shape of the PIXm 3.1.0 package's examples
//! (`Patient-Patient-MohrAlice-*.json`, `Parameters-pixm-*.json`): one
//! person known in several identifier domains, a duplicate resolved onto
//! the surviving record, and the cross-reference read back. Every value
//! comes from [`synthetic`](crate::synthetic), and the
//! [`guard`](crate::guard) holds each file to it.

use fhir_types::codec::{DecodeError, Json, Path, Value};
use fhir_types::r4::resource::Resource;
use std::path::PathBuf;

/// The fixture tree, absolute at compile time.
pub const FIXTURE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/fhir");

/// One committed fixture document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Fixture {
    /// ITI-104: the hospital's `Patient`, with its record number and the
    /// BSN.
    FeedHospital,
    /// ITI-104: the laboratory's `Patient` of the same person, under its
    /// own number only.
    FeedLaboratory,
    /// ITI-104: a duplicate hospital record resolved onto the surviving
    /// one, `active` false with a `replaced-by` link.
    FeedDuplicateResolved,
    /// ITI-83: the `Parameters` a Consumer posts, from the laboratory number
    /// to the hospital domain and the BSN.
    PixQueryIn,
    /// ITI-83: the `Parameters` the Manager answers that query with.
    PixQueryOut,
    /// ITI-78: the searchset `Bundle` of a demographics query.
    DemographicsSearchset,
    /// ITI-119: the `Parameters` of a `$match` request.
    MatchIn,
    /// ITI-119: the searchset `Bundle` a `$match` answers with, graded.
    MatchOut,
    /// ITI-93: the PMIR message `Bundle` that creates a patient.
    PmirFeedCreate,
}

impl Fixture {
    /// Every fixture, in declaration order.
    pub const ALL: [Self; 9] = [
        Self::FeedHospital,
        Self::FeedLaboratory,
        Self::FeedDuplicateResolved,
        Self::PixQueryIn,
        Self::PixQueryOut,
        Self::DemographicsSearchset,
        Self::MatchIn,
        Self::MatchOut,
        Self::PmirFeedCreate,
    ];

    /// Returns the transaction the fixture belongs to, as `ITI-n`.
    #[must_use]
    pub const fn transaction(self) -> &'static str {
        match self {
            Self::FeedHospital | Self::FeedLaboratory | Self::FeedDuplicateResolved => "ITI-104",
            Self::PixQueryIn | Self::PixQueryOut => "ITI-83",
            Self::DemographicsSearchset => "ITI-78",
            Self::MatchIn | Self::MatchOut => "ITI-119",
            Self::PmirFeedCreate => "ITI-93",
        }
    }

    /// Returns the path of the file under `fixtures/fhir/`.
    #[must_use]
    pub const fn relative_path(self) -> &'static str {
        match self {
            Self::FeedHospital => "iti-104/patient-hospital.json",
            Self::FeedLaboratory => "iti-104/patient-laboratory.json",
            Self::FeedDuplicateResolved => "iti-104/patient-duplicate-resolved.json",
            Self::PixQueryIn => "iti-83/parameters-in.json",
            Self::PixQueryOut => "iti-83/parameters-out.json",
            Self::DemographicsSearchset => "iti-78/searchset.json",
            Self::MatchIn => "iti-119/parameters-in.json",
            Self::MatchOut => "iti-119/bundle-out.json",
            Self::PmirFeedCreate => "iti-93/bundle-create.json",
        }
    }

    /// Returns the `resourceType` the document carries.
    #[must_use]
    pub const fn resource_type(self) -> &'static str {
        match self {
            Self::FeedHospital | Self::FeedLaboratory | Self::FeedDuplicateResolved => "Patient",
            Self::PixQueryIn | Self::PixQueryOut | Self::MatchIn => "Parameters",
            Self::DemographicsSearchset | Self::MatchOut | Self::PmirFeedCreate => "Bundle",
        }
    }

    /// Returns the absolute path of the file.
    #[must_use]
    pub fn path(self) -> PathBuf {
        PathBuf::from(FIXTURE_ROOT).join(self.relative_path())
    }

    /// Returns the file's text, as a request body is sent.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError::Read`] when the file cannot be read.
    pub fn text(self) -> Result<String, FixtureError> {
        let path = self.path();
        std::fs::read_to_string(&path).map_err(|source| FixtureError::Read { path, source })
    }

    /// Returns the file as a JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError::Read`] when the file cannot be read and
    /// [`FixtureError::Json`] when it is not JSON.
    pub fn document(self) -> Result<Value, FixtureError> {
        let text = self.text()?;
        serde_json::from_str(&text).map_err(|source| FixtureError::Json {
            path: self.path(),
            source,
        })
    }

    /// Returns the file decoded into the FHIR R4 model, refusing any member
    /// the resource does not define.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError::Read`] and [`FixtureError::Json`] as
    /// [`Fixture::document`] does, [`FixtureError::NotAnObject`] when the
    /// document is not an object, and [`FixtureError::Decode`] when the model
    /// refuses it.
    pub fn resource(self) -> Result<Resource, FixtureError> {
        let document = self.document()?;
        let object = document
            .as_object()
            .ok_or_else(|| FixtureError::NotAnObject { path: self.path() })?;
        Resource::from_json(object, &mut Path::root(self.resource_type())).map_err(|source| {
            FixtureError::Decode {
                path: self.path(),
                source,
            }
        })
    }
}

/// A fixture could not be read or decoded.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FixtureError {
    /// The file could not be read.
    #[error("cannot read the fixture {}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not JSON.
    #[error("the fixture {} is not JSON", path.display())]
    Json {
        /// The file.
        path: PathBuf,
        /// What the JSON reader reported.
        #[source]
        source: serde_json::Error,
    },
    /// The document is not a JSON object.
    #[error("the fixture {} is not a JSON object", path.display())]
    NotAnObject {
        /// The file.
        path: PathBuf,
    },
    /// The FHIR R4 model refused the document.
    #[error("the fixture {} is not a valid FHIR R4 resource", path.display())]
    Decode {
        /// The file.
        path: PathBuf,
        /// What the model reported.
        #[source]
        source: DecodeError,
    },
}
