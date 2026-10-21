<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The published crates

FerroPIX publishes the libraries of the identity specifications it owns, so
every FerroHEALTH product, and anyone else, can use them. Each crate carries
the name of the specification it implements and holds one feature per
profile or function, with no feature on by default.

| Crate | What it implements | Version on `main` | API reference |
|---|---|---|---|
| [`ihe-iti`](ihe-iti.md) | The IHE ITI identity profiles: PIXm, PDQm, PMIR, mCSD and XCPD, with the IUA authorizer and each profile's audit record | {{version:ihe-iti}} | [docs.rs/ihe-iti](https://docs.rs/ihe-iti) |
| [`nl-generic-functions`](nl-generic-functions.md) | The Netherlands Generic Functions: NVI localization, Mitz consent, LRZa addressing and Nuts authentication | {{version:nl-generic-functions}} | [docs.rs/nl-generic-functions](https://docs.rs/nl-generic-functions) |

Both crates depend on no application: they are the specifications'
transactions as Rust, for any caller. The FerroPIX server, when it lands,
adds the server sides of the profiles as features of `ihe-iti`.

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
Publishing, so a release carries no long-lived token.
