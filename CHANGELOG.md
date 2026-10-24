<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Changelog
<!-- ANCHOR: book -->

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `ihe-atna` moves to 0.2.6, the FerroAUDIT release whose forwarder closes
  a syslog connection with `close_notify` before it reconnects and adds
  `Forwarder::stop`, so a product that pins 0.2.6 resolves `ihe-iti` beside
  it. FerroPIX's server delivers its audit records over the FHIR Feed and
  opens no syslog connection, so nothing else changes (#194, `ihe-iti`
  0.0.50).

## [0.0.3] - 2026-10-24

### Added

- The span export of `[telemetry] otlp_endpoint` (#143): `serve` sends one
  `request` span per request over OTLP/gRPC, with the method, the matched
  route template and the status and nothing that names a patient, a caller
  or a query.

- The operator API's reveal and domain writes (#90).
  `POST /operator/persons/{id}/reveal` unmasks one identifier, carried
  identifier or pseudonym the person holds, with a required reason: a
  `reveal` steward action naming the person, the record and the domain, and
  a BALP Privacy Disclosure record with the person as subject and the reason
  as a detail, are written in one transaction before the value is answered
  once, `no-store`, shown 15 seconds by default and never more than 30.
  Behind the administrator scope, `POST /operator/domains` adds a local or
  authoritative domain whose Source an issuer lists, and
  `PATCH /operator/domains/{id}` renames one, gives it another Source or
  retires it for good, each a `domain-create` or `domain-update` steward
  action with its BALP record; the guard takes the new Source the moment
  the write commits. The migration `0014_steward_subjects` adds the
  identifier and pseudonymisation domain a steward action names, and the
  grants move to `0015`. The trail and its export carry both columns.
- The operator API the console reads (#90, `ferropix_server::operator`),
  under `{base}/operator` behind `[operator] enabled` and the steward or
  administrator scope the permission table names, with its documents in
  `ferropix-operator-api`. `GET /operator/overview` answers the dashboard;
  `POST /operator/search/identifier` finds the persons one identifier is
  cross-referenced to by its HMAC, a carried authoritative identifier
  included, and `POST /operator/search/demographics` runs the scorer over
  the blocking keys, each paged (20 by default, at most 50);
  `GET /operator/persons/{id}` answers the person with its identifiers per
  domain masked, its source records, its history (at most 200 entries) and
  its pseudonyms masked; `GET /operator/domains` and
  `GET /operator/domains/{id}` the identifier domains with their record
  counts and last 20 feeds; `GET /operator/matching` the `[matching]`
  section in effect with every field's weights; `GET /operator/actions` a
  page of the steward trail filtered by actor, kind, person and time, and
  `POST /operator/actions/export` the same as CSV, at most 10,000 rows.
  Every lookup that carries an identifier is a `POST`, every answer is
  `no-store`, and no refusal quotes the request. A person view writes a
  `view` steward action and a BALP Patient Read record, a search one BALP
  Patient Query record per person found or a Query record, and an export an
  `export` action and an Export record, each stored before the answer. The
  console's client reads the dashboard typed and walks every document field
  by field, so a renamed field fails its build.
- The operator console's image and release lane (#99).
  `docker/console/Dockerfile` puts the static console binary and its site
  bundle on the same distroless base, compiling nothing: user `65532`, port
  `3000`, the bundle under `/site`, and a `HEALTHCHECK` on
  `ferropix-console healthcheck`. A `v*` tag now builds, attests and
  attaches the console's musl binaries and site bundle and pushes
  `ghcr.io/ferrohealth/ferropix-console` for both platforms with SLSA
  provenance and SPDX SBOMs (`release-console.yml`), checked with
  `gh attestation verify`; the vulnerability record, the tag gate and the
  weekly scan cover it beside the server image. `compose.yaml` gains the
  `console` profile, signed in through the development issuer with the
  client secret as a file secret. CI builds the console image from a
  placeholder tree. The book's console page documents the image, its port
  and its verification.
- The PDQm Patient Demographics Supplier with the Match Operation Option
  (#70, #72, #73, `ferropix_server::pdqm`). ITI-78 answers
  `GET /fhir/Patient` and `POST /fhir/Patient/_search` alike with every
  parameter of PDQm 3.2.0 §2:3.78.4.1.2.1, AND and OR, `:exact`, the domain
  filter, and `searchset` paging by a cursor (`_count` 20 by default, at
  most 100), and `GET /fhir/Patient/{id}` reads one person. A string
  matches as a case- and accent-insensitive prefix, an `identifier` by the
  HMAC of its normalised value, so search opens no sealed value, and a
  `birthdate` with every FHIR prefix. Each `Patient` is the person, with
  its master identifier, the identifiers a PIX query answers for it and the
  demographics of an `authoritative` domain's record first. The six cases
  of §2:3.78.4.1.3 answer as PDQm names them, a merged person by
  `[pdqm] deprecated`. ITI-119 `POST /fhir/Patient/$match` scores the input
  with the scorer over the blocking keys, one entry per person most likely
  first with `search.score` and `match-grade` from the `[matching]`
  thresholds, `onlyCertainMatches`, `count`, and a `warning` entry for an
  input of fewer than three fields. Every search, read and match writes its
  PDQm Supplier BALP record, one per person answered, before the answer.
  The `CapabilityStatement` instantiates the PDQm Supplier statements with
  `read`, `search-type`, the search parameters and `$match`. The migration
  `0013_search` installs `unaccent` into `pix` with the functions the
  search reads, and the grants move to `0014`.
- The server image and the SLSA Build Level 3 release lanes (#74, #75,
  #76). `docker/Dockerfile` puts the static musl binary on distroless static
  pinned by digest, compiling nothing: user `65532`, ports `8080` and
  `2575`, `/var/lib/ferropix` with its `0700` audit spool owned by the
  runtime user, and a `HEALTHCHECK` on `ferropix healthcheck`. A `v*` tag
  now builds the binary per architecture with `cargo auditable`
  (`release-build.yml`), verifies those tarballs and pushes
  `ghcr.io/ferrohealth/ferropix` for `linux/amd64` and `linux/arm64`
  (`release-image.yml`), each with SLSA provenance and SPDX SBOMs a
  consumer checks with `gh attestation verify`; records every vulnerability
  finding of the image and the binaries joined with its OpenVEX judgement,
  refusing a release with an unjudged one (`scripts/checks/vex-coverage.sh`);
  and tags the image only after the HIGH and CRITICAL gate
  (`scan-and-tag.yml`). `image-scan.yml` scans the published image each
  week. `compose.yaml` runs the image beside PostgreSQL and a development
  OpenID Provider with file secrets, a read-only root, every capability
  dropped and ports on `127.0.0.1`; the quickstart configuration's issuer
  moved to that provider. CI lints the Dockerfile with hadolint and runs the
  quickstart to readiness on every change. `scripts/release/stage-dist.sh`
  stages a release's binaries for a local build. The book's Deployment page
  documents the image, its ports, its verification and the quickstart.
- The PIXm Manager (#66, #68, #52, #69). `$ihe-pix` (ITI-83) answers by
  `GET` and by `POST` of a `Parameters` body, the two forms alike, in FHIR
  JSON and XML, from `store::xref::lookup` through `ihe-iti`'s
  `pixm::server` reader and writer, with the five answers of PIXm 3.1.0
  §2:3.83.4.2.2. An identifier in an `authoritative` domain that no Source
  feeds, such as the BSN a hospital's `Patient` carries, is found and
  answered too, and one carried by records of two persons answers `409`
  `multiple-matches` (#167). ITI-104 adds, revises, resolves a duplicate
  and, with `[pixm] remove_patient`, removes through the feed transaction: `201` or
  `200` with an `information` issue and a `Location` naming the person,
  `403` for a client that is not the domain's Source, and `400` or `422`
  for a refused message. Each route passes the permission table and fills
  the audit place it took with its PIXm BALP record; an ITI-83 answer is
  withheld with `503` when its record cannot be stored. No query, body or
  identifier reaches a log line, a span or a refusal. `GET /fhir/metadata`
  serves the `CapabilityStatement` in JSON or XML, instantiating the PIXm
  Manager actor statement with `conditionalUpdate`, `conditionalDelete`
  only with the Remove Patient Option, and the `ihe-pix` operation;
  `_format` is honoured on every served route and an unsupported one
  answers `406`. `fhir::status` is the one table of every outcome the face
  answers. The book's new page "The PIXm Manager" says what it accepts.
- The conformance gate (#162, `scripts/checks/conformance.sh`): corpus
  tests in `ihe-iti` read and write every example of the vendored PIXm
  3.1.0, PDQm 3.2.0 and PMIR 1.6.0 packages, and check each audit example
  against the record `ihe-iti` writes. Each corpus ratchets against its pass
  list under `conformance/`, which names every failing example with its
  reason, and renders a shields.io endpoint badge under
  `conformance/badges/`. Today: PIXm 12 of 13, PDQm 10 of 11, PMIR 7 of 7,
  audit records 7 of 13 (the six Manager and Supplier records are the
  server's). CI runs the gate under `--check` in `conclusion`.
- The README opens on the family's badge row: CI, CodeQL, OpenSSF
  Scorecard, the SonarCloud quality gate and coverage, the licence, the
  latest release, the documentation, the crates.io and docs.rs badges of
  `ihe-iti` and `nl-generic-functions`, and the four conformance badges.
- The server's feed transaction (#60, `ferropix_server::feed`): an ITI-104
  add or revise, resolve duplicate or remove, as `ihe-iti` reads it, is one
  PostgreSQL transaction. The record is locked by `(domain, hmac)` or
  inserted and takes a version whose demographics hold no identifier; every
  identifier the `Patient` carries in a known domain is its own row, found
  by its HMAC and sealed in a sensitive domain, after its domain's rule
  normalised it (a BSN to nine digits passing the eleven-proof). Records
  that carry one `authoritative` identifier are joined into the oldest of
  their persons unless the birth year or a stated gender differs, which
  records a candidate instead; a `local` value never links across domains.
  Every record sharing a blocking key is scored and a pair graded
  `possible` or better becomes an open candidate, linked alone only with
  `auto_link = true` (#61). The advisory locks on identifiers, then the
  records, then the persons, each in key order, let two feeds that join the
  same persons both commit as one person. A resolved duplicate merges the
  persons with `kind = source` and a PIX query of the subsumed identifier
  answers the survivor's set (`store::xref`); a removal answers not found
  and keeps the versions. Every system action is a `steward_action` row,
  and `feed::record` writes the PIXm Feed Manager BALP record after the
  commit. The migration `0012_feed` adds `carried_identifier` and the
  blocking columns, the grants move to `0013`, and `db reseal` rewrites the
  carried identifiers too. `ferropix matching estimate` prints each field's
  `u` from the store's value frequencies.
- The installation report to FerroPULSE (#153). `ferropix serve` sends one
  `start` report (at most one per ten minutes) and one `daily` report on
  FerroPULSE's report envelope v2 with the product `ferropix`: the instance
  id kept under `usage_report.state_dir`, the version, the commit, the
  deployment kind `usage_report.deployment` states, the host size as ranges,
  the profile, the services switched on, the identifier domains as a range,
  the readiness of the store and the audit spool, and, once a day, latency
  per route group and the count of `4xx` answers from the request log. It
  never carries an identifier, a domain's system or name, a host name or a
  request path. It is on by default and announced at every start;
  `usage_report.enabled = false`, an unset `state_dir` or a CI environment
  (`GITHUB_ACTIONS`, `CI`) keeps it off. Every report is validated against
  the schema `scripts/vendor/ferropulse-schemas.sh` vendors from the pin in
  `docs/VERSIONS.md`, and `scripts/checks/usage-report-off.sh` holds every
  launcher of the server in a test lane to the switch.
- The server's probabilistic scorer (#61, `ferropix_server::matching`): a
  Fellegi-Sunter weight per field from the `[matching]` `m` and `u` (each
  field's default documented on the book's new Matching page), the
  comparators of each field (Jaro-Winkler on names, exact with half credit
  on birth dates, the four-character prefix on postal codes, the token
  overlap of addresses, telephone numbers on their last nine digits), a
  score in `[0, 1]` by the logistic of the total against a fixed midpoint
  of 10 bits, the grades `certain`, `probable`, `possible` and
  `certainly-not` against the thresholds, the blocking keys, and the
  evidence of every field with its values masked. The scorer reads no
  store; the feed records its candidates with #60.
- The server's BALP audit trail through `ihe-atna` (#63). `serve` opens the
  ITI-20 spool under `audit.spool_dir` and refuses to start on a directory
  open to other users, unwritable or holding a foreign file, and sends the
  records to `audit.url` over the FHIR Feed: a delivered record is removed,
  a refused one quarantined. Every audited route takes its record's place
  in the spool before it runs, so a full spool refuses the request with
  `503` and a `transient` issue. The records of ITI-83 and ITI-104 (the
  PIXm Query and Feed Manager audit profiles), ITI-78 and ITI-119 (the
  PDQm Query and Match Supplier profiles) and of a steward's view or
  decision are built from the token's caller, with the patient as the
  subject and a search's raw request base64 in `entity.query`, one record
  per person found. Readiness gains the `audit_spool` indicator, and the
  dependency report names the repository with its last outcome. The PIXm
  and PDQm packages now vendor the Manager and Supplier audit profiles and
  examples.
- The server's health family and request log (#64). `GET {base}/health/readiness`
  runs a registry of indicators, each bounded to two seconds, and answers
  `503` with a readiness document naming each one that is down; the
  indicators land with the subsystems they check (the store, the audit
  spool, the MLLP listener, the feed sender). `ferropix healthcheck` exits 1
  on that `503`. `GET {base}/operator/dependencies` answers the same report
  with the upstreams' last outcomes behind the steward scope. One request
  log line per request under the target `ferropix_server::request` records
  the request id, the method, the matched route template, the status, the
  latency, the caller and the domain a feed wrote to, and never the path as
  sent, a query, a body or a header value, so a BSN sent by `GET` to
  `$ihe-pix` stays out of the log. `ferropix-operator-api` is the new crate
  of the documents the server writes and the console reads. The usage
  report moves to #153, which waits on FerroPULSE taking the `ferropix`
  product.
- The operator console's browser journeys (#98), behind `FERROPIX_JOURNEYS`
  and run by CI's `journeys` job in headless Chrome for Testing 155.0.8059.79
  (pinned in `docs/VERSIONS.md`, installed locally by
  `scripts/release/journey-tools.sh`): sign-in and sign-out through the test
  OpenID Provider, the preferences round-tripping through their cookies, and
  the WCAG 2.2 AA battery over every screen's refused state in both themes,
  with a self-test that a wrong theme fails it. The console marks `<html>`
  `data-hydrated` once its islands run, which every wait waits for.
- The console's pseudo-locale (#97): a language cookie naming `qps` draws
  every console and library word accented and two fifths longer. Your
  preferences never offers it; the pseudo-locale journey walks every screen
  in it and fails on a word that skipped the catalog or a page that scrolls
  sideways at 1440 or 390 px.
- Authorization of the FerroPIX server through `ferrohealth-smart` (#62):
  `serve` reads the RFC 8414 metadata and key set of every
  `[[auth.issuer]]` at start, and every route of `{base}/fhir` and
  `{base}/operator` has a row in one permission table. A Consumer needs
  `system/Patient.rs`, a Patient Identity Source `system/Patient.cud` with
  its token's `client_id` equal to the domain's `source_client_id` and
  listed by the issuer that signed it, a PMIR subscriber
  `system/Subscription.cruds`, and the operator API `auth.steward_scope`
  or, for its domain writes, `auth.admin_scope`; each IHE transaction also
  admits its IUA scope (`ITI-83`, `ITI-104`, `ITI-78`, `ITI-119`). A
  missing token answers `401` and a missing scope `403`, each an
  `OperationOutcome` with a `security` issue, and a Source changing another
  domain `403` with a `forbidden` issue; no refusal names a claim, the
  token or a request value. `GET {base}/fhir/metadata` and
  `{base}/fhir/.well-known/smart-configuration` name the token endpoint and
  the scopes. The transaction routes answer `501` until their faces land.
  Basic users now need the `[dev]` table beside `profile = "development"`.
- The ITI TF-2 texts of the HL7 v2 transactions ITI-8, ITI-9, ITI-10,
  ITI-21 and ITI-22 (#84), pinned by checksum in `docs/VERSIONS.md` and
  fetched by `scripts/vendor/ihe-iti-tf.sh` into the local cache, never
  committed, because each page reproduces HL7 tables whose rights HL7
  reserves. The IHE citation guard checks a citation of them against the
  cached pages, and the book's conformance page says so.
- `ferropix-server`, the FerroPIX server crate with its binary `ferropix`
  (#58): `serve`, `config check`, `healthcheck` and `db migrate`. The
  configuration tree of
  `docs/architecture.md` §3 loads from one TOML file named by `--config` or
  `FERROPIX_CONFIG`, with `FERROPIX__<SECTION>__<KEY>` overrides, and every
  table refuses an unknown key. A secret is a `Secret` or `SecretUrl` that
  renders `***` (a URL with its user information and query replaced), read
  from its key or its `_file` sibling; `profile = "production"`, the
  default, takes the `_file` form only and refuses the `[dev]` table, Basic
  users and plain `http` bases off loopback. A `[[domain]]` seed whose
  `source_client_id` no `[[auth.issuer]]` lists in its `clients` is refused,
  as is a system seeded twice. `serve` runs `{base}/health` and
  `{base}/health/readiness` behind the request id, the panic catch, the
  request timeout, the body ceiling and the bounded drain on `SIGTERM`.
  `app/ferropix-server/quickstart.toml` is the smallest production
  configuration, and the book's
  [Configuration](https://ferropix.eu/docs/operate/configuration.html) page
  lists every key, held by a test.
- The cross-reference store of `ferropix-server` (#59): schema `pix` in
  PostgreSQL 18, built from a migration set embedded in the binary, with
  every relation of `docs/architecture.md` §4.2 and the temporal link
  constraint that keeps a source record in one person at any instant.
  `ferropix db migrate` and `serve` (unless `[db] migrate = false`) apply
  the pending migrations, check the sealing key and insert each
  `[[domain]]` the store lacks; `serve` first refuses a role that can read
  any schema beside `pix`, and its guard admits as Sources the domains the
  store holds after the seed; the readiness route reports the store as the
  `database` indicator. An identifier in a sensitive domain is sealed
  with AES-256-GCM and found by its HMAC-SHA256 in one index probe, so a
  dump shows no identifier. `db.data_key_file` now holds the base64 of 32
  random bytes, and a start under another key is refused. `ferropix db
  reseal --old-key-file <PATH>` moves every identifier column to the
  configured key in resumable batches. No identifier or name reaches a log
  line, a `Debug` rendering or an error of the store.
- `ihe-iti` 0.0.45 adds the server sides of PIXm and PDQm, typed and with
  no store and no HTTP server, so FerroPIX and FerroFED can each put them on
  their own stack. `pixm-server` is the PIX Manager (#67): `$ihe-pix` read
  from the `GET` query or the posted `Parameters` into one request, with the
  `OperationOutcome` of a second `sourceIdentifier`, a token without a
  system and an unknown parameter, the `Parameters` answer and the three
  failures of ITI-83 written, and the ITI-104 conditional update and delete
  on `Patient?identifier=` read, held to the PDQm Patient profile, the
  condition's identifier and the `replaced-by` link. `pdqm-server` is the
  Patient Demographics Supplier (#71): every ITI-78 search parameter read
  with `:exact`, AND, OR and the domain filter, the parameters it drops
  reported and left out of the `self` link, the `searchset` Bundle with its
  `total` and paging links written, the `$match` input read as `Parameters`
  or a bare `Patient`, and the Match Output Bundle written with each
  entry's `search.score` and `match-grade`. Both read and write FHIR JSON
  and FHIR XML (`ihe_iti::server::format::Format`). `TargetIdentifier::new`
  and `PatientReference::new` of `ihe_iti::pixm::identifier` are public, for
  the Manager's answer. The crate's README gains its PMIR section (#54).
- `ferropix-console`, FerroPIX's first operator console (#119), built new on
  FerroUI 0.3.6 (`ferrohealth-ui = "=0.3.6"`, with its `split` feature for
  the `--split` site build, #127, #131, #137, #139) in its FerroPIX brand as part
  of the family console rollout (FerroHEALTH/FerroUI#161): a Leptos islands
  app the server renders whole, with every island FerroUI's and every page
  working without its WebAssembly. It has the OpenID Connect sign-in
  (authorization code with PKCE) and the server-side session behind one
  `__Host-` cookie with refresh and sign-out, the `FERROPIX_CONSOLE__`
  configuration with `_file` secrets, the nonce Content-Security-Policy and
  the other browser security headers, Your preferences on FerroUI's three
  tabs with the reveal duration and hide on leave, About with the server's
  `CapabilityStatement`, search by an identifier masked as typed with
  FerroUI's hold-to-show eye, and every page of the reviewed design (dashboard,
  review queue and compare, search, patient with merge and unmerge, sources,
  identifier domains, pseudonymisation domains, configuration, audit trail).
  The FerroPIX server and its operator API are not built yet (#57, #90), so
  each page that reads them draws the design's refused state: the server
  refused the connection, did not answer, refused the console, or serves no
  operator API yet. The planned name `ferropix-viewer` is `ferropix-console`
  throughout; nothing was published under the old name. The book gains
  [The operator console](https://ferropix.eu/docs/operate/console.html).
- `docs/architecture.md`, the design of record for the server and the
  operator console: the process and its configuration, the PostgreSQL
  cross-reference store with its history and transaction boundaries, the
  matching in three tiers with the steward in the middle, the PIXm, PDQm
  and PMIR faces and the HL7 v2 listener, pseudonyms, authorization and
  audit, the two images released at SLSA Build Level 3, the CI lanes, and
  the console's information architecture, screens, wireframes and FerroUI
  component inventory. `CLAUDE.md` and the book name the console, the
  PostgreSQL store and the two images as planned, by milestone.
- The documentation site at <https://ferropix.eu/>: a landing page in the
  FerroHEALTH design with the FerroPIX brand, and the book under `/docs/`
  (what FerroPIX is and what is built or planned, getting started, the
  published crates with every feature, configuration, deployment, security
  and audit, operations, the specification pins, contributing and this
  changelog), built and link-checked on every pull request and deployed
  from `main` by the Docs workflow. The brand gains a favicon set and a mark
  for a dark ground (#45, #46, #47, #49).
- The guards that keep the book in step with the code, in CI's required
  `conclusion`: `book-guard.sh` refuses a change to a crate's source or
  features, the server or a deployment file that does not update the book
  (the `no-book` label is the escape), `docs-claims.sh` holds each crate's
  Features table to its `Cargo.toml`, refuses a typed crate version and a
  planned item without its tracker link, and `writing-style.sh` refuses an
  em dash, a banned word or AI attribution in the published prose. Each has
  a `--self-test` (#48).
- The release checks the family's Security changelog rules: every
  `### Security` entry names a GHSA advisory or an OpenVEX statement under
  `security/vex/`, and a Security section beside other changes opens with
  "Shipped with functional changes because:" and its reason (Regulation (EU)
  2024/2847 Annex I Part II(2), (4), (8)). CI checks `[Unreleased]`, and the
  release `plan` job the section it cuts, through `scripts/release/plan.sh`,
  which `scripts/release/preflight.sh` replays before the tag
  (`docs/release.md`). The release notes state the month the five years of
  security fixes end (Art 13(19)). The accepted `paste` advisory
  (RUSTSEC-2024-0436) is published as a generated OpenVEX statement,
  `security/vex/rust-advisories.openvex.json`, and `SECURITY.md` names the
  `info@cadasto.com` reporting route and the advisory each fixed
  vulnerability is published as (#123).

### Changed

- `ferropix-console` moves to FerroUI 0.3.3 (#127): the search page's
  identifier is FerroUI's masked input with the hold-to-show eye inside the
  field, which shows the typed identifier only while held and stays masked
  without WebAssembly; a refusal with no answer shows its summary alone as
  its title, the words "No answer:" read by assistive technology; and the
  site's `--split` build keeps the date picker in a chunk of its own through
  the library's `split` feature.
- `ferropix-console` moves to FerroUI 0.3.4 (#131): the Search entry of the
  navigation draws the design's `user-round-search` glyph in place of the
  magnifier.
- `ferropix-console` moves to FerroUI 0.3.5 (#137). The console draws no part
  0.3.5 changes, so its pages look and behave as before.
- `ferropix-console` moves to FerroUI 0.3.6 (#139). The console draws no
  activity chart, the one part whose interface 0.3.6 changes, so its pages
  look and behave as before.

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

### Fixed

- A PIXm feed body in FHIR JSON whose narrative is not well-formed XHTML,
  so that its FHIR XML form reads back changed, is refused as having no
  XML form; the server accepted it and would have answered with a
  different Patient in XML (#190, `ihe-iti` 0.0.49).
- `ihe-atna` moves to 0.2.5, the FerroAUDIT release that fixes a syslog
  frame lost before a dropped connection. FerroPIX delivers its audit
  records to a FHIR repository over HTTP and opens no syslog connection,
  so no `Connection::close` call is missing (#191).

- The PIXm ITI-83 reader refuses an empty `_format` with a 400, where it
  accepted one its `Parameters` form cannot carry (#185, `ihe-iti` 0.0.48).

- The PIXm client percent-encodes a space in its ITI-83 query as `%20`
  (RFC 3986 §2.1), where it wrote `+`, which a Manager reading the query
  as RFC 3986 takes as a literal plus (#164, `ihe-iti` 0.0.47).

- `ihe-iti` 0.0.46 fixes three request readers the new fuzz lane caught
  (#79). The ITI-83 `GET` reader refuses with a `400` and a `value` issue a
  query value no FHIR `string` holds (a vertical tab or a form feed), which
  it accepted although the same request posted as `Parameters` was refused.
  A FHIR JSON body whose XML form does not read back, such as a Patient
  whose narrative `div` is no XHTML, is refused by every body reader with
  the new `BodyError::NoXmlForm`, so a Manager or a Supplier never stores a
  resource it could not answer with in XML. The `self` link of an ITI-78
  search writes a space as `%20`, so following it reads the same search:
  it wrote the form encoding's `+`, which a URL query reads as itself.
  `QueryError::NotAString` is the new variant of the first refusal. The
  fuzz lane (`.github/workflows/fuzz.yml`, the `fuzz/` targets) runs cargo
  fuzz weekly over the ITI-83, ITI-104, ITI-78 and ITI-119 readers and the
  format parsers, on the nightly `docs/VERSIONS.md` pins.

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
