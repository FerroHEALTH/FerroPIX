<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# VEX statements

Vulnerability Exploitability eXchange documents, in
[OpenVEX](https://github.com/openvex/spec/blob/main/OPENVEX-SPEC.md) format,
stating whether a finding affects what FerroPIX publishes: the crates the
release uploads to crates.io (`pkg:cargo/ihe-iti`,
`pkg:cargo/nl-generic-functions`), the server image (`pkg:oci/ferropix`)
and the server binary (`pkg:cargo/ferropix-server`).

A VEX document is the machine-readable form of an argument. Each statement
names the vulnerability, the product, a `status`, a `justification` from the
OpenVEX vocabulary, and an `impact_statement` a reader can check. An ignore
list records the decision without the reasoning, which nobody can re-evaluate
later.

## Rules

- **`not_affected` needs a justification from the OpenVEX vocabulary**, and
  the `impact_statement` says concretely why the code is unreachable in
  FerroPIX's use. "Low risk" is not a justification.
- **A finding that can be fixed is fixed.** A statement is for a finding
  whose fix belongs to someone else, such as an unmaintained crate with no
  successor.
- **A scanner reads these files** (`trivy --vex`, or any OpenVEX consumer),
  so a statement that stops being true stops hiding anything: the finding
  returns.

## Documents

| File | Subject | Authored |
|---|---|---|
| `rust-advisories.openvex.json` | The Rust dependency advisories the advisory gate accepts. Each statement carries a `ferropix:reachability` block (our own extension; OpenVEX defines none) naming the crates the affected package is reached through. | **generated** |

No document covers the server image yet, because no scan has found
anything in it to judge. The release's vulnerability record
(`scripts/checks/vex-coverage.sh`) refuses a release while any finding of
its image or binary scan lacks a statement here, and the weekly
`image-scan.yml` applies every document in this directory; a statement for
the image names the product `pkg:oci/ferropix` and is written by hand, in
its own `*.openvex.json`.

## The generated document

`rust-advisories.openvex.json` is produced by
`scripts/security/vex-generate.sh` from two inputs, and is never edited by
hand:

- **`deny.toml`** `[advisories].ignore`: the authoritative set of advisory
  ids. It is the gate that decides whether a build passes, so it is the only
  place the id list lives.
- **`security/vex/rust-advisories.toml`**: the reasoning, the OpenVEX
  `status`, the `justification` and the `impact_statement` for each id.

The generator refuses to emit anything unless the two sets match in both
directions, and `scripts/checks/vex-advisories.sh`, the `vex` CI job,
regenerates the document and fails on any difference. An ignore added to
`deny.toml` without a published justification is a red build.

Agreement is not truth: an ignore and its justification stay in agreement
while a dependency upgrade resolves the advisory underneath both.
`scripts/checks/advisory-exceptions.sh`, in the `cargo-deny` CI job, promotes
cargo-deny's `advisory-not-detected` diagnostic to an error, so an exception
that has outlived its finding fails the build.

A statement can also agree, describe an advisory that still fires, and argue
from a dependency path that is not the real one. So each statement carries
its path as data (`carriers`, `workspace_roots`), and
`scripts/checks/vex-reachability.sh`, also in the `cargo-deny` job, compares
both sets with the graph cargo resolves, in both directions.

To change a statement: edit `rust-advisories.toml` (and `deny.toml` if the id
set changes), bump the document's `version` and `timestamp`, then run
`scripts/security/vex-generate.sh`. `carriers` and `workspace_roots` come from
`cargo tree -i <crate>[@<version>] --workspace --all-features --target all -e
normal,build,dev`. The scripts need `jq`, and the generator `yq` too.
