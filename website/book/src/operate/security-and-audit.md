<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Security and audit

A patient index handles the most identifying data in a health system, so
both crates are built to keep it out of anywhere it was not sent. This page
says what they guarantee today. The server's own access control and audit
are planned in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).

## No identifying data where it was not sent

- Every identifier, every demographic criterion, a pseudonymised BSN, a SAML
  assertion and a token travels as a `secrecy::SecretString` or in a type
  that redacts its content in `Debug` and has no `Display`.
- No error carries a value, a request URL or body, or the free text the
  other server wrote, so an error you log cannot leak one.
- A client's `Debug` shows its URLs with the userinfo replaced by `***` and
  leaves out the `reqwest::Client`, whose default headers may hold a
  credential.
- Build each `reqwest::Client` you pass in with `redirect::Policy::none()`:
  the request carries the identifier, and a redirect would send it on.
- A PDQm query sends its criteria in the request body, and a PIXm client
  built for `POST` keeps the source identifier out of the URL too.
- The PMIR feed drops every demographic as it reads an ITI-93 message.

## Access tokens

The PIXm, PDQm, mCSD and PMIR clients send the credential their
`reqwest::Client` carries in its default headers or, built
`.with_authorizer(…)`, ask an `ihe_iti::authorizer::Authorizer` for the
headers of every request: an OAuth 2.0 access token incorporated as IUA
ITI-72 §3.72.4.2 asks. After a `401` the request is sent once more with new
headers (§3.72.4.3), never more than twice in all. The authorizer is handed
the request URL without its query, so no identifier or search criterion
reaches it, and a request it cannot authenticate is not sent.
`nl-generic-functions` has the same seam for the Generic Functions, behind
its `authorizer` feature.

## Audit

The audit trail is [`ihe-atna`](https://crates.io/crates/ihe-atna), the
family's audit crate from FerroAUDIT: ATNA ITI-20 with its syslog sender,
spool and forwarder, and the BALP FHIR feed. `ihe-iti` writes the record
each profile defines for its own transactions on it:

- **`balp`:** every PIXm, PDQm, mCSD and PMIR client takes an
  `ihe_atna::balp::AuditRecorder` through `.audited(…)` and records the BALP
  `AuditEvent` of each transaction.
- **`atna`:** `XcpdClient::audited` hands the ITI-55 Initiating Gateway audit
  message of every exchange to a recorder, written as the DICOM audit message
  in `ihe-atna`'s ATNA model.

The client waits for the recorder before it returns the answer, the
exchange's timeout bounds the record too, and a record the recorder refuses
fails the exchange as an audit failure, so no answer is used without its
audit. An audited exchange's record names the user it was made for.

## Reporting a vulnerability

Report a vulnerability privately through GitHub's
[private vulnerability reporting](https://github.com/FerroHEALTH/FerroPIX/security/advisories/new),
never in a public issue. The supported versions and the response times are
in
[`SECURITY.md`](https://github.com/FerroHEALTH/FerroPIX/blob/main/SECURITY.md),
and the manufacturer's vulnerability handling, disclosure and advisories are
in the family book's [security page](https://ferrohealth.eu/docs/security.html).
