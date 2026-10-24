// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Arbitrary bytes as an ITI-78 search (PDQm 3.2.0 §2:3.78.4.1.2), read both
//! as the query of a `GET` (`PatientSearch::from_query`) and as the form body
//! of a `POST` to `Patient/_search` (`PatientSearch::from_form`), through the
//! shared percent-decoding and token parsers. The `self` link of a search
//! that reads names every parameter the search respected
//! (§2:3.78.4.1.2.1.1), so following it must read the same search; a refusal
//! quotes nothing from the request. An `Err` is the reader's job; a panic is
//! a bug.

#![no_main]

use std::sync::LazyLock;

use ferropix_fuzz::assert_quotes_nothing;
use ihe_iti::pdqm::server::search::{Criterion, PatientSearch, Token};
use libfuzzer_sys::fuzz_target;
use secrecy::ExposeSecret;
use url::Url;

static ENDPOINT: LazyLock<Url> = LazyLock::new(|| {
    let Ok(url) = Url::parse("https://pdq.example.org/fhir/Patient") else {
        panic!("the endpoint of the target is a URL");
    };
    url
});

fuzz_target!(|data: &[u8]| {
    if let Ok(query) = std::str::from_utf8(data) {
        check(PatientSearch::from_query(query));
    }
    check(PatientSearch::from_form(data));
});

fn check(read: Result<PatientSearch, ihe_iti::pdqm::server::search::SearchError>) {
    match read {
        Ok(search) => round_trip(&search),
        Err(error) => assert_quotes_nothing(&error, &error.refusal()),
    }
}

/// The criteria of `search`, each value exposed, for comparison.
fn criteria(search: &PatientSearch) -> Vec<String> {
    let token = |token: &Token| format!("{:?}|{}", token.system(), token.value().expose_secret());
    search
        .criteria()
        .iter()
        .map(|criterion| match criterion {
            Criterion::Id(values) => format!(
                "id {:?}",
                values
                    .iter()
                    .map(|v| v.expose_secret().to_owned())
                    .collect::<Vec<_>>()
            ),
            Criterion::Active(values) => format!("active {values:?}"),
            Criterion::Text {
                parameter,
                matching,
                values,
            } => format!(
                "{parameter:?} {matching:?} {:?}",
                values
                    .iter()
                    .map(|v| v.expose_secret().to_owned())
                    .collect::<Vec<_>>()
            ),
            Criterion::Identifier(tokens) => {
                format!(
                    "identifier {:?}",
                    tokens.iter().map(token).collect::<Vec<_>>()
                )
            }
            Criterion::Telecom(tokens) => {
                format!("telecom {:?}", tokens.iter().map(token).collect::<Vec<_>>())
            }
            Criterion::Birthdate(dates) => format!(
                "birthdate {:?}",
                dates
                    .iter()
                    .map(|date| format!("{:?} {}", date.prefix(), date.date().expose_secret()))
                    .collect::<Vec<_>>()
            ),
            Criterion::Gender(gender) => format!("gender {gender:?}"),
            _ => panic!("a criterion of a kind this target does not know"),
        })
        .collect()
}

fn round_trip(search: &PatientSearch) {
    let link = search.self_link(&ENDPOINT);
    let Ok(again) = PatientSearch::from_query(link.url().query().unwrap_or_default()) else {
        panic!("the self link of a search that reads does not read");
    };
    assert_eq!(
        criteria(&again),
        criteria(search),
        "the self link changes the criteria"
    );
    assert_eq!(
        again.domains(),
        search.domains(),
        "the self link changes the domains"
    );
    assert_eq!(
        again.count(),
        search.count(),
        "the self link changes _count"
    );
    assert!(
        again.dropped().is_empty(),
        "the self link names a dropped parameter"
    );
}
