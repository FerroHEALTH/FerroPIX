<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->
<!-- The [Unreleased] sections of CHANGELOG.md at the merge base, the head and main of #942, with a stub released tail. -->

# Changelog

## [Unreleased]

### Added

- The facade's subject search parameters take the `:identifier` modifier
  (#923): `Condition?patient:identifier=[system]|[value]` matches the
  `Reference.identifier` of a resource's subject (R4 search.html
  §3.1.1.4.12). That identifier is the subject the identity map keys by
  system and value, which the facade renders as an identifier. A subject
  rendered as a literal reference carries none and matches nothing, and so
  does `|[value]`. `[value]` alone and `[system]|` answer `400 not-supported`,
  because the map keys a subject by system and value. No specification
  governs the identity map: our own design.
- The FHIR facade serves `Patient`, `Practitioner`, `PractitionerRole` and
  `Organization` from party contexts over the ITS-REST 1.1.0 Demographic API,
  DEVELOPMENT, behind `[cdr] demographic` (#470). Read, vread,
  `GET [type]/[id]/_history`, create, update (over `If-Match` or the latest
  version) and `DELETE [type]/[id]`, which answer `not-supported` for a type no
  party context serves; a party context with the key off is a boot refusal. The
  id is the party's `versioned_object_uid` and `meta.versionId` its version
  tree id, with no identity-map entry; a Demographic API `400` answers `500`,
  and `404`, `409`, `412` and `422` answer as themselves. The
  `CapabilityStatement` declares the six interactions and no search per party
  type. A `PractitionerRole` is not created or updated yet (#911). No
  specification governs a party context: our own design.

- The facade's search chains through the patient identifier (#492):
  `Observation?patient.identifier=MRN-1`, `subject.identifier` and
  `subject:Patient.identifier`, on every type a search declaration answers.
  With `[facade.identifier_search] enabled`, a configured demographic AQL
  query in Cadasto's experimental extension resolves the identifier to one
  party, the party's uid names the patient, and the EHR comes from the
  identity map or `GET /ehr?subject_id=`. An unknown identifier answers an
  empty searchset with no clinical query, two parties answer `400
  multiple-matches` asking for a system, and `_count=0` answers the total
  alone. A value naming a system needs `system_query`. The
  `CapabilityStatement` documents the chain on each subject parameter while
  it is on. No specification governs the resolution: our own design.
- A party program runs through the engine (#905). `WebTemplateIndex::
  build_party` builds a canonical `PERSON`, `ORGANISATION` or `ROLE` from the
  node values a party context writes, through the `openehr-rm` types: the
  objects between two template nodes are the ones the OPT 1.4 constrains
  there, named by their rubric, every list attribute is an array, no
  composition attribute is written, and the result decodes strictly as the
  root's class (`PartyError::Invalid` otherwise). `openehr-sdt` 0.0.87 builds
  no correct party (#897, FerroHEALTH/FerroEHR#3810). `fhirconnect`'s
  `engine::traverse::party::to_party` and `from_party` map a FHIR resource to a
  party and back; `WebTemplateIndex::read` reads any `CanonicalDocument`. No
  specification governs a party build: our own design.
- FHIR `Organization` round-trips through the KDS laboratory request chain
  (#543). The published `KDS_Laborauftrag` template is vendored by
  `scripts/vendor/kds-laborauftrag-opt.sh` at the openFHIR commit
  `docs/VERSIONS.md` pins, and a FerroBRIDGE project context compiles the
  library's `organisation.v1.yml` verbatim, reached through the service
  request's `reference` to its requester: a contained `Organization` maps into
  the `Einsender` cluster and comes back as a created `Organization` the
  request references, and both lens laws hold. `organisation.v1.yml` joins the
  mapping-library pass list and `kds_laborauftrag` the round-trip list; every
  published file of the chain that does not compile is pinned by file and
  line.
- FHIRconnect party contexts compile (#898). A context whose operational
  template is rooted at a `PERSON`, `ORGANISATION` or `ROLE` is a party
  context, read from the RM type of the template root, and `Program::party`
  names the class. A party context refuses at load, each with a code of its
  own: `$composition` (`fc-party-composition-variable`), a path naming a
  composition attribute the defaults or `ctx/` set
  (`fc-party-composition-field`), `participationsFunction`
  (`fc-party-participation`), `link` (`fc-party-link`) and entering a context
  whose `start` slots a composition (`fc-party-sub-context-composition`).
  `to_openehr` and `to_fhir` refuse a party program with
  `EngineError::PartyProgram` until the party build lands (#897). No
  specification governs a party context: our own design. Breaking for a
  consumer that matches `ResolveCode` or `EngineError` exhaustively: add the
  new arms.

### Changed

- **Breaking for a library consumer** of `fhirconnect` 0.1.148:
  `fhirconnect::resolve::program::binding::PartyClass` is gone, and
  `Program::party` answers `openehr_mapping_core::party::PartyClass`; a
  consumer imports it from there. `EngineError` gains `CompositionProgram` and
  `PartyBuild`, and a consumer that matches it exhaustively adds the arms.
- The operator console draws with `ferrohealth-ui` 0.1.4 (#904). The
  dashboard's build instant reads on one line as the library draws every
  date-time, in UTC and cut to the second, with the value as the bridge
  sent it in its title. On a phone the dashboard's tiles are compact
  one-line tiles, and a disabled field keeps its edge in the dark theme.
- The README, the landing page and the build-order page name v0.0.9 as the
  latest release, and `scripts/checks/versions.sh` compares the release each
  names with the workspace version and its changelog date, so the next cut
  fails the guard until they move with it (#908).

### Fixed

- A pull request merged after a release no longer files its changelog
  entries under that release (#913). `scripts/gh/watch-prs.sh` merges
  `CHANGELOG.md` by entry through the new `scripts/gh/changelog-merge.sh`,
  which keeps a branch's entries under `[Unreleased]`, also when a clean line
  merge would have placed them under a version cut meanwhile.
  `scripts/checks/changelog-released.sh` fails CI when a released section from
  0.0.9 on differs from its text at its tag.

## [0.0.9] - 2026-10-19

### Added

- A released entry.
