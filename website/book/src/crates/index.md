<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The published crates

FerroPIX publishes the libraries of the identity specifications it owns, so
every FerroHEALTH product, and anyone else, can call an IHE actor or a Dutch
Generic Function from Rust. Each crate carries the name of the
specification it implements and holds one feature per profile or function,
with no feature on by default. The FerroPIX server is built on both.

| Crate | What it implements | Version on `main` | API reference |
|---|---|---|---|
| [`ihe-iti`](ihe-iti.md) | The IHE ITI identity profiles: the PIXm, PDQm, PMIR, mCSD and XCPD clients, the server sides of PIXm, PDQm and PMIR, and each profile's audit record | {{version:ihe-iti}} | [docs.rs/ihe-iti](https://docs.rs/ihe-iti) |
| [`nl-generic-functions`](nl-generic-functions.md) | The Netherlands Generic Functions: NVI localization, Mitz consent, LRZa addressing and Nuts authentication | {{version:nl-generic-functions}} | [docs.rs/nl-generic-functions](https://docs.rs/nl-generic-functions) |

Neither crate depends on an application: they are the specifications'
transactions as Rust, for any caller.

## Add a crate

Name the features you call. The FerroHEALTH products pin family crates by
exact version:

```toml,cargo
[dependencies]
ihe-iti = { version = "={{version:ihe-iti}}", default-features = false, features = ["pixm"] }
ihe-atna = { version = "{{dep:ihe-atna}}", default-features = false }
ferrohealth-smart = { version = "{{dep:ferrohealth-smart}}", default-features = false }
```

`ihe-atna` is the family's audit crate: a client names whom an exchange is
made for with its `ihe_atna::user::OnBehalfOf`, so you depend on the version
`ihe-iti` does. Every HTTP client is built from the `OutboundClient` of
`ferrohealth-smart`, which an endpoint table you configure builds: its URL,
TLS, timeouts, retries and credential
([Configuration](../operate/configuration.md#building-a-client)). The
versions above are the ones on `main` when this book was built;
[crates.io](https://crates.io/crates/ihe-iti) lists the published releases.

## A first call

The documentation of each module opens with a complete example that CI
compiles with every change. The one of `ihe_iti::pixm`
([its source](https://github.com/FerroHEALTH/FerroPIX/blob/main/crates/ihe-iti/src/pixm/mod.rs))
builds a `PixmClient` from an endpoint table and asks a PIX Manager which
identifier another domain holds for a patient (ITI-83). Three things in it
hold for every client of both crates:

- **Identifiers travel as `secrecy::SecretString`.** The types that carry
  them redact their value in `Debug` and have no `Display`, and no error
  carries a value, a URL or the other server's free text, so a log line
  cannot leak one by accident. You read a value with `expose_secret()`
  where you mean to.
- **An unknown patient is an answer, an outage is an error.**
  `CrossReference::SourceNotFound` is the profile's "patient unknown" (PIXm
  3.1.0 §2:3.83.4.2.2.2); a timeout, a refusal or a `404` without a
  `not-found` issue is a typed `PixmError`.
- **No redirect is followed.** The request carries the patient identifier,
  and a redirect would send it to a host you did not choose.

Call a Manager by `POST` (`.invoked_by(Invocation::Post)`), as every
FerroHEALTH client does, so the identifier stays out of the URL
([Lookups use POST](ihe-iti.md#lookups-by-a-patient-identifier-use-post)).

## Versions

Each crate keeps its own version line, apart from the FerroPIX product
version ({{version:ferropix}} on `main`). A change to what a crate ships
moves that crate's version in the same pull request, and a version is never
reused. Not every version is published, so the published sequence has gaps;
crates.io lists what is published:
[ihe-iti](https://crates.io/crates/ihe-iti/versions),
[nl-generic-functions](https://crates.io/crates/nl-generic-functions/versions).
The changes of each version are in the [changelog](../changelog.md).

The crates are published from this repository through crates.io Trusted
Publishing, so a release carries no long-lived token, and each release
attaches every `.crate` file with its checksum and Sigstore bundle.
