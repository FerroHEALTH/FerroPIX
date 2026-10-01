<!-- SPDX-FileCopyrightText: Vernum Projecten B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# nl-generic-functions

The Netherlands Generic Functions for health data exchange (the Nuts
`nl-generic-functions-ig`) in Rust: one crate for the implementation guide,
with a feature per function.

| Feature | Generic Function | Service |
|---|---|---|
| `nvi` | GF-Localization | the national index (NVI) |
| `mitz` | GF-Consent | Mitz |
| `lrza` | GF-Addressing | the national address book (LRZa) |
| `nuts-auth` | GF-Authentication | the Nuts profile |

The crate depends on no application. The implementation guide is published at
<https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>.

The function modules hold their place and land with their FerroFED issues
(<https://github.com/FerroHEALTH/FerroFED>).

## Licence

Business Source License 1.1 (`LICENSE`): free for every non-production use and
for non-commercial production use; a commercial licence for other production
use; Apache License 2.0 four years after each version.
