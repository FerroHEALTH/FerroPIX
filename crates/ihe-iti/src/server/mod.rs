// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the server sides of the FHIR profiles share.
//!
//! With `pixm-server` or `pdqm-server`: the two FHIR encodings a server reads
//! and writes, and the answer a reader or a writer hands back with its HTTP
//! status.
//!
//! Every IHE transaction this crate serves requires the server to read and
//! write both FHIR JSON and FHIR XML (PIXm 3.1.0 §2:3.104.4.1.2, PDQm 3.2.0
//! §2:3.78.4.1.2.4 and §2:3.119.4.1.2.1). [`format::Format`] names the
//! encoding of a request body from its `Content-Type` and the encoding of the
//! answer from `_format` and `Accept` (FHIR R4 http.html, "Content Types and
//! encodings"). [`answer::Answer`] is a resource to send with its status,
//! encoded on demand in the format negotiated.
//!
//! No type here holds a store, a socket or an HTTP framework: the caller
//! takes the request apart, hands the query, the media types and the body to
//! the profile's reader, and puts the answer's status, media type and bytes
//! on its own response.

pub mod answer;
pub mod format;
pub(crate) mod token;
