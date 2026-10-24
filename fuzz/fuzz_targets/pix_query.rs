// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Arbitrary bytes as the query of an ITI-83 `GET`
//! (`Patient/$ihe-pix?sourceIdentifier=…`, PIXm 3.1.0 §2:3.83.4.1.2), read by
//! `PixQuery::from_query` through the shared percent-decoding and token
//! parsers. A query that reads is posted back as the `Parameters` of the
//! Query Parameters In profile and must read the same; a refusal quotes
//! nothing from the request. An `Err` is the reader's job; a panic is a bug.

#![no_main]

use ferropix_fuzz::{assert_quotes_nothing, pix_round_trip};
use ihe_iti::pixm::server::query::PixQuery;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };
    match PixQuery::from_query(query) {
        Ok(read) => pix_round_trip(&read),
        Err(error) => assert_quotes_nothing(&error, &error.refusal()),
    }
});
