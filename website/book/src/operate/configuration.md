<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Configuration

The FerroPIX server, the `ferropix` binary, reads one TOML file and the
environment. This page lists every key. The second half of the page covers
the crates, which you configure in your own code.

## The server

The server loads and checks every section below at start, and `serve` runs
the HTTP listener with its health routes and the guard in front of the FHIR
face and the operator API (`[auth]`). The services behind the sections
are planned in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3):
the store and its migrations
([#59](https://github.com/FerroHEALTH/FerroPIX/issues/59)), the usage
report ([#153](https://github.com/FerroHEALTH/FerroPIX/issues/153)), and
the PIXm and PDQm faces
([#66](https://github.com/FerroHEALTH/FerroPIX/issues/66),
[#70](https://github.com/FerroHEALTH/FerroPIX/issues/70)). A key whose
service is not built yet is still checked, so a configuration you write today
keeps working when the service lands.

### Commands

| Command | What it does |
|---|---|
| `ferropix serve` | Reads the metadata and key set of every `[[auth.issuer]]`, then serves until `SIGTERM` or `SIGINT` and finishes the requests in flight within `server.drain_timeout_ms`. An issuer that cannot be read, or issuers none of whose metadata names a `token_endpoint`, stop the start. |
| `ferropix config check` | Reads and resolves the configuration, secret files included, and exits 0 when the server would start on it. Otherwise it prints the first error on one line and exits 78. |
| `ferropix healthcheck` | Asks `GET {base}/health/readiness` of the server on this host, with a three-second timeout, and exits 0 only on `200`; a `503` from a readiness indicator that is down exits 1. Use it as the container health check. |
| `ferropix db migrate` | Applies the pending migrations. Planned with the store ([#59](https://github.com/FerroHEALTH/FerroPIX/issues/59)); today it exits 2 and names the issue. |

Every command takes `--config <PATH>`. A refused configuration exits 78
(`EX_CONFIG`), so an orchestrator can tell a bad configuration from a server
that failed to serve.

### The file and the environment

- The file is the one `--config` names, or else the one `FERROPIX_CONFIG`
  names. Without either, the defaults stand, and the start is refused for
  the keys that have none.
- `FERROPIX__<SECTION>__<KEY>` overrides any key, with two underscores
  between levels: `FERROPIX__SERVER__LISTEN=0.0.0.0:8080` sets
  `server.listen`, and `FERROPIX__PMIR__FEED__MAX_ATTEMPTS=5` sets
  `pmir.feed.max_attempts`. The value is read as the type the key wants.
- Every table refuses a key it does not know, so a misspelt key stops the
  start and the error names it.
- An error names the key and never the value, so no secret reaches your
  terminal or your log.

### Profiles

`profile = "production"` is the default. `profile = "development"` is for a
run on your own machine, and only it admits:

- the `[dev]` table;
- Basic users under `[auth.basic]`;
- a secret set inline, such as `db.url`, where production takes the
  `_file` key;
- a plain `http` public base URL on a host other than a loopback one;
- a plain `http` audit repository.

### Secrets

A secret is set under its key or read from the file its `<key>_file`
sibling names, never both. The file's text is trimmed, and a file that holds
only whitespace is refused. Under production, only the `_file` form is
accepted. A secret renders as `***` wherever the server prints its
configuration, and a URL that may carry a password renders with its user
information and its query replaced by `***`.

### The quickstart

`app/ferropix-server/quickstart.toml` is the smallest production
configuration: the loopback base, one issuer, one identifier domain, and
both `[db]` secrets read from files under `/run/secrets/`. The compose
quickstart that mounts it beside PostgreSQL is planned
([#75](https://github.com/FerroHEALTH/FerroPIX/issues/75)).

### The top level

| Key | Default | What it sets |
|---|---|---|
| `profile` | `"production"` | `"production"` or `"development"`; see [Profiles](#profiles). |

### `[server]`

| Key | Default | What it sets |
|---|---|---|
| `server.listen` | `"127.0.0.1:8080"` | The socket address the HTTP listener binds. |
| `server.base_path` | `""` | The path every route is mounted under: empty for the root, or `/` and path segments with no trailing `/`. |
| `server.public_base_url` | none, required | The URL a client reaches `{base}` at: the base of every resource location the FHIR face names, and the identifier system of FerroPIX's own master domain. `https`; plain `http` on a loopback host, or under development. No user information, query or fragment. |
| `server.body_limit_bytes` | `1048576` | The largest request body read; a larger one is answered `413`. |
| `server.request_timeout_ms` | `30000` | How long one request may take; a slower one is answered `408`. |
| `server.drain_timeout_ms` | `10000` | How long the requests in flight may take to finish after `SIGTERM`; a connection still open after it is dropped. |

The server routes, under `server.base_path`:

| Route | Answer |
|---|---|
| `GET {base}/health` | `200` while the process is up. |
| `GET {base}/health/readiness` | `200` today; the readiness indicators are planned ([#64](https://github.com/FerroHEALTH/FerroPIX/issues/64)). |

Every answer carries `x-request-id`: the one the request sent when it is
printable ASCII of at most 128 characters, or a minted UUID. A handler that
fails answers `500` with that id and no detail.

### `[telemetry]`

| Key | Default | What it sets |
|---|---|---|
| `telemetry.filter` | `"info"` | The log filter, in the `tracing-subscriber` `EnvFilter` syntax. |
| `telemetry.format` | `"auto"` | `"json"`, `"pretty"`, or `"auto"`: pretty on a terminal, JSON otherwise. |
| `telemetry.otlp_endpoint` | `""` | The OTLP collector, `http` or `https`. Checked today; the span export is planned ([#143](https://github.com/FerroHEALTH/FerroPIX/issues/143)). |

### `[db]`

| Key | Default | What it sets |
|---|---|---|
| `db.url` | none | The PostgreSQL URL, `postgres://` or `postgresql://`. Development only; under production use `db.url_file`. |
| `db.url_file` | none | The file holding the PostgreSQL URL. One of `db.url` and `db.url_file` is required. |
| `db.pool_size` | `10` | The most connections the pool holds. |
| `db.migrate` | `true` | Whether `serve` applies the pending migrations at start. |
| `db.data_key_file` | none, required | The file holding the key every identifier value is sealed under at rest. |

### `[auth]`

Every request to the FHIR face and the operator API carries a bearer token
one of these issuers signed. `ferrohealth-smart`, the FerroHEALTH resource
server, checks the section: each issuer is an `https` URL (plain `http` on a
loopback host), at least one audience is set, and only asymmetric algorithms
are admitted. A section with no issuer and no Basic user is refused.

| Key | Default | What it sets |
|---|---|---|
| `auth.audience` | `[]` | The audiences, of which a token's `aud` must contain one. |
| `auth.algorithms` | `["RS256", "RS384", "PS256", "PS384", "ES256", "ES384"]` | The signature algorithms admitted. |
| `auth.token_profile` | `"rfc9068"` | `"rfc9068"`, the JWT access token profile, or `"jwt"` for an authorization server that issues plain JWTs. |
| `auth.clock_skew_seconds` | `60` | The clock skew a token's times are checked with, at most 300. |
| `auth.jwks_max_age_seconds` | `600` | How long a fetched key set is used. |
| `auth.jwks_refresh_interval_seconds` | `60` | The shortest time between two fetches of the key set. |
| `auth.jwks_file` | `""` | A key set file to read in place of the issuer's `jwks_uri`. |
| `auth.timeout_ms` | `10000` | How long one call to an authorization server may take. |
| `auth.challenge_realm` | `""` | The protection space the `WWW-Authenticate` challenges name (RFC 9110 §11.5); empty for the server's own. |
| `auth.steward_scope` | `"__ferropix.steward"` | The scope of a data steward. |
| `auth.admin_scope` | `"__ferropix.admin"` | The scope of an administrator; it must differ from the steward scope. |

Both product scopes are a full URI or a string that starts with `__`, the
extension forms of SMART App Launch.

Each `[[auth.issuer]]` entry names one trusted issuer. A token field the
entry leaves out takes the section's value.

| Key | Default | What it sets |
|---|---|---|
| `auth.issuer[].issuer` | none, required | The issuer identifier every token's `iss` equals. |
| `auth.issuer[].clients` | `[]` | The client ids this issuer may vouch for as a Patient Identity Source. A `[[domain]]` whose `source_client_id` no issuer lists is refused. |
| `auth.issuer[].audience` | the section's | The audiences. |
| `auth.issuer[].algorithms` | the section's | The signature algorithms. |
| `auth.issuer[].token_profile` | the section's | The token profile. |
| `auth.issuer[].clock_skew_seconds` | the section's | The clock skew. |
| `auth.issuer[].jwks_max_age_seconds` | the section's | How long a key set is used. |
| `auth.issuer[].jwks_refresh_interval_seconds` | the section's | The shortest time between two fetches. |
| `auth.issuer[].jwks_file` | the section's | A key set file. |

`[auth.basic]` holds Basic users, for the development profile only, and
only beside the `[dev]` table. A Basic user's user-id also counts as a
Source client id. Who needs which scope is on the
[Security and audit](security-and-audit.md#who-may-call-the-server) page.

| Key | Default | What it sets |
|---|---|---|
| `auth.basic.cache_ttl_seconds` | `0` | How long a verified credential is remembered. |
| `auth.basic.users[].user` | none | The user-id. |
| `auth.basic.users[].password_hash` | none | The Argon2id PHC string of the password. |
| `auth.basic.users[].password_hash_file` | none | The file holding that string. |
| `auth.basic.users[].scopes` | `[]` | The scopes the user is granted. |

### `[[domain]]`

Each entry is an identifier domain written to the store at start when it is
absent. An entry never updates a domain the store holds, so the database
stays the record. A system named twice is refused.

| Key | Default | What it sets |
|---|---|---|
| `domain[].system` | none, required | The identifier system: a `urn:oid:` OID in the FHIR `oid` form, or another absolute URI. |
| `domain[].name` | none, required | The name a steward reads. |
| `domain[].kind` | none, required | `"local"` (a system's own numbers), `"authoritative"` (a national number the deterministic match links on) or `"master"` (FerroPIX's own). |
| `domain[].source_client_id` | none, required | The client id of the domain's one Patient Identity Source. |
| `domain[].sensitive` | `true` | Whether the domain's identifier values are sealed at rest. |

### `[matching]`

| Key | Default | What it sets |
|---|---|---|
| `matching.link_at` | `0.95` | The score at or above which a pair grades `certain`. |
| `matching.review_at` | `0.80` | The score at or above which a pair grades `probable`. |
| `matching.possible_at` | `0.60` | The score at or above which a pair grades `possible`. The three hold `0 < possible_at <= review_at <= link_at <= 1`. |
| `matching.auto_link` | `false` | Whether a `certain` pair is linked without a steward. |
| `matching.blocking_keys` | all three | The keys a pair must share one of to be scored: `"birth_year_family_phonetic"`, `"postal_code_birth_date"`, `"shared_identifier"`. |
| `matching.fields.<field>.m` | the scorer's | The probability that the field agrees for a true pair. |
| `matching.fields.<field>.u` | the scorer's | The probability that the field agrees for a random pair; `0 < u < m < 1`. |

`<field>` is one of `family_name`, `given_names`, `birth_date`, `gender`,
`postal_code`, `address`, `telephone`, `email`, `mothers_maiden_name` and
`local_identifier`.

### `[pixm]`, `[pdqm]` and `[pmir]`

| Key | Default | What it sets |
|---|---|---|
| `pixm.enabled` | `true` | Whether the PIXm Manager serves ITI-83 and ITI-104. |
| `pixm.remove_patient` | `false` | Whether ITI-104 serves the Remove Patient Option. |
| `pdqm.enabled` | `true` | Whether the PDQm Supplier serves ITI-78 and ITI-119. |
| `pdqm.deprecated` | `"inactive"` | How a merged patient is answered: `"inactive"` (with its replaced-by link, the survivor included) or `"omit"`. |
| `pmir.enabled` | `false` | Whether the PMIR Registry serves ITI-93 and ITI-94. |
| `pmir.feed.max_attempts` | `10` | The most delivery attempts of one outbound ITI-93 message. |
| `pmir.feed.initial_backoff_ms` | `1000` | The wait before the first retry. |
| `pmir.feed.max_backoff_ms` | `300000` | The longest wait between two retries. |

### `[hl7v2]`

| Key | Default | What it sets |
|---|---|---|
| `hl7v2.enabled` | `false` | Whether the MLLP listener runs. |
| `hl7v2.listen` | `"0.0.0.0:2575"` | The socket address the listener binds. |
| `hl7v2.default_charset` | `"ASCII"` | The HL7 table 0211 character set of a message with an empty MSH-18. |
| `hl7v2.frame_limit_bytes` | `1048576` | The largest message one frame may carry. |
| `hl7v2.idle_timeout_ms` | `300000` | How long a connection may sit idle between frames. |
| `hl7v2.frame_timeout_ms` | `30000` | How long one frame may take to arrive. |
| `hl7v2.senders[].application` | none | The sending application, MSH-3. |
| `hl7v2.senders[].facility` | none | The sending facility, MSH-4. |
| `hl7v2.senders[].domain` | none | The system of the identifier domain the sender feeds. |

A message from a sender not listed is refused. A sending application and
facility listed twice is refused.

### `[audit]`

| Key | Default | What it sets |
|---|---|---|
| `audit.url` | `""` | The FHIR base of the audit repository the BALP records are sent to; `https`, or plain `http` under development. |
| `audit.spool_dir` | `"/var/lib/ferropix/audit"` | The directory the records wait in until they are delivered. |
| `audit.spool_max_bytes` | `67108864` | The most bytes the spool holds. |
| `audit.spool_max_events` | `100000` | The most records the spool holds. |
| `audit.spool_write_timeout_ms` | `2000` | The longest storing one record may take. |
| `audit.timeout_ms` | `5000` | How long one delivery may take. |
| `audit.retry_max_ms` | `60000` | The longest wait between two delivery attempts. |

`serve` opens the spool at start and refuses to start when the directory
is open to its group or to other users, cannot be written, or holds a file
the server did not write. Without `audit.url` the records stay in the spool
until you name a repository, and the server warns at start. A request whose
record has no room in the spool is refused with `503`, as the
[Security and audit](security-and-audit.md#the-servers-audit-trail) page
says.

### `[operator]`

| Key | Default | What it sets |
|---|---|---|
| `operator.enabled` | `false` | Whether the operator API is mounted under `{base}/operator`, behind the steward and administrator scopes. |

### `[usage_report]`

| Key | Default | What it sets |
|---|---|---|
| `usage_report.enabled` | `true` | Whether the installation report is sent. |
| `usage_report.endpoint` | `"https://report.ferropulse.eu/v1/report"` | The collector; `https`, or plain `http` to a loopback host, with no credentials. |
| `usage_report.state_dir` | none | The writable directory the instance id lives in; without it the report stays off. |
| `usage_report.deployment` | `"unknown"` | How the server was deployed: `"compose"`, `"helm"`, `"binary"` or `"unknown"`. |

### `[dev]`

The development profile only.

| Key | Default | What it sets |
|---|---|---|
| `dev.loopback` | `true` | Whether every listener must bind a loopback address. |

## The crates

What you configure in your own code is the crates: which features you
compile, and how you build each client.

### Features

Every profile and function is a feature, and none is on by default.

| Crate | Features | Pages |
|---|---|---|
| `ihe-iti` | `pixm`, `pdqm`, `pmir`, `mcsd`, `xcpd`, and the audit records `balp` (with a FHIR profile) and `atna` (with `xcpd`) | [ihe-iti](../crates/ihe-iti.md#features) |
| `nl-generic-functions` | `nvi`, `mitz`, `lrza`, `nuts-auth`, and `authorizer` | [nl-generic-functions](../crates/nl-generic-functions.md#features) |

### Building a client

The settings below hold for every HTTP client of both crates. The API
reference on docs.rs has each constructor and builder method.

| Setting | Where | What to give it |
|---|---|---|
| The server's base URL | the constructor, as a `url::Url` | The FHIR base of the Manager, Supplier, Registry or directory. Its userinfo, if any, is shown as `***` in `Debug`. |
| The HTTP client | the constructor, as a `reqwest::Client` | Built with `redirect::Policy::none()`, and with the mutual TLS identity where the profile asks for one (XCPD, Mitz). |
| A timeout per exchange | each call, as a `Duration` | The bound of the whole exchange; with an audit feature it bounds the audit record as well. |
| Access tokens | `.with_authorizer(…)` | An authorizer that makes the headers of each request (IUA ITI-72); see [Security and audit](security-and-audit.md#access-tokens). |
| The audit recorder | `.audited(…)`, with `balp` or `atna` | The recorder that routes each record to your audit repository; see [Security and audit](security-and-audit.md#the-crates-audit-trail). |
| The PIXm invocation | `.invoked_by(Invocation::Post)` | `POST`, as every FerroHEALTH client calls a Manager, so the identifier stays out of the URL; leave the `GET` default only for a Manager that refuses the `POST` form ([Lookups use POST](../crates/ihe-iti.md#lookups-by-a-patient-identifier-use-post)). |
| The mCSD budget | `budget::Budget` | The deadline and the caps on pages, bytes and entries of one walk; by default 200 pages, 64 MiB and 50,000 entries. |
