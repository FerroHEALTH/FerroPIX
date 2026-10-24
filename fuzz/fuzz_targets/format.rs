// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Arbitrary bytes as the media types a FHIR server negotiates (FHIR R4
//! http.html, "Content Types and encodings"): a request's `Content-Type`, its
//! `_format` and its `Accept`, separated by NUL bytes, each absent when its
//! part is empty. Whatever format is chosen must name itself again through
//! its own media type, and a refusal quotes nothing from the request. An
//! `Err` is the parser's job; a panic is a bug.

#![no_main]

use ferropix_fuzz::{assert_quotes_nothing, split_at_nul};
use ihe_iti::server::format::Format;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (content_type, rest) = split_at_nul(data);
    let (format, accept) = split_at_nul(rest);
    let part = |bytes: &[u8]| match std::str::from_utf8(bytes) {
        Ok("") | Err(_) => None,
        Ok(text) => Some(text.to_owned()),
    };
    let (content_type, format, accept) = (part(content_type), part(format), part(accept));
    if let Some(body) = Format::of_content_type(content_type.as_deref()) {
        assert_eq!(Format::of_content_type(Some(body.media_type())), Some(body));
    }
    match Format::negotiate(format.as_deref(), accept.as_deref()) {
        Ok(answer) => {
            assert_eq!(
                Format::negotiate(Some(answer.media_type()), None),
                Ok(answer)
            );
            assert_eq!(
                Format::negotiate(None, Some(answer.media_type())),
                Ok(answer)
            );
        }
        Err(error) => assert_quotes_nothing(&error, &error.refusal()),
    }
});
