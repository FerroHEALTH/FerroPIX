<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IETF OAuth RFCs

Vendored verbatim by `scripts/vendor/ietf-oauth.sh`. Never edit a file here:
change the pins in docs/VERSIONS.md and re-run the script.

- Source: the RFC Editor, <https://www.rfc-editor.org/>, the plain-text
  publication of each RFC
- Pins, one row of docs/VERSIONS.md per RFC, by URL and sha256:
- RFC 6749: <https://www.rfc-editor.org/rfc/rfc6749.txt>, sha256 `f204fc8661d6c92d2ec6e0b54808f961a9ad26e792f57f312d9528335519bd71`
- RFC 7519: <https://www.rfc-editor.org/rfc/rfc7519.txt>, sha256 `fecd930e9ccf2276b95c0017c6c4ff5d09352e4bc3c7629946447894e0f97248`
- RFC 7521: <https://www.rfc-editor.org/rfc/rfc7521.txt>, sha256 `d5d97b3e691c9bbc495c277cc2cd79316b82486991468fc346a96ab59ba4b3c8`
- RFC 7523: <https://www.rfc-editor.org/rfc/rfc7523.txt>, sha256 `ae24f77a8fc4338903c805c6ace38def1f23d40194aea87b123b13c5b3d2d915`
- RFC 7662: <https://www.rfc-editor.org/rfc/rfc7662.txt>, sha256 `2b7d688cb849f093e860557ac97e6cddac2556d69a4386561b22cdf97bf13657`
- RFC 8414: <https://www.rfc-editor.org/rfc/rfc8414.txt>, sha256 `16c816e4e0fdbffb7e910ff3017867bf39debe9cb7f52f5cbc508a052ed660e8`
- RFC 8693: <https://www.rfc-editor.org/rfc/rfc8693.txt>, sha256 `358d658014738bf4dc56c8aae2094ce716ede23b6d5a80488a7dc8e8d72da609`
- RFC 8705: <https://www.rfc-editor.org/rfc/rfc8705.txt>, sha256 `6e45a1ee94c6a6a177a4cb077fe67d74bd70c1218c9362f2db2f153239612414`
- RFC 9126: <https://www.rfc-editor.org/rfc/rfc9126.txt>, sha256 `a79d0e30fcc24a22b79c8e18aa82362f6e63a7b8a5d58b480e746360e97388db`
- RFC 9396: <https://www.rfc-editor.org/rfc/rfc9396.txt>, sha256 `d6a8f032d8a585daae1c33a8c7b6e539d199f886ec8cc1c7898436f7f2eed29c`
- RFC 9449: <https://www.rfc-editor.org/rfc/rfc9449.txt>, sha256 `3842c58e1f6043389416023b9bb8d765048266024982fbbd90640e05943f4e13`
- Fetched: 2026-10-20
- Upstream licence: each RFC carries the notice "This document is subject to BCP 78 and the IETF Trust's Legal
  Provisions Relating to IETF Documents (https://trustee.ietf.org/license-info)
  in effect on the date of publication of this document". The IETF Trust
  Legal Provisions 5.0, §3.c, grant every person "a non-exclusive,
  royalty-free, worldwide right and license under all copyrights and rights
  of authors: i. to copy, publish, display and distribute IETF Contributions
  and IETF Documents in full and without modification". The files are those
  documents, in full and unmodified.
- Layout: `rfc<number>.txt`, the RFC Editor's file name
- Files: 11
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `36413b343e1f4e81e08bfea0ff643480c8503ad646f6a3c9fad50b93f32220d6`
- Read by: the access token request of `crates/nl-generic-functions`
  feature `nuts-auth`: the authorization server metadata of RFC 8414 and
  the DPoP proof of RFC 9449, whose texts its corpus tests read

## What is taken

The RFCs the GF-Authentication pages of the Netherlands Generic Functions IG
cite (6749, 7523, 7662, 9449), the ones Nuts RFC021 builds its grant on
(7519, 7521, 8414), and the four FerroFED's onward grants read: RFC 9126
and RFC 9396 of its BgZ/eOverdracht track, RFC 8705 for mutual TLS, and RFC
8693, whose `act` claim names the party a token's subject delegated to. The other RFCs
the OAuth family references are cited, not taken.

| File | sha256 |
|---|---|
| `rfc6749.txt` | `f204fc8661d6c92d2ec6e0b54808f961a9ad26e792f57f312d9528335519bd71` |
| `rfc7519.txt` | `fecd930e9ccf2276b95c0017c6c4ff5d09352e4bc3c7629946447894e0f97248` |
| `rfc7521.txt` | `d5d97b3e691c9bbc495c277cc2cd79316b82486991468fc346a96ab59ba4b3c8` |
| `rfc7523.txt` | `ae24f77a8fc4338903c805c6ace38def1f23d40194aea87b123b13c5b3d2d915` |
| `rfc7662.txt` | `2b7d688cb849f093e860557ac97e6cddac2556d69a4386561b22cdf97bf13657` |
| `rfc8414.txt` | `16c816e4e0fdbffb7e910ff3017867bf39debe9cb7f52f5cbc508a052ed660e8` |
| `rfc8693.txt` | `358d658014738bf4dc56c8aae2094ce716ede23b6d5a80488a7dc8e8d72da609` |
| `rfc8705.txt` | `6e45a1ee94c6a6a177a4cb077fe67d74bd70c1218c9362f2db2f153239612414` |
| `rfc9126.txt` | `a79d0e30fcc24a22b79c8e18aa82362f6e63a7b8a5d58b480e746360e97388db` |
| `rfc9396.txt` | `d6a8f032d8a585daae1c33a8c7b6e539d199f886ec8cc1c7898436f7f2eed29c` |
| `rfc9449.txt` | `3842c58e1f6043389416023b9bb8d765048266024982fbbd90640e05943f4e13` |
