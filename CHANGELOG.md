<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Changelog
<!-- ANCHOR: book -->

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Entries wait as fragments under `changelog.d/` until a release assembles them.

## [Unreleased]

## [0.0.26] - 2026-11-10

### Changed

- Breaking for a crate user: `ihe-iti` 0.3.0 and `nl-generic-functions` 0.3.0 require `ferrohealth-smart` 0.17, whose `OutboundClient` and endpoint tables their clients are built from, and `ihe-iti` requires `ihe-atna` 0.11. In smart 0.17 every token and introspection endpoint a configuration table names is a `SecretUrl`, so a password in its userinfo reaches no rendering of the running configuration. Upgrade: take `ferrohealth-smart` 0.17 and `ihe-atna` 0.11, and build `OauthSection::token_endpoint` with `SecretUrl::new` and read it with `.expose()`. The server and the console move to `ferrohealth-smart` 0.17.0, `ferrohealth-session` 0.11.0, `ferrohealth-config` 0.4.3, `ferrohealth-server` 0.8.2, `ihe-atna` 0.11.0, `ferrobridge-term` 0.4.0 and `ferrobridge-hl7v2` 0.6.0, with no configuration key changed (#591).

### Fixed

- The Helm chart 0.3.18, published ahead of this release, and this release's chart 0.3.19 fix the defaults. Chart 0.3.17 could not start FerroPIX 0.0.25 at its defaults: its values set `config.auth.audience`, which the server stopped reading when the audience moved into each `[[auth.issuer]]` entry, so every install, standalone or as a dependency, stopped at start with "unknown field `audience`". The values carry no audience now, the values schema refuses `config.auth.audience` by name and points at `config.auth.issuer[].audience` (in the charts of this release, 0.3.18 and 0.3.19, only; the chart of the next release refuses it as any other value the server does not read), and the chart refuses to render without a `config.auth.issuer` entry, each with its `issuer`, `kind` and `audience`. Upgrade: move the audience into each issuer entry and give each its `kind`. CI and the release's chart leg now render the chart over every value set under `deploy/helm/ci`, the defaults among them, and run the server's and the console's `config check` over the configuration it renders, so a chart whose values its images refuse is never published (#585).

## [0.0.25] - 2026-11-10

### Changed

- Breaking for a crate user: every client of `ihe-iti` 0.2.0 (`PixmClient`, `PdqmClient`, `McsdClient`, `PmirSubscriber`, `XcpdClient`) and the `NviClient` and `MitzClient` of `nl-generic-functions` 0.2.0 is built from `ferrohealth-smart` 0.16's `OutboundClient`, as `ihe-atna`'s `FeedRepository::new` is, so a product configures each service with its shared endpoint table: the base URL, the TLS, the timeouts, no redirect, the `retry` table and the credential, HTTP Basic or a client credentials token with its `DPoP` proof and its one renewal on a `401` (IUA ITI-72 §3.72.4.3). Upgrade: build `OutboundClient::build(&endpoint, key, tls)` and pass it, `PixmClient::new(client)` for `PixmClient::new(base, http)`, and the same for the others; `MitzClient::new` and `unencrypted_for_development` return `InvalidInput`. The call's `timeout` bounds every try the `retry` table allows. A credential that cannot be had is each client's `Unauthenticated` error over the outbound client's `SendError`, which `XcpdError` and `MitzError` gain, and `XcpdClient::discover` refuses a gateway off the origin of the table's `url` with `XcpdError::ForeignGateway` before anything is sent (#580).

### Removed

- Breaking for a crate user: `ihe-iti` 0.2.0 drops the `authorizer` and `smart` features with the `ihe_iti::authorizer` module (`Authorizer`, `Authorizable` and `with_authorizer`, `AuthorizerError`, `Retry`, `send`, and `smart::TokenAuthorizer`), and `nl-generic-functions` 0.2.0 drops `NviClient::with_authorizer` and `mitz::error::ClientError`. The credential an authorizer made comes from the endpoint table of the client's `OutboundClient`; the send every client goes through is the new `outbound` feature's `ihe_iti::outbound::send` (#580).
- The server and the console no longer refuse by name the configuration keys
  v0.0.24 renamed or moved, under the one-release rule for removal aids: the
  millisecond, second, day and byte keys of FerroPIX's own sections
  (`timeout_ms`, `retention_days`, `spool_max_bytes` and the rest), the
  earlier `[terminology]` keys (`base_url`, `timeout_ms`, `retry`,
  `credentials`, `tls`) and `audit.url`, `audit.timeout_ms` and
  `audit.retry_max_ms` that moved under `endpoint`, the renamed keys of the
  shared `[server]`, `[auth]`, `[telemetry]`, `[session]` and `[oidc]`
  sections, and the console's `pix.timeout_ms`. Each now fails at start as
  an unknown key, named in the refusal; write the key that replaced it, as
  the v0.0.24 entries say (#582).

## [0.0.24] - 2026-11-10

### Changed

- The ITI-21 RSP^K22 is checked against RSP_K21 as IHE ITI TF-2 constrains it before it leaves: QRI optional (Table 3.21-6) and QPD-3 repeating, one parameter a repetition (§3.21.4.1.2.3), through `ferrobridge-hl7v2` 0.4.0's `IHE_ITI_21`. An answer that does not conform, such as a PID with no name, is answered `AE`, as the RSP^K23 is (#538).
- The compose quickstart and the Helm chart's CI values run the development OpenID Provider `mock-oauth2-server` 6.1.5, and the release and image scans run Trivy 0.76.2 (#557).
- `ihe-iti` 0.0.84 requires `ferrohealth-smart` 0.15 (caret `0.15.0`) and `ihe-atna` 0.9 (caret `0.9.0`), and `nl-generic-functions` 0.0.51 requires `ferrohealth-smart` 0.15 and `ihe-iti` 0.0.84. Both name `ferrohealth-smart` and `ihe-atna` types in their public API, so a product on those lines needs this release; no signature of either crate moves. Upgrade: move `ferrohealth-smart` to 0.15.0 and `ihe-atna` to 0.9.0 beside them (#561).
- Breaking for an operator: the server and the console move to `ferrohealth-smart` 0.15.1, `ferrohealth-session` 0.9.0, `ihe-atna` 0.9.1, `ferrohealth-config` 0.4.0, `ferrohealth-server` 0.8.0, `ferrohealth-telemetry` 0.4.0 and `ferrohealth-usage-report` 0.4.0, so the lock holds one copy of each family crate. Every span of time and size in the sections those crates own is now a string with its unit, under a key that names none, and a bare number is refused. Upgrade: in `[server]` write `body_limit = "1MiB"`, `request_timeout`, `header_read_timeout`, `idle_timeout`, `drain_delay`, `shutdown_timeout`, `retry_after` and `compression_min_size` (each was `…_ms`, `…_s` or `…_bytes`) and `[server.tls] reload_interval`; in each `[[auth.issuer]]` write `clock_skew`, `jwks_max_age`, `jwks_refresh_interval` and `timeout`, with `introspection.cache_max_age`, `[auth.basic] cache_ttl` and the OAuth tables' `refresh_margin` and `timeout`; move `auth.ca_file` to `[auth.tls] trust_roots_file`, and the OTLP collector's `trust_roots_file` and `client_identity_file` under `[telemetry.otlp.tls]`. In the console write `[session] sign_in_timeout`, `idle_timeout` and `absolute_timeout` (`"30min"`, not `idle_timeout_s = 1800`) and `[oidc] timeout`, and move `oidc.ca_file` to `[oidc.tls] trust_roots_file`. Each old key fails the start as an unknown key, and the loader names the new one where it can. The `[auth]` keys of the shapes before `ferrohealth-smart` 0.9 now fail as unknown keys too, without naming their replacement (#561).
- Breaking for an operator: FerroPIX's own sections write every span of time and size the same way, under a key that names no unit. Upgrade: `[audit] spool_max_size = "64MiB"` and `spool_write_timeout`, with the delivery's `timeout` and `retry.max_backoff` under `[audit.endpoint]` (below); `[outbox] retention = "30d"`; `[pmir.feed] initial_backoff` and `max_backoff`; `[hl7v2] frame_limit`, `idle_timeout` and `frame_timeout`, each consumer's `connect_timeout` and `reply_timeout`, and `[hl7v2.notification] initial_backoff` and `max_backoff`; `[xcpd] delivery_initial_backoff`, `delivery_max_backoff` and `delivery_timeout`; the terminology server's `timeout` with its `retry.initial_backoff` and `retry.max_backoff` under `[terminology.endpoint]` (below); and the console's `[pix] timeout`. In this release each old key refuses the start and names the key that replaces it, all of them in one refusal; from the next release each fails as an unknown key (#561).
- Breaking for a crate user: `ihe-iti` 0.1.0 requires `ferrohealth-smart` 0.16 (caret `0.16.0`) and `ihe-atna` 0.10 (caret `0.10.0`), and `nl-generic-functions` 0.1.0 requires `ferrohealth-smart` 0.16 and `ihe-iti` 0.1.0. Both name `ferrohealth-smart` and `ihe-atna` types in their public API, so the move of those public dependencies takes each crate's minor step, and a product on those lines needs this release; no signature of either crate moves. Upgrade: move `ferrohealth-smart` to 0.16.0 and `ihe-atna` to 0.10.0 beside them (#571).
- Breaking for an operator: the server reaches the audit repository and the terminology server through `ferrohealth-smart`'s outbound client, each configured by its endpoint table, so the lock holds one copy of each family crate (`ferrohealth-smart` 0.16.2, `ferrohealth-session` 0.10.0, `ihe-atna` 0.10.0, `ferrohealth-config` 0.4.1, `ferrohealth-server` 0.8.1, `ferrobridge-term` 0.3.0, `ferrobridge-hl7v2` 0.5.0). Upgrade: write the audit repository as `[audit.endpoint]` with `url`, `timeout` and `[audit.endpoint.retry] max_backoff` (was `audit.url`, `audit.timeout_ms` and `audit.retry_max_ms`); its `max_backoff` is also the wait between two delivery rounds, and the table takes `connect_timeout`, `retry.max_attempts`, `tls` and `credentials` (HTTP Basic or the client credentials grant) as well. Write the terminology server as `[terminology.endpoint]` with `url`, `timeout`, `retry`, `credentials` and `tls` (was `terminology.base_url`, `timeout_ms`, `[terminology.retry]`, `[terminology.credentials]` and `[terminology.tls]`); compose and the Helm chart (0.3.16) name it `FERROPIX__TERMINOLOGY__ENDPOINT__URL` and `config.terminology.endpoint.url`. In this release each old key refuses the start and names its place under the endpoint table; from the next release each fails as an unknown key (#571).
- A configuration the server or the console does not start on is refused with a line that opens `cannot start` (exit 78), and `healthcheck` with one that opens `not up` (exit 1), through `ferrohealth-server`'s run path; the server's line opened `the configuration is refused` before (#571).
- `ihe-iti` 0.1.0 and `nl-generic-functions` 0.1.0 require the FerroBRIDGE crates by caret ranges, `fhir-search` `0.1.130` and `fhir-types` `0.1.130`, where `ihe-iti` 0.0.84 required `fhir-search` exactly, so a product whose other family crates take another compatible patch of them builds with one copy (#543) (#571).
- The keys of the family sections that v0.0.24 renamed are refused by name in this release, each with the key that replaces it, all of them in one refusal, read from the file, the environment and `--set` before the rest of the configuration. On the server: `[server]` `request_timeout_ms`, `header_read_timeout_ms`, `idle_timeout_ms`, `body_limit_bytes`, `drain_delay_ms`, `shutdown_timeout_ms`, `retry_after_s`, `compression_min_bytes` and `tls.reload_interval_s`; `auth.ca_file`, `auth.basic.cache_ttl_seconds`, `auth.dpop.max_age_seconds` and `clock_skew_seconds`; each `[[auth.issuer]]` entry's `clock_skew_seconds`, `jwks_max_age_seconds`, `jwks_refresh_interval_seconds`, `timeout_ms` and `introspection.cache_max_age_seconds`; the OAuth tables' `refresh_margin_ms`, `timeout_ms`, `ca_file`, `client_certificate_file`, `client_key_file` and `previous_key_overlap_ms`; and the OTLP collector's `trust_roots_file` and `client_identity_file`. On the console: `[session]` `sign_in_timeout_s`, `idle_timeout_s` and `absolute_timeout_s`, `[oidc]` `timeout_s` and `ca_file`, `[oidc.tls]` `client_certificate`, `client_key` and `client_key_file`, and the OTLP collector's files. From the next release each fails as an unknown key (#575).

### Removed

- The named refusal of `[xcpd] client_certificate_file`, `client_key_file` and `reply_trust_file`, which v0.0.22 retired in favour of `[xcpd.tls]`. Each key now fails the start as an unknown key of `[xcpd]` (#561).

### Fixed

- The server exports its spans and metrics over the TLS `[telemetry.otlp.tls]` names: it installs its telemetry through `ferrohealth-server`'s installer, which reads the collector's trust roots and client identity, where before it built the exporter without them (#571).

## [0.0.23] - 2026-11-09

The re-cut of v0.0.22, whose release run published nothing. Read the
v0.0.22 section below for what this release changes: the HL7 v2 face needs
the new `[terminology]` section before you upgrade. Nothing else in the
server, the console or the crates differs from v0.0.22.

### Fixed

- v0.0.22 was tagged, and its release run published nothing: the performance gate handed the v0.0.21 server it measures against a `[terminology]` setting that release does not know, so the run could not judge it. The gate now hands each server only the settings its own source defines. v0.0.23 is the first published release with the changes of v0.0.22, and changes nothing else in the server, the console or the crates (#568).

## [0.0.22] - 2026-11-09

A release that breaks the HL7 v2 face's configuration. The face maps PID
through `ferrobridge-hl7v2` 0.2.0 and translates every v2 table value on a
FHIR R4 terminology server, so a server with `[hl7v2]` on needs the new
`[terminology]` section, pointing at a server loaded with HL7 Terminology,
`hl7.fhir.uv.v2mappings` 1.0.0 and FerroBRIDGE's value package, before you
upgrade. The XCPD gateway reads its outbound TLS from `[xcpd.tls]`, and the
three keys it replaces are refused by name in this release only. `ihe-iti`
0.0.83 and `nl-generic-functions` 0.0.50 are on `ferrohealth-smart` 0.13
and `ihe-atna` 0.8, and the workspace holds one copy of each.

### Added

- `ihe-iti`: `PixmClient::capabilities` reads the PIX Manager's
  `CapabilityStatement` with `GET [base]/metadata` (FHIR R4 http.html
  §capabilities) through the client's own transport and authorizer, and
  answers every other outcome as a typed `CapabilitiesError`: refused with
  the status, not a FHIR server, timeout, transport or unauthenticated.
  `nl-generic-functions` follows `ihe-iti` 0.0.79 (#522).

### Changed

- `ihe-iti` 0.0.78 and `nl-generic-functions` 0.0.45 require
  `ferrohealth-smart` 0.10 (caret `0.10.0`), and `nl-generic-functions`
  0.0.45 requires `ihe-iti` 0.0.78. Both name `ferrohealth-smart` types in
  their public API (`ihe-iti`'s `authorizer::smart::TokenAuthorizer` over
  `client::cache::TokenSource`, the Nuts grant's issuer rule), so a product
  on `ferrohealth-smart` 0.10 needs this release. No signature of either
  crate moves: the one 0.10.0 break is the `cache` field of
  `config::onward::OnwardSection`, which neither builds. Upgrade: move
  `ferrohealth-smart` to 0.10.0 beside `ihe-iti` 0.0.78 and
  `nl-generic-functions` 0.0.45 (#518).
- The server and the operator console run on `ferrohealth-smart` 0.10.1,
  `ferrohealth-session` 0.4.0 and `ihe-atna` 0.5.2, the releases the
  published `ihe-iti` and `nl-generic-functions` already build on, so the
  lock holds one `ferrohealth-smart` (#521).
- `ihe-iti` reads and writes every FHIR search value through the family's
  FHIR R4 search grammar, `fhir-search` (exact `=0.1.129`, with no default
  feature), the one copy FerroBRIDGE's facade and `ihe-atna` read theirs
  with: the PIXm Manager, the PDQm Supplier and the PMIR Registry read their
  tokens, splits and escapes through it, and the PDQm `birthdate` its prefix
  and date. As FHIR R4 search.html §3.1.1.4.19 requires, a `\` before any
  character other than `\`, `,`, `$` and `|` is refused
  (`SearchError::Escape` and `pixm::server::query::QueryError::Escape`, both
  new), and a token with a second unescaped `|` is refused where it was read
  into the code. A `birthdate` must be a calendar day, so `1923-02-30` is
  refused. `pdqm::query::DatePrefix` gives way to
  `fhir_search::prefix::Prefix`; to upgrade, pass `Prefix` to
  `PatientQuery::birthdate` and read it from `DateValue::prefix`.
  `nl-generic-functions` escapes its NVI search values with the same
  grammar (#523).
- The HL7 v2 face writes every answer through `ferrobridge-hl7v2` 0.1.203:
  the acknowledgment by its `ack::render`, and the RSP^K23, the RSP^K22 and
  ITI-10's ADT^A31 by its `write::Writer`. The RSP^K23 and the ADT^A31 are
  checked against their structure before they leave: a RSP^K23 that does
  not conform is answered `AE`, and an ADT^A31 that does not conform fails
  the change that would queue it (#524).
- The HL7 v2 face maps PID to a `Patient`, and a `Patient` back into the PID of an ITI-21 answer, through the HL7 v2-to-FHIR structure maps of `ferrobridge-hl7v2` 0.2.0, in place of FerroPIX's own copy of them, and translates every table value (PID-8, the name type, the address type, the telecommunication use and equipment type) by `ConceptMap/$translate` on a FHIR R4 terminology server. Breaking for an operator of the HL7 v2 face: name the server in the new `[terminology]` section (`base_url`, `timeout_ms`, `[terminology.retry]`, `[terminology.credentials]` with HTTP Basic or the client credentials grant, and `[terminology.tls]`), loaded with HL7 Terminology, the guide's package `hl7.fhir.uv.v2mappings` 1.0.0 and FerroBRIDGE's value package; with `[hl7v2]` on and no `terminology.base_url` the server refuses to start. A feed or a query the terminology server does not translate is answered `AR`, so the sender sends it again. A cell phone (XTN-3 `CP`) is fed as a `phone` in `mobile` use, a number with no equipment type keeps a `system` carrying the data-absent-reason `unknown` where the face fed `phone` before, and an ITI-21 PID leaves XTN-2 empty on an email the `Patient` gives no `use`, where it wrote `NET` (#525).
- The XCPD Responding Gateway reads its delivery's outbound TLS from
  `[xcpd.tls]`, the family's `TlsClientSection` of `ferrohealth-server`
  0.6.3: `client_identity_file` (or `client_identity`) holds the client
  certificate chain and its private key in one PEM, `trust_roots_file` the
  roots trusted beside the platform's, and `min_version` the oldest TLS
  version, `1.3` unless lowered to `1.2`. Upgrade: join the files of
  `[xcpd] client_certificate_file` and `client_key_file` into one PEM and
  name it in `[xcpd.tls] client_identity_file`, and move
  `[xcpd] reply_trust_file` to `[xcpd.tls] trust_roots_file`; the old keys
  refuse to load and name their replacement. A reply endpoint that speaks
  only TLS 1.2 now needs `[xcpd.tls] min_version = "1.2"` (#526).
- `ihe-iti` requires `ihe-atna` 0.6 and the server pins `ihe-atna` 0.6.0.
  None of the APIs they use change: the syslog `Repository` now takes its
  TLS configuration from the caller and ITI-81 reads through `fhir-search`,
  and neither crate builds a syslog repository or reads ITI-81 (#528).
- `ihe-iti` 0.0.81 and `nl-generic-functions` 0.0.48 require
  `ferrohealth-smart` 0.12 (caret `0.12.0`), and `nl-generic-functions`
  0.0.48 requires `ihe-iti` 0.0.81. Both name `ferrohealth-smart` types in
  their public API, so a product on `ferrohealth-smart` 0.12 needs this
  release. No signature of either crate moves. Upgrade: move
  `ferrohealth-smart` to 0.12.0 beside `ihe-iti` 0.0.81 and
  `nl-generic-functions` 0.0.48 (#530).
- `ihe_iti::authorizer::smart::TokenAuthorizer` proves each request when
  its `TokenSource` binds tokens to a `DPoP` key
  (`ClientCredentials::with_dpop`): the token goes as `Authorization: DPoP`
  with a `DPoP` proof of the request's method and URL and the token's
  `ath` (RFC 9449 §4.2, §7.1). A `401` whose `DPoP` challenge demands a
  nonce keeps the token and sends the request once more with a proof
  carrying the nonce (§9); any other `401` still drops the token and
  resends once with a new one. A bearer source is sent as before (#530).
- `ihe-iti` requires `ihe-atna` 0.7.1, the release on `fhir-search`
  0.1.129, and the server and the operator console pin `ihe-atna` 0.7.1, `ferrohealth-smart` 0.12.0 and
  `ferrohealth-session` 0.6.0, so the workspace builds on a single
  `ferrohealth-smart`, the 0.12 line `ihe-iti` and `nl-generic-functions`
  take. None of the APIs they use change (#536).
- `ihe-iti` 0.0.83 requires `ferrohealth-smart` 0.13 and `ihe-atna` 0.8,
  and `nl-generic-functions` 0.0.50 requires `ferrohealth-smart` 0.13. The
  server and the operator console pin `ferrohealth-smart` 0.13.0,
  `ferrohealth-session` 0.7.0 and `ihe-atna` 0.8.0, so the lock holds one
  `ferrohealth-smart`. The 0.13 line refuses a malformed JSON document
  without quoting it (GHSA-mhv7-3975-6xq3) and bounds the token endpoint's
  answer; none of the APIs these crates use change (#540).
- `SECURITY.md` names `security@cadasto.com` as the email route for a
  vulnerability report, in place of `info@cadasto.com`, which stays the
  address for licensing and other business questions (#547).
- The named refusal of `[xcpd] client_certificate_file`, `client_key_file` and `reply_trust_file`, which names the `[xcpd.tls]` key that replaces each, holds for this release only: from v0.0.23 each key fails the start as an unknown key of `[xcpd]` (#558).

### Removed

- `ihe_iti::rest::escape`, replaced by `fhir_search::escape::escape`, which
  the `rest` feature now brings in (#523).

## [0.0.21] - 2026-11-08

A security release that breaks a configuration with a Basic machine user.
FerroPIX moves to `ferrohealth-smart` 0.9.2, which makes a Basic credential
a person's: a Basic user holding a `system/` scope is refused when the
configuration loads, so give each Basic user only `user/` scopes and move
each machine on Basic to a client credentials client before you upgrade.
`ihe-iti` 0.0.77 and `nl-generic-functions` 0.0.44 move to `ihe-atna` 0.5.1,
and the console moves to FerroUI 0.6.9.

### Changed

- `ihe-iti` 0.0.77 requires `ihe-atna` 0.5 (caret `0.5.1`), and
  `nl-generic-functions` 0.0.44 requires `ihe-iti` 0.0.77. `ihe-iti`'s
  public API names `ihe-atna` types (the PIXm consumer's `OnBehalfOf`, the
  `xcpd` audit message), so a product on `ihe-atna` 0.5 needs this release.
  `ihe-atna`'s API did not change between 0.4.0 and 0.5.1, so no `ihe-iti`
  signature moves. The server pins `ihe-atna` 0.5.1. Upgrade: move
  `ihe-atna` to 0.5.1 beside `ihe-iti` 0.0.77 (#512).

### Fixed

- The console moves to `ferrohealth-ui` 0.6.9, whose session watch no longer throws on a form with a control named `target` or `method` (#508).

### Security

Shipped with functional changes because: the fix is the dependency move itself, which carries the family's other patch releases with it.

- A Basic user holding a `system/` scope is refused when the configuration loads, naming the user and the scope: `ferrohealth-smart` 0.9.1 makes a Basic credential a person's, and FerroPIX on 0.9.0 gave such a user system-wide reach (GHSA-jhg2-jwjp-hq74, CWE-269, CWE-863). Upgrade step: give each Basic user only `user/` scopes, and move each machine on Basic to a client credentials client of a `machines` issuer (#508).

## [0.0.20] - 2026-11-06

A fix release with one change to who may feed a domain. A Patient Identity
Source is admitted only with a machine's client credentials token: a
person's token through the Source's client is refused, so move any Source
that fed that way to the client credentials grant before you upgrade. The
console moves to FerroUI 0.6.8, so a tab you sign out in shows the
signed-out page, and the compose quickstart's console signs a steward in.

### Changed

- A Patient Identity Source is admitted only with a machine's token, one the issuer's profile classifies as app-only from the client credentials grant. A person's token through the Source's client, a first-party client listed in `[[trust.issuer]] clients`, is answered `403` with a `forbidden` issue whose diagnostics say a Source sends a client credentials token, on ITI-104 and on the ITI-93 feed. A Source that fed with a person's token requests a client credentials token for its `source_client_id` (#496).

### Fixed

- The console moves to `ferrohealth-ui` 0.6.8, whose session watch no longer sends a tab that is signing out to the sign-in page that says the session expired: signing out right after bringing a tab forward lands on the signed-out page (FerroHEALTH/FerroUI#654). FerroUI 0.6.7 draws several live regions as `<output>` and named `<section>` elements with the same roles and looks (#493).
- The compose quickstart signs a steward in to the console. The development issuer signs a console login and its renewal with the console's client id as `aud` and `client_id` and the steward scope `__ferropix.steward`, `quickstart.toml` admits that audience beside `http://localhost:8080/fhir`, and it mounts the operator API. Before, the server refused every console login's token and the operator API was not mounted (#499).

## [0.0.19] - 2026-11-06

A family upkeep release that breaks the configuration. FerroPIX moves to
`ferrohealth-smart` 0.9.0, `ferrohealth-session` 0.3.0 and `ihe-atna`
0.4.0, and the published crates `ihe-iti` 0.0.76 and
`nl-generic-functions` 0.0.43 move with it. `[auth]` takes the 0.9.0
shape: every issuer is an `[[auth.issuer]]` table with its `kind` and its
profile, and a configuration in the old shape refuses the start, naming
what replaces it. A Basic user is a person and never a Source: a machine
signs in with a client credentials token. Rewrite `[auth]` by the upgrade
steps below before you upgrade.

### Changed

- Breaking for an operator: `[auth]` takes the `ferrohealth-smart` 0.9.0
  shape. Every issuer is an `[[auth.issuer]]` table with a required `kind`
  (`"persons"`, `"machines"`, `"patients"` or `"federation"`) and a profile
  (`profile = "rfc9068"` by default, or `"entra"`, `"keycloak"`, `"okta"`,
  `"nl"`), and holds every setting of its tokens itself. A configuration in
  the old shape refuses the start, naming what replaces it. Upgrade steps:
  write a flat `auth.issuer = "…"` as an `[[auth.issuer]]` table; move
  `audience`, `algorithms`, `clock_skew_seconds`, `jwks_max_age_seconds`,
  `jwks_refresh_interval_seconds`, `jwks_file`, `timeout_ms`, `mode`,
  `role_claims` and `[auth.introspection]` from `[auth]` onto each issuer
  table (`[auth.issuer.introspection]`); add `kind` to each issuer, with
  `kind = "machines"` for an issuer that signs only the client credentials
  tokens of Sources, consumers and subscribers, or `kind = "persons"` with
  the console's client in `first_party_clients` for one that also signs
  the stewards' logins; replace `token_profile = "jwt"` with
  `token_types = ["JWT"]` and the `required_claims` the issuer's tokens
  carry, and drop `token_profile = "rfc9068"`; and write each
  `scopeless_subjects` entry as `{ issuer = "…", sub = "…" }` (#487).
- Breaking for a development configuration: a Basic user is a person, a
  steward or an administrator, and never a Source or another machine. An
  `[[auth.basic.users]]` entry granting a `system/` scope refuses the start,
  naming the user, and a Basic user-id no longer counts as a domain's
  `source_client_id`. Upgrade: move each machine on Basic to a client
  credentials token (RFC 6749 §4.4) of an `[[auth.issuer]]`, list its
  client under that issuer's `[[trust.issuer]] clients`, and keep on Basic
  only the people who sign in by hand. `GET /operator/sources` no longer
  lists Basic users, and the operator API's `Admission` has no `basic_user`
  kind (#487).
- Breaking for a crate user: `ihe-iti` 0.0.76 requires `ferrohealth-smart`
  0.9 (caret `0.9.0`) and `ihe-atna` 0.4 (caret `0.4.0`), and
  `nl-generic-functions` 0.0.43 requires `ferrohealth-smart` 0.9 and
  `ihe-iti` 0.0.76. The `smart` feature's
  `authorizer::smart::TokenAuthorizer` takes a `TokenSource` of the 0.9
  line, and the `xcpd` ITI-55 audit message writes the `ihe-atna` 0.4
  message shape. Upgrade: move `ferrohealth-smart` to 0.9.0 and `ihe-atna`
  to 0.4.0 in the same change as these crates, or the build holds two
  copies of each (#487).
- The server and the console move to `ferrohealth-smart` 0.9.0,
  `ferrohealth-session` 0.3.0 and `ihe-atna` 0.4.0, which writes every
  string of a BALP `AuditEvent` with its control and bidirectional
  formatting characters escaped as code points (#487).

## [0.0.18] - 2026-11-06

A patch release with two fixes. The outbox sender no longer spins while
nothing waits in the outbox, which kept an idle server and its PostgreSQL
busy at over a CPU core. `[auth] scopeless_subjects` and `role_claims` now
hold per issuer (GHSA-pfgx-xq28-2629); a deployment that trusts several
issuers rewrites those entries before it upgrades. The published crates
`ihe-iti` and `nl-generic-functions` keep their versions.

### Fixed

- The outbox sender sleeps again while nothing waits in the outbox. Its look
  at the next due row answered "due now" for an empty outbox, or one holding
  only delivered or abandoned rows, so an idle server ran its claim back to
  back: about 8,000 PostgreSQL transactions a second and over a CPU core
  between the server and its database. A claim that takes no row is now
  always followed by a wait of at least a second (#490).

### Security

Shipped with functional changes because: the one other change is the fix of the outbox sender's idle busy loop, which keeps over a CPU core busy on every running FerroPIX, and holding either fix back for a release of its own would leave a running deployment with one of the two defects for longer.

- `[auth] scopeless_subjects` and `role_claims` hold per issuer, through
  `ferrohealth-smart` 0.8.9 (GHSA-pfgx-xq28-2629, CWE-863, CWE-639). Before
  it, beside several `[[auth.issuer]]` entries both lists applied to every
  trusted issuer's tokens: a token from any of them whose `sub` was listed
  was granted system-wide access without a scope, and any of them could
  assert roles. An entry is now written `<issuer>#<value>` and holds for
  that issuer alone; beside several issuers a bare entry, or one naming an
  issuer the section does not trust, refuses the start. Upgrade step for a
  deployment with several issuers: write each `scopeless_subjects` entry as
  `<issuer>#<sub>`, and set `role_claims = []` (the permission table reads
  scopes, not roles) or qualify each path, since the default `role_claims`
  is bare and refused. A deployment with one issuer changes nothing (#488).

## [0.0.17] - 2026-11-05

A family upkeep release. The server, the console and the testkit move to
the newest FerroHEALTH family crates, `ferrohealth-server` 0.5.0 among
them, with no change to what FerroPIX answers or reads from its
configuration. RUSTSEC-2026-0440 reaches only the test harness and does not
affect any shipped artefact; its OpenVEX statement records why. The
published crates `ihe-iti` and `nl-generic-functions` keep their versions.

### Changed

- The server, the console and the testkit run on the newest FerroHEALTH
  family releases: `ferrobridge-hl7v2` 0.1.182, `ferrohealth-config` 0.2.9,
  `ferrohealth-server` 0.5.0, `ferrohealth-session` 0.2.3,
  `ferrohealth-telemetry` 0.2.5 and `ferrohealth-usage-report` 0.2.1. None
  changes what FerroPIX answers or how it is configured (#484).

### Security

Shipped with functional changes because: no shipped artefact is affected (the VEX statement below records why), so there is no security update to separate from the other changes this release carries.

- RUSTSEC-2026-0440 (`astral-tokio-tar` is unmaintained, with no patched
  version) does not affect FerroPIX: `testcontainers` reaches the crate only
  in the test harness, and the published `ihe-iti` and `nl-generic-functions` crates do not contain it.
  `deny.toml` accepts the advisory until a `testcontainers` release drops
  the crate (`security/vex/rust-advisories.openvex.json`) (#481).

## [0.0.16] - 2026-11-05

### Changed

- The console journeys and the book's console screenshots run Chrome for
  Testing 157.0.8103.38 (#473).

## [0.0.15] - 2026-11-04

### Changed

- The server, the console and the testkit run on the newest FerroHEALTH
  family releases: `ferrobridge-hl7v2` 0.1.177, `ferrohealth-config`
  0.2.8, which masks a refused configuration value of any type in its
  message and names a key inside an array of tables by its index,
  `ferrohealth-session` 0.2.2, `ferrohealth-telemetry` 0.2.4, whose panic
  line carries the `request_id` of the request it broke, and `ihe-atna`
  0.3.3, whose BALP record names the data of an access by
  `what.identifier` when its id is no FHIR literal reference (#460).

## [0.0.14] - 2026-11-04

### Changed

- The server and console images build on the distroless static base the
  `nonroot` tag resolves to on 2026-11-03, the compose quickstart's
  development issuer and the testkit's stub OpenID Provider run
  `mock-oauth2-server` 6.1.4, the console journeys run Chrome for Testing
  156.0.8078.80, CI audits the workflows with zizmor 1.32.0 through
  `taiki-e/install-action` 2.90.0, and the release lanes write the build
  SBOMs with syft 1.56.0 (#400).
- The operator console draws with FerroUI 0.6.6 and signs in through
  `ferrohealth-session` 0.2.1, whose `[oidc]` section takes `resolve`, to
  reach a provider host at a fixed address, and `[oidc.tls]`, the client
  certificate the console presents to the provider and the lowest TLS
  version of that connection. The server takes `ferrohealth-smart` 0.8.8,
  which that release requires (#451).
- `ihe-iti` 0.0.74 and `nl-generic-functions` 0.0.41 ship a `Cargo.lock`
  that resolves `ferrohealth-smart` 0.8.8. Their code is unchanged (#451).

### Fixed

- The release vulnerability record, the scan before the image tags and the
  weekly published-image scan install Trivy from its GitHub release tarball,
  checked against a sha256 pinned per version. `aquasecurity/setup-trivy`
  downloaded it from `get.trivy.dev`, which stopped resolving on 2026-11-03,
  so every one of those lanes failed (#449).
- The console's Sign out goes to the provider's end-session endpoint only
  when the session it ends holds an ID Token, and otherwise to the
  signed-out page, through `ferrohealth-session` 0.2.1. A Basic sign-out on
  a console with a provider configured went to the provider, which held no
  session of the operator's to end (#456).

## [0.0.13] - 2026-11-03

### Added

- `[server] idle_timeout_ms` (120000 by default) closes a kept-alive HTTP/1.1
  connection that sends no next request within that time, with a clean end
  of stream. The default outlasts the 90 s a common HTTP client keeps an idle
  connection, so the client normally closes first (#384).
- The console's single-key shortcuts are back on FerroUI's `keys` module:
  on a pair `l` links (or asks first when the link merges), `n` decides
  Not a match, `d` defers, `r` puts the focus in the reason, and `[` and
  `]` lead to the neighbouring pairs; in the review queue `o` or `d` opens
  the focused row's side panel. Each control announces its key in
  `aria-keyshortcuts`, and the shortcut legend and the queue's hint list
  `o` and `d` again (#390).

### Changed

- The server takes `ferrohealth-telemetry` 0.2.3. Each exported request span
  is a server span named `{method} {route}`, such as
  `GET /fhir/Patient/$ihe-pix` (the method alone when no route matched),
  where it was named `request`, and its status is an error on a 5xx, under
  the OpenTelemetry HTTP semantic conventions. A method outside the
  standard set reads `_OTHER` in the request line and the span. `/metrics`
  gains `http_server_active_requests`, the requests in flight by method and
  scheme. `[telemetry.otlp]` refuses `trust_roots_file` or
  `client_identity_file` beside an endpoint that is not `https://`, and a
  file that is no PEM of its kind, where it exported in plain text with the
  files ignored. Migration: point the endpoint at `https://` when the
  collector speaks TLS, or drop the two keys when it does not (#382).
- FerroPIX runs on `ferrohealth-server` 0.4.1, which keeps a product's own
  fallback and the layers around it when the stack merges its routes
  (FerroHEALTH/FerroSYS#199). FerroPIX was not exposed to the regression
  0.4.1 fixes: every release ran on 0.3.8, which predates it, and the change
  that took 0.4.0 declared the fallback of the FHIR face and the operator API
  inside their guard, so an anonymous request to a path no route matches is
  refused with `401` before and after (#386).
- The operator console moves to FerroUI 0.6.0 and draws every page whole on
  its server: a link, a search, a filter or a decision loads one complete
  page, and FerroUI's small script modules add the keyboard, the side panels,
  the dialogs and the session warning on top, with every page working when
  they do not load. The console runs no WebAssembly and links no Leptos
  crate; its binary serves FerroUI's stylesheet, theme and script modules
  under `/ferrohealth-ui/0.6.0/`, cached for good, so the image carries the
  binary alone. The Content-Security-Policy now admits scripts and styles
  from the console's own origin alone, `script-src 'self'; style-src 'self'`,
  with no nonce and no `'wasm-unsafe-eval'`. The review queue's At a glance
  button now opens its side panel on a click, where the row's link took the
  press before (#388).
- The operator console moves to FerroUI 0.6.2 and `ferrohealth-server`
  0.4.2. A select in a dense options row and a long identifier or link in
  a stacked table no longer scroll the page sideways at 320 px. The
  console's Content-Security-Policy is unchanged (`script-src 'self';
  style-src 'self'`, no nonce) and now comes from the family's
  server-rendered console headers, FerroUI's script modules are served
  under `/ferrohealth-ui/0.6.2/`, and About names FerroUI 0.6.2 (#394).
- `ihe-iti` 0.0.67 and `nl-generic-functions` 0.0.34 drop the functions that only forwarded to another, so callers name the target: `PatientQuery::default()` and `Paging::default()` for the removed `new()`, `mcsd::replica::Scope::default()` for `Scope::everything()`, `Answer::ok` for `pdqm::server::retrieved` and `pmir::server::subscription::current`, and `Answer::created` for `pmir::server::subscription::created`. `server::format::FormatError`, with its `UnsupportedMediaType` and `NotAcceptable` variants, replaces `Unacceptable` as the error of `Format::negotiate` and `Format::unsupported_media_type()` as the `415` refusal. Every answer and message on the wire is unchanged (#404).
- `ihe-iti` 0.0.68 reads and writes the SOAP 1.2 envelope of ITI TF-2 Appendix V once, in the new `soap` feature: `soap::read` and `soap::answer` read an envelope through a `soap::Visitor`, `soap::write` writes one, and `soap::FaultCode`, `soap::Malformed` and the `ENVELOPE`, `ADDRESSING`, `ANONYMOUS` and `MEDIA_TYPE` constants are its own. `xcpd` reads and writes through it. `nl-generic-functions` 0.0.35's `mitz` feature now depends on `ihe-iti` with `soap` and reads the Mitz answer's envelope through it. A product changes `xcpd::error::FaultCode` and `mitz::error::FaultCode` to `ihe_iti::soap::FaultCode` (Mitz now names `VersionMismatch`, `MustUnderstand` and `DataEncodingUnknown`, which it reported as another code), and the `NotSoap`, `NotXml`, `DocumentType`, `TooDeep` and `NotAnEnvelope` variants of `xcpd::error::Malformation` and `mitz::error::Malformation` to `Malformation::Envelope(soap::Malformed::…)`. Every request, answer and fault on the wire is unchanged (#405).
- `ihe-iti` 0.0.69 holds the request authorizer once, in the new `authorizer` feature, which `pixm`, `pdqm`, `mcsd`, `pmir` and `smart` turn on and which compiles no FHIR model: `authorizer::send`, the send that resends once when the authorizer asks, and its `Unsent` are public. `nl-generic-functions` 0.0.36 drops its `authorizer` module and feature: the NVI client authenticates through `ihe_iti::authorizer`, so a product that implements an authorizer for it changes `nl_generic_functions::authorizer::{Authorizer, Authorized, AuthorizerError, Retry}` to `ihe_iti::authorizer::…` and depends on `ihe-iti` with `authorizer`. The token renewal stays with `ferrohealth-smart`'s `TokenSource`. Every request either crate sends is unchanged (#406).
- `ihe-iti` 0.0.70 and `nl-generic-functions` 0.0.37 write each shape the PDQm, PIXm, mCSD, PMIR, XCPD, NVI and Mitz clients share once. `authorizer::send` is generic over the client's error, which implements the new `exchange::Unanswered` and `authorizer::Unauthenticated`, and `authorizer::Unsent` is gone; `exchange::read` reads an answer to a bound, and `soap::post`, with the new `soap::Refusal`, sends a SOAP 1.2 message over HTTP, so the `soap` feature now compiles `reqwest`. The PDQm and PIXm `Malformation` hold `NotFhirJson`, `TooLarge`, `NotJson`, `NotAResource` and `Decode` in the new `outcome::Unreadable`, matched as `Malformation::Unreadable(Unreadable::NotFhirJson)`. `mcsd::directory::DirectoryEntry<Organization>` and `DirectoryEntry<Endpoint>`, over the sealed `DirectoryResource`, replace `DirectoryOrganization` and `DirectoryEndpoint`. `pixm::server::feed::Revision::into_patient` and `server::answer::Answer::into_resource`, which nothing called, are removed. Every request and answer on the wire is unchanged (#407).
- The operator console signs in and keeps its sessions through `ferrohealth-session` 0.1.0, the FerroHEALTH console sign-in crate, in place of its own copy. `[oidc]` now needs only `issuer`, `client_id` and `redirect_uri`: the provider's endpoints are discovered when the console starts, and `authorization_endpoint`, `token_endpoint`, `jwks_uri`, `end_session_endpoint` and the new `revocation_endpoint` each take the place of the discovered one; `ca_file` and `timeout_s` (default 10) are new. `[session]` gains `store` (`memory`, the default, or `sealed` with `key` or `key_file`), and refuses an `absolute_timeout_s` shorter than `idle_timeout_s`. `config check` calls no provider: a section that leaves the endpoints to discovery is read when the console starts. The routes `/login`, `/auth/callback`, `/logout` and `/session` keep their paths; a sign-out now revokes the session's token when the provider has a revocation endpoint, and a console with no provider ends the session and lands on the signed-out page in place of answering `503`. A page whose session lives on but whose access token the provider could not renew sends the steward to the new `/sign-in/unrenewed`, which says so and leads back to the page (#418).
- `ihe-iti` 0.0.71 and `nl-generic-functions` 0.0.38: a client's `with_authorizer` is now the one method of the `ihe_iti::authorizer::Authorizable` trait, which the PIXm, PDQm, mCSD and NVI clients and the PMIR subscriber implement, so a caller imports the trait to call it. The FHIR search value escaping and the FHIR JSON media type check move into the new `rest` feature as `ihe_iti::rest::escape` and `ihe_iti::rest::fhir_json`, which `pixm`, `pdqm`, `mcsd`, `pmir` and the `nvi` feature of `nl-generic-functions` turn on. Every message, answer and audit record is unchanged (#425).
- `ihe-iti` 0.0.72 and `nl-generic-functions` 0.0.39: the new `mcsd::directory::identifier_pairs` reads the `system` and `value` of a list of FHIR identifiers, which `DirectoryEntry<Organization>::identifiers` and `nl_generic_functions::lrza::ura` share, so the `lrza` feature now takes `ihe-iti` with `mcsd`. The mCSD client reads its search and history pages through one pager, and the mCSD, PDQm and PMIR readers share one owner of an entry's `fullUrl`, a request URL's logical id and the `next` link. Every message, answer and audit record is unchanged (#430).
- The operator console takes `ferrohealth-session` 0.1.1. Its `[oidc] post_logout_redirect_uri`, like `redirect_uri`, must be an `http` or `https` URL with a host and no userinfo. The session route now answers a gone session with an RFC 9457 problem whose detail is the catalog's `session-gone`. The provider, redirect and scope rules of `[oidc]`, and the refusal of a sign-in page as a return, are the crate's now; every configuration refusal names the same key as before (#432).

### Removed

- The console's `[server] site_root` key, its
  `FERROPIX_CONSOLE__SERVER__SITE_ROOT` variable and the `/site` directory of
  the console image: there is no site bundle to serve. A configuration that
  still sets the key is refused at start, naming it, so remove it; the Helm
  chart (0.3.5) no longer sets it. The console's own single-key shortcuts
  (`l`, `n`, `d`, `r`, `[` and `]` on a pair, `o` and `d` on the review
  queue) are gone until FerroUI binds a control's key (#390); every action
  keeps its button or link, and `/`, `?`, `j`, `k`, Enter and the digits
  stay (#388).

### Fixed

- The console dashboard's Listeners card shows the feed sender's real state.
  It read a readiness indicator the server never registers, so the row read
  Off on every deployment; the console and the server now take the
  indicator names from one definition in the operator API (#383).
- On a kept-alive HTTP/1.1 connection, `[server] header_read_timeout_ms`
  counts from the first byte of each request head. It used to count from the
  end of the previous answer, so it also closed an idle connection after
  30 s, sometimes as a client sent its next request into it, which a `POST`
  cannot safely retry (#384).

## [0.0.12] - 2026-10-29

### Changed

- The operator API, and the refusals the HTTP stack writes outside the FHIR
  face and the XCPD gateway, answer every error as RFC 9457 problem details,
  `application/problem+json`, where they answered a FHIR `OperationOutcome`.
  The status codes are unchanged. FerroPIX's problem types are defined once,
  each `type` a section of the book's Problem types page
  (<https://ferropix.eu/docs/operate/problems.html>), and a problem quotes
  nothing from the request. The FHIR face and the XCPD gateway keep their
  `OperationOutcome` and SOAP answers. Breaking for a client of the operator
  API that read the `issue` array: read `type`, `status` and `detail`
  instead. The console reads the new body (#379).
- `ferrohealth-server` 0.3.8: a `[server.tls]` table that sets
  `client_ca_file` while `client_auth` is `"none"`, the default included, is
  refused at load. Set `client_auth = "required"` (or `"optional"`) beside
  the CA, or remove it (#379).

## [0.0.11] - 2026-10-28

### Fixed

- A response to a request whose body FerroPIX left unread, such as a `404`
  to a `POST` of a path it does not serve or a declared-length `413`, now
  carries `Connection: close`, so a client opens a new connection for its
  next request where it used to meet a closed socket (RFC 9112 §9.6) (#371).
- Every request leaves one request line, a timeout `408`, a declared-length
  `413` and the `500` of a panicked handler included, and a `413` for a body
  with no declared length that passes `[server] body_limit_bytes` is
  answered with an `OperationOutcome`, as the declared-length `413` is (#371).
- Every accepted connection sets `TCP_NODELAY`, so a small response no
  longer waits on the client's delayed acknowledgment, and an HTTP/2 request
  still running when the drain bound passes is cancelled before the server
  exits, as an HTTP/1.1 request already was (#371).
- `[auth] ca_file` is trusted under `mode = "introspection"` too: with
  `ferrohealth-smart` 0.8.6 the server reads the issuer's metadata, calls
  the introspection endpoint and requests its own token trusting it, so an
  issuer behind a private certificate authority is reached with the setting
  that reaches it under `mode = "jwt"` (#373).
- The console takes `ferrohealth-ui` 0.5.5: the skip link moves the focus
  to the main area once the page has hydrated, so the next Tab reaches the
  page's first control (WCAG 2.2 SC 2.4.1); a search answered with `401`
  sends every other tab of the console to sign-in; and the search panel no
  longer panics when a navigation leaves the page while it opens (#373).

### Security

Shipped with functional changes because: the fix exists only in `ferrohealth-server` 0.3.7, which carries functional fixes in the same serving path beside it, so there is no security-only release of the crate for FerroPIX to take. The release also takes the newest shared crates (#373), so the fix ships on the family's current line.

- The server and the console take `ferrohealth-server`
  0.3.7 in place of 0.3.4 (GHSA-2mc2-h3jq-c5wg). Every earlier release held
  a plain-HTTP connection that never sent a byte for as long as the client
  kept it open, so silent connections from a client with no credentials
  could exhaust the process's file descriptors and stop it answering, health
  checks included; the listener then retried a failed `accept` at once and
  logged every failure. Such a connection now closes once
  `[server] header_read_timeout_ms` passes (30 s by default), counted from
  accept, and after an `EMFILE` or `ENFILE` the listener pauses up to 1 s
  before it accepts again and logs one warning every 10 seconds (#371).

## [0.0.10] - 2026-10-27

### Added

- The book's steward's guide walks the review queue, the three decisions,
  the merge and the unmerge in the console, with what each records and
  how a stale decision is answered (#106).
- The console's review queue draws the open pairs in FerroUI's work-list
  (#109): both leading names, their birth dates and domains, the score
  with its bar, the grade and the time queued. `j` and `k` move, Enter
  opens the pair, and `o` or `d` opens it at a glance in a side panel with
  Open to decide and Defer. The filters and the paging are posted, so the
  queue's state stays out of the address.
- The console's compare-and-decide page (#110): the two records side by
  side with the differing rows first, the score breakdown on the
  thresholds' scale, the blocking key, each record's patient with its
  identifiers masked, and the docked decision bar with Link (`l`), Not a
  match (`n`), Defer (`d`), the reason (`r`) and the neighbouring pairs
  (`[`, `]`). A link that merges two patients asks first in a danger
  dialog that names the effect, a decision posts the versions the page
  showed, a stale decision comes back in the bar with the ways on, and a
  decision without a reason is never sent.
- The console's merge and unmerge pages (#111): Merge into… previews the
  merge of the patient you name, the older surviving, with the
  identifiers that move, the pseudonyms that resolve to the survivor, the
  subscriptions told and the candidates closed; Unmerge… shows the latest
  merge into the patient with its reason and the preview of its revert,
  or names the merge to revert first. Both ask in a danger dialog with
  your reason, post the versions the preview read, and draw a stale answer
  in place.
- The console's pseudonymisation domains page draws every domain with its
  namespace, consumer, count of pseudonyms and state, and Resolve a
  pseudonym (#116): the domain, the pseudonym and a reason, posted and
  answered with the patient it belongs to, which the server records as a
  disclosure. The pseudonym never reaches an address and the answer never
  shows it. An administrator adds, renames, gives another consumer or
  retires a domain from the same page, and the patient's Pseudonyms tab
  links to it.
- `ihe-iti` 0.0.66 writes the audit records of the PIXm Manager and the
  PDQm Supplier behind `balp` (#350): `pixm::audit::manager_query` and
  `manager_feed` for ITI-83 and ITI-104, `pdqm::audit::supplier_records`
  for ITI-78 and ITI-119, from a `received::Received` request. The server
  writes its records through them, and the audit corpus checks all 13 IHE
  examples, both actors of each transaction.

### Changed

- Only a pseudonymisation domain's own consumer reads its pseudonyms
  through PIXm ITI-83 (#339). Another Consumer that names the domain as
  `targetSystem` now gets no pseudonym for it, where it read a minted one
  before, and a query naming no `targetSystem` carries the pseudonyms of
  the caller's own domains alone, by `GET` and by `POST`. A Consumer that
  read another domain's pseudonyms must ask as that domain's consumer.
- The Helm chart is 0.3.2: the server and the console containers carry an
  ephemeral-storage request and limit beside their memory limit
  (`resources`, `console.resources`; 256 MiB for the server, which holds the
  audit spool on its default `emptyDir`), and `deploy/helm/validate.sh`
  refuses a container without both limits (#360).

### Fixed

- The console refuses `session.secure_cookie = false` unless it is plain
  HTTP on loopback: its `oidc.redirect_uri`, or, when it signs in nowhere,
  its `server.listen` address (#358). Before, a console with no `[oidc]`
  section took it on any listen address, and sent its cookies without
  `Secure`.

## [0.0.9] - 2026-10-27

### Added

- The console hydrates the whole page on FerroUI 0.5, so a link, a search,
  a trail filter, a reveal or a domain's form keeps the page and loads no
  new document (#214). The top bar's search panel offers the pages, the
  identifier domains and the preferences whose names hold the words, never
  a person: a patient search runs on Enter or the panel's last row, posted
  and audited. `?` opens the shortcut legend on every screen, and `1` to `4`
  open the patient record's tabs (#204).
- The console leaves for sign-in the moment its session ends, on FerroUI's
  session watch (#322): two minutes before the idle timeout a signed-in
  page warns with Stay signed in and Sign out, signing out in one tab ends
  every tab, and the sign-in leads back to the page you were on. The new
  `/session` route answers what the session has left (`GET`, which never
  extends it) and extends it on the steward's own activity (`POST`), with
  `401` once the session is gone.

### Changed

- W3C DID Resolution 1.0 moves to the Candidate Recommendation Draft of
  2026-10-26, re-vendored by version and digest, as the freshness check
  reported (#275).
- The Helm chart is 0.3.1, the same templates and values as 0.3.0, with
  v0.0.9 as its appVersion.

## [0.0.8] - 2026-10-27

### Added

- Both binaries take `--set key=value`, any number of times, which sets a key over the file and the environment (#295). `RUST_LOG` sets the server's `telemetry.filter`. `config check` prints every key of the running configuration as `key = value  # source`, naming the file, the default, the variable or the `--set` argument that set it, with every secret shown as `***`.
- The server serves `GET {base}/metrics`, every metric in the Prometheus text format, with no token, and its request log records each request in `http_server_request_duration_seconds` (OpenTelemetry `http.server.request.duration`) by method, route template and status (#296). `[telemetry]` gains `logged_query_parameters`, `environment` and the `[telemetry.otlp]` table (`endpoint`, `traces`, `metrics`, `sample_ratio`, `trust_roots_file`, `client_identity_file`), which pushes the metrics beside the spans. The operator console reads the same `[telemetry]` section, with `RUST_LOG` as the alias of its `telemetry.filter`.
- The server takes the `[server]` keys every FerroHEALTH server shares (#299): `header_read_timeout_ms`, `max_concurrent_requests` with `retry_after_s` (past the bound a request is answered `503` with `Retry-After`, and the health routes never are), `trusted_proxies` with `forwarded_header`, `compression` with `compression_min_bytes`, and `[server.tls]`, which makes the listener speak TLS with an optional client certificate, reads its files again on `SIGHUP`, and has `healthcheck` probe over it.
- The server's `[auth]` takes every key of `ferrohealth-smart` (#303): `ca_file`, trust roots an issuer behind a private certificate authority is read with; `mode = "introspection"` with `[auth.introspection]`, which checks an opaque token at the issuer's RFC 7662 endpoint; a flat `issuer`; `scopes_supported` and `capabilities` for the discovery document; `role_claims` and `scopeless_subjects`.
- The server and the operator console serve `GET {base}/health/liveness`, which answers `200` while the process serves and checks nothing (#297).
- The Helm chart 0.3.0 sets `enableServiceLinks: false` on the server's and the console's pods, and takes `hostUsers` (default `false`, each pod in its own user namespace) and `supplementalGroupsPolicy` (default `Strict`) from its values (#291). A cluster whose nodes do not support user namespaces sets `hostUsers: true`, or the pods do not start.

### Changed

- **Breaking:** the server and the operator console read their configuration through `ferrohealth-config` 0.2.5 and hold their secrets in `ferrohealth-secret` 0.2.0, both pinned exactly, and FerroPIX's own loaders, provenance and secret types are deleted (#295). The file, `FERROPIX_CONFIG`, `FERROPIX_CONSOLE_CONFIG` and every `FERROPIX__…` and `FERROPIX_CONSOLE__…` key keep their names. What changes for an operator:
  - A secret file loses only its trailing line ending. Leading and other whitespace is now part of the secret, where it was trimmed, so a file holding only spaces is a secret of spaces and no longer refused as empty.
  - A refused configuration names every refusal (up to 16) on one line, each with its key, the variable or `--set` argument that set it, and the closest known key, where it named the first fault with its line and column.
  - A top-level key can be set from the environment, `FERROPIX__PROFILE=development`, where a variable needed a section and a key. A list key also takes comma-separated values.
  - The console reads a `FERROPIX_CONSOLE__` value as the type its key wants, where it read any value that parsed as TOML as that TOML value.
- **Breaking:** the server and the operator console run their console log, request log, span export and panic hook on `ferrohealth-telemetry` 0.2.2, pinned exactly, and the request id on `ferrohealth-server`, and FerroPIX's own `telemetry.rs`, `request_log.rs` and `request_id.rs` are deleted (#296). What changes for an operator:
  - `telemetry.otlp_endpoint` is removed: set `[telemetry.otlp] endpoint`. An `https` collector is trusted through the platform's roots and `trust_roots_file`, where it was the WebPKI roots compiled in.
  - `telemetry.filter` defaults to `info,hyper=warn,tower=warn,h2=warn`, where it was `info`.
  - The request log is written under the target `ferrohealth_telemetry::request`, where it was `ferropix_server::request`. Its `request_id` is the id the server minted, and the new `client_named` says whether the client sent one; the client's `X-Request-Id` is echoed on the response and no longer logged. The line gains `query`, empty unless `telemetry.logged_query_parameters` names a parameter.
  - The request line's `caller` and `domain` are left off a line they do not apply to, where they were `-`, and `domain` separates several domains with a comma, where it used a space.
  - The steward trail of `GET /operator/actions` records the server's request id. A BALP record keeps the `X-Request-Id` of the exchange, the client's when it sent a legal one.
  - The body of a `500` from a panicking handler names the server's request id, which the log line carries, where it named the client's.
  - The exported `request` span carries the server's request id.
  - The console's log takes `[telemetry]` (`format`, `filter`) where it read only `RUST_LOG`, and it renders as the server's does.
- **Breaking:** the server and the operator console serve the health routes of `ferrohealth-server` over the readiness registry and lifecycle of `ferrohealth-health` 0.1.1, pinned exactly, and FerroPIX's own registry and readiness documents are deleted (#297). What changes for an operator:
  - `GET {base}/health` answers the status of readiness with `{"state":"up"}` or `{"state":"down"}`, where it was liveness with no body (the console's answered `{"status":"up"}`). Point a liveness probe at `GET {base}/health/liveness`; the chart does.
  - The readiness document lists `indicators` as `[{"name", "state"}]` with a `phase`, where it was an object by name, and the open route no longer says why an indicator is down: `GET {base}/operator/dependencies` gives the detail behind the steward scope.
  - Readiness answers `503` while the server boots and from the moment `SIGTERM` or `SIGINT` arrives, so traffic stops before the drain.
  - The console probes the server's `GET {base}/health/liveness`.
- **Breaking:** the server sends its installation report through `ferrohealth-usage-report` 0.2.0, pinned exactly, and FerroPIX's own reporter, envelope types, state directory and window are deleted (#298). The `[usage_report]` keys, the state directory's files and the report's `status` keep their names. What changes for an operator:
  - The daily report leaves a counter out while it is zero, where it always sent `responses_4xx`.
  - The boot line ends "no clinical content, identifiers, URLs or request paths", the wording every FerroHEALTH product writes.
  - A refused `usage_report.endpoint` is named with the crate's rule, at load.
- **Breaking:** the server and the operator console run their HTTP stack, drain, health probe and shared subcommands on `ferrohealth-server` 0.3.4, pinned exactly, and FerroPIX's own middleware, panic catch, drain, signal handling, `healthcheck.rs` (both binaries) and the console's security headers and same-origin check are deleted (#299). `[server]` is the crate's `ServerSection`. What changes for an operator:
  - `server.public_base_url` is the top-level key `public_base_url` (`FERROPIX__PUBLIC_BASE_URL`); the Helm chart's `config.server.public_base_url` is `config.public_base_url`.
  - `server.drain_timeout_ms` is removed: set `server.shutdown_timeout_ms` (default `10000`), the time the requests in flight get once the listener closed, and `server.drain_delay_ms` (default `0`), the time the listener keeps accepting with readiness already `503`.
  - `[server.caller_rate]` is refused at load, since FerroPIX mounts no per-caller limit.
  - A `408`, a `413` and a load `503` written by the stack, and the `500` of a panicking handler, are FHIR `OperationOutcome`s (`timeout`, `too-long`, `throttled`, `exception`), where they were an empty body or `{"error":"internal","request_id":…}`.
  - Responses are compressed with gzip or Brotli when the client accepts one and the body is at least 1 KiB; `server.compression = false` turns it off.
  - `ferropix-console healthcheck` asks `GET /health/readiness`, where it asked `GET /health`, and both probes report `healthy` or `unhealthy: …` on one line. The console's drain on `SIGTERM` is bounded at ten seconds, where it waited for every connection.
  - The console's responses carry `Cross-Origin-Opener-Policy: same-origin`.
- **Breaking:** the server's `[auth]` is `ferrohealth-smart`'s `AuthSection`, and FerroPIX's own copy of the section is deleted (#303). What changes for an operator:
  - `[[auth.issuer]] clients` moves to the new `[[trust.issuer]]` table (`issuer`, `clients`), whose `issuer` must be one `[auth]` names.
  - `auth.steward_scope` is `auth.product_scope` (empty for `__ferropix.steward`), and `auth.admin_scope` is `operator.admin_scope`.
  - `auth.challenge_realm` is `auth.realm`, and an empty value names none.
  - `auth.enabled = false` is refused.
  - The server pins `ferrohealth-smart` 0.8.2 exactly, which refuses a misspelt key inside an `[[auth.issuer]]` entry by its name, and `ihe-iti` 0.0.65 and `nl-generic-functions` 0.0.33 require it as a caret, `0.8.2`. The Helm chart's README declares a Source's client under `config.trust.issuer`.
- **Breaking:** the server pins `ihe-atna` 0.3.0 exactly, and `ihe-iti` 0.0.64 requires it as a caret, `0.3.0` (#300). A product that takes `ihe-iti` moves its own `ihe-atna` pin to the 0.3 line with it. The BALP records FerroPIX writes do not change: 0.3.0 adds `User::with_policies`, which FerroPIX does not call. FerroPIX does not write the EHDS access record of `ehds-logging`: Regulation (EU) 2025/327 puts that log on an EHR system (Annex II 3.2), one that holds a priority category of Article 14(1), and the cross-reference holds none.
- The server pins `ferrohealth-smart` 0.8.1 exactly, and `ihe-iti` 0.0.63 and `nl-generic-functions` 0.0.32 require it as a caret, `0.8.1` (#294). A product that takes either crate moves its own `ferrohealth-smart` pin to the 0.8 line with it. Between 0.7.4 and 0.8.1 the crate renders every secret and masked URL through `ferrohealth-secret`.
- The operator console signs stewards in, refreshes their access tokens and signs them out through `ferrohealth_smart::oidc::RelyingParty`, the family's OpenID Connect relying-party client, and its own copy of the client is deleted (#294). The `[oidc]` keys, the routes, the cookies and the session store are unchanged. When a token answer names no scope, the scopes a session holds now include `offline_access` where `[oidc] offline_access` asked for it.

### Fixed

- The compose quickstart declares the healthchecks of the server and the console on their services, the same `healthcheck` subcommands the images' HEALTHCHECK runs, so `podman compose up --wait` starts the console and returns as `docker compose` does (#292). Podman runs the published images with no healthcheck, so a `service_healthy` dependent never started.

## [0.0.7] - 2026-10-26

### Added

- The server sends ITI-10, the PIX Update Notification, to the HL7 v2 consumers `[[hl7v2.consumers]]` names (#113). Every cross-reference change, whether a feed that creates or links a record, a merge, a revert or a removal, writes one ADT^A31 per consumer whose domains of interest the patient holds an identifier in to the outbox in the same transaction, listing the patient's identifiers in those domains (ITI TF-2 3.10.4.1.2). The sender delivers each consumer's notifications in order over MLLP through `ferrobridge-hl7v2` 0.1.158, checks the acknowledgment, retries within `[hl7v2.notification]`, and writes the Manager's audit record of ITI TF-2 3.10.5.1.1 for each notification the consumer answered. The dependency report names each consumer with its latest outcome (`consumer`, a new upstream kind), and the testkit has a stub MLLP consumer.
- The server sends ITI-93 to its PMIR subscribers (#104). Every committed change to a patient, whether from a feed, a Source's merge, an ITI-93 change, or a steward's link, merge or revert, writes one message per matching active subscription to the outbox in the same transaction, in the subscription's payload format. The sender delivers each subscription's messages in order with its channel headers, retries within `[pmir.feed]`, then sets the subscription to `error` with the reason, and an enable resumes from the next message. The dependency report names each subscriber with its latest outcome, and the new `outbox` readiness indicator reports a stuck sender. The unused `feed_outbox` table is dropped.

### Changed

- `ihe-iti` 0.0.62 and `nl-generic-functions` 0.0.31 take caret requirements on the family's shared crates, `ferrohealth-smart` 0.7.4 and, for `ihe-iti`, `ihe-atna` 0.2.13, so a FerroAUTH or FerroAUDIT patch reaches a product's lockfile without a release of these crates first (#281). The server pins both exactly, `ferrohealth-smart` 0.7.4 and `ihe-atna` 0.2.13.

### Fixed

- The release lane installs kubeconform beside helm, so the chart job validates and publishes the chart (#278). The v0.0.6 release published its images and binaries but no chart, and the chart for v0.0.6 is published through the chart fix lane.
- The chart job logs in to the registry before it attests the chart, so the provenance attestation is pushed beside the chart (#278). Chart 0.2.0 was pushed without its attestation; chart 0.2.1, the same templates and values for v0.0.6, is published with it. Chart 0.2.2 carries v0.0.7.

## [0.0.6] - 2026-10-26

### Added

- Pseudonyms (#114, #115):
  - An ITI-83 query that names a pseudonymisation domain as `targetSystem` answers the person's pseudonym in it. The domain's consumer mints it on the first ask, as one value however many first asks arrive at once. Any other caller reads only a minted pseudonym, a query that names no `targetSystem` mints nothing, and each mint writes its own BALP Create record.
  - The domains are seeded from `[[pseudonym_domain]]` with their `consumer_client_id`. The operator API lists, adds, changes and retires them, a steward mints through `POST /operator/pseudonyms`, and the reverse resolution `POST /operator/pseudonyms/resolve` is a steward action and a BALP disclosure.
  - A pseudonym resolves to the survivor while its person is merged, and to its own person again after the revert.
- The quickstart seeds the BSN and the pseudonymised BSN as `authoritative`, sealed identifier domains (#117). A pseudonymised BSN links two records deterministically, as a BSN does, and never reaches a log. A BSN that fails the eleven-proof is refused at the feed. The book's Dutch identifiers page states what is held, and that no BSNk service is called.
- `ihe-iti` 0.0.61 has the feature `smart` (#268):
  `authorizer::smart::TokenAuthorizer` is the IUA authorizer over
  `ferrohealth-smart`'s client-credentials `TokenSource`. Each PIXm, PDQm,
  mCSD or PMIR request carries the current token as a bearer token marked
  sensitive. A `401` drops the refused token and sends the request once
  more with a new one, and a token that cannot be obtained is an
  `AuthorizerError` whose source is the `TokenError`.
- Each release publishes the Helm chart to
  `oci://ghcr.io/ferrohealth/charts/ferropix` (#269): the reusable
  `build-chart.yml` packages it with the release as its app version and the
  server and console images pinned by the digests the release scanned,
  pushes it and attests its build provenance. `publish-chart.yml` publishes
  a chart fix between releases. The chart, now 0.2.0, documents the Service
  that serves PIXm and PDQm and the values that declare a domain and its
  Source, and adds the named template `ferropix.fhirBaseUrl` for a parent
  chart.
- The server receives ITI-93 as the PMIR Patient Identity Registry (#103):
  `POST {base}/fhir` from the master domain's Source, by its client id,
  applies each change of the feed message in JSON or XML. A create takes the
  Patient's UUID id as the patient's id, and a create of a held patient
  updates it; an update revises it; a merge (`replaced-by`) merges the
  patient into the survivor as the Source's own merge; a delete removes its
  master identifier. A non-UUID id is refused per entry with `invalid`, a
  patient the server does not hold with `404`, and an unmerge with `405`.
  Every message writes one BALP record. A served `Patient` no longer lists
  its master identifier twice.
- Matching never merges two master identities (#103): a master-domain
  record keeps its own patient, two master patients an identifier names are
  held apart as a candidate with the `master_identities` conflict, and a
  master identity survives any other patient. Between two patients of one
  kind the one created first survives, whatever the format of its id.
- The server answers ITI-94 as the PMIR Patient Identity Registry (#105):
  `POST {base}/fhir/Subscription` creates an active subscription held to
  the PMIR Subscription profile with one of the four criteria, and `GET`,
  the enable and disable `PUT` and `DELETE` of `Subscription/{id}` read,
  switch and delete it; the channel endpoint is `https`, every request is
  audited, the `CapabilityStatement` declares the `Subscription` resource,
  and a deleted subscription leaves the dashboard's count in error.
  `ihe-iti` 0.0.59 adds the `created` and `current` answers of a
  `Subscription`.
- The outbox keeps a delivered or abandoned message for `[outbox]
  retention_days` (30) and then prunes it, `prune_batch` at a time, once an
  hour (#251); a pending message is never pruned. The server's role prunes
  through the `pix.prune_outbox` function and holds no `DELETE` on the
  outbox.
- `ihe-iti` 0.0.58 carries the PMIR Patient Identity Registry side behind
  `pmir-server` (#102): `pmir::server::feed::FeedMessage` reads an ITI-93
  feed message in FHIR JSON or XML with its Patients, `feed::response`
  writes the response with a correlated refusal per entry,
  `pmir::server::subscription::RequestedSubscription` reads an ITI-94
  `Subscription` with one of the four criteria and `Criteria::matches` tests
  a Patient against them, and `pmir::server::notification::message` builds
  the ITI-93 message a subscriber receives.
- A durable outbox for outbound messages (#239): an XCPD asynchronous answer
  is written to `pix.outbox`, its payload sealed under the store's key,
  before the request is accepted with `202`, so a restart delivers it; the
  sender retries with a doubling wait, abandons a message whose attempts are
  spent, and finishes the message in flight on `SIGTERM` within the drain;
  `db reseal` rewrites the waiting payloads. ITI-10 will deliver through the
  same outbox.
- IHE's twelve ITI-55 example messages are vendored by
  `scripts/vendor/ihe-xcpd-examples.sh` (#228), and `ihe-iti`'s tests read
  them: the Responding Gateway reads IHE's request, every element its
  answers and its `NS250` refusal write is one IHE's examples carry, and the
  Initiating Gateway reads IHE's Case 2, 3 and 5 answers.
- The W3C Web Services Addressing 1.0 SOAP Binding Recommendation is vendored
  by `scripts/vendor/w3c-ws-addressing.sh` (#241), so the XCPD Responding
  Gateway's citation of its §5.2.2 is checked against the text in the
  repository.
- The XCPD asynchronous delivery presents a client certificate for mutual
  TLS (#240): `[xcpd] client_certificate_file` and `client_key_file` name the
  PEM chain and key, and `reply_trust_file` the authorities trusted for a
  reply endpoint beside the system roots.

### Changed

- The pins the freshness check reported behind upstream move (#252): W3C DID
  Resolution 1.0 to the Candidate Recommendation Draft of 2026-10-25,
  re-vendored, the console journeys' Chrome for Testing to 156.0.8078.47,
  and CI's yq to 4.54.1, each by version and digest.
- `ihe-iti` 0.0.57 names the receiver of a Responding Gateway answer by its
  device alone, as IHE's example answers do (#228); the answer no longer
  repeats the initiating community as the receiver's organization.

### Fixed

- The IHE citation guard reads a vendor cache that is a symbolic link, and
  in a git worktree with none of its own the main worktree's cache (#253),
  so a worktree checks the cache-only citations the main checkout can.
- The IHE citation guard reads the seventh-level headings profiles.ihe.net
  renders as `<h7>` (#237), so a citation of such a section, ITI TF-2
  §3.21.4.1.2.5.2 among them, is checked instead of refused when the HL7 v2
  cache is fetched.

### Security

Shipped with functional changes because: the advisory fix is a dependency move, and the unreleased functional changes already on `main` ship in the same release.

- The workspace takes `ferrohealth-smart` 0.7.1, which fixes GHSA-vphj-3f8c-5hpx, and `ihe-atna` 0.2.11. `ihe-iti` 0.0.60 and `nl-generic-functions` 0.0.30 are released against them.

## [0.0.5] - 2026-10-25

### Added

- The XCPD Responding Gateway answers the asynchronous exchange of ITI TF-2
  Appendix V.5 (#232): a request whose `ReplyTo` starts with one of
  `[xcpd] reply_endpoints` is accepted `202` and its answer posted to that
  endpoint with `RelatesTo` and `To`, retried with a doubling wait up to
  `[xcpd] delivery_attempts`; any other `ReplyTo` is a `Sender` fault. The
  Deferred Response Option is not offered. `ihe-iti` 0.0.55 writes the
  answer's `wsa:To` for an asynchronous request, and ITI TF-2 Appendix V is
  vendored beside ITI-38.
- The server answers ITI-55 as an XCPD Responding Gateway (#231) at
  `POST {base}/xcpd`, behind `[xcpd]`: a shared identifier is looked up in
  the cross-reference and demographics are scored by the matcher, settling
  the five cases of ITI TF-2 §3.55.4.2.3; the answer carries the patient's
  identifiers in the `[xcpd] domains`, a Deferred request is refused with
  `NS250`, a query from an initiating community not listed is Case 5, and
  every query writes the Responding Gateway audit record of §3.55.5.1.2.
- `ihe-iti` 0.0.54 carries the Responding Gateway side of ITI-55 in the
  `xcpd` feature (#230): `xcpd::responding::query::read` reads a Cross
  Gateway Patient Discovery request in each mode of ITI TF-2 §3.55.1,
  `xcpd::responding::answer` writes the answer of the five cases of
  §3.55.4.2.3, the `NS250` acknowledgement of a Deferred request and a SOAP
  fault, and `xcpd::responding::audit::RespondingAuditEvent` is the
  Responding Gateway audit message of §3.55.5.1.2.
- Automatic probabilistic linking keeps its ground (#112): with
  `[matching] auto_link = true`, each record a `certain` pair moves gets a
  link of kind `probabilistic` with the score and the evidence the pair was
  graded on, and the merge is an `auto-link` system action. The book's
  Matching page explains the switch and how to earn it.
- A steward's merge and its revert in the operator API (#108).
  `GET /operator/merges/preview` answers the whole effect of a merge or a
  revert and writes nothing; `POST /operator/merges` merges two persons with
  the versions the steward read, and `POST /operator/merges/{id}/revert`
  reverts the survivor's latest merge, replaying its moves backwards and
  making the merged person active again. A person that changed since the
  preview is `409`, and a revert out of order is `409` with the merge to
  revert first in its preview. Each is one steward action with its reason
  and one BALP record.
- The HL7 v2 face refuses ITI-22, the Patient Demographics and Visit
  Query, by name: a QBP^ZV1 is acknowledged `AR` with ERR code `200` and a
  text that points to ITI-21, since FerroPIX keeps no visit data and the
  transaction returns a PV1 per patient (ITI TF-2 §3.22.4.2.2.7) (#199).
- The review queue and the steward's decisions in the operator API (#107).
  `GET /operator/candidates` lists the open candidates and the deferred
  ones whose day has come, by grade, score and age, filtered by grade,
  domain, age and deferral; `GET /operator/candidates/{id}` answers one with
  its evidence, the two records at their versions and their persons'
  identifiers masked; `POST /operator/candidates/{id}/decide` links (a merge
  of the two persons, the older surviving), holds the pair apart, or defers
  it to a day, refused with `409` when the candidate was decided or a record
  moved since the versions it names. Each decision is one steward action
  with its reason and one BALP record, and a not-a-match pair stays out of
  the queue until a record's demographics change.

### Changed

- `ihe-iti` 0.0.53 takes `ihe-atna = "=0.2.10"` and `nl-generic-functions`
  0.0.29 takes `ferrohealth-smart = "=0.7.0"`, the family's newest pins, and
  the server runs on both; a product that takes `ihe-iti` moves to
  `ihe-atna = "=0.2.10"` with it, and one that takes `nl-generic-functions`
  with `nuts-auth` to `ferrohealth-smart = "=0.7.0"` (#225).
- The dashboard's auto-link rate counts only the records the scorer
  linked alone in the last 24 hours, where it counted every record whose
  patient held another record, a deterministic link included (#112).
- The operator console draws with FerroUI 0.4.0 (#215). The top bar's
  search still submits to the search page, which lists every match in the
  page; whether it answers under the field as you type waits on what a
  search typed that way records in the audit trail (#214).
- The HL7 v2 face takes `ferrobridge-hl7v2` 0.1.143 without its v2-to-FHIR
  mapper, so the server no longer links `fhirconnect`, `ferrobridge-term` or
  the openEHR crates. Its ITI-8, ITI-9 and ITI-21 audit records name the
  sending system's own network address, where they named the listener's;
  ITI-8's `ADT_A01` for A04, A05 and A08 is selected by the crate
  (`structure_declared`), and table 0357's `204` is the crate's
  `ErrorCode` (#213).
- `ihe-iti` 0.0.52 takes `ihe-atna = "=0.2.7"` and `fhir-types` 0.1.128 or
  later, and `nl-generic-functions` 0.0.28 takes `ferrohealth-smart =
  "=0.6.0"`, the family's newest pins, so a product on those lines (FerroEHR)
  resolves both crates beside its own pins. A product that takes `ihe-iti`
  moves to `ihe-atna = "=0.2.7"` with it, and one that takes
  `nl-generic-functions` with `nuts-auth` to `ferrohealth-smart = "=0.6.0"`
  (#211).
- The changelog structure guard refuses a blank line between two bullets of
  an `[Unreleased]` list, which would split the list in the release notes,
  and has a self-test that CI runs; released sections keep their form
  (#208).

## [0.0.4] - 2026-10-25

### Added

- The HL7 v2 face (#83): one MLLP Release 1 listener on `[hl7v2] listen`
  over FerroBRIDGE's `ferrobridge-hl7v2` and `hl7v2-types`, pinned exactly,
  with the frame ceiling, the idle and frame timeouts, the MSH-18 character
  sets, the senders of `[[hl7v2.senders]]` (a message from another is
  answered `AR`), a receipt per feed message so a message sent again is
  acknowledged with its first code and not applied twice, the
  `hl7v2_listener` readiness indicator and the drain on `SIGTERM` (#85).
  ITI-8 ADT^A01, A04, A05 and A08 add or revise a patient and ADT^A40 merges
  two, through the feed transaction ITI-104 goes through, so a v2 feed and a
  FHIR feed of the same patient write the same version and join one person
  (#86). ITI-9 QBP^Q23 answers RSP^K23 from the cross-reference ITI-83 reads,
  with the six cases of ITI TF-2 §3.9.4.2.2.6 (#87). ITI-21 QBP^Q22 answers
  RSP^K22 from the search ITI-78 runs, with the continuation protocol over a
  pointer that lives ten minutes and QCN^J01 (#88). Every message writes the
  Manager's or the Supplier's audit record before its answer leaves.
  `[hl7v2.namespaces]` names a domain's HL7 namespace id, and a sender with
  no `domain` may query and not feed. The usage report counts the face's
  messages in the `hl7v2` group with `hl7v2_ae` and `hl7v2_ar`.
- The Helm chart, `deploy/helm/ferropix` (#100): the server's Deployment
  with its startup, readiness and liveness probes on the health family, the
  HTTP Service and, with the HL7 v2 face on, the MLLP Service, the operator
  console's Deployment and Service, the PostgreSQL URL, the sealing key and
  the console's client secret as Secrets mounted as files, a NetworkPolicy,
  a PodDisruptionBudget and the usage report stating `helm`. Every
  container holds the Restricted Pod Security Standard with a read-only
  root. `deploy/helm/validate.sh` (CI's `helm-chart` job) lints, renders and
  holds it to the Kubernetes schemas with `kubeconform`, and
  `deploy/helm/kind-check.sh` installs it on a kind cluster.
- The operator API's sources and configuration (#120, #121).
  `GET /operator/sources` lists every system allowed to feed (each FHIR
  client an issuer lists, each Basic user of the development profile and
  each `[[hl7v2.senders]]` entry) with its channel, its admission, the
  domains that name it and its last activity, and
  `GET /operator/sources/{id}` one source with its last 20 messages, never
  a value. `GET /operator/configuration` shows the matching configuration in
  file mode, each setting with where its value came from: the file, the
  environment, the default, or built in. FerroPIX keeps no configuration
  store, so both are read-only and changed in the file.
- The operator console's pages drawn from the operator API (#89): the
  dashboard with its stat cards, the queue depth and feed charts, the
  listeners and the oldest pairs (#92); search by identifier and by
  demographics with graded results, the top bar's field read by the shape
  of what was typed (#93); the patient record with its identifiers, records
  side by side, history and pseudonyms, and the audited reveal (#94); the
  identifier domains with the administrator's editor, drawn only when the
  provider granted the token `[pix] admin_scope` (#95); the audit trail
  with posted filters and the CSV export (#96); the sources and the
  configuration in file mode (#120, #121). A refused call shows the
  server's issue in place.
- The console's browser journeys over the filled screens (#98): the WCAG 2.2
  AA battery runs over every screen drawn from synthetic operator API
  documents as well as over its refused state, in both themes and at
  320 px; `/` focuses the top bar's search, `j` and `k` walk the results
  and Enter opens the person; a dialog closed with Escape gives the focus
  back to its button. The book's console page shows every page in the light
  theme with its dark capture, taken behind `FERROPIX_SCREENSHOTS` at 1440
  and 390 px, and a guard fails when a route has no capture or the captures
  name another release.

### Changed

- `nl-generic-functions` 0.0.27 takes `ferrohealth-smart = "=0.5.0"` and
  drops its private copies of the RFC 8414 issuer checks the Nuts grant
  makes: the canonical issuer text, the issuer comparison, the endpoint on
  the issuer's origin and the refusal of JSON that repeats a name now come
  from the crate's `metadata` module. The grant keeps its own refusal of an
  endpoint with userinfo or a fragment. A product that takes
  `nl-generic-functions` moves to `ferrohealth-smart = "=0.5.0"` with it
  (#198).
- `[hl7v2]` refuses a `default_charset` the decoder does not read and two
  senders of one domain at load (ITI TF-2 §3.8.4.1.3: one Patient Identity
  Source per domain) (#85).
- The operator console moves to FerroUI 0.3.7, whose setting rows mark a
  value the environment set or nothing set.
- The changelog guard passes a pull request that adds a bullet to the
  newest `## [X.Y.Z]` section while no `vX.Y.Z` tag exists, so a fix that
  lands between the release pull request and the tag records itself in the
  notes the tag publishes, without the `no-changelog` label (#193).
- `ihe-atna` moves to 0.2.6, the FerroAUDIT release whose forwarder closes
  a syslog connection with `close_notify` before it reconnects and adds
  `Forwarder::stop`, so a product that pins 0.2.6 resolves `ihe-iti` beside
  it. FerroPIX's server delivers its audit records over the FHIR Feed and
  opens no syslog connection, so nothing else changes (#194, `ihe-iti`
  0.0.50).
- `SECURITY.md` states the response times every product of the Ferro family
  states: an acknowledgement within about 5 business days in place of seven
  days, an initial assessment within about 10 business days, both best
  effort, and a fixed release published within 7 days of the confirmed
  assessment for a Critical vulnerability, 30 days for High, 90 days for
  Medium and with the next release for Low.

### Fixed

- `ihe-iti`'s FHIR JSON body reader refuses a body that has no FHIR XML form
  at all as it refuses one whose XML form reads back changed, so the
  narrative the fuzzer found is still refused under `fhir-types` 0.1.128,
  whose XML writer now refuses that narrative instead of writing it changed
  (PIXm 3.1.0 §2:3.104.4.1.2; `ihe-iti` 0.0.51).

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
