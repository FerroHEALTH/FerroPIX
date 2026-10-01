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
| `pmir` | Patient Master Identity Registry | ITI-93, ITI-104 |
| `xcpd` | Cross-Community Patient Discovery | ITI-55 |

The crate depends on no application, so a federation gateway, a master patient
index or any other caller can use it as it is. Only the `xcpd` feature may carry
the SOAP 1.2, HL7 v3 and SAML XUA stack; a build without it compiles none of it.
The profiles are published at <https://profiles.ihe.net/ITI/>.

The profile modules hold their place and land with their FerroFED issues
(<https://github.com/FerroHEALTH/FerroFED>).

## Licence

Business Source License 1.1 (`LICENSE`): free for every non-production use and
for non-commercial production use; a commercial licence for other production
use; Apache License 2.0 four years after each version.
