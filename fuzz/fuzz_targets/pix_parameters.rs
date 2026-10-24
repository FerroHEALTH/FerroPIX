// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Arbitrary bytes as the body of an ITI-83 `POST` to `Patient/$ihe-pix`,
//! the `Parameters` of the Query Parameters In profile (FHIR R4 Operations,
//! "Executing an Operation"), read by `PixQuery::from_body` in FHIR XML when
//! the body opens with `<` and in FHIR JSON otherwise. A request that reads
//! is written again in both encodings and must read the same; a refusal
//! quotes nothing from the request. An `Err` is the reader's job; a panic is
//! a bug.

#![no_main]

use ferropix_fuzz::{assert_quotes_nothing, pix_round_trip, sniff};
use ihe_iti::pixm::server::query::PixQuery;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    match PixQuery::from_body(sniff(data), data) {
        Ok(read) => pix_round_trip(&read),
        Err(error) => assert_quotes_nothing(&error, &error.refusal()),
    }
});
