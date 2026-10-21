<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Operations

The server's health checks, metrics, logs and upgrade path are planned with
the server in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).
This page covers what operating the crates involves today.

## Releases and support

- **Only the newest release is supported.** When a release is published,
  every earlier one stops receiving fixes: there are no maintenance
  branches and no backports. Security fixes keep being published for at
  least five years after each release, in the newest release
  ([`SECURITY.md`](https://github.com/FerroHEALTH/FerroPIX/blob/main/SECURITY.md)).
- **A FerroPIX release is cut from its milestone.** The release workflow
  refuses a tag whose milestone still holds an open issue, and closes the
  milestone once every leg has published. The
  [milestones](https://github.com/FerroHEALTH/FerroPIX/milestones) are the
  roadmap.
- **The crates are published between releases.** A crate's version moves
  with any change to what it ships, so a fix reaches crates.io without
  waiting for a product release.

## Upgrading a crate

Read the crate's entries in the [changelog](../changelog.md) for the versions
you skip. A change that breaks the API is marked **API break** with what to
change. The family pins its crates by exact version, so `ihe-iti` moves
together with the `ihe-atna` version it names.

## Reporting a defect

[Open an issue](https://github.com/FerroHEALTH/FerroPIX/issues/new/choose)
when a transaction answers differently from what its specification names, or
identifying data reaches a log line. Never paste patient data or a real
identifier. The family's
[post-market procedure](https://ferrohealth.eu/docs/post-market.html) says
how complaints and incidents are handled.
