// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The scoring features a record's head keeps beside its blocking keys.
//!
//! A head's features are the [`Subject`] its version's demographics give,
//! without the local identifiers (a domain's kind can change, so they are
//! read from the identifiers the version carries), written as compact JSON
//! by the statement that moves the head. A candidate is scored from its
//! features, so no candidate's demographics are read or parsed into a
//! `Patient`. [`FEATURES_VERSION`] names the normalisation the features were
//! written under; a head written under another one is refilled when the
//! store starts, and a read never scores it. No specification governs the
//! features: our own design (`docs/architecture.md` §5.2).

use std::collections::BTreeSet;

use crate::matching::subject::{IdentifierKey, Subject};

/// The version of the features' form and of the normalisation behind them,
/// raised by every change that alters what [`encode`] writes for a
/// `Patient`, the blocking keys included.
pub const FEATURES_VERSION: i16 = 1;

/// Returns the features of `subject`, its local identifiers left out.
///
/// # Errors
/// Returns [`FeaturesError::Unwritable`] when the JSON writer refuses the
/// subject, which a subject of strings and numbers never causes.
pub fn encode(subject: &Subject) -> Result<String, FeaturesError> {
    serde_json::to_string(subject).map_err(|_json| FeaturesError::Unwritable)
}

/// Reads the features `features`, written under `version`, into the subject
/// with `local` its local identifiers.
///
/// # Errors
/// Returns [`FeaturesError::Stale`] when the head holds no features or holds
/// features of another version than [`FEATURES_VERSION`], and
/// [`FeaturesError::Malformed`] when they do not read.
pub fn decode(
    features: Option<&str>,
    version: Option<i16>,
    local: BTreeSet<IdentifierKey>,
) -> Result<Subject, FeaturesError> {
    let (Some(features), Some(FEATURES_VERSION)) = (features, version) else {
        return Err(FeaturesError::Stale { found: version });
    };
    let mut subject: Subject =
        serde_json::from_str(features).map_err(|_json| FeaturesError::Malformed)?;
    subject.local_identifiers = local;
    Ok(subject)
}

/// Features that cannot be written or scored.
///
/// It carries neither the features nor the parser's message, which may
/// quote a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeaturesError {
    /// The head's features were written under another version, or not yet.
    #[error(
        "a record head holds scoring features of version {found:?}, and this server reads version {FEATURES_VERSION}; run db migrate"
    )]
    Stale {
        /// The version the head names, `None` when it holds none.
        found: Option<i16>,
    },
    /// The features do not read as a subject.
    #[error("a record head's scoring features do not read")]
    Malformed,
    /// The subject cannot be written.
    #[error("the scoring features cannot be written")]
    Unwritable,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES};
    use uuid::Uuid;

    use super::{FEATURES_VERSION, FeaturesError, decode, encode};
    use crate::matching::subject::{IdentifierKey, Subject};
    use crate::store::demographics;

    /// The corpus the golden encoding is pinned over: every field, several
    /// values, a date to the year, accents, a Dutch prefix and an empty one.
    fn corpus() -> Vec<String> {
        let family = FAMILY_NAMES[0];
        let given = GIVEN_NAMES[0];
        let mother = FAMILY_NAMES[1];
        vec![
            format!(
                r#"{{"resourceType":"Patient",
                "name":[{{"use":"maiden","family":"X{family}"}},{{"use":"official","family":"van der {family}","given":["{given}","B."]}}],
                "birthDate":"1958-03-14","gender":"female",
                "address":[{{"use":"work","postalCode":"9999 zz"}},{{"use":"home","line":["Kade 1","Bis"],"city":"Voorbeeld","postalCode":"1234 ab"}}],
                "telecom":[{{"system":"phone","value":"06-12345678"}},{{"system":"email","value":"A@Example.org"}},{{"system":"phone","value":"+31 20 1234567"}}],
                "extension":[{{"url":"http://hl7.org/fhir/StructureDefinition/patient-mothersMaidenName","valueString":"{mother}"}}]}}"#
            ),
            String::from(
                r#"{"resourceType":"Patient","name":[{"given":["Zoë","Éva"]}],"birthDate":"1971","gender":"unknown"}"#,
            ),
            String::from(r#"{"resourceType":"Patient"}"#),
        ]
    }

    fn subject(text: &str) -> Subject {
        Subject::from_patient(&demographics::patient(text).unwrap(), BTreeSet::new())
    }

    #[test]
    fn the_encoding_of_the_corpus_is_pinned_and_a_change_raises_the_version() {
        let family = FAMILY_NAMES[0].to_lowercase();
        let given = GIVEN_NAMES[0].to_lowercase();
        let mother = FAMILY_NAMES[1].to_lowercase();
        let expected = [
            format!(
                r#"{{"n":[{{"f":"van der {family}","g":["{given}","b"]}},{{"f":"x{family}"}}],"b":{{"y":1958,"d":[3,14]}},"s":"female","p":["1234AB","9999ZZ"],"a":[["1","bis","kade","voorbeeld"]],"t":["612345678","201234567"],"e":["a@example.org"],"m":"{mother}"}}"#
            ),
            String::from(r#"{"n":[{"g":["zoe","eva"]}],"b":{"y":1971}}"#),
            String::from("{}"),
        ];
        for (text, expected) in corpus().iter().zip(expected) {
            assert_eq!(
                expected,
                encode(&subject(text)).unwrap(),
                "a change to the features' form or the normalisation behind them raises FEATURES_VERSION and updates this pin"
            );
        }
        assert_eq!(1, FEATURES_VERSION);
    }

    #[test]
    fn the_features_read_back_as_the_subject_with_its_local_identifiers() {
        let local: BTreeSet<IdentifierKey> = [IdentifierKey::new(Uuid::nil(), [7; 32])].into();
        for text in corpus() {
            let mut expected = subject(&text);
            let features = encode(&expected).unwrap();
            expected.local_identifiers = local.clone();
            let read = decode(Some(&features), Some(FEATURES_VERSION), local.clone()).unwrap();
            assert!(expected == read, "the features of {text} read back");
        }
    }

    #[test]
    fn missing_stale_or_malformed_features_are_refused() {
        let features = encode(&subject(&corpus()[0])).unwrap();
        assert_eq!(
            Err(FeaturesError::Stale { found: None }),
            decode(None, None, BTreeSet::new()).map(|_| ())
        );
        assert_eq!(
            Err(FeaturesError::Stale {
                found: Some(FEATURES_VERSION + 1)
            }),
            decode(Some(&features), Some(FEATURES_VERSION + 1), BTreeSet::new()).map(|_| ())
        );
        assert_eq!(
            Err(FeaturesError::Malformed),
            decode(Some(r#"{"z":1}"#), Some(FEATURES_VERSION), BTreeSet::new()).map(|_| ())
        );
    }
}
