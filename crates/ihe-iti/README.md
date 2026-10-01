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

The other profile modules hold their place and land with their FerroFED issues
(<https://github.com/FerroHEALTH/FerroFED>).

## Licence

Business Source License 1.1 (`LICENSE`): free for every non-production use and
for non-commercial production use; a commercial licence for other production
use; Apache License 2.0 four years after each version.
