<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Configuration

The FerroPIX server, the `ferropix` binary, reads one TOML file, the
environment and its `--set` arguments, through `ferrohealth-config`, the
FerroHEALTH configuration loader. This page lists every key. The second half of the page covers
the crates, which you configure in your own code.

## The server

The server loads and checks every section below at start, and refuses to
start on the first fault, naming its key. `serve` then starts the store
([The store](#the-store)) and runs the HTTP listener with its health routes
and the guard in front of the FHIR face and the operator API (`[auth]`),
each service you switch on (`[pixm]`, `[pdqm]`, `[pmir]`, `[hl7v2]`,
`[xcpd]`, `[operator]`), the usage report
([`[usage_report]`](#usage_report)) and, with a `[telemetry.otlp]` table,
the span and metric export. Every TOML block in this book is read by the
server's own configuration loader in CI, so a key it shows is a key the
server reads.

### Commands

| Command | What it does |
|---|---|
| `ferropix serve` | Starts the store ([The store](#the-store)), reads the metadata and key set of every `[[auth.issuer]]`, then serves until `SIGTERM` or `SIGINT`. On the signal readiness answers `503` at once, the listener keeps accepting for `server.drain_delay`, then closes, and the requests in flight get `server.shutdown_timeout` to finish. An issuer that cannot be read, or issuers none of whose metadata names a `token_endpoint`, stop the start. |
| `ferropix config check` | Reads and resolves the configuration, secret files included, prints every key as `key = value  # source` (the file, the default, the environment variable or the `--set` argument that set it, with every secret shown as `***`), and exits 0 when the server would start on it. Otherwise it prints the refusals on one line, each naming its key, and exits 78. |
| `ferropix healthcheck` | Asks `GET {base}/health/readiness` of the server on this host, over TLS when `[server.tls]` is set, with a three-second timeout, and exits 0 only on `200`; a `503` from a readiness indicator that is down, or from a server that is draining, exits 1. Use it as the container health check. |
| `ferropix db migrate` | Applies the pending migrations and the `[[domain]]` and `[[pseudonym_domain]]` seeds, checks the sealing key, and exits 0. It does not apply the boot gate, so it can run as a role with more rights than the one `serve` connects as. |
| `ferropix db reseal --old-key-file <PATH>` | Rewrites every identifier column under the key `db.data_key_file` names, reading the key the store is written under now from `<PATH>`, and exits 0 when no row is left under the old key. |

Every command takes `--config <PATH>` and any number of `--set key=value`
arguments. A refused configuration exits 78
(`EX_CONFIG`), so an orchestrator can tell a bad configuration from a server
that failed to serve.

### The file and the environment

- The file is the one `--config` names, or else the one `FERROPIX_CONFIG`
  names. Without either, the defaults stand, and the start is refused for
  the keys that have none.
- `FERROPIX__<SECTION>__<KEY>` overrides any key, with two underscores
  between levels: `FERROPIX__SERVER__LISTEN=0.0.0.0:8080` sets
  `server.listen`, `FERROPIX__PMIR__FEED__MAX_ATTEMPTS=5` sets
  `pmir.feed.max_attempts`, and `FERROPIX__PROFILE=development` sets the
  top-level `profile`. The value is read as the type the key wants; a list
  takes TOML array syntax or comma-separated values.
- `RUST_LOG` sets `telemetry.filter`, over the file and over
  `FERROPIX__TELEMETRY__FILTER`.
- `--set key=value` sets the dotted key last, over the file and the
  environment: `--set server.listen=0.0.0.0:8080`.
- Every table refuses a key it does not know, so a misspelt key, in the file,
  in a `FERROPIX__` variable or in a `--set` argument, stops the start and the
  error names it with the closest known key. Another variable that starts
  with `FERROPIX_` and is no override, such as `FERROPIX_CONFIG`, is left
  alone.
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
sibling names, never both. The file's trailing line ending is removed, and
any other whitespace is part of the secret; a file with nothing left is
refused. Under production, only the `_file` form is
accepted. A secret renders as `***` wherever the server prints its
configuration, and a URL that may carry a password renders with its user
information and its query replaced by `***`.

### The quickstart

`app/ferropix-server/quickstart.toml` is the smallest production
configuration: the loopback base, one issuer, one identifier domain, and
both `[db]` secrets read from files under `/run/secrets/`. Its issuer is
the development OpenID Provider of the compose quickstart, a `persons`
issuer: it signs the Sources' client credentials tokens and the console's
logins, as plain JWT bearer tokens, so the entry sets `token_types =
["JWT"]` and the claims those tokens carry. It signs a login's access token
with the claims of its ID Token, whose audience is the console's client id
alone, so the entry lists `ferropix-console` as a second audience. The
quickstart also mounts the operator API, which the console reads. The
compose quickstart mounts it beside PostgreSQL
([Deployment](deployment.md#the-compose-quickstart)).

### The top level

| Key | Default | What it sets |
|---|---|---|
| `profile` | `"production"` | `"production"` or `"development"`; see [Profiles](#profiles). |
| `public_base_url` | none, required | The URL a client reaches `{base}` at: the base of every resource location the FHIR face names, and the identifier system of FerroPIX's own master domain. `https`; plain `http` on a loopback host, or under development. No user information, query or fragment. |

### `[server]`

`[server]` is the section every FerroHEALTH server shares, read by
`ferrohealth-server`. A span of time is a string of a whole number and its
unit, `"500ms"`, `"30s"`, `"10min"`, `"12h"` or `"7d"`, and a size is one such
as `"512KiB"` or `"1MiB"`; a bare number is refused, and a key that holds one
carries no unit in its name. FerroPIX's own sections write them the same
way.

| Key | Default | What it sets |
|---|---|---|
| `server.listen` | `"127.0.0.1:8080"` | The socket address the HTTP listener binds; a host name is refused. |
| `server.base_path` | `""` | The path every route is mounted under: empty for the root, or `/` and path segments with no trailing `/`. |
| `server.body_limit` | `"1MiB"` | The largest request body read; a larger one is answered `413`. |
| `server.request_timeout` | `"30s"` | How long one request may take; a slower one is answered `408`. |
| `server.header_read_timeout` | `"30s"` | How long a client may take to send a request head, counted from accept or from the first byte of the head; a client that has not sent the whole head by then is disconnected. |
| `server.idle_timeout` | `"2min"` | How long a kept-alive HTTP/1.1 connection may wait for its next request after an answer; past it the server closes the connection with a clean end of stream. HTTP/2 connections keep their own protocol's rules. |
| `server.drain_delay` | `"0s"` | How long the listener keeps accepting after `SIGTERM`, with readiness already answering `503`, so an orchestrator stops routing first. |
| `server.shutdown_timeout` | `"10s"` | How long the requests in flight may take to finish once the listener closed; a connection still open after it is closed. |
| `server.max_concurrent_requests` | unset | How many requests the server serves at once; past it a request is answered `503` with `Retry-After`. The health routes are never refused. Unset, there is no bound. |
| `server.retry_after` | `"1s"` | How long a request refused for load is told to wait, a whole number of seconds. |
| `server.trusted_proxies` | `[]` | The reverse proxies whose forwarded client address is believed, each an address or a CIDR block such as `10.0.0.0/8`; empty, every request is named by its peer. |
| `server.forwarded_header` | `"forwarded"` | The header a trusted proxy names the client in: `"forwarded"` (RFC 7239) or `"x-forwarded-for"`. |
| `server.compression` | `true` | Whether a response is compressed with gzip or Brotli when the client accepts one. |
| `server.compression_min_size` | `"1KiB"` | The smallest response body that is compressed. |

`[server.caller_rate]` is refused: FerroPIX mounts no per-caller rate limit.

`[server.tls]` makes the listener speak TLS; without it the listener speaks
plain HTTP behind a proxy that terminates TLS. `SIGHUP` reads its files
again.

| Key | Default | What it sets |
|---|---|---|
| `server.tls.certificate_file` | none, required | The certificate chain in PEM, the leaf first. |
| `server.tls.key_file` | none, required | The private key of the leaf, in PEM. |
| `server.tls.client_auth` | `"none"` | Whether the listener asks a client certificate: `"none"`, `"optional"` or `"required"`. |
| `server.tls.client_ca_file` | unset | The CA bundle a client certificate must chain to; required when `client_auth` is `"optional"` or `"required"`, and refused at load beside `"none"`, the default included. |
| `server.tls.reload_interval` | unset | How often the files are read again; unset, only `SIGHUP` reads them. |
| `server.tls.healthcheck_identity_file` | unset | The client certificate chain and key, in one PEM, that `ferropix healthcheck` presents to a listener that asks one. |
| `server.tls.min_version` | `"1.3"` | The oldest TLS version the listener speaks, `"1.3"` or `"1.2"`. |

Every refusal the stack writes itself, a `408`, a `413` and a `503` for
load, is a FHIR `OperationOutcome` (`timeout`, `too-long`, `throttled`) on
the FHIR face and the XCPD gateway, as is the `500` of a handler that
failed, which names the server's request id. On every other route each is
an `about:blank` [problem](problems.md) whose `instance` is the server's
request id, which the request log names, even when the request sent an
`X-Request-Id` of its own.

The server routes, under `server.base_path`:

| Route | Answer |
|---|---|
| `GET {base}/health/liveness` | `200` while the process serves. |
| `GET {base}/health/readiness` | `200` while the server serves and no readiness indicator is down, the `database` indicator of the store among them, and `503` otherwise ([Operations](operations.md)). |
| `GET {base}/health` | The status of readiness, with `{"state":…}` alone. |
| `GET {base}/metrics` | Every metric in the Prometheus text format, among them `http_server_request_duration_seconds` by method, route template and status, and `http_server_active_requests`, the requests in flight by method and scheme. It needs no token, so keep it off the public side of your proxy. |

The server mints its own UUID for every request, which the request log and
the steward trail of `GET {base}/operator/actions` carry. Every answer carries `x-request-id`:
the one the request sent when it is printable ASCII of at most 128
characters, which is echoed and never logged, or else the server's id. A
handler that fails answers `500` naming the server's id and no detail.

### `[telemetry]`

The section is `ferrohealth-telemetry`'s, the one every FerroHEALTH product
reads.

| Key | Default | What it sets |
|---|---|---|
| `telemetry.filter` | `"info,hyper=warn,tower=warn,h2=warn"` | The log filter, in the `tracing-subscriber` `EnvFilter` syntax. A filter that does not parse refuses the start. |
| `telemetry.format` | `"auto"` | `"json"`, `"pretty"`, or `"auto"`: pretty on a terminal, JSON otherwise. |
| `telemetry.logged_query_parameters` | `[]` | The query parameters whose values the request line carries in its `query` field. Name only parameters that hold no identifier or free text; the ITI-83 and ITI-78 parameters carry identifiers and demographics. |
| `telemetry.environment` | none | The deployment environment the exported spans and metrics name (`deployment.environment.name`), such as `production`. |
| `telemetry.otlp.endpoint` | none | The collector's OTLP/gRPC endpoint; `https://` speaks TLS. It may carry a credential, so `config check` shows it masked. A `[telemetry.otlp]` table without it refuses the start. |
| `telemetry.otlp.traces` | `true` | Whether the spans are exported: one server span per request, named `{method} {route}` such as `GET /fhir/Patient/$ihe-pix`, with the method, the matched route template, the status and the server's request id, and an error status on a 5xx. No raw path, query string, body, header value, identifier or token, and no log event. |
| `telemetry.otlp.metrics` | `true` | Whether the metrics are pushed. |
| `telemetry.otlp.sample_ratio` | `1.0` | The share of the traces that are sampled, from `0.0` to `1.0`. |
| `telemetry.otlp.tls.trust_roots_file` | none | A PEM file of trust roots the collector's certificate may chain to, beside the platform's. The server refuses to start when the endpoint is not `https://` or the file holds no PEM certificate. |
| `telemetry.otlp.tls.client_identity_file` | none | A PEM file holding a client certificate chain and its key, for mutual TLS with the collector. The server refuses to start when the endpoint is not `https://` or the file lacks a PEM certificate or a PEM private key. |
| `telemetry.otlp.tls.min_version` | `"1.3"` | The oldest TLS version spoken to the collector, `"1.2"` to lower it. |

Without a `[telemetry.otlp]` table nothing is exported.

### `[db]`

| Key | Default | What it sets |
|---|---|---|
| `db.url` | none | The PostgreSQL URL, `postgres://` or `postgresql://`. Development only; under production use `db.url_file`. |
| `db.url_file` | none | The file holding the PostgreSQL URL. One of `db.url` and `db.url_file` is required. |
| `db.pool_size` | `10` | The most connections the pool holds. |
| `db.migrate` | `true` | Whether `serve` applies the pending migrations at start. With `false`, run `ferropix db migrate` before `serve`. |
| `db.data_key_file` | none, required | The file holding the sealing key: the base64 of 32 random bytes, which `openssl rand -base64 32` makes. Any other content is refused. |

### The store

The server keeps the cross-reference in schema `pix` of a PostgreSQL 18
database. `ferropix db migrate`, and `serve` unless `db.migrate = false`, run
these steps in order:

1. **The migrations.** The migration set is built into the binary and
   applied in order, each file once, under a lock, so two servers that start
   together apply it once. The migrator creates schema `pix` and keeps its
   record of applied files in `pix._sqlx_migrations`. A file changed after it
   was applied stops the start.
2. **The key check.** The first start records which key the identifiers are
   written under. A later start with a different `db.data_key_file`, or one
   during an unfinished `db reseal`, is refused, because the server would
   otherwise find no record by its identifier and answer every lookup as
   unknown.
3. **The seed.** Each `[[domain]]` the store lacks is inserted. A domain the
   store holds already is left as it is, since the store is the record; when
   it differs from the seed, the server logs a warning naming its system.
   Each `[[pseudonym_domain]]` the store lacks is inserted the same way.

Before these steps, `serve` refuses to start when the role it connects as
can read anything outside `pix`: a superuser, a member of
`pg_read_all_data`, or a role with `SELECT` on any table, view or sequence
in another schema. Connect `serve` as a role that has `CREATE` on the
database (the migrations create the schema and the `btree_gist` extension,
which PostgreSQL trusts any such role to create) and nothing else. To
migrate with another role, run `ferropix db migrate` as that role first;
the last migration grants a role named `ferropix` the rows it needs, so
`serve` can connect as `ferropix` with `db.migrate = false`.

**Sealing.** In a domain with `sensitive = true`, an identifier value is
stored encrypted with AES-256-GCM under the sealing key, and every
identifier is found by an HMAC-SHA256 under a key derived from it, so a
database dump shows no identifier and a lookup is one index probe. This
holds for the identifier a record is fed under and for every other
identifier the fed `Patient` carries in a domain FerroPIX knows. Before it
is sealed, a value takes its domain's form: a BSN keeps its nine digits
(spaces, dots and dashes dropped, a dropped leading zero restored) and must
pass the eleven-proof; any other value is trimmed and keeps its case. The
demographics are stored plain, because search and matching read them, and
without any identifier: the `identifier`, `link`, `contained`, `text`, `id`
and `meta` of the fed `Patient` are left out.

**Rotating the key.** Stop every server, put the new key in the file
`db.data_key_file` names, keep the old key in another file, and run
`ferropix db reseal --old-key-file <old>`. It rewrites the identifier
columns of the records and of the identifiers each version carries in
batches of 500, one transaction each, and a run that stops
part way resumes when you run it again. Until it finishes, `serve` refuses
to start. Afterwards the old key opens nothing, and you can destroy it.

### `[auth]`

Every request to the FHIR face and the operator API carries a bearer token
one of the `[[auth.issuer]]` issuers signed, or, for an issuer under
`mode = "introspection"`, a token that issuer's introspection endpoint
vouches for. `[auth]` is the section every FerroHEALTH server shares, read by
`ferrohealth-smart`, the FerroHEALTH resource server: each issuer is an
`https` URL (plain `http` on a loopback host, and never with a user or
password in it, which refuses the start and appears in no output) with its
`kind`, at least one
audience is set on each, and only asymmetric algorithms are admitted. A
section with no issuer and no Basic user is refused, and so is
`enabled = false`: FerroPIX always authenticates its callers.

| Key | Default | What it sets |
|---|---|---|
| `auth.enabled` | `true` | Must stay `true`. |
| `auth.realm` | `""` | The protection space the `WWW-Authenticate` challenges name (RFC 9110 §11.5); empty for none. |
| `auth.tls.trust_roots_file` | unset | A PEM file of trust roots added to the platform's, for an issuer behind a private certificate authority: every issuer's metadata and key set are read trusting it, and for an introspected issuer the metadata, introspection and token calls too. Read once at start; a file that holds no certificate refuses the start. |
| `auth.tls.client_identity_file` | unset | A PEM file holding the client certificate chain and its key that those calls present for mutual TLS. |
| `auth.tls.min_version` | `"1.3"` | The oldest TLS version those calls speak, `"1.2"` to lower it. |
| `auth.product_scope` | `""` | The scope of a data steward, a full URI or a string that starts with `__`; empty for `__ferropix.steward`. |
| `auth.scopes_supported` | `[]` | The scopes the SMART discovery document lists; empty for every scope the permission table enforces. List only scopes FerroPIX honours. |
| `auth.capabilities` | `[]` | SMART capabilities the discovery document adds, each one SMART App Launch defines or a full URI. |
| `auth.scopeless_subjects` | `[]` | The subjects granted system-wide access without a scope, each a table `{ issuer, sub }` naming an issuer the section trusts, since a `sub` is unique only within its issuer (RFC 7519 §4.1.2). An entry admits only a machine's token of that issuer. |
| `auth.federation` | `[]` | One `[[auth.federation]]` table for each `"federation"` issuer, which that issuer requires: how the tokens of another party's authorization server are read and classified. A table that names no `"federation"` issuer refuses the start. |
| `auth.attributes` | `[]` | At most one `[[auth.attributes]]` table for a `"persons"`, `"patients"` or `"machines"` issuer: where its tokens state the professional's attributes and the purpose of use, `mapping = "iua"` for the IHE IUA claims under `extensions.ihe_iua` (ITI TF-2 §3.71.4.2.2.1.1). A token that misstates them is refused, and the audit record writes the user's name, role, purpose of use, provider id and organisation on its user agent, as BALP 1.1.4 §3:5.7.5.4 maps them. |
| `auth.party` | `[]` | Refused when set: FerroPIX binds no login to a party. |

FerroPIX refuses the start on a table it would read and never apply, naming
the key: `[[auth.party]]`, `[auth.idp]` (the identity provider adapters only
a party binding reads), `[auth.dpop]` (FerroPIX checks no `DPoP` proof,
RFC 9449), a Basic user's `roles` and an issuer's `role_claims`.

The issuer `auth.scopeless_subjects`, `[[auth.federation]]`,
`[[auth.attributes]]` and `[[auth.party]]` name renders as `[[auth.issuer]]`
does: a user or password in it appears in no output.

Each `[[auth.issuer]]` table names one trusted issuer with every setting of
its tokens: no section-level value stands in for one the table leaves out.
`kind` is required and says whose tokens the issuer signs:

- `"persons"`: people signing in through the deployment's own clients,
  each listed in `first_party_clients`, and the client credentials tokens
  of its machines;
- `"machines"`: machines, through the client credentials grant (RFC 6749
  §4.4) only;
- `"patients"` and `"federation"`, the other two kinds `ferrohealth-smart`
  defines: a patient's token and another party's.

The profile is how the issuer's tokens read: `profile` picks a preset,
`"rfc9068"` (the JWT access token profile of RFC 9068, the default),
`"entra"`, `"keycloak"`, `"okta"` or `"nl"`, and any profile key set on the table
takes the place of the preset's. A token whose `sub` is its client is a
machine's under `"rfc9068"` (RFC 9068 §2.2).

```toml
[auth]
scopeless_subjects = [
  { issuer = "https://idp.example.org/realms/pix", sub = "service-account-sync" },
]

[[auth.issuer]]
issuer = "https://idp.example.org/realms/pix"
kind = "persons"
profile = "keycloak"
audience = ["https://pix.example.org/fhir"]
first_party_clients = ["ferropix-console"]

[[auth.issuer]]
issuer = "https://partner.example.net"
kind = "machines"
profile = "rfc9068"
audience = ["https://pix.example.org/fhir"]
```

| Key | Default | What it sets |
|---|---|---|
| `auth.issuer[].issuer` | none, required | The issuer identifier every token's `iss` equals. |
| `auth.issuer[].kind` | none, required | `"persons"`, `"machines"`, `"patients"` or `"federation"`. |
| `auth.issuer[].profile` | `"rfc9068"` | The preset the issuer's profile starts from. |
| `auth.issuer[].audience` | `[]` | The audiences, of which a token's `aud` must contain one; at least one. |
| `auth.issuer[].algorithms` | `["RS256", "RS384", "PS256", "PS384", "ES256", "ES384"]` | The signature algorithms admitted. |
| `auth.issuer[].clock_skew` | `"1min"` | The clock skew a token's times are checked with, at most `"5min"`. |
| `auth.issuer[].jwks_max_age` | `"10min"` | How long a fetched key set is used. |
| `auth.issuer[].jwks_refresh_interval` | `"1min"` | The shortest time between two fetches of the key set. |
| `auth.issuer[].jwks_file` | `""` | A key set file to read in place of the issuer's `jwks_uri`. |
| `auth.issuer[].timeout` | `"10s"` | How long one call to the authorization server may take. |
| `auth.issuer[].mode` | `"jwt"` | `"jwt"` checks a token against the issuer's key set; `"introspection"` posts it to the issuer's RFC 7662 introspection endpoint, for a token the server cannot read itself. |
| `auth.issuer[].sender_constraint` | `"none"` | `"required"` admits only a token bound to a `DPoP` key or a client certificate. |
| `auth.issuer[].role_claims` | unset | Refused when set: the permission table reads scopes, so a role would never apply. |
| `auth.issuer[].first_party_clients` | the profile's | The deployment's own clients, through which a person's token is admitted. |
| `auth.issuer[].token_types` | the profile's | The `typ` header values admitted, `["at+jwt", "application/at+jwt"]` under `"rfc9068"`. |
| `auth.issuer[].required_claims` | the profile's | The claims every token carries. |
| `auth.issuer[].client_claims` | the profile's | The claims the client is read from, the first present. |

The other profile keys of `ferrohealth-smart` (`person_key`,
`tenant_claim`, `app_only`, `guest`, `delegates`, `scope_claim` and the
rest) are set on the table the same way.

`[auth.issuer.introspection]` configures the introspection check of an
issuer under `mode = "introspection"`. FerroPIX authenticates its call with
exactly one of `[auth.issuer.introspection.client]` (the client
authentication of a token request, RFC 7662 §2.1) and
`[auth.issuer.introspection.bearer]` (a token from the client credentials
grant). Each takes the keys of an OAuth 2.0 client: `issuer` or
`token_endpoint`, `client_id`, `client_auth` (`"client_secret_basic"` by
default, `"client_secret_post"`, `"private_key_jwt"`, `"tls_client_auth"`,
`"self_signed_tls_client_auth"`), `client_secret` or `client_secret_file`,
`private_key_file`, `kid`, `assertion_alg`, `scope`, `resource`,
`audience`, `refresh_margin`, `timeout`, `assertion_audience` and the
`tls` table (`trust_roots_file`, `client_identity_file`, `min_version`). `client` stands
in the issuer when it names neither `issuer` nor `token_endpoint`.

| Key | Default | What it sets |
|---|---|---|
| `auth.issuer[].introspection.endpoint` | `""` | The introspection endpoint; empty for the one the issuer's metadata names. |
| `auth.issuer[].introspection.cache_max_age` | `"1min"` | How long an active answer is used before the endpoint is asked again; `"0s"` keeps none. |
| `auth.issuer[].introspection.cache_capacity` | `10000` | The most answers held at once. |

A key of an earlier shape refuses the start as an unknown key, named with
its section: the flat `auth.issuer = "…"`, a section-level issuer key,
`token_profile`, and a `scopeless_subjects` entry written as a string.

### `[trust]`

`[[trust.issuer]]` says, per issuer, which clients its tokens may vouch for
as the Patient Identity Source of a domain (PIXm `ITI-104`). A `[[domain]]`
whose `source_client_id` no entry lists is refused.

| Key | Default | What it sets |
|---|---|---|
| `trust.issuer[].issuer` | none, required | An issuer `[auth]` names; each issuer at most once. |
| `trust.issuer[].clients` | `[]` | The client ids this issuer may vouch for as a Patient Identity Source. |

`[auth.basic]` holds Basic users, for the development profile only, and
only beside the `[dev]` table. A Basic user is a person, a steward or an
administrator on your own machine, and never a Source: an entry granting a
`system/` scope refuses the start, naming the user. A machine, a Source
among them, signs in with a client credentials token of an
`[[auth.issuer]]`. Who needs which scope is on the
[Security and audit](security-and-audit.md#who-may-call-the-server) page.

| Key | Default | What it sets |
|---|---|---|
| `auth.basic.cache_ttl` | `"5min"` | How long a verified credential is remembered; `"0s"` remembers none. |
| `auth.basic.users[].user` | none | The user-id. |
| `auth.basic.users[].password_hash` | none | The Argon2id PHC string of the password. |
| `auth.basic.users[].password_hash_file` | none | The file holding that string. |
| `auth.basic.users[].scopes` | `[]` | The scopes the user is granted, never a `system/` scope: a `system/` scope fails the configuration load, naming the user and the scope. |
| `auth.basic.users[].roles` | `[]` | Refused when set: the permission table reads scopes. |

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

### `[[pseudonym_domain]]`

Each entry is a pseudonymisation domain written to the store at start when
it is absent, never updated ([Pseudonyms](../integrate/pseudonyms.md)). A
namespace named twice, or one an identifier domain holds, is refused.

| Key | Default | What it sets |
|---|---|---|
| `pseudonym_domain[].namespace` | none, required | The system of the pseudonyms: a `urn:oid:` OID in the FHIR `oid` form, or another absolute URI. |
| `pseudonym_domain[].name` | none, required | The name a steward reads. |
| `pseudonym_domain[].consumer_client_id` | none | The client id whose ITI-83 query mints a pseudonym in the domain; without one, only a steward mints. |

### `[matching]`

| Key | Default | What it sets |
|---|---|---|
| `matching.link_at` | `0.95` | The score at or above which a pair grades `certain`. |
| `matching.review_at` | `0.80` | The score at or above which a pair grades `probable`. |
| `matching.possible_at` | `0.60` | The score at or above which a pair grades `possible`. The three hold `0 < possible_at <= review_at <= link_at <= 1`. |
| `matching.auto_link` | `false` | Whether a `certain` pair is linked without a steward. |
| `matching.blocking_keys` | all three | The keys a pair must share one of to be scored: `"birth_year_family_phonetic"`, `"postal_code_birth_date"`, `"shared_identifier"`. |
| `matching.fields.<field>.m` | the field's default in [Matching](matching.md#the-fields-and-their-defaults) | The probability that the field agrees for a true pair. |
| `matching.fields.<field>.u` | the field's default in [Matching](matching.md#the-fields-and-their-defaults) | The probability that the field agrees for a random pair; `0 < u < m < 1`. |

`<field>` is one of `family_name`, `given_names`, `birth_date`, `gender`,
`postal_code`, `address`, `telephone`, `email`, `mothers_maiden_name` and
`local_identifier`.

### `[pixm]`, `[pdqm]` and `[pmir]`

| Key | Default | What it sets |
|---|---|---|
| `pixm.enabled` | `true` | Whether the PIXm Manager serves ITI-83 and ITI-104. |
| `pixm.remove_patient` | `false` | Whether ITI-104 serves the Remove Patient Option. |
| `pdqm.enabled` | `true` | Whether the PDQm Supplier serves ITI-78 and ITI-119. |
| `pdqm.deprecated` | `"inactive"` | How a merged patient is answered by ITI-78: `"inactive"` (with `active = false` and its replaced-by link, the survivor included in a search) or `"omit"` (left out of a search, and `404` on a read). |
| `pmir.enabled` | `false` | Whether the PMIR Registry serves ITI-93 and ITI-94. |
| `pmir.feed.max_attempts` | `10` | The most delivery attempts of one outbound ITI-93 message. |
| `pmir.feed.initial_backoff` | `"1s"` | The wait before the first retry. |
| `pmir.feed.max_backoff` | `"5min"` | The longest wait between two retries. |

### `[hl7v2]`

| Key | Default | What it sets |
|---|---|---|
| `hl7v2.enabled` | `false` | Whether the MLLP listener runs. |
| `hl7v2.listen` | `"0.0.0.0:2575"` | The socket address the listener binds. |
| `hl7v2.default_charset` | `"ASCII"` | The HL7 table 0211 character set of a message with an empty MSH-18: `ASCII`, `UNICODE UTF-8` or an `8859/n` part. |
| `hl7v2.frame_limit` | `"1MiB"` | The largest message one frame may carry. |
| `hl7v2.idle_timeout` | `"5min"` | How long a connection may sit idle between frames. |
| `hl7v2.frame_timeout` | `"30s"` | How long one frame may take to arrive. |
| `hl7v2.senders[].application` | none | The sending application, MSH-3. |
| `hl7v2.senders[].facility` | none | The sending facility, MSH-4. |
| `hl7v2.senders[].domain` | none | The system of the identifier domain the sender feeds; leave it out for a sender that only queries. |
| `hl7v2.namespaces` | none | A table from a domain's system to its HL7 namespace id, the first subcomponent of an assigning authority. |
| `hl7v2.application` | `"FERROPIX"` | The Manager's sending application, MSH-3 of an ITI-10 notification. |
| `hl7v2.facility` | `"FERROPIX"` | The Manager's sending facility, MSH-4 of an ITI-10 notification. |
| `hl7v2.consumers[].name` | none | The name the outbox, the log and the dependency report know the consumer by. |
| `hl7v2.consumers[].address` | none | The `host:port` of the consumer's MLLP listener. |
| `hl7v2.consumers[].application` | none | The receiving application, MSH-5. |
| `hl7v2.consumers[].facility` | none | The receiving facility, MSH-6. |
| `hl7v2.consumers[].domains` | `[]` | The systems of the identifier domains of interest to the consumer; empty for every domain. |
| `hl7v2.consumers[].connect_timeout` | `"5s"` | How long opening the connection may take. |
| `hl7v2.consumers[].reply_timeout` | `"30s"` | How long the acknowledgment may take once connected. |
| `hl7v2.notification.max_attempts` | `10` | The most attempts one ITI-10 notification is given. |
| `hl7v2.notification.initial_backoff` | `"1s"` | The wait before the first retry. |
| `hl7v2.notification.max_backoff` | `"5min"` | The longest wait between two attempts. |

A message from a sender not listed is answered `AR`. A sending application
and facility listed twice, two senders of one domain, and one namespace id
for two domains are refused at load. A consumer without its name, address,
application or facility, two consumers of one name, and an address without
a port are refused too. The consumers are notified while `hl7v2.enabled`
is `true`. The listener finishes the messages in flight within
`server.shutdown_timeout`, the HTTP server's bound. The face needs a
terminology server, `[terminology]` below: with `hl7v2.enabled` and no
`terminology.endpoint.url` the server refuses to start.

### `[terminology]`

The FHIR R4 terminology server the HL7 v2 face translates every v2 table
value on, by `ConceptMap/$translate`: PID-8, the name type of PID-5, the
address type of PID-11 and the telecommunication use and equipment type of
PID-13 and PID-14, on the way in and on the way back into an ITI-21
answer. FerroPIX holds no copy of a table, so load the server with the
value maps the [HL7 v2 face](../integrate/hl7v2-face.md#the-terminology-server)
page names. The server is `[terminology.endpoint]`, the endpoint table of
`ferrohealth-smart` every FerroHEALTH product writes each outbound
integration with, reached through its outbound client.

| Key | Default | What it sets |
|---|---|---|
| `terminology.endpoint.url` | `""` | The R4 base of the terminology server: `https`, or plain `http` on a loopback host, with no userinfo. Empty for none, which only a server with the HL7 v2 face off may run with. |
| `terminology.endpoint.timeout` | `"10s"` | How long one call may take, connection included. |
| `terminology.endpoint.connect_timeout` | none | How long opening the connection may take; unset, only the timeout bounds it. |
| `terminology.endpoint.retry.max_attempts` | `3` | How many times a call is sent at most, the first try included. |
| `terminology.endpoint.retry.initial_backoff` | `"200ms"` | The wait before the second attempt. |
| `terminology.endpoint.retry.max_backoff` | `"2s"` | The longest wait between two attempts. |
| `terminology.endpoint.credentials.user` | `""` | The HTTP Basic user, with `terminology.endpoint.credentials.password_file` (a secret). |
| `terminology.endpoint.credentials.oauth` | none | The OAuth 2.0 client credentials grant instead of Basic: `issuer` or `token_endpoint`, `client_id`, `client_secret_file`, `scope` and the other keys of `ferrohealth-smart`'s `[<lane>.credentials.oauth]` table. |
| `terminology.endpoint.tls.client_identity_file` | none | The certificate chain and private key presented for mutual TLS, in one PEM. |
| `terminology.endpoint.tls.trust_roots_file` | none | PEM certificates trusted for the server's certificate, beside the platform roots. |
| `terminology.endpoint.tls.min_version` | `"1.3"` | The oldest TLS version spoken, `"1.2"` to lower it. |

A feed whose table values the server cannot translate, because it refuses
the call or cannot be reached, is answered `AR` with no receipt, so the
same message is applied when sent again; an ITI-21 query is answered `AR`
the same way. A code the server's map translates to nothing is left out of
the `Patient`, as a code with no FHIR equivalent always was.

### `[xcpd]`

| Key | Default | What it sets |
|---|---|---|
| `xcpd.enabled` | `false` | Whether `POST {base}/xcpd` answers ITI-55 as the XCPD Responding Gateway. |
| `xcpd.home_community_id` | none | The community's `homeCommunityId`, `urn:oid:` and an OID. |
| `xcpd.device` | none | The OID of the gateway's device, the sender of every answer. |
| `xcpd.domains` | none | The `urn:oid:` systems of the domains whose identifiers an answer carries; the first a patient holds is its `Patient.id`. |
| `xcpd.initiating_communities` | `[]` | The `homeCommunityId`s of the initiating communities answered; a query from another is answered with Case 5. |
| `xcpd.reply_endpoints` | `[]` | The URL prefixes an asynchronous request's `ReplyTo` may start with, each `https`, or `http` on a loopback host. |
| `xcpd.delivery_attempts` | `5` | The most attempts to post one asynchronous answer. |
| `xcpd.delivery_initial_backoff` | `"1s"` | The wait before the first retry. |
| `xcpd.delivery_max_backoff` | `"1min"` | The longest wait between two attempts. |
| `xcpd.delivery_timeout` | `"10s"` | How long one attempt may take. |
| `xcpd.tls.client_identity_file` | none | The certificate chain and private key the delivery presents for mutual TLS, in one PEM; or `xcpd.tls.client_identity`, inline. |
| `xcpd.tls.trust_roots_file` | none | PEM certificates trusted for a reply endpoint's server certificate, beside the platform roots. |
| `xcpd.tls.min_version` | `"1.3"` | The oldest TLS version the delivery speaks, `"1.2"` to lower it. |

A value that is no OID, a domain that is no `urn:oid:` system, a domain or
community listed twice, a reply endpoint that is neither `https` nor `http`
on a loopback host or that carries userinfo, a zero delivery bound, one of
the two client identity files without the other, and an identity or trust
file that cannot be read or is empty are refused at load; an identity or
trust file that holds no PEM the delivery reads refuses the start. See
[The XCPD Responding Gateway](../integrate/xcpd-gateway.md).

### `[outbox]`

| Key | Default | What it sets |
|---|---|---|
| `outbox.retention` | `"30d"` | How long a delivered or abandoned outbound message is kept before it is pruned; a pending one is never pruned. |
| `outbox.prune_batch` | `500` | The most messages one prune deletes in one statement. |

The sender prunes once an hour. The server's database role has no `DELETE`
on the outbox: it calls `pix.prune_outbox`, which deletes only what the rule
allows. A zero value is refused at load.

### `[audit]`

The repository is `[audit.endpoint]`, the endpoint table of
`ferrohealth-smart`, reached through its outbound client.

| Key | Default | What it sets |
|---|---|---|
| `audit.endpoint.url` | `""` | The FHIR base of the audit repository the BALP records are sent to; `https`, or plain `http` under development. |
| `audit.endpoint.timeout` | `"5s"` | How long one delivery may take. |
| `audit.endpoint.connect_timeout` | none | How long opening the connection may take; unset, only the timeout bounds it. |
| `audit.endpoint.retry.max_attempts` | `3` | How many times one delivery round sends a record. |
| `audit.endpoint.retry.initial_backoff` | `"1s"` | The wait before the second try of a round. |
| `audit.endpoint.retry.max_backoff` | `"1min"` | The longest wait between two tries, and the wait between two rounds. |
| `audit.endpoint.credentials.user` | `""` | The HTTP Basic user, with `audit.endpoint.credentials.password_file` (a secret). |
| `audit.endpoint.credentials.oauth` | none | The OAuth 2.0 client credentials grant instead of Basic, the keys of `ferrohealth-smart`'s `[<lane>.credentials.oauth]` table. |
| `audit.endpoint.tls.client_identity_file` | none | The certificate chain and private key presented for mutual TLS, in one PEM. |
| `audit.endpoint.tls.trust_roots_file` | none | PEM certificates trusted for the repository's certificate, beside the platform roots. |
| `audit.endpoint.tls.min_version` | `"1.3"` | The oldest TLS version spoken, `"1.2"` to lower it. |
| `audit.spool_dir` | `"/var/lib/ferropix/audit"` | The directory the records wait in until they are delivered. |
| `audit.spool_max_size` | `"64MiB"` | The most the spool holds. |
| `audit.spool_max_events` | `100000` | The most records the spool holds. |
| `audit.spool_write_timeout` | `"2s"` | The longest storing one record may take. |

`serve` opens the spool at start and refuses to start when the directory
is open to its group or to other users, cannot be written, or holds a file
the server did not write. Without `audit.endpoint.url` the records stay in the spool
until you name a repository, and the server warns at start. A request whose
record has no room in the spool is refused with `503`, as the
[Security and audit](security-and-audit.md#the-servers-audit-trail) page
says.

### `[operator]`

| Key | Default | What it sets |
|---|---|---|
| `operator.enabled` | `false` | Whether the operator API is mounted under `{base}/operator`, behind the steward and administrator scopes. |
| `operator.admin_scope` | `"__ferropix.admin"` | The scope of an administrator: a full URI or a string that starts with `__`, other than the steward scope. |

### `[usage_report]`

`ferropix serve` sends an installation report to FerroPULSE, the
FerroHEALTH collector, through `ferrohealth-usage-report`, the reporter
every FerroHEALTH product uses: one report at each start (at most one per ten
minutes) and one a day. It is on by default, and the server writes one line
at every start saying whether it is on, what it carries and how to switch it
off. It stays off when `enabled = false`
(`FERROPIX__USAGE_REPORT__ENABLED=false`), when no `state_dir` is set, when
that directory cannot be written, and when the environment sets
`GITHUB_ACTIONS` or `CI` to `true` or `1`.

A report carries:

- a random instance id, kept in `state_dir/instance-id` so a restart is not
  a new installation;
- the version, the commit, the licence type (`non-commercial`), the
  `deployment` you set, the uptime, and the CPUs and memory as ranges;
- the profile, the services you switched on (`hl7v2`, `operator`, `pdqm`,
  `pixm`, `pmir`), the number of identifier domains as a range (`0`, `1-2`,
  `3-5`, `6-10`, `11-50`, `51+`), and whether the store and the audit spool
  were ready;
- once a day, the request count as a range, latency per route group
  (`metadata`, `pix_query`, `pix_feed`, `pdq_query`, `pdq_match`,
  `pmir_feed`, `pmir_subscription`, `operator`, `hl7v2`, `other`) with the
  server errors of each, and the counts of `4xx` answers and of HL7 v2
  messages answered `AE` or `AR`, each left out while it is zero.

It never carries an identifier, a patient's data, a domain's system or name,
a host name, a URL or a request path: a request is counted by the route
template it matched, and the health and metrics routes are not counted.

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
| `ihe-iti` | `pixm`, `pdqm`, `pmir`, `mcsd`, `xcpd`, `soap`, `rest`, `outbound`, and the audit records `balp` (with a FHIR profile) and `atna` (with `xcpd`) | [ihe-iti](../crates/ihe-iti.md#features) |
| `nl-generic-functions` | `nvi`, `mitz`, `lrza` and `nuts-auth` | [nl-generic-functions](../crates/nl-generic-functions.md#features) |

### Building a client

The settings below hold for every HTTP client of both crates. The API
reference on docs.rs has each constructor and builder method.

| Setting | Where | What to give it |
|---|---|---|
| The endpoint table | the constructor, as the `OutboundClient` of `ferrohealth-smart` built from it | Its `url` is the FHIR base of the Manager, Supplier, Registry or directory, or the SOAP endpoint of Mitz or an XCPD gateway; its userinfo, if any, is shown as `***` in `Debug`. It carries the TLS and the mutual TLS identity where the profile asks for one (XCPD, Mitz), the timeouts, the `retry` table and the credential, and follows no redirect. |
| A timeout per exchange | each call, as a `Duration` | The bound of the whole exchange; with an audit feature it bounds the audit record as well. |
| Access tokens | the table's `credentials` | HTTP Basic, or the client credentials grant whose token each request carries (IUA ITI-72); see [Security and audit](security-and-audit.md#access-tokens). |
| The audit recorder | `.audited(…)`, with `balp` or `atna` | The recorder that routes each record to your audit repository; see [Security and audit](security-and-audit.md#the-crates-audit-trail). |
| The PIXm invocation | `.invoked_by(Invocation::Post)` | `POST`, as every FerroHEALTH client calls a Manager, so the identifier stays out of the URL; leave the `GET` default only for a Manager that refuses the `POST` form ([Lookups use POST](../crates/ihe-iti.md#lookups-by-a-patient-identifier-use-post)). |
| The mCSD budget | `budget::Budget` | The deadline and the caps on pages, bytes and entries of one walk; by default 200 pages, 64 MiB and 50,000 entries. |
