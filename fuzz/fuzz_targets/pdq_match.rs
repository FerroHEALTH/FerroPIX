// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Arbitrary bytes as an ITI-119 `$match` request (PDQm 3.2.0
//! §2:3.119.4.1.2): the query up to the first NUL byte, and the body after
//! it, a PDQm Match Input `Parameters` or a bare `Patient`, in FHIR XML when
//! it opens with `<` and in FHIR JSON otherwise. The Patient of a request
//! that reads is posted again as a bare Patient in both encodings and must
//! read the same; a refusal quotes nothing from the request. An `Err` is the
//! reader's job; a panic is a bug.

#![no_main]

use ferropix_fuzz::{assert_quotes_nothing, sniff, split_at_nul, write};
use ihe_iti::pdqm::server::matching::MatchRequest;
use ihe_iti::server::format::Format;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (query, body) = split_at_nul(data);
    let Ok(query) = std::str::from_utf8(query) else {
        return;
    };
    match MatchRequest::from_request(query, sniff(body), body) {
        Ok(read) => round_trip(&read),
        Err(error) => assert_quotes_nothing(&error, &error.refusal()),
    }
});

fn round_trip(read: &MatchRequest) {
    for format in [Format::Json, Format::Xml] {
        let Ok(again) = MatchRequest::from_request("", format, &write(read.patient(), format))
        else {
            panic!("a match input that reads does not read back as a bare {format:?} Patient");
        };
        assert!(
            again.patient() == read.patient(),
            "the Patient changes across {format:?}"
        );
    }
}
