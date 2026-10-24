<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The operator console

The FerroPIX Console is the data steward's tool: the review queue, search,
the patient record, the registry of sources and identifier domains, the
configuration and the audit trail. It is its own program, `ferropix-console`,
a client of the FerroPIX server that keeps no patient data of its own. It is
drawn with FerroUI, the component library every FerroHEALTH console shares,
in the FerroPIX brand ({{dep:ferrohealth-ui}}).

The console is built and runs from source today. It reads the FerroPIX
server's [operator API](operator-api.md), whose documents it compiles
against; the pages that draw them are planned in the
[v0.0.4 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/4).
Until a page draws its document, it says that it cannot be shown: the
server refused the connection, did not answer in time, refused the console,
or answers but serves no operator API yet. A page never shows a row the
server did not send. The console ships as its own container image,
published by the release lane from the first tagged release
([The image](#the-image)).

## Build and run it

The console is a Leptos app the server renders whole; the browser loads a
small WebAssembly bundle for the interactive parts, and every page works
without it. Build the site bundle and the binary with
[cargo-leptos](https://github.com/leptos-rs/cargo-leptos), then serve:

```sh
cd app/ferropix-console
cargo leptos build --release --split
ferropix-console serve --config console.toml
```

| Command | What it does |
|---|---|
| `ferropix-console serve` | Serves the console until the process is asked to stop. |
| `ferropix-console config check` | Reads and resolves the configuration, secrets included, and exits. |
| `ferropix-console healthcheck` | Asks the console on this host whether it answers, for a container runtime. |

## The image

`ghcr.io/ferrohealth/ferropix-console`, for `linux/amd64` and
`linux/arm64`, carries the static binary and the site bundle on
`gcr.io/distroless/static-debian13:nonroot`, pinned by digest; nothing is
compiled inside it.

| Property | Value |
|---|---|
| Entrypoint | `/usr/local/bin/ferropix-console`, default job `serve` |
| User | `65532:65532` |
| Port | `3000/tcp` (`FERROPIX_CONSOLE__SERVER__LISTEN=0.0.0.0:3000`) |
| Site bundle | `/site` (`FERROPIX_CONSOLE__SERVER__SITE_ROOT`), root-owned |
| Health | `HEALTHCHECK` runs `ferropix-console healthcheck`, which asks `GET /health` over loopback |

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

The configuration is a TOML file named by `--config` or
`FERROPIX_CONSOLE_CONFIG`. Every key can be set from the environment as
`FERROPIX_CONSOLE__<SECTION>__<KEY>`, so `FERROPIX_CONSOLE__PIX__BASE_URL`
sets `[pix] base_url`. A secret can be read from a file with the `_file`
form of its key, the shape a container runtime mounts a secret in. An unknown
key is refused, and a refusal names the key, never its value.

```toml
[server]
listen = "0.0.0.0:3000"
site_root = "/site"
locale = "en"

[pix]
base_url = "https://pix.example.org/"
timeout_ms = 30000

[oidc]
issuer = "https://idp.example.org/realms/ferropix"
authorization_endpoint = "https://idp.example.org/realms/ferropix/protocol/openid-connect/auth"
token_endpoint = "https://idp.example.org/realms/ferropix/protocol/openid-connect/token"
jwks_uri = "https://idp.example.org/realms/ferropix/protocol/openid-connect/certs"
client_id = "ferropix-console"
client_secret_file = "/run/secrets/console-client-secret"
redirect_uri = "https://console.example.org/auth/callback"
```

| Section | What it sets |
|---|---|
| `[server]` | Where the console listens, where its site bundle is, and the language a page falls back to. |
| `[pix]` | The FerroPIX server's base URL and how long one call to it may take. |
| `[session]` | Whether the cookies are `Secure` (only off on loopback), and the sign-in and session timeouts and bounds. |
| `[oidc]` | The OpenID Provider stewards sign in at. Without it the console offers no sign-in. |

## Sign-in and the session

Stewards sign in at an OpenID Provider with the authorization code grant and
PKCE. The console checks the ID Token against the provider's keys, its
issuer, its audience and the sign-in's nonce before a session begins. The
session lives on the console's server; the browser holds one opaque
`HttpOnly`, `SameSite=Lax` cookie and never a token. The console calls the
FerroPIX server with the steward's own access token, so the server records
the steward as the actor of every call, and renews it before it expires.
Sign out ends the session and, when the provider publishes one, signs the
steward out there too.

## What the console never does

- It puts no identifier other than the person id FerroPIX minted in an
  address: a search posts what was typed, and the identifier field is masked
  as it is typed. The eye inside the field shows the identifier only while
  you hold it (pointer, touch, Space or Enter); without the page's
  WebAssembly the eye is disabled and the field stays masked.
- It keeps nothing in browser storage, and every page is `Cache-Control:
  no-store`.
- It runs no script but its own WebAssembly bundle: every response carries a
  Content-Security-Policy whose scripts run by a nonce minted for that
  response.

## Your preferences

The theme, the density, reduced motion, the language, the first day of the
week, the shortcut hints, how long a revealed identifier stays and whether
leaving a page hides it are each a cookie of their own, set in Your
preferences behind your name and read when each page is drawn. They are only
for you, in this browser, and name no patient.

A language cookie naming `qps` draws the console in its pseudo-locale:
every word accented and two fifths longer, in brackets. It ships no
catalog, and Your preferences never offers it; it is how the console's
browser journeys find a word that skipped the catalog and a layout with no
room for a longer translation.
