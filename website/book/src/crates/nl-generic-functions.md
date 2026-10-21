<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# nl-generic-functions

The Netherlands Generic Functions for health data exchange (the Nuts
`nl-generic-functions-ig`, package `fhir.nl.gf` 0.3.0) in Rust: one crate for
the implementation guide, with a feature per function. Identification and
localization are a patient index's own functions in the Netherlands, which
is why the crate lives in FerroPIX. The API reference is on
[docs.rs/nl-generic-functions](https://docs.rs/nl-generic-functions). The
version on `main` is {{version:nl-generic-functions}}.

## Features

| Feature | Generic Function | Service | What the crate holds |
|---|---|---|---|
| `nvi` | GF-Localization | the national index (NVI) | the Localization Service search: which care providers (by URA) hold data for a patient (by pseudonymised BSN) |
| `mitz` | GF-Consent | Mitz | the closed authorization question: may a data holder make a patient's data of each category available to a data user, answered `Permit` or `Deny` |
| `lrza` | GF-Addressing | the national address book (LRZa) | the URA of an NL-GF `Organization` |
| `nuts-auth` | GF-Authentication | the Nuts profile | the access token request of Nuts RFC021, with a `DPoP`-bound token, and the holder's `did:web` DID document |
| `authorizer` | GF-Authentication | | the trait a data user implements to give each request its own headers (the IG's GFI-005), with no FHIR model; `nvi` turns it on |

## Identifiers: BSN and the pseudonymised BSN

The `identification` module, built with `nvi`, `lrza` or `mitz`, holds the
identifier systems the functions share. A patient is named by a
pseudonymised BSN, never by the BSN itself, and a care provider by its URA.
A pseudonymised BSN is personal data like the BSN it stands for: its value is
a `secrecy::SecretString`, its `Debug` is redacted and it has no `Display`.

The BSNk polymorphic pseudonyms are not built: the crate does not call the
BSNk pseudonym service.

## NVI (`nvi`)

The NVI client asks the Localization Service which care providers hold data
for a patient. It never puts the pseudonym in an error, a `Debug` rendering
or anything but the request to the service, follows a `next` link only on
its own search endpoint, and hands an authorizer the request URL without its
query, so the pseudonym never reaches it.

## Mitz (`mitz`)

The Mitz client follows the VZVZ *Implementatiehandleiding Open en gesloten
autorisatievraag* 3.8.2: a SOAP 1.2 request carrying one XACML 3.0
`XACMLAuthzDecisionQuery`, over mutual TLS. Only `Permit` and `Deny` are
decisions; `Indeterminate`, `NotApplicable`, a fault, a timeout and an answer
that does not hold to the question are typed errors. The BSN travels only in
the request to Mitz, and the client follows no redirect.

## Nuts (`nuts-auth`)

The Nuts client requests an access token with a Verifiable Presentation of
the holder's credentials as the grant (Nuts RFC021) and a `DPoP`-bound
token. It sends no credential before the authorization server's Presentation
Definition says they answer it, and never puts a credential, the
presentation, a key, a proof or the token in an error or a `Debug`
rendering. The authorization server's issuer is an `https` URL, or plain
`http` on a loopback host for a test, under the RFC 8414 rules of
[`ferrohealth-smart`](https://crates.io/crates/ferrohealth-smart).
