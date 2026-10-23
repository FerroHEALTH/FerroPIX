<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ferropix-console

The FerroPIX Console, the data steward's operator console: the
`ferropix-console` binary, a Leptos app rendered on the server in islands
mode, with a backend-for-frontend in front of the FerroPIX server. It draws
every page with FerroUI (`ferrohealth-ui`) in the FerroPIX brand. This crate
is never published.

The console keeps no patient data. Stewards sign in at an OpenID Provider
(authorization code with PKCE); the session lives on the console's server
behind one opaque cookie, and the console calls the FerroPIX server with the
steward's own token. Until the server's operator API exists (#90), each page
that reads it draws its refused state.

Build the site bundle and the binary with cargo-leptos, from this directory:

```sh
cargo leptos build --release --split
```

The operator documentation, the subcommands and the configuration are the
book's [operator console page](https://ferropix.eu/docs/operate/console.html).
