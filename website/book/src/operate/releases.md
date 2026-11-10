<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Releases, upgrades and support

This page says what each release publishes, how you move a server or a
crate to a newer one, and which releases receive fixes.

## What a release publishes

A FerroPIX release is cut from its milestone: the release workflow refuses
a tag whose [milestone](https://github.com/FerroHEALTH/FerroPIX/milestones)
still holds an open issue, and closes the milestone once every part has
published. Each release publishes, from one tag:

| Artifact | Where |
|---|---|
| The server image | `ghcr.io/ferrohealth/ferropix:<version>`, for `linux/amd64` and `linux/arm64` |
| The console image | `ghcr.io/ferrohealth/ferropix-console:<version>`, the same platforms |
| The Helm chart | `oci://ghcr.io/ferrohealth/charts/ferropix`, its app version the release, both images pinned by digest |
| The static binaries | `ferropix` and `ferropix-console` for `x86_64` and `aarch64` Linux (musl), on the [GitHub release](https://github.com/FerroHEALTH/FerroPIX/releases), each with its checksum, SBOMs and Sigstore bundles |
| The crates whose version moved | `ihe-iti` and `nl-generic-functions` on crates.io, and each `.crate` with its checksum on the GitHub release |
| The vulnerability record | every finding of a scan of the images and binaries, at every severity, with the OpenVEX statement that judges it |

Every artifact carries SLSA Build Level 3 provenance; verify one before you
run it ([Verifying an image](deployment.md#verifying-an-image)). The
release notes are the version's section of the [changelog](../changelog.md).

The product version and the crate versions are two lines. The product
version is the release tag. Each crate keeps its own version, moved by the
change that alters what it ships, so a crate fix reaches crates.io without
waiting for a product release ([The published crates](../crates/index.md#versions)).

## Upgrading the server

1. Read the changelog's sections for every version you skip. A change that
   asks something of you (a configuration key renamed or removed, a
   behaviour that changed) says what to change.
2. Run `ferropix config check` of the new version over your configuration
   ([Configuration](configuration.md#commands)). A key a release renamed or
   removed is refused by name in that one release;
   from the next release on it is refused as an unknown key. So upgrade one
   release at a time when you skip several, or read each section's Removed
   entries.
3. Apply the migrations. `serve` applies the pending migrations at start
   unless `db.migrate = false`; with it, run `ferropix db migrate` of the
   new version first, as the role that may create in the schema
   ([The store](configuration.md#the-store)). The migrations only move
   forward.
4. Start the new version. With the chart, `helm upgrade` to the chart of
   the release; with compose, set `FERROPIX_VERSION` or check out the
   release's `compose.yaml`.

The sealing key and the audit spool carry across an upgrade untouched;
[rotating the key](configuration.md#the-store) is its own step.

## Upgrading a crate

Read the crate's entries in the [changelog](../changelog.md) for the
versions you skip. A change that breaks the API is marked **API break** with
what to change. The family pins its crates by exact version, so `ihe-iti`
moves together with the `ihe-atna` and `ferrohealth-smart` versions it
names.

## Support

- **Only the newest release is supported.** When a release is published,
  every earlier one stops receiving fixes: there are no maintenance
  branches and no backports.
- **Security fixes keep being published for at least five years** after
  each release, in the newest release, as
  [`SECURITY.md`](https://github.com/FerroHEALTH/FerroPIX/blob/main/SECURITY.md)
  says with the Cyber Resilience Act articles behind it.
- **A vulnerability is reported privately** through GitHub's
  [private vulnerability reporting](https://github.com/FerroHEALTH/FerroPIX/security/advisories/new),
  never in a public issue ([Security and audit](security-and-audit.md#reporting-a-vulnerability)).
- **Any other defect is an issue** on the
  [tracker](https://github.com/FerroHEALTH/FerroPIX/issues/new/choose)
  ([Operations](operations.md#reporting-a-defect)).
