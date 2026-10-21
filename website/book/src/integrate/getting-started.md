<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Getting started

Today you use FerroPIX through its published crates: you call a PIXm
Manager, a PDQm Supplier, a PMIR Registry, an mCSD directory or an XCPD
gateway that someone else runs, from your own Rust code. The FerroPIX server
is planned in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3); its
quickstart joins this page when it lands.

## Add the crate

Each profile is a feature, and no feature is on by default, so you compile
only the profiles you use. The FerroHEALTH products pin family crates by
exact version:

```toml
[dependencies]
ihe-iti = { version = "={{version:ihe-iti}}", default-features = false, features = ["pixm"] }
ihe-atna = { version = "{{dep:ihe-atna}}", default-features = false }
```

`ihe-atna` is the family's audit crate. The clients name whom an exchange
is made for with its `ihe_atna::user::OnBehalfOf`, so you depend on the same
version `ihe-iti` does. The versions above are the ones on `main` when this
book was built; [crates.io](https://crates.io/crates/ihe-iti) lists the
published releases.

The example below also uses `reqwest`, `secrecy`, `url` and `tokio`:

```sh
cargo add reqwest secrecy url
cargo add tokio --features macros,rt-multi-thread
```

## Ask a PIXm Manager for a patient's other identifiers

This is the example of the `ihe_iti::pixm` module documentation, which CI
compiles with every change. It asks a PIX Manager which identifier one other
domain holds for the patient a source identifier names (ITI-83):

```rust,ignore
use std::time::Duration;

use ihe_iti::pixm::PixmClient;
use ihe_iti::pixm::identifier::{CrossReference, SourceIdentifier, TargetSystem};
use ihe_atna::user::OnBehalfOf;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let client = PixmClient::new(Url::parse("https://pix.example.org/fhir/")?, http)?;
    let source = SourceIdentifier::new(
        "urn:oid:1.3.6.1.4.1.21367.13.20.1000",
        SecretString::from("IHERED-994"),
    )?;
    let targets = [TargetSystem::new("urn:oid:1.3.6.1.4.1.21367.13.20.3000")?];
    let on_behalf = OnBehalfOf::System;
    match client
        .cross_reference(&source, &targets, &on_behalf, Duration::from_secs(2))
        .await?
    {
        CrossReference::Matched(found) => {
            for identifier in found.identifiers() {
                let _value = identifier.value().expose_secret();
            }
        }
        CrossReference::SourceNotFound => {}
        _ => {}
    }
    Ok(())
}
```

Three things in it hold for every client of the crate:

- **Build the HTTP client with `redirect::Policy::none()`.** The request
  carries the patient identifier, and a redirect would send it to a host you
  did not choose.
- **Identifiers travel as `secrecy::SecretString`.** The types that carry
  them redact their value in `Debug` and have no `Display`, and no error
  carries a value, a URL or the server's free text, so a log line cannot leak
  one by accident. You read a value with `expose_secret()` where you mean to.
- **An unknown patient is an answer, an outage is an error.**
  `CrossReference::SourceNotFound` is the profile's "patient unknown"; a
  timeout, a refusal or a `404` without a `not-found` issue is a typed
  `PixmError`.

## Next

- [The published crates](../crates/index.md): every feature of `ihe-iti` and
  `nl-generic-functions`, what each client checks, and the API reference on
  docs.rs.
- [Security and audit](../operate/security-and-audit.md): access tokens on
  each request, and the audit record of each exchange.
