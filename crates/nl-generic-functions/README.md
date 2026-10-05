<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# nl-generic-functions

The Netherlands Generic Functions for health data exchange (the Nuts
`nl-generic-functions-ig`, package `fhir.nl.gf` 0.3.0) in Rust: one crate for
the implementation guide, with a feature per function.

| Feature | Generic Function | Service | What the crate holds |
|---|---|---|---|
| `nvi` | GF-Localization | the national index (NVI) | the Localization Service search: which care providers (by URA) hold data for a patient (by pseudonymised BSN), each request authenticated by the transport, a default header, or an authorizer the caller supplies that makes its headers, as a `DPoP`-bound token needs |
| `mitz` | GF-Consent | Mitz | the closed authorization question ("gesloten autorisatievraag"): may a data holder make a patient's data of each category available to a data user, asked on the BSN and answered `Permit` or `Deny` |
| `lrza` | GF-Addressing | the national address book (LRZa) | the URA of an NL-GF `Organization`, so a directory reader can join a localization custodian to its organisation |
| `nuts-auth` | GF-Authentication | the Nuts profile | the access token request of Nuts RFC021: the authorization server metadata, the Presentation Definition for a scope, a JWT Verifiable Presentation of the holder's credentials signed with its `did:web` key, and a `DPoP`-bound token, proven through a prover the caller supplies; and the holder's DID document, built from its keys, with the path its `did:web` DID resolves to |

The identifier systems the functions share (the pseudonymised BSN and the
URA) are in `identification`, compiled with `nvi`, `lrza` or `mitz`.

The NVI client never puts the pseudonym in an error, a `Debug` rendering or
anything but the request to the service, and it follows a `next` link only on
its own search endpoint. An authorizer is handed the request URL without its
query, so the pseudonym never reaches it.

The Nuts client never puts a credential, the presentation, a key, a proof or
the token in an error or a `Debug` rendering, and sends no credential before
the authorization server's Presentation Definition says they answer it.

The Mitz client follows the VZVZ *Implementatiehandleiding Open en gesloten
autorisatievraag* 3.8.2: a SOAP 1.2 request carrying one XACML 3.0
`XACMLAuthzDecisionQuery`, over mutual TLS. Only `Permit` and `Deny` are
decisions; `Indeterminate`, `NotApplicable`, a fault, a timeout and an answer
that does not hold to the question are typed errors. The BSN travels only in
the request to Mitz, and the client follows no redirect.

The crate depends on no application. The implementation guide is published at
<https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>.

## Licence

Business Source License 1.1 (`LICENSE`): free for every non-production use and
for non-commercial production use; a commercial licence for other production
use; Apache License 2.0 four years after each version.
