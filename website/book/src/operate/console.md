<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The operator console

The FerroPIX Console is the data steward's tool: the review queue, search,
the patient record, the registry of sources and identifier domains, the
configuration and the audit trail. It is its own program, `ferropix-console`,
a client of the FerroPIX server that keeps no patient data of its own. It is
drawn with FerroUI, the component library every FerroHEALTH console shares,
in the FerroPIX brand ({{dep:ferrohealth-ui}}).

The console reads the FerroPIX server's [operator API](operator-api.md),
whose documents it compiles against, once per page before it draws it, with
your own access token. A page never shows a row the server did not send:
when the server refused the connection, did not answer in time or refused
the console, the page says so in place, with the server's issue, and one
refused part never blanks a page.

The browser draws every page after the first itself, so a link, a search, a
filter or a reveal keeps the page you are on and loads no new document; with
no WebAssembly each is a plain link or form post that draws the page again.

## Searching from the top bar

As you type in the top bar's field, its panel offers the console's pages,
the identifier domains and your preferences whose names hold the words. It
never offers a person: a patient search runs only when you press Enter or
choose the panel's last row, Search patients for "…", and every patient
search is a posted, audited query. The words travel in the request's body,
never in an address.

## Keyboard shortcuts

`?` opens the list of shortcuts on every screen, and so does Keyboard
shortcuts in your menu. `/` puts the focus in the search, `j` and `k` (or
the arrow keys) walk a list and Enter opens the row, and `1` to `4` open the
patient record's Identifiers, Records, History and Pseudonyms tabs. Shortcuts
never act while you type in a field, and a phone shows none.

## The pages

The [steward's guide](steward-guide.md) walks the review work in the order
you do it.

| Page | What it shows |
|---|---|
| Dashboard, `/` | The open queue with the pairs waiting more than seven days, the share of records linked automatically over 24 hours, the versions fed today and the subscriptions in error; the queue's depth and the feeds per hour by channel as charts, each with its figures table; the HTTP and MLLP listeners and the feed sender; the oldest pairs in the queue. |
| Review queue, `/queue` | The pairs the scorer found worth a look, the grade first, then the score, then the longest wait: both leading names, their birth dates and domains, the score with its bar, the grade and the time queued. `j` and `k` move the focused row and Enter opens the pair; `o` or `d` opens it at a glance in a side panel, with Open to decide and Defer (a date and your reason). The filters (grade, domain system, days waiting, the deferred pairs) and the paging are posted, so none of them reaches an address. |
| Compare and decide, `/queue/{candidate}` | The two records side by side, the rows that differ first; the score breakdown with each field's weight, the total and the thresholds on one scale; the blocking key and any hard conflict; each record's patient with its identifiers, masked. The docked bar decides with your reason: Link (`l`), Not a match (`n`) or Defer (`d`) with the date the pair comes back; `r` puts the focus in the reason, and `[` and `]` lead to the neighbouring pairs. A link that merges two patients asks first in a dialog that names the effect. The decision carries the versions the page showed, so a pair someone decided first comes back as stale, with the next pair and a reload as the ways on, and nothing is sent without a reason. |
| Search, `/search` | By identifier (the domain chosen from the server's domains, the value masked as you type) and by demographics, each posted, so nothing you type reaches an address. The results open each patient; a demographics search adds the grade and score. The top bar's field reads what you typed: a date or a name searches by demographics with the field filled, anything else is an identifier searched in every domain. |
| Patient, `/patients/{person}` | The leading demographics, then four tabs: the identifiers per domain, masked, each with its reveal; the source records side by side, each field marked against the leading demographics; the history on a timeline; the pseudonyms, masked, with a link to Resolve a pseudonym. A reveal asks your reason, is recorded by the server as a privacy disclosure, and shows the value for the seconds Your preferences set. |
| Merge, `/patients/{person}/merge` | Merge into… on the patient's page: name the other patient by its id, then the preview shows both patients with the older surviving (one press swaps them), the identifiers that move, the pseudonyms that then resolve to the survivor, the subscriptions told and the candidates closed. Merge… asks in a dialog that names how many identifiers move and takes your reason; nothing is posted before it confirms. A patient that changed after the preview comes back as stale, with the preview read again. |
| Unmerge, `/patients/{person}/unmerge` | Unmerge… on the patient's page, while a merge into it stands: the merge with its reason, what the revert moves back, and Unmerge…, which asks with your reason. When a later merge of the survivor has to be reverted first, the page names it and offers no revert. |
| Sources, `/sources` | Every system allowed to feed, its channel, how it is admitted, the domains that name it and its last activity, and one source's last messages. Sources are set in the server's configuration file. |
| Identifier domains, `/domains` | Every domain with its system, kind, Source, record count and last feed, and one domain with its last feeds. An administrator adds a domain, renames one, gives it another Source or retires it, each with a reason; the server's refusal shows in place. |
| Pseudonymisation domains, `/pseudonym-domains` | Every pseudonymisation domain with its namespace, its consumer, how many pseudonyms it holds and its state, and Resolve a pseudonym: the domain, the pseudonym and your reason, posted, answered with the patient it belongs to and recorded by the server as a disclosure. The pseudonym never reaches an address and the answer never shows it. An administrator adds a domain, renames one, names another consumer or retires it, each with a reason. |
| Configuration, `/configuration` | The matching thresholds, field weights, blocking keys and the built-in deterministic rules, read-only, each value marked when the environment set it or nothing did. |
| Audit trail, `/audit` | The steward actions, newest first, under posted filters (actor, kind, person, dates), each row with its reason, its references and its request id, and the export of the filtered rows as CSV, itself an audited action. |

The console ships as its own container image,
published by the release lane from the first tagged release
([The image](#the-image)).

## What each page looks like

Every page below is drawn from synthetic documents, at 1440 pixels wide in
the light theme, with a link to the dark one.

![The sign-in page a visitor reads](img/console/sign-in.png)

The sign-in page a visitor reads. [Dark theme](img/console/sign-in-dark.png).

![The sign-in page after you signed out](img/console/signed-out.png)

The sign-in page after you signed out. [Dark theme](img/console/signed-out-dark.png).

![The sign-in page after your session expired](img/console/expired.png)

The sign-in page after your session expired. [Dark theme](img/console/expired-dark.png).

![The dashboard](img/console/dashboard.png)

The dashboard. [Dark theme](img/console/dashboard-dark.png).

![The review queue](img/console/queue.png)

The review queue. [Dark theme](img/console/queue-dark.png).

![A pair to compare and decide](img/console/queue-pair.png)

A pair to compare and decide. [Dark theme](img/console/queue-pair-dark.png).

![Search by identifier](img/console/search.png)

Search by identifier. [Dark theme](img/console/search-dark.png).

![Search by demographics](img/console/search-demographics.png)

Search by demographics. [Dark theme](img/console/search-demographics-dark.png).

![A patient's record on its identifiers tab](img/console/patient.png)

A patient's record on its identifiers tab. [Dark theme](img/console/patient-dark.png).

![Merge, naming the other patient](img/console/patient-merge.png)

Merge, naming the other patient. [Dark theme](img/console/patient-merge-dark.png).

![Unmerge, with the preview of the revert](img/console/patient-unmerge.png)

Unmerge, with the preview of the revert. [Dark theme](img/console/patient-unmerge-dark.png).

![The sources](img/console/sources.png)

The sources. [Dark theme](img/console/sources-dark.png).

![One source with its last messages](img/console/source.png)

One source with its last messages. [Dark theme](img/console/source-dark.png).

![The identifier domains](img/console/domains.png)

The identifier domains. [Dark theme](img/console/domains-dark.png).

![One identifier domain with its last feeds](img/console/domain.png)

One identifier domain with its last feeds. [Dark theme](img/console/domain-dark.png).

![The pseudonymisation domains and Resolve a pseudonym](img/console/pseudonym-domains.png)

The pseudonymisation domains and Resolve a pseudonym. [Dark theme](img/console/pseudonym-domains-dark.png).

![The matching configuration](img/console/configuration.png)

The matching configuration. [Dark theme](img/console/configuration-dark.png).

![The audit trail](img/console/audit.png)

The audit trail. [Dark theme](img/console/audit-dark.png).

![About](img/console/about.png)

About. [Dark theme](img/console/about-dark.png).

![Your preferences](img/console/preferences.png)

Your preferences. [Dark theme](img/console/preferences-dark.png).

![The language preference](img/console/preferences-language.png)

The language preference. [Dark theme](img/console/preferences-language-dark.png).

![The keyboard preference](img/console/preferences-keyboard.png)

The keyboard preference. [Dark theme](img/console/preferences-keyboard-dark.png).

![The page an unknown address draws](img/console/not-found.png)

The page an unknown address draws. [Dark theme](img/console/not-found-dark.png).

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
| `ferropix-console config check` | Reads and resolves the configuration, secrets included, prints every key as `key = value  # source` with every secret shown as `***`, and exits. |
| `ferropix-console healthcheck` | Asks `GET /health/readiness` of the console on this host, for a container runtime, and exits 0 only on `200`; a console that is draining exits 1. |

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
| `[pix]` | The FerroPIX server's base URL, how long one call to it may take, and `admin_scope`, the scope the server's `[operator] admin_scope` names (default `__ferropix.admin`). |
| `[session]` | Whether the cookies are `Secure`, and the sign-in and session timeouts and bounds. `secure_cookie = false` is refused unless the console is plain HTTP on loopback: its `oidc.redirect_uri`, or its `server.listen` address when it signs in nowhere. |
| `[oidc]` | The OpenID Provider stewards sign in at. Without it the console offers no sign-in. |
| `[telemetry]` | The console log: `format` and `filter`, the section the server reads ([`[telemetry]`](configuration.md#telemetry)). `RUST_LOG` sets `telemetry.filter`. |

## Sign-in and the session

Stewards sign in at an OpenID Provider with the authorization code grant and
PKCE, through the OpenID Connect client of `ferrohealth-smart`, the
FerroHEALTH authorization crate. The console checks the ID Token against the provider's keys, its
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
Sign out ends the session and, when the provider publishes one, signs the
steward out there too.

A session ends after `[session] idle_timeout_s` without a request from you,
and after `absolute_timeout_s` whatever you do. Two minutes before the idle
timeout, every signed-in page shows a warning with the time left,
**Stay signed in** and **Sign out**. Your own typing and clicks keep the session
alive, at most once a minute; the page's own checks never do. When the
session ends, or you sign out in another tab, every open tab of the console
loads the sign-in page that says your session ended, and signing in again
brings you back to the page you were on. Without the page's WebAssembly
there is no warning: the next page you open goes to the sign-in page.

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
