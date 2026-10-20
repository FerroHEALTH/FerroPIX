<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the Nuts specifications

Vendored verbatim by `scripts/vendor/nuts-rfc.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://github.com/nuts-foundation/nuts-specification>
- Pin: commit `7c0de533b8cce537812b6647542e5719c93433a8`
- Documents: *RFC003 OAuth2 Authorization*, *RFC021 VP Token Grant Type* and
  *RFC022 Discovery Service*, published by the Nuts foundation as Requests for
  Comments
- Fetched: 2026-10-20
- Upstream licence: the repository has no licence file. Each document states
  its licence in its Copyright Notice, verbatim: "This document is released under the [Attribution-ShareAlike 4.0 International \(CC BY-SA 4.0\) license](https://creativecommons.org/licenses/by-sa/4.0/)."
  (<https://creativecommons.org/licenses/by-sa/4.0/legalcode>). The files are
  redistributed unmodified, with that notice, so no adaptation is made.
- Layout: the upstream paths, unchanged
- Files: 3
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `e7db37cb88a230b634bc7a54385e020ff15dc85b2ac791a0fd88e17aca676b21`
- Read by: the access token request of `crates/nl-generic-functions`
  feature `nuts-auth`, held to RFC021

## What is taken

The three documents GF-Authentication on the Nuts profile rests on: RFC021, the grant the client side sends; RFC003, the
authorization server and access token it replaces; and RFC022, the service
discovery a participant reaches another's authorization server through. The
other RFCs (the network, the registry, the credential formats of the earlier
Nuts node) and the repository's images serve no reader here and are not
taken.

| File | sha256 | git blob id |
|---|---|---|
| `rfc/rfc003-oauth2-authorization.md` | `196c9c245a011be260a918c323f1a024ecb778f1249ba850cbc6e5d1174b00f4` | `35ed40548f8a551e5dd430f166e2e3f5212effb5` |
| `rfc/rfc021-vp_token-grant-type.md` | `b21c77f293f5dfc9b282dd78e3c17a7e3fb4e2dea21f259ffa754730a1045fe8` | `892d38ff4439a1aeab2e1e796351be166cf3c3a9` |
| `rfc/rfc022-discovery-service.md` | `ec4dea334cc757d252916d35d75318b5579a747713599f06fc047998fca36d1d` | `4d038d9084fdc1bd8cd820f83591d4faacfee606` |
