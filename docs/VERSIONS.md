<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Pinned version matrix

This file is the single source of truth for every version pin in FerroPIX.
When it and a file that repeats a pin disagree, that is drift. Fix the
disagreement; never let either side win silently.
`scripts/checks/versions.sh` reads the toolchain rows back against
`rust-toolchain.toml` and the root `Cargo.toml`, the console's build tools
against the files that install them, and every vendored tree under
`docs/specs/` against a row of the corpus table.

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
(`.claude/rules/console-ui.md`). The console's binary serves FerroUI's
stylesheet and script modules, so the console has no build tool of its own
beside cargo. `scripts/checks/versions.sh` holds the root
`Cargo.toml` pin to this row and the tag the FerroUI documents are read at
to `v` plus the pinned version.

| Item | Pin | Repeated in |
|---|---|---|
| FerroUI (`ferrohealth-ui`) | `=0.6.0` | `Cargo.toml` `[workspace.dependencies]`; `app/ferropix-console/src/pages/about.rs` (`FERROUI`, held to the pin by its test); the FerroUI documents read at the tag `v0.6.0` |

## The console's browser journeys

The browser journeys (`tools/ferropix-testkit`, #98) drive the console in
headless Chrome over WebDriver, through chromedriver, with the `thirtyfour`
client, a test-only dependency of the testkit. Chrome and chromedriver come
as one Chrome for Testing release, which `browser-actions/setup-chrome`
installs in the `journeys` job of `ci.yml`, so the browser and its driver
always match. Both pins are the ones FerroBRIDGE, FerroFED and FerroTERM
run. The weekly `scripts/checks/pin-freshness.sh` reports a newer Stable
release.

For a local run, `scripts/release/journey-tools.sh target/journey-tools`
installs the same release into `target/journey-tools`, where the journeys
look first. It takes each download URL from the Chrome for Testing JSON API
and checks each zip against the SHA-256 below before it unpacks it. Chrome
for Testing publishes no SHA-256; each digest is the one the family took of
a download whose MD5 equals the `x-goog-hash` the storage bucket serves with
it, and the `mac-arm64` pair verified again here on 2026-10-24.

| Item | Pin | Repeated in |
|---|---|---|
| thirtyfour | `0.38.1` | `Cargo.toml` |
| Chrome for Testing (Chrome and chromedriver) | `156.0.8078.47` | `.github/workflows/ci.yml`, `scripts/release/journey-tools.sh` |
| Chrome for Testing, `chrome-linux64.zip` SHA-256 | `e6c94490f0b6b337225aeb2d2a5034858516b7017356980ba94f5176d8774a50` | `scripts/release/journey-tools.sh` |
| Chrome for Testing, `chrome-mac-arm64.zip` SHA-256 | `ad7f2b49a4dc679af286b1b1b386670d93ca7868079f69012d21ba0ac8293df3` | `scripts/release/journey-tools.sh` |
| Chrome for Testing, `chromedriver-linux64.zip` SHA-256 | `2a5024610c53f340a1aaa5d9a3e2a4fa448be7d71cca5238bd8c3c7f16181df4` | `scripts/release/journey-tools.sh` |
| Chrome for Testing, `chromedriver-mac-arm64.zip` SHA-256 | `f42d25b984e2185ee48a628f0e76d49fcdf2fa83f20f7c286880dddeb4a28cc9` | `scripts/release/journey-tools.sh` |

`scripts/checks/versions.sh` reads each pin back from the files its row
names.

## The fuzz lane

`.github/workflows/fuzz.yml` runs the targets of `fuzz/` with cargo-fuzz,
whose sanitizer flags need a nightly toolchain. The lane is the one place
a nightly appears, and it is pinned by date so a weekly run builds what the
last one built; the pin-freshness lane reads it against the newest nightly
and cargo-fuzz against its newest release. Nothing built there ships.

| Item | Pin | Repeated in |
|---|---|---|
| Nightly toolchain (fuzz lane) | `nightly-2026-10-19` | `.github/workflows/fuzz.yml` |
| cargo-fuzz | `0.13.2` | `.github/workflows/fuzz.yml` |

## Container images

The images the testkit (`tools/ferropix-testkit`) starts behind
`FERROPIX_E2E`, each pinned by tag and by the digest of its image index,
resolved on 2026-10-24. The suite holds each `PinnedImage` constant of
`tools/ferropix-testkit/src/containers.rs` to its row here, and the
pin-freshness lane reads each row against its registry. PostgreSQL is the
official image at the store's major version; the stub OpenID Provider is
NAV's `mock-oauth2-server`, published under the MIT licence, public and
pulled with no credential. No specification governs the choice: our own
design.

| Item | Pin | Repeated in |
|---|---|---|
| PostgreSQL test image | `postgres:18.6@sha256:edb0c08dc33befd0262ab3b5cf1c4f3f8cfd8b09e88c05b45adee29d6761ddb6` | the `POSTGRES` constant in `tools/ferropix-testkit/src/containers.rs` |
| Stub OpenID Provider image | `ghcr.io/navikt/mock-oauth2-server:6.1.3@sha256:2a813381bf282da46ff36ff386059e7903e25865056af8149f4fb3f1441916b8` | the `STUB_PROVIDER` constant in `tools/ferropix-testkit/src/containers.rs` |

## The images and the release lanes

The images' base, the images of the compose quickstart, the
Dockerfile linter and the tools the release lanes install. Each image is
pinned by tag and by the digest of its image index, resolved on 2026-10-24;
the pin-freshness lane reads each against its registry, and the distroless
base, whose `nonroot` tag names no version, against the digest that tag
resolves to now. The quickstart's PostgreSQL is the testkit's pin, and its
development OpenID Provider is the testkit's stub provider. Trivy is
installed by the digest-pinned `aquasecurity/setup-trivy` at the version
each workflow's `TRIVY_VERSION` names; cargo-auditable, cargo-cyclonedx and
syft by the digest-pinned `taiki-e/install-action`, whose manifest holds
each at the newest release it carries. No specification governs the
choice: our own design, FerroBRIDGE's set.

| Item | Pin | Repeated in |
|---|---|---|
| distroless static base | `gcr.io/distroless/static-debian13:nonroot@sha256:18f4cc26c9ffa7f20a359298be7deadac5c607c8e03a02b8119f1a5fc2f82c99` | `docker/Dockerfile`, `docker/console/Dockerfile` |
| PostgreSQL quickstart image | `postgres:18.6@sha256:edb0c08dc33befd0262ab3b5cf1c4f3f8cfd8b09e88c05b45adee29d6761ddb6` | `compose.yaml` |
| Quickstart issuer image | `ghcr.io/navikt/mock-oauth2-server:6.1.3@sha256:2a813381bf282da46ff36ff386059e7903e25865056af8149f4fb3f1441916b8` | `compose.yaml` |
| hadolint image | `v2.15.1@sha256:32dac94127fd60b7b7e3fbfc65e1383b9b5e25c9bfd7b8536de7a539fe68a12d` | `.github/workflows/ci.yml` |
| Trivy | `0.76.1` | `.github/workflows/release.yml`, `.github/workflows/scan-and-tag.yml`, `.github/workflows/image-scan.yml` |
| cargo-auditable | `0.7.7` | `.github/workflows/release-build.yml`, `.github/workflows/release-console.yml` |
| cargo-cyclonedx | `0.5.9` | `.github/workflows/release-build.yml` |
| syft | `1.54.1` | `.github/workflows/release-build.yml`, `.github/workflows/release-image.yml`, `.github/workflows/release-console.yml` |

`scripts/checks/versions.sh` reads each pin back from the files its row
names, and holds the server image tag `compose.yaml` defaults to equal to
the workspace version.

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
| IHE ITI Technical Framework | Volumes 1 and 2, each page pinned by sha256 in the corpus table; the ITI-8, ITI-9 and ITI-10 pages state Revision 20.0, ITI-21 Revision 20.2 and ITI-22 Revision 20.1 | PIX and PDQ: ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22, and ITI-44 to ITI-47 as the TF-2 texts decide, with the actors and options of each |
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
| W3C DID Resolution 1.0 | `https://www.w3.org/TR/2026/CRD-did-resolution-1.0-20261026/` sha256 `2ca67fcccc42028c8f4dbf67560517d2c3218b09232cdd24f587e0456b647351`, W3C Candidate Recommendation Draft 2026-10-26 | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| W3C Bitstring Status List 1.0 | `https://www.w3.org/TR/2025/REC-vc-bitstring-status-list-20250515/` sha256 `3cdf3358a09f3b02f2a97e5dac13cf2b63115c2db1260bf7b7c236d8702924ec`, W3C Recommendation 2025-05-15 | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| W3C Web Services Addressing 1.0 SOAP Binding | `https://www.w3.org/TR/2006/REC-ws-addr-soap-20060509/` sha256 `6b1a44d6465c77deb9dbf6ad3fbfe435ece5b7b95b08a1782a81183f4bdc0931`, W3C Recommendation 2006-05-09 | `scripts/vendor/w3c-ws-addressing.sh`, `docs/specs/w3c-ws-addressing/PROVENANCE.md` |
| did:web Method Specification | `w3c-ccg/did-method-web` commit `ea423c114e6f2537498ee6f94e8d794c64f60c18`, the Credentials Community Group report and its licence | `scripts/vendor/w3c-did-vc.sh`, `docs/specs/w3c-did-vc/PROVENANCE.md` |
| DIF Presentation Exchange 2.0.0 | `decentralized-identity/presentation-exchange` commit `7cbe949c95fe1e19413b24c9b49dc34f76f5d76a`, the v2.0.0 specification text and JSON Schemas | `scripts/vendor/dif-pe.sh`, `docs/specs/dif-pe/PROVENANCE.md` |
| DIF Claim Format Registry | `decentralized-identity/claim-format-registry` commit `4a15817a7717efdda29912a1eef6e59135d8d02a`, the claim format designation schemas Presentation Exchange 2.0.0 references | `scripts/vendor/dif-pe.sh`, `docs/specs/dif-pe/PROVENANCE.md` |
| Mitz closed authorization question | pin-set digest `1f0196ab826e5093f6f7b8cbb935b106c22e19b6f9b246d4cce8c157602a14cd` over the VZVZ Confluence page `828314367` (space `MA11`, "Bijlage Architectuurdocumenten"), attachment version `1` of each document: `VZVZ_Mitz_Implementatiehandleiding_OpenGesloten_v3.8.2.pdf` sha256 `a5ce8f0d7eba8969a395a8560cf69f9e359f9da4c76145adc0748b9907ac0cf3`, `VZVZ_Mitz_PvE_AMC_Aansluiting_Mitz-connector_v3.8.1.ad1.pdf` sha256 `b1f18b48969475ce969472299179b37663cdcf67b067e9275532e0fa35bb59cb`, `VZVZ_Mitz_Implementatiehandleiding_Berichtauthenticatie_v3.8.1.ad1.pdf` sha256 `9659bdcd20a4deebc699357a455aaca7b3643c78a53b078329edcb899ef02f01`; no licence is stated, so the documents are pinned and fetched into the git-ignored `.vendor-cache/mitz/`, never committed | `scripts/vendor/mitz.sh`, `docs/specs/mitz/PROVENANCE.md` |
| IHE PIXm FHIR package, Swiss pin | package `ihe.iti.pixm` 3.0.4, pin-set digest `68b36cfa85cc04551e30b3628ce16218c1b980495c06bb835f34b0ccc706063a` | `scripts/vendor/ch.sh`, `docs/specs/ihe-pixm-ch/PROVENANCE.md` |
| IHE PDQm FHIR package, Swiss pin | package `ihe.iti.pdqm` 3.1.0, pin-set digest `ae4fb56c9eba92fcdf5637c86135618e37b60642c99013a8ca06ba747c9c4e73` | `scripts/vendor/ch.sh`, `docs/specs/ihe-pdqm-ch/PROVENANCE.md` |
| IHE IUA supplement, Swiss pin | `IHE/ITI.IUA` Revision 2.3, pin-set digest `2a5f13a87ccf307fecda8c48e77a61fd09c440089b2172be0edea912dea85439` | `scripts/vendor/ch.sh`, `docs/specs/ihe-iua-ch/PROVENANCE.md` |
| IHE ITI Technical Framework Volume 1 pages | ITI TF Revision 20.2 chapters 13, 18 and 27, pin-set digest `838b2f672e0bc34841d7fe297fd561c6f49c42fdd12b5a15119eb10a7234aeb7` | `scripts/vendor/ihe-iti-tf.sh`, `docs/specs/ihe-iti-tf/PROVENANCE.md` |
| IHE ITI Technical Framework Volume 2 pages | the ITI-38 page (Revision 20.2) and Appendix V (Revision 20.0), committed, and the ITI-55 page (Revision 20.1), cache only, pin-set digest `a0db03ab543e50569896e04fb4097bf6fd3a332d0b8106416b1d1de3d86572bf` | `scripts/vendor/ihe-iti-tf.sh`, `docs/specs/ihe-iti-tf-vol2/PROVENANCE.md` |
| IHE XCPD example messages | the twelve ITI-55 example files of IHE's implementation material (ITI TF-2 Appendix W), committed, pin-set digest `9ac6d57712fadf4d7b905b2bc5aa3596e7bbb7524cb3151e44230ab71e4e55e6` | `scripts/vendor/ihe-xcpd-examples.sh`, `docs/specs/ihe-xcpd-examples/PROVENANCE.md` |
| IHE ITI Technical Framework Volume 2 HL7 v2 pages | the ITI-8, ITI-9 and ITI-10 pages (Revision 20.0), the ITI-21 page (Revision 20.2) and the ITI-22 page (Revision 20.1), all cache only, pin-set digest `ff7b90dfcd52d54ab9bec50dcec64fdd0ac811a6b02f45d3314c01385dd9358d` | `scripts/vendor/ihe-iti-tf.sh`, `docs/specs/ihe-iti-tf-hl7v2/PROVENANCE.md` |

The weekly `scripts/checks/pin-freshness.sh` reads each W3C row against the
newest version the W3C API lists. W3C republishes the DID Resolution 1.0
Candidate Recommendation Draft almost daily, and `nl-generic-functions`
checks only that its vendored text is present, so that row is stale only
once a newer draft is more than 90 days ahead of the pin; within that lag
it reads as current, with the newer draft and its distance printed beside
it.

## The usage report contract

The installation report the server sends to FerroPULSE follows FerroPULSE's
report envelope v2. Its JSON Schema and the contract's own examples are
vendored by `scripts/vendor/ferropulse-schemas.sh` from this pin, and the
server's tests validate every report it builds against them.

| Item | Pin | Repeated in |
|---|---|---|
| FerroPULSE report envelope v2 | `FerroHEALTH/FerroPULSE` commit `208262ad71f6addbb97c4b8025fcb9c95b3751d0` path `schemas/report-envelope.v2.json` sha256 `e609f17a5bfa78c6c08bdd3a28025889e20e0fde5c72fa881e7ed47b6abc8bc7`, with the examples under `schemas/examples/report-envelope.v2/` | `scripts/vendor/ferropulse-schemas.sh`, `app/ferropix-server/tests/fixtures/ferropulse/PROVENANCE.md` (the contract is `docs/report-v2.md` at the same commit) |
