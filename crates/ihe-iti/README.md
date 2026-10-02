<!-- SPDX-FileCopyrightText: Vernum Projecten B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ihe-iti

The IHE IT Infrastructure (ITI) profiles in Rust: one crate for the technical
framework, with a feature per profile.

| Feature | Profile | Transactions |
|---|---|---|
| `pixm` | Patient Identifier Cross-reference for Mobile | ITI-83 |
| `pdqm` | Patient Demographics Query for Mobile | ITI-78 |
| `mcsd` | Mobile Care Services Discovery | ITI-90 |
| `pmir` | Patient Master Identity Registry | ITI-93, ITI-94 |
| `xcpd` | Cross-Community Patient Discovery | ITI-55 |

The crate depends on no application, so a federation gateway, a master patient
index or any other caller can use it as it is. Only the `xcpd` feature may carry
the SOAP 1.2, HL7 v3 and SAML XUA stack; a build without it compiles none of it.
The profiles are published at <https://profiles.ihe.net/ITI/>.

## PIXm (`pixm`)

`ihe_iti::pixm::PixmClient` is the Patient Identifier Cross-reference
Consumer of ITI-83 (PIXm 3.1.0): `GET [base]/Patient/$ihe-pix` with one
`sourceIdentifier` and any number of `targetSystem` domains, read into a
cross-reference, the profile's not-found answer, or a typed error. A `404` is
"patient unknown" only when its `OperationOutcome` carries a `not-found` issue,
so an outage or a misrouted request is never mistaken for a patient with no
identifiers. Every answer is held to the `$ihe-pix` `OperationDefinition`: the
two out parameters, an assigning authority on each identifier, identifiers only
from the domains asked about, and never the source identifier itself.

Identifier values travel in `secrecy::SecretString`, with redacted `Debug` and
no `Display`, and no error carries a value, the request URL or the Manager's
free text. Build the `reqwest::Client` you pass in with
`redirect::Policy::none()`: the request URL holds the source identifier.

## PDQm (`pdqm`)

`ihe_iti::pdqm::PdqmClient` is the Patient Demographics Consumer of ITI-78
(PDQm 3.2.0): a `POST [base]/Patient/_search` with the criteria of a
`PatientQuery` (every Patient search parameter ITI-78 names, with `:exact` on
the string ones), read into a page of matching Patients with the `total`, each
match's `fullUrl`, `search.score` and `match-grade`, the warnings of any
`OperationOutcome` entry, and the `next` link that `next_page` follows on the
Supplier's own origin. No match is a `total` of `0`; a `404` is "identifier
domain not recognised" only when the query names a domain and the
`OperationOutcome` carries a `not-found` issue. Every answer is held to the
Query Patient Resource Response Message profile: a `searchset` with a
`total`, and a `fullUrl` on every entry. The Patients are decoded as FHIR R4,
not held to the PDQm Patient profile, as the profile asks of a Consumer.

The criteria travel in the request body, so no URL carries them. They, every
matched Patient and every page link redact their content in `Debug`, with no
`Display`, and no error carries a value, a URL or the Supplier's free text.
Build the `reqwest::Client` you pass in with `redirect::Policy::none()`.

## mCSD (`mcsd`)

`ihe_iti::mcsd::directory::Directory` reads the `Organization` and `Endpoint`
resources a care services directory publishes (mCSD 4.0.0) from one FHIR R4
JSON `Bundle` of type `collection` or `searchset`. It resolves the references
between them inside the Bundle as FHIR R4 §2.36.4.1 does: a relative
`[type]/[id]` against the root of the referring entry's REST `fullUrl`, an
absolute reference against an entry's `fullUrl`. A reference it cannot
resolve is reported as outside the Bundle, never guessed. It refuses an entry
of another resource type, a resource with a `modifierExtension`, and a
repeated `fullUrl` or logical id. Each resource stays as `fhir-types` decodes
it, with accessors for what addressing reads; what a caller accepts as a
connection type, a status or an identifier is the caller's policy.

The other profile modules hold their place and land with their FerroFED issues
(<https://github.com/FerroHEALTH/FerroFED>).

## Licence

Business Source License 1.1 (`LICENSE`): free for every non-production use and
for non-commercial production use; a commercial licence for other production
use; Apache License 2.0 four years after each version.
