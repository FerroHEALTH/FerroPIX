<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Running the console

**Audience:** operators who build, deploy and configure the operator
console. [The operator console](console.md) shows what a data steward does
with it.

## Build and run it

The console is one binary: its server draws every page and serves FerroUI's
stylesheet and script modules itself, so it needs no build step beside
cargo. Build it and serve:

```sh
cargo build --release -p ferropix-console
target/release/ferropix-console serve --config console.toml
```

| Command | What it does |
|---|---|
| `ferropix-console serve` | Serves the console until the process is asked to stop. |
| `ferropix-console config check` | Reads and resolves the configuration, secrets included, prints every key as `key = value  # source` with every secret shown as `***`, and exits. |
| `ferropix-console healthcheck` | Asks `GET /health/readiness` of the console on this host, for a container runtime, and exits 0 only on `200`; a console that is draining exits 1. |

## The image

`ghcr.io/ferrohealth/ferropix-console`, for `linux/amd64` and
`linux/arm64`, carries the static binary on
`gcr.io/distroless/static-debian13:nonroot`, pinned by digest; nothing is
compiled inside it.

| Property | Value |
|---|---|
| Entrypoint | `/usr/local/bin/ferropix-console`, default job `serve` |
| User | `65532:65532` |
| Port | `3000/tcp` (`FERROPIX_CONSOLE__SERVER__LISTEN=0.0.0.0:3000`) |
| Health | `HEALTHCHECK` runs `ferropix-console healthcheck`, which asks `GET /health/readiness` over loopback |

The console keeps its sessions in memory and writes no file, so run it with
a read-only root filesystem. Its tags follow the server's: `<version>`, and
`<major>.<minor>` and `latest` for a release that is not a pre-release,
applied only after the image passed the release's vulnerability gate.
Verify an image against the workflow that built it:

```sh
gh attestation verify oci://ghcr.io/ferrohealth/ferropix-console:<version> \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/release-console.yml
```

The compose quickstart runs it beside the server under the `console`
profile, signed in through the development issuer
([Deployment](deployment.md#the-compose-quickstart)):

```sh
openssl rand -hex 16 > secrets/console-oidc-client-secret
docker compose --profile console up --wait
```

## Configuration

The console reads its configuration through `ferrohealth-config`, the
FerroHEALTH configuration loader. The file is a TOML file named by
`--config` or `FERROPIX_CONSOLE_CONFIG`. Every key can be set from the
environment as `FERROPIX_CONSOLE__<SECTION>__<KEY>`, so
`FERROPIX_CONSOLE__PIX__BASE_URL` sets `[pix] base_url`, and with
`--set key=value` on the command line, which wins over both. A secret can be
read from a file with the `_file` form of its key, the shape a container
runtime mounts a secret in; the file's trailing line ending is removed. An
unknown key, in the file, a variable or a `--set` argument, is refused, and a
refusal names the key, never its value. A server variable such as
`FERROPIX__SERVER__LISTEN` is left alone.

```toml,console
[server]
listen = "0.0.0.0:3000"
locale = "en"

[pix]
base_url = "https://pix.example.org/"
timeout = "30s"

[oidc]
issuer = "https://idp.example.org/realms/ferropix"
client_id = "ferropix-console"
client_secret_file = "/run/secrets/console-client-secret"
redirect_uri = "https://console.example.org/auth/callback"
```

| Section | What it sets |
|---|---|
| `[server]` | Where the console listens and the language a page falls back to. |
| `[pix]` | The FerroPIX server's base URL, how long one call to it may take, and `admin_scope`, the scope the server's `[operator] admin_scope` names (default `__ferropix.admin`). |
| `[session]` | Where the sessions are kept (`store`), whether the cookies are `Secure`, and the sign-in and session timeouts and bounds. `secure_cookie = false` is refused unless the console is plain HTTP on loopback: its `oidc.redirect_uri`, or its `server.listen` address when it signs in nowhere. |
| `[oidc]` | The OpenID Provider stewards sign in at. Without it the console offers no sign-in. |
| `[jose]` | `post_quantum_only = true` accepts an ID Token signed with ML-DSA-44, ML-DSA-65 or ML-DSA-87 (RFC 9964) alone. Off, the default, the console accepts those beside RS256 to PS512, ES256, ES384 and EdDSA. |
| `[telemetry]` | The console log: `format` and `filter`, the section the server reads ([`[telemetry]`](configuration.md#telemetry)). `RUST_LOG` sets `telemetry.filter`. |

`[oidc]` and `[session]` are the sections of `ferrohealth-session`, the
FerroHEALTH console sign-in crate. With `issuer` alone the console discovers
the provider's endpoints when it starts (RFC 8414, OpenID Connect Discovery
1.0); `authorization_endpoint`, `token_endpoint`, `jwks_uri`,
`end_session_endpoint` and `revocation_endpoint` each take the place of the
discovered one. `config check` calls no provider: a section that names the
authorization, token and key-set endpoints is checked whole, and one that
leaves them to discovery is read when the console starts. `resolve`
reaches a provider host at a fixed address (each entry `host=ip:port`) for
a provider whose name resolves only inside a private network. `[oidc.tls]`
holds the provider connection's TLS, read by `ferrohealth-server` as every
other outbound table is: `trust_roots_file` adds PEM trust roots for a
provider behind a private certificate authority, checked for revocation,
`trust_roots_without_revocation_file` those of an authority that publishes
no revocation service, `client_identity_file`
(or `client_identity`) the client certificate chain and key the console
presents (RFC 8705 §2), and `min_version` the lowest TLS version (`"1.3"`,
the default, or `"1.2"`). `timeout` (default `"10s"`) bounds one call to
the provider.

`issuer`, `redirect_uri` and `post_logout_redirect_uri` take no userinfo:
the console refuses one written as `https://user:password@…` and does not
start, and the refusal names the key, never the URL. Any rendering of the
section shows such a URL as `https://***@…`, as it shows the endpoints, so
the password never reaches a terminal or a support report (RFC 3986
§3.2.1).

`[session] store` is `memory` (the default), which keeps the sessions in the
console's process with an opaque id in the cookie, or `sealed`, which keeps
the whole session in one AES-256-GCM sealed cookie for a console that runs
as several instances. A sealed store needs `key` or `key_file` (base64 of at
least 64 bytes, the same on every instance), keeps no refresh token, and
refuses `oidc.offline_access`. Its spans of time are strings of a whole
number and a unit: `sign_in_timeout` (default `"5min"`), `idle_timeout`
(default `"30min"`) and `absolute_timeout` (default `"12h"`), which may not
be shorter than `idle_timeout`.

## Sign-in and the session

Stewards sign in at an OpenID Provider with the authorization code grant and
PKCE, through `ferrohealth-session` and the OpenID Connect client of
`ferrohealth-smart`, the FerroHEALTH authorization crate. The console checks the ID Token against the provider's keys, its
issuer, its audience and the sign-in's nonce before a session begins. The
session lives on the console's server; the browser holds one opaque
`HttpOnly`, `SameSite=Lax` cookie and never a token. The console calls the
FerroPIX server with the steward's own access token, so the server records
the steward as the actor of every call, and renews it before it expires.
The console draws an administrator's forms, such as adding or changing an
identifier domain, only when the provider granted the token `[pix]
admin_scope`: the scopes the token answer names, or the scopes the console
asked for when the answer names none (RFC 6749 §5.1). Ask for the scope in
`[oidc] scopes`. The server checks the scope again on every write.
Sign out ends the session, revokes its token when the provider has a
revocation endpoint (RFC 7009), and, when the provider publishes one, signs
the steward out there too. When the provider cannot renew an access token
in time, the session lives on and the next page you open goes to the
sign-in page that says your access could not be renewed; signing in again,
or opening the page again a moment later, brings you back to it.

A session ends after `[session] idle_timeout` without a request from you,
and after `absolute_timeout` whatever you do. Two minutes before the idle
timeout, every signed-in page shows a warning with the time left,
**Stay signed in** and **Sign out**. Your own typing and clicks keep the session
alive, at most once a minute; the page's own checks never do. When the
session ends, or you sign out in another tab, every open tab of the console
loads the sign-in page that says your session ended, and signing in again
brings you back to the page you were on. The tab you sign out in shows the
sign-in page that says you signed out, also when you sign out right after
bringing it forward. Without the page's scripts there
is no warning: the next page you open goes to the sign-in page.
