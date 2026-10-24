// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Arbitrary bytes as an ITI-104 Patient Identity Feed (PIXm 3.1.0
//! §2:3.104.4.1.2, §2:3.104.4.2.2, §2:3.104.4.3.2): the query of the
//! conditional `PUT` or `DELETE` up to the first NUL byte, and the `Patient`
//! body after it, in FHIR XML when it opens with `<` and in FHIR JSON
//! otherwise. The query is read as a Remove Patient condition and the whole
//! as an update. A Patient that reads is written again in both encodings and
//! must read as the same message; a refusal quotes nothing from the request.
//! An `Err` is the reader's job; a panic is a bug.

#![no_main]

use ferropix_fuzz::{assert_quotes_nothing, sniff, split_at_nul, write};
use ihe_iti::pixm::identifier::SourceIdentifier;
use ihe_iti::pixm::server::feed::{Feed, Removal};
use ihe_iti::server::format::Format;
use libfuzzer_sys::fuzz_target;
use secrecy::ExposeSecret;

fuzz_target!(|data: &[u8]| {
    let (query, body) = split_at_nul(data);
    let Ok(query) = std::str::from_utf8(query) else {
        return;
    };
    if let Err(error) = Removal::from_delete(query) {
        assert_quotes_nothing(&error, &error.refusal());
    }
    match Feed::from_update(query, sniff(body), body) {
        Ok(feed) => round_trip(query, &feed),
        Err(error) => assert_quotes_nothing(&error, &error.refusal()),
    }
});

/// The identifiers a message names, exposed for comparison.
fn named(feed: &Feed) -> Vec<(String, String)> {
    let pair = |identifier: &SourceIdentifier| {
        (
            identifier.system().to_owned(),
            identifier.value().expose_secret().to_owned(),
        )
    };
    match feed {
        Feed::Revise(revision) => vec![pair(revision.identifier())],
        Feed::ResolveDuplicate(merge) => vec![pair(merge.subsumed()), pair(merge.survivor())],
        _ => panic!("a message of a kind this target does not know"),
    }
}

fn round_trip(query: &str, feed: &Feed) {
    let patient = match feed {
        Feed::Revise(revision) => revision.patient(),
        Feed::ResolveDuplicate(merge) => merge.patient(),
        _ => panic!("a message of a kind this target does not know"),
    };
    for format in [Format::Json, Format::Xml] {
        let Ok(again) = Feed::from_update(query, format, &write(patient, format)) else {
            panic!("a feed that reads does not read back as {format:?}");
        };
        assert_eq!(
            named(&again),
            named(feed),
            "the message changes across {format:?}"
        );
        let patient_again = match &again {
            Feed::Revise(revision) => revision.patient(),
            Feed::ResolveDuplicate(merge) => merge.patient(),
            _ => panic!("a message of a kind this target does not know"),
        };
        assert!(
            patient_again == patient,
            "the Patient changes across {format:?}"
        );
    }
}
