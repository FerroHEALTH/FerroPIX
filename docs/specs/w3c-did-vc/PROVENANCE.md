<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the W3C identifier and credential specifications

Vendored verbatim by `scripts/vendor/w3c-did-vc.sh`. Never edit a file here:
change the pins in docs/VERSIONS.md and re-run the script.

- Source: the W3C Technical Reports, <https://www.w3.org/TR/>, each at its
  dated URL, and the Credentials Community Group repository
  <https://github.com/w3c-ccg/did-method-web>
- Pins, one row of docs/VERSIONS.md each:
- W3C Verifiable Credentials Data Model 1.1: <https://www.w3.org/TR/2022/REC-vc-data-model-20220303/>, sha256 `d7c796e1f7e9a233d9037eec03e9d9fe7d69220a71639b07f5038684949a3091`
- W3C Decentralized Identifiers 1.0: <https://www.w3.org/TR/2022/REC-did-core-20220719/>, sha256 `5e44345740d9bfaa852d3b66c57e98c9beb6c5bf6083b0126dd5daac377b9993`
- W3C DID Resolution 1.0: <https://www.w3.org/TR/2026/CRD-did-resolution-1.0-20261001/>, sha256 `2b194843d37f5b7f01a4f5464f64d672dc4be43dafe5f2f49114c6070819586e`
- W3C Bitstring Status List 1.0: <https://www.w3.org/TR/2025/REC-vc-bitstring-status-list-20250515/>, sha256 `3cdf3358a09f3b02f2a97e5dac13cf2b63115c2db1260bf7b7c236d8702924ec`
- did:web Method Specification: commit `ea423c114e6f2537498ee6f94e8d794c64f60c18` of `w3c-ccg/did-method-web`, its
  `index.html` (the report's source) and `LICENSE.md`
- Fetched: 2026-10-20
- Upstream licence: the W3C documents carry the W3C copyright notice and
  name the W3C Software and Document License, the 2015 text
  (<https://www.w3.org/Consortium/Legal/2015/copyright-software-and-document>) for the two 2022 Recommendations and the 2023 text
  (<https://www.w3.org/copyright/software-license-2023/>) for the later documents; both permit
  copying and distributing the documents in any medium for any purpose,
  provided the URL of the original, the copyright notice and the status of the
  document are kept; each file is the original, unmodified, with all three.
  The did:web repository's `LICENSE.md`, vendored in
  `did-method-web/`, licenses its reports under the same licence.
- Layout: one file per W3C document, named for it; `did-method-web/` at the
  repository's own paths
- Files: 6
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `601a111d016a24f1f0acfe10cfbde2b416b728e1355771ee7652a872424e9399`
- Read by: the Verifiable Presentation of the access token request of
  `crates/nl-generic-functions` feature `nuts-auth`: VC Data Model 1.1
  §4.10 and §6.3.1, and the did:web holder identifier

## What is taken

The documents the GF-Authentication pages of the Netherlands Generic
Functions IG cite for the entity identifier (DID 1.0, the did:web method and
DID Resolution, GFI-001), the credential and its presentation (VC Data Model
1.1, GFI-002 and GFI-004) and revocation (Bitstring Status List 1.0,
GFI-003). DID Resolution and Bitstring Status List are the verifier's side of
the exchange; they are taken because the IG binds them, so the record of
what the track requires is complete.

| File | sha256 |
|---|---|
| `did-core-1.0.html` | `5e44345740d9bfaa852d3b66c57e98c9beb6c5bf6083b0126dd5daac377b9993` |
| `did-method-web/LICENSE.md` | `cad8da6404199f732387340029f0f91d55acb98092bf45089064b4a9b8bb0295` |
| `did-method-web/index.html` | `7e48b1ad2d658452bc9ce23b975d243abe095f7696c06671fc332705a1290610` |
| `did-resolution-1.0.html` | `2b194843d37f5b7f01a4f5464f64d672dc4be43dafe5f2f49114c6070819586e` |
| `vc-bitstring-status-list-1.0.html` | `3cdf3358a09f3b02f2a97e5dac13cf2b63115c2db1260bf7b7c236d8702924ec` |
| `vc-data-model-1.1.html` | `d7c796e1f7e9a233d9037eec03e9d9fe7d69220a71639b07f5038684949a3091` |
