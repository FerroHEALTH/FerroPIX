<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Changelog
<!-- ANCHOR: book -->

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The documentation site at <https://ferropix.eu/>: a landing page in the
  FerroHEALTH design with the FerroPIX brand, and the book under `/docs/`
  (what FerroPIX is and what is built or planned, getting started, the
  published crates with every feature, configuration, deployment, security
  and audit, operations, the specification pins, contributing and this
  changelog), built and link-checked on every pull request and deployed
  from `main` by the Docs workflow. The brand gains a favicon set and a mark
  for a dark ground (#45, #46, #47, #49).

### Changed

- The repository builds with Rust 1.99.1 in place of 1.98.1, and the MSRV
  stays at 1.98: the FerroHEALTH family builds with the latest stable
  toolchain and declares the release before it as its MSRV. No published
  crate changes.
- `ihe-iti` 0.0.40 splits the `audit` feature in two: `balp`, the BALP
  `AuditEvent` of the PIXm, PDQm, mCSD and PMIR transactions on
  `ihe-atna/balp`, and `atna`, the ITI-55 audit message of `xcpd` on
  `ihe-atna/atna`, which compiles no FHIR model. A product that enabled
  `audit` enables `balp`, `atna` or both. Published between releases (#32).
- `ihe-iti` 0.0.41 takes `ihe-atna = "=0.2.1"` and drops its own copies of
  the record bound, the URL redaction and the FHIR base join: they are
  `ihe-atna`'s `balp::recording`, `redact` and `fhir_base` (FerroHEALTH/FerroAUDIT#8,
  #9, #61), taken through `ihe-atna`'s `recording` and `redact` features, so
  a profile without `balp` or `atna` takes no audit stack. **API breaks:**
  `ihe_iti::recording` is gone (use `ihe_atna::balp::recording`, whose `Late`
  is made with `Late::new()`); the `EventKind` constants of the audit modules
  are functions, since `ihe-atna` 0.2's `EventKind` holds a `Vec`
  (`pixm::audit::query_consumer()`, `pdqm::audit::query_consumer()` and
  `match_consumer()`, `mcsd::audit::query()` and `updates()`,
  `pmir::audit::feed_kind()`, `subscription_create()`, `subscription_read()`,
  `subscription_delete()` and `subscription_search()`). A product that
  implements `ihe_atna::balp::AuditRecorder` for `ihe-iti`'s clients moves
  to `ihe-atna = "=0.2.1"` with it (#35).
- `nl-generic-functions` 0.0.26 takes `ferrohealth-smart = "=0.4.0"`, so a
  product resolves one copy of it. The three checks the Nuts grant adds to
  the RFC 8414 issuer rule (the exact canonical issuer text, every endpoint
  on the issuer's origin, and a refusal of JSON that repeats a name) stay
  private in `nuts_auth`, as 0.4.0 does not carry them (#36).
- `ihe-iti` 0.0.42 takes `ihe-atna = "=0.2.2"`, which changes only the
  declared MSRV, raising it to 1.98; a product that takes `ihe-iti` moves to `ihe-atna
  =0.2.2` with it (#39).
- `ihe-iti` 0.0.43 takes `ihe-atna = "=0.2.3"`, the only supported release;
  0.0.42 was not published (#41).
- `ihe-iti` 0.0.44 takes `ihe-atna = "=0.2.4"`, the only supported release
  (#43).

## [0.0.2] - 2026-10-20

The move from FerroFED (FerroHEALTH/FerroFED#367): `ihe-iti` 0.0.39 and
`nl-generic-functions` 0.0.25 are published from FerroPIX, each copied with
its history and with the corpora it reads re-vendored at FerroFED's pins.
FerroFED pins both by exact version from crates.io.

### Added

- `ihe-iti` 0.0.39, moved from FerroFED with its history: the IHE PIXm,
  PDQm, PMIR, mCSD and XCPD clients and the IUA authorizer, a feature each.
  The `atna` and `balp` features are gone: the audit trail is the `ihe-atna`
  crate of FerroAUDIT. The audit record each profile defines for its own
  transactions stays here behind the new `audit` feature, on `ihe-atna`
  `=0.1.0`. A product changes `ihe_iti::user` to `ihe_atna::user`,
  `ihe_iti::balp` and `ihe_iti::atna` to `ihe_atna::balp` and
  `ihe_atna::atna`, and the features `balp` and `atna` to `audit` (#25).
- `nl-generic-functions` 0.0.25, moved from FerroFED with its history: NVI
  localization, the Mitz closed authorization question, LRZa addressing and
  the Nuts access token request, a feature each. `nuts-auth` no longer
  depends on `oauth-server-metadata`, which is retired: it takes the RFC
  8414 issuer rule and metadata location from `ferrohealth-smart` `=0.3.1`.
  A plain `http` issuer is now accepted on a loopback host only (#26).
- The IHE citation guard, `scripts/checks/ihe-citations.sh`, and the corpora
  its audit citations read at FerroFED's pins: the BALP package and
  narrative pages and the ITI-20 page with the RESTful ATNA supplement
  (#25).

- The corpora `ihe-iti` and `nl-generic-functions` read, vendored under
  `docs/specs/` by FerroFED's scripts at FerroFED's pins and byte-identical
  to FerroFED's copies: the IHE PIXm, PDQm, PMIR and mCSD packages and
  narrative pages, the ITI Technical Framework pages, the IUA supplement,
  the Swiss pins of PIXm, PDQm and IUA, the Netherlands Generic Functions IG
  source, the Mitz documents (cache only), the Nuts RFCs, the IETF OAuth
  RFCs, the W3C DID and VC texts and DIF Presentation Exchange, each with
  its `scripts/vendor/*.sh`, its `PROVENANCE.md` and its row in
  `docs/VERSIONS.md` (#24).

### Changed

- The minimum supported Rust version is 1.98, the version `ihe-atna`
  requires (#25).
- Two version lines, the model FerroBRIDGE uses: the product version is the
  workspace version and the release tag, and each published crate keeps its
  own version, which never adopts the product version. The crate-version
  guard holds both, and refuses a change to a crate's packaged content that
  does not bump it unless the pull request carries `no-crate-bump`. The
  release lane names each crate's files by the crate's own version (#23).

## [0.0.1] - 2026-10-20

The repository setup: the working discipline, the tracker helpers and the
guards, CI and the release lane, the workspace skeleton and the GitHub
settings. No crate is published in this release; the first crates arrive
with the move from FerroFED in 0.0.2.

### Added

- The tracker helpers under `scripts/gh/` (`fields.sh`, `rel.sh`,
  `labels.sh`, `watch-prs.sh`, `changelog-merge.sh`, and `project.sh`, which
  says FerroPIX has no roadmap board) and the guards under `scripts/checks/`
  (comment style, the pin matrix and licence headers, file length, `TODO`
  pointers, the crate version, the changelog and the contribution licence),
  each with a `--self-test` (#11).
- CI with the single required `conclusion` check, CodeQL, OpenSSF Scorecard,
  Dependabot, the release lane that refuses a tag whose milestone is not
  empty and closes it after the release, the crates.io lane through Trusted
  Publishing, and the pull request and issue templates (#12).
- `docs/VERSIONS.md`, the pin matrix the guards read (#11).
- The working discipline: `CLAUDE.md` with the scope the owner settled on
  2026-10-20, the rules, the `spec-researcher` and `implementer` agents, the
  `spec-lookup`, `next-task`, `phase-done` and `phase-status` skills, the
  hooks (the destructive-command hook with its self-test) and the memory
  (#13).
- The specification versions of every identity profile that moves from
  FerroFED in `docs/VERSIONS.md` (#13).
- `SUPPORT.md`, `GOVERNANCE.md` and `CODE_OF_CONDUCT.md`, and a README and
  CONTRIBUTING.md that name what FerroPIX is, how to build and test it and
  the specifications that govern it (#14).
- The owner's rule for moving the vendored corpora with the crates, in
  `.claude/rules/vendored-inputs.md` and the memory (#20).
- The Cargo workspace skeleton: the root manifest with the family lint set
  and the release profile, the pinned toolchain, and the cargo-deny, clippy
  and rustfmt configuration. The workspace has no member until the move from
  FerroFED; CI reads the manifest under the pinned toolchain meanwhile (#15).

### Changed

- `SECURITY.md` supports only the newest release, with the Cyber Resilience
  Act commitment to publish security fixes for at least five years after
  each release (#14).
