<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Deployment

FerroPIX publishes no binary and no container image yet. The server and a
published container image pinned by digest are planned in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).
There is no compose file and no Helm chart, and none is planned in that
milestone.

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
- **an authorization server** whose access tokens it reads;
- **an ATNA audit repository** that receives its BALP records.

This page gains the image, its ports, its verification and its
configuration when the server lands.
