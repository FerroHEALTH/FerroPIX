<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ferropix-console

The FerroPIX Console, the data steward's operator console: the
`ferropix-console` binary, which draws every page on the server with FerroUI
(`ferrohealth-ui`) in the FerroPIX brand and serves FerroUI's stylesheet and
script modules itself, with a backend-for-frontend in front of the FerroPIX
server. This crate is never published.

The console keeps no patient data. Stewards sign in at an OpenID Provider
(authorization code with PKCE); the session lives on the console's server
behind one opaque cookie, and the console calls the FerroPIX server with the
steward's own token. A page whose operator API document the server does not
send draws its refused state.

Build the binary with cargo, from the repository root:

```sh
cargo build --release -p ferropix-console
```

The subcommands, the image and the configuration are the book's
[Running the console](https://ferropix.eu/docs/operate/console-deployment.html)
page, and what a steward does with it is the
[operator console page](https://ferropix.eu/docs/operate/console.html).
