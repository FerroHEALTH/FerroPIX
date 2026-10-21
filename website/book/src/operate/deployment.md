<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Deployment

FerroPIX publishes no binary and no container image yet. The server, on
PostgreSQL, its container image pinned by digest and a compose quickstart
that runs it beside PostgreSQL are planned in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).
A second image, the operator console, and a Helm chart are planned for the
milestone after it.

What deploys today is the two crates, inside your own application: they
build with the Rust toolchain your application uses, at or above the
minimum supported Rust version in
[Conformance and specification pins](../evaluate/conformance.md#the-toolchain),
and need nothing at run time beyond the servers they call.

## What the server is planned to run beside

As the milestone plans it, a FerroPIX deployment sits beside:

- **the clinical systems that ask it,** FerroEHR among them, which is
  planned to resolve its `ehr_id` through PIXm and PDQm
  ([FerroHEALTH/FerroEHR#3380](https://github.com/FerroHEALTH/FerroEHR/issues/3380));
- **a PostgreSQL 18 database** that holds the cross-reference;
- **an authorization server** whose access tokens it reads;
- **an ATNA audit repository** that receives its BALP records;
- **the operator console**, its own image, which reads the server's
  operator API with a signed-in steward's token.

This page gains the image, its ports, its verification and its
configuration when the server lands.
