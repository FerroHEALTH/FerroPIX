<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Pinned version matrix

This file is the single source of truth for every version pin in FerroPIX.
When it and a file that repeats a pin disagree, that is drift. Fix the
disagreement; never let either side win silently.
`scripts/checks/versions.sh` reads the toolchain rows back against
`rust-toolchain.toml` and the root `Cargo.toml`, and every vendored tree
under `docs/specs/` against a row of the corpus table.

No specification governs this file; it is FerroPIX's own design.

## Toolchain

<!-- ANCHOR: toolchain -->
| Item | Pin | Repeated in |
|---|---|---|
| Rust toolchain | `1.99.1` | `rust-toolchain.toml` |
| Minimum supported Rust version | `1.98` | `Cargo.toml` `[workspace.package] rust-version` |
| Edition | `2024` | `Cargo.toml` `[workspace.package] edition`, `rustfmt.toml` |
<!-- ANCHOR_END: toolchain -->

## Documentation toolchain

The tools that build and check the site under `website/`.
`scripts/site/toolchain.sh` reads these rows, the Docs workflow and the CI
link check install exactly these versions, and `scripts/site/assemble.sh`
refuses another mdBook. They are the versions the FerroHEALTH family book
pins, so every book in the family builds with the same tools.

| Item | Pin | Repeated in |
|---|---|---|
| mdBook | `0.5.4` | `scripts/site/toolchain.sh` reads it |
| mdbook-lint | `0.14.4` | `scripts/site/toolchain.sh` reads it |
| lychee | `0.24.2` | `scripts/site/toolchain.sh` reads it |

## The console's component library

The operator console draws with FerroUI, pinned exactly and moved in
lockstep with every FerroHEALTH console on each release
(`.claude/rules/leptos-ui.md`).

| Item | Pin | Repeated in |
|---|---|---|
| FerroUI (`ferrohealth-ui`) | `=0.3.6` | `Cargo.toml` `[workspace.dependencies]`; `app/ferropix-console/src/pages/about.rs` (`FERROUI`, held to the pin by its test); the FerroUI documents read at the tag `v0.3.6` |

## Specifications

These are the specification versions the identity crates implement, at
FerroFED's pins (FerroFED's `docs/VERSIONS.md`, read on 2026-10-20). Each
vendored text is a row of the corpus table below.

<!-- ANCHOR: specifications -->
| Item | Version | Transactions |
|---|---|---|
| IHE PIXm | `ihe.iti.pixm` 3.1.0 | ITI-83 Mobile Patient Identifier Cross-reference Query, ITI-104 Patient Identity Feed FHIR |
| IHE PDQm | `ihe.iti.pdqm` 3.2.0 | ITI-78 Mobile Patient Demographics Query, ITI-119 Patient Demographics Match |
| IHE PMIR | `ihe.iti.pmir` 1.6.0 | ITI-93 Mobile Patient Identity Feed, ITI-94 Subscribe to Patient Updates |
| IHE mCSD | `ihe.iti.mcsd` 4.0.0 | ITI-90, ITI-91 (a feature of `ihe-iti`) |
| IHE XCPD | ITI Technical Framework Vol 2 Rev 20.1 (2024-12-12), no FHIR package | ITI-55 (a feature of `ihe-iti`) |
| IHE IUA | ITI Technical Framework Supplement Rev 2.5 (2026-06-18) | ITI-71, ITI-72 (a feature of `ihe-iti`) |
| Netherlands Generic Functions | `fhir.nl.gf` 0.3.0 | the functions of `nl-generic-functions` |
| IHE ITI Technical Framework | Volumes 1 and 2, the revision the move pins | PIX and PDQ: ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22, and ITI-44 to ITI-47 as the TF-2 texts decide, with the actors and options of each |
| HL7 FHIR | R4, 4.0.1 | the resources and the RESTful API PIXm and PDQm are profiled on |
| HL7 v2 | the version each PIX or PDQ transaction names in ITI TF-2 | the HL7 v2 PIX and PDQ transactions, where in scope |
<!-- ANCHOR_END: specifications -->

## Corpora

A corpus is pinned by commit, immutable publication or checksum, never by a
moving tag or a `latest` URL, and vendored by a committed `scripts/vendor/*.sh`
with a `PROVENANCE.md`. Every row is FerroFED's row for the same corpus,
copied verbatim at FerroFED `8cefc8f7` (2026-10-20), so the upstream files
here are byte-identical to FerroFED's copies (owner, 2026-10-20,
FerroHEALTH/FerroFED#367). A text whose licence does not allow
redistribution is fetched into the git-ignored `.vendor-cache/` and only its
`PROVENANCE.md` is committed.

| Item | Pin | Repeated in |
|---|---|---|
| IHE PIXm FHIR package | `ihe.iti.pixm` version `3.1.0` from `packages.fhir.org`, tarball sha256 `19e2e8eaf3030ac7b4d809c5e1eeb8face02c8635318aeb6d35bc2bb889de0d0`, the ITI-83 artefacts | `scripts/vendor/ihe-pixm.sh`, `docs/specs/ihe-pixm/PROVENANCE.md` |
| IHE PDQm FHIR package | `ihe.iti.pdqm` version `3.2.0` from `packages.fhir.org`, tarball sha256 `61e09fbee991ff7c131b6ba5474921001e07782209961f3e85cee5f3ebcaedc2`, the ITI-78 and ITI-119 artefacts | `scripts/vendor/ihe-pdqm.sh`, `docs/specs/ihe-pdqm/PROVENANCE.md` |
| IHE mCSD FHIR package | `ihe.iti.mcsd` version `4.0.0` from `packages.fhir.org`, tarball sha256 `933a143d7bb14c66731a32f52a084c6cb92476aca1b917db77a4640f8a5290ad`, the ITI-90 and ITI-91 artefacts | `scripts/vendor/ihe-mcsd.sh`, `docs/specs/ihe-mcsd/PROVENANCE.md` |
| IHE PMIR FHIR package | `ihe.iti.pmir` version `1.6.0` from `packages.fhir.org`, tarball sha256 `ec9d25fc64ac2f3087f921c14c0da56afc7e794caa80298db3f130bc0a40fe70`, the ITI-93 and ITI-94 artefacts | `scripts/vendor/ihe-pmir.sh`, `docs/specs/ihe-pmir/PROVENANCE.md` |
| IHE BALP FHIR package | `ihe.iti.balp` version `1.1.4` from `packages.fhir.org`, tarball sha256 `be46dda3088ee9d486d7163458dc5a4d7192e8a1c587fd47e2beb70e4bcefa90`, the whole package but its manifest | `scripts/vendor/ihe-balp.sh`, `docs/specs/ihe-balp/PROVENANCE.md` |
| IHE BALP narrative pages | the IG of package `ihe.iti.balp` 1.1.4 at `profiles.ihe.net/ITI/BALP/1.1.4/`, pin-set digest `f682913c6c5d5d2599c1dcb3c41a22373240b126a2051dab6b58ca8d56d1524d`, the Volume 1 (1:52) and Volume 3 (3:5.7) pages | `scripts/vendor/ihe-iti-pages.sh`, `docs/specs/ihe-balp-pages/PROVENANCE.md` |
| IHE ITI-20 Record Audit Event | `profiles.ihe.net/ITI/TF/Volume2/ITI-20.html`, Revision 20.2, page sha256 `881c7d6423fdf5ecaf4f9f50f8d25be61c3ed8ef97c87eff9591bd7fdf51570d` and its figure `media/Figure_3.20.4-1.png` sha256 `7aba1a2437e3492202460e150a6dda85b8a1886035bd2aa8daa4c892a579b734` | `scripts/vendor/ihe-atna.sh`, `docs/specs/ihe-atna/PROVENANCE.md` |
| IHE RESTful ATNA supplement | `IHE_ITI_Suppl_RESTful-ATNA.pdf` from `www.ihe.net`, Rev. 3.6, sha256 `d8451a4a0d951662b6a04b745084c33afff6196db5647f2cf79d9149dfa7265a` | `scripts/vendor/ihe-atna.sh`, `docs/specs/ihe-atna/PROVENANCE.md` |
| IHE IUA supplement | `IHE/ITI.IUA` tag `2.5`, the Revision 2.5 Trial Implementation supplement text (ITI-71, ITI-72, ITI-102, ITI-103) and its figures | `scripts/vendor/ihe-iua.sh`, `docs/specs/ihe-iua/PROVENANCE.md` |
| IHE PIXm narrative pages | the IG of package `ihe.iti.pixm` 3.1.0 at `profiles.ihe.net/ITI/PIXm/3.1.0/`, pin-set digest `b3cfa964a0c8813c068d0193bc35f5e606cead23e845a0de0925f041fe9ad2a1`, the Volume 1 (1:41), ITI-83 and ITI-104 pages | `scripts/vendor/ihe-iti-pages.sh`, `docs/specs/ihe-pixm-pages/PROVENANCE.md` |
| IHE PDQm narrative pages | the IG of package `ihe.iti.pdqm` 3.2.0 at `profiles.ihe.net/ITI/PDQm/3.2.0/`, pin-set digest `a3271aa6b45fad9ceb2da373c6d4cab0eb7649e06fcb278431f967bde976e0b6`, the Volume 1 (1:38), ITI-78 and ITI-119 pages | `scripts/vendor/ihe-iti-pages.sh`, `docs/specs/ihe-pdqm-pages/PROVENANCE.md` |
| IHE PMIR narrative pages | the IG of package `ihe.iti.pmir` 1.6.0 at `profiles.ihe.net/ITI/PMIR/1.6.0/`, pin-set digest `ba4747caf6acc383df4ffb041b253272ad58ead796345fb78dc38740f58c4b09`, the Volume 1 (1:49), ITI-93 and ITI-94 pages | `scripts/vendor/ihe-iti-pages.sh`, `docs/specs/ihe-pmir-pages/PROVENANCE.md` |
| IHE mCSD narrative pages | the IG of package `ihe.iti.mcsd` 4.0.0 at `profiles.ihe.net/ITI/mCSD/4.0.0/`, pin-set digest `982bda79f08b954960230d43f546ba15321a393236da2bcf4a5cbecfddfadacf`, the Volume 1 (1:46), ITI-90 and ITI-91 pages | `scripts/vendor/ihe-iti-pages.sh`, `docs/specs/ihe-mcsd-pages/PROVENANCE.md` |
| Netherlands Generic Functions IG source | `nuts-foundation/nl-generic-functions-ig` tag `v0.3.0`, commit `5367430787042c218996f11570f904bd3cd37a83`, the source of package `fhir.nl.gf` version `0.3.0`: the localization, consent, care services, identification and authentication pages (with the six GFI transactions) and the FSH profiles, capability statements and examples | `scripts/vendor/nl-gf.sh`, `docs/specs/nl-gf/PROVENANCE.md` |
| Nuts specifications | `nuts-foundation/nuts-specification` commit `7c0de533b8cce537812b6647542e5719c93433a8`, RFC003 (OAuth2 Authorization), RFC021 (VP Token Grant Type) and RFC022 (Discovery Service) | `scripts/vendor/nuts-rfc.sh`, `docs/specs/nuts-rfc/PROVENANCE.md` |
| IETF RFC 6749 | `https://www.rfc-editor.org/rfc/rfc6749.txt` sha256 `f204fc8661d6c92d2ec6e0b54808f961a9ad26e792f57f312d9528335519bd71`, The OAuth 2.0 Authorization Framework | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 7519 | `https://www.rfc-editor.org/rfc/rfc7519.txt` sha256 `fecd930e9ccf2276b95c0017c6c4ff5d09352e4bc3c7629946447894e0f97248`, JSON Web Token (JWT) | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 7521 | `https://www.rfc-editor.org/rfc/rfc7521.txt` sha256 `d5d97b3e691c9bbc495c277cc2cd79316b82486991468fc346a96ab59ba4b3c8`, Assertion Framework for OAuth 2.0 Client Authentication and Authorization Grants | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 7523 | `https://www.rfc-editor.org/rfc/rfc7523.txt` sha256 `ae24f77a8fc4338903c805c6ace38def1f23d40194aea87b123b13c5b3d2d915`, JWT Profile for OAuth 2.0 Client Authentication and Authorization Grants | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 7662 | `https://www.rfc-editor.org/rfc/rfc7662.txt` sha256 `2b7d688cb849f093e860557ac97e6cddac2556d69a4386561b22cdf97bf13657`, OAuth 2.0 Token Introspection | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 8414 | `https://www.rfc-editor.org/rfc/rfc8414.txt` sha256 `16c816e4e0fdbffb7e910ff3017867bf39debe9cb7f52f5cbc508a052ed660e8`, OAuth 2.0 Authorization Server Metadata | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 8693 | `https://www.rfc-editor.org/rfc/rfc8693.txt` sha256 `358d658014738bf4dc56c8aae2094ce716ede23b6d5a80488a7dc8e8d72da609`, OAuth 2.0 Token Exchange | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 8705 | `https://www.rfc-editor.org/rfc/rfc8705.txt` sha256 `6e45a1ee94c6a6a177a4cb077fe67d74bd70c1218c9362f2db2f153239612414`, OAuth 2.0 Mutual-TLS Client Authentication and Certificate-Bound Access Tokens | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 9126 | `https://www.rfc-editor.org/rfc/rfc9126.txt` sha256 `a79d0e30fcc24a22b79c8e18aa82362f6e63a7b8a5d58b480e746360e97388db`, OAuth 2.0 Pushed Authorization Requests | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 9396 | `https://www.rfc-editor.org/rfc/rfc9396.txt` sha256 `d6a8f032d8a585daae1c33a8c7b6e539d199f886ec8cc1c7898436f7f2eed29c`, OAuth 2.0 Rich Authorization Requests | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| IETF RFC 9449 | `https://www.rfc-editor.org/rfc/rfc9449.txt` sha256 `3842c58e1f6043389416023b9bb8d765048266024982fbbd90640e05943f4e13`, OAuth 2.0 Demonstrating Proof of Possession (DPoP) | `scripts/vendor/ietf-oauth.sh`, `docs/specs/ietf-oauth/PROVENANCE.md` |
| W3C Verifiable Credentials Data Model 1.1 | `https://www.w3.org/TR/2022/REC-vc-data-model-20220303/` sha256 `d7c796e1f7e9a233d9037eec03e9d9fe7d69220a71639b07f5038684949a3091`, W3C Recommendation 2022-03-03 | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| W3C Decentralized Identifiers 1.0 | `https://www.w3.org/TR/2022/REC-did-core-20220719/` sha256 `5e44345740d9bfaa852d3b66c57e98c9beb6c5bf6083b0126dd5daac377b9993`, W3C Recommendation 2022-07-19 | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| W3C DID Resolution 1.0 | `https://www.w3.org/TR/2026/CRD-did-resolution-1.0-20261001/` sha256 `2b194843d37f5b7f01a4f5464f64d672dc4be43dafe5f2f49114c6070819586e`, W3C Candidate Recommendation Draft 2026-10-01 | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| W3C Bitstring Status List 1.0 | `https://www.w3.org/TR/2025/REC-vc-bitstring-status-list-20250515/` sha256 `3cdf3358a09f3b02f2a97e5dac13cf2b63115c2db1260bf7b7c236d8702924ec`, W3C Recommendation 2025-05-15 | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| did:web Method Specification | `w3c-ccg/did-method-web` commit `ea423c114e6f2537498ee6f94e8d794c64f60c18`, the Credentials Community Group report and its licence | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| DIF Presentation Exchange 2.0.0 | `decentralized-identity/presentation-exchange` commit `7cbe949c95fe1e19413b24c9b49dc34f76f5d76a`, the v2.0.0 specification text and JSON Schemas | `scripts/vendor/dif-pe.sh`, `docs/specs/dif-pe/PROVENANCE.md` |
| DIF Claim Format Registry | `decentralized-identity/claim-format-registry` commit `4a15817a7717efdda29912a1eef6e59135d8d02a`, the claim format designation schemas Presentation Exchange 2.0.0 references | `scripts/vendor/dif-pe.sh`, `docs/specs/dif-pe/PROVENANCE.md` |
| Mitz closed authorization question | pin-set digest `1f0196ab826e5093f6f7b8cbb935b106c22e19b6f9b246d4cce8c157602a14cd` over the VZVZ Confluence page `828314367` (space `MA11`, "Bijlage Architectuurdocumenten"), attachment version `1` of each document: `VZVZ_Mitz_Implementatiehandleiding_OpenGesloten_v3.8.2.pdf` sha256 `a5ce8f0d7eba8969a395a8560cf69f9e359f9da4c76145adc0748b9907ac0cf3`, `VZVZ_Mitz_PvE_AMC_Aansluiting_Mitz-connector_v3.8.1.ad1.pdf` sha256 `b1f18b48969475ce969472299179b37663cdcf67b067e9275532e0fa35bb59cb`, `VZVZ_Mitz_Implementatiehandleiding_Berichtauthenticatie_v3.8.1.ad1.pdf` sha256 `9659bdcd20a4deebc699357a455aaca7b3643c78a53b078329edcb899ef02f01`; no licence is stated, so the documents are pinned and fetched into the git-ignored `.vendor-cache/mitz/`, never committed | `scripts/vendor/mitz.sh`, `docs/specs/mitz/PROVENANCE.md` |
| IHE PIXm FHIR package, Swiss pin | package `ihe.iti.pixm` 3.0.4, pin-set digest `68b36cfa85cc04551e30b3628ce16218c1b980495c06bb835f34b0ccc706063a` | `scripts/vendor/ch.sh`, `docs/specs/ihe-pixm-ch/PROVENANCE.md` |
| IHE PDQm FHIR package, Swiss pin | package `ihe.iti.pdqm` 3.1.0, pin-set digest `ae4fb56c9eba92fcdf5637c86135618e37b60642c99013a8ca06ba747c9c4e73` | `scripts/vendor/ch.sh`, `docs/specs/ihe-pdqm-ch/PROVENANCE.md` |
| IHE IUA supplement, Swiss pin | `IHE/ITI.IUA` Revision 2.3, pin-set digest `2a5f13a87ccf307fecda8c48e77a61fd09c440089b2172be0edea912dea85439` | `scripts/vendor/ch.sh`, `docs/specs/ihe-iua-ch/PROVENANCE.md` |
| IHE ITI Technical Framework Volume 1 pages | ITI TF Revision 20.2 chapters 13, 18 and 27, pin-set digest `838b2f672e0bc34841d7fe297fd561c6f49c42fdd12b5a15119eb10a7234aeb7` | `scripts/vendor/ihe-iti-tf.sh`, `docs/specs/ihe-iti-tf/PROVENANCE.md` |
| IHE ITI Technical Framework Volume 2 pages | the ITI-38 page (Revision 20.2), committed, and the ITI-55 page (Revision 20.1), cache only, pin-set digest `84c3ff8cfad148b5c066ae364d1f8a3044c6210ed806e9b87f363d8dda2d5546` | `scripts/vendor/ihe-iti-tf.sh`, `docs/specs/ihe-iti-tf-vol2/PROVENANCE.md` |
