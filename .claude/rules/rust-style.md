<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Rust style: idiomatic Rust

Applies to every hand-written `.rs` file in the repository. **FerroPIX is
modern idiomatic Rust of our own design**, held to the specifications it
implements: the IHE ITI Technical Framework, PIXm, PDQm and PMIR, HL7 FHIR R4
and HL7 v2 (`spec-adherence.md`). FerroFED's identity code is prior art: read
it for how a problem was solved, never port it blindly.

## Build idiomatic, compiling, tested code

- **Use proper crates; do not hand-roll** what the pinned set or the family
  provides: the FHIR model (`fhir-types`), the HL7 v2 tables and parsing
  (FerroBRIDGE's `hl7v2-types` and its MLLP codec and parser, never a second
  v2 parser), the authorization (`ferrohealth-smart`), the audit senders
  (`ihe-atna`). Add a dependency through
  the root `Cargo.toml [workspace.dependencies]` table (`dep.workspace =
  true`), and verify its version against crates.io or docs.rs the day it is
  added, never from memory. A dependency of a published crate reaches every
  product that pins it, so it is justified against what it brings.
- **Every crate you touch compiles, is clippy-clean with and without
  `--all-features`, and is tested before you move on.**
- **Read the governing specification first** for any transaction, query,
  linking rule, status or acknowledgment (`spec-adherence.md`).

## File and module size

A hand-written Rust file is at most 1000 lines, and a file over 750 lines is
split into a module folder before it grows further. The shape of a split is
a folder `<name>/mod.rs` holding the module doc, the `mod` declarations and
the items every child shares, with one child file per concern. A split never
creates a test file under `src/`, adds no `pub use`, and moves code without
changing behaviour.

Enforced by `scripts/checks/file-length.sh` (CI guard tier): a hard breach
fails, and a breach listed in `scripts/checks/file-length-allow.txt` passes
only while it does not grow. The list only tightens: a listed file that
shrank, fell to 750 lines or under, or is gone passes with a one-line note,
your pull request leaves the list alone, and the `Ratchet tighten` workflow
lowers or removes the entry on `main` (`file-length.sh --update`, which
never raises or adds an entry; `--record --issue N` adds a new breach with
the issue that splits it, #392).

## Code is written once

No specification governs this: our own design (#392, the rule VernumBOEK,
FerroEHR and FerroBRIDGE hold under the same name).

- **Share knowledge and decisions, never text that only looks alike.** Two
  copies that change for the same reason (one business rule, one wire rule,
  one security rule) move to one owner in the same change. Two pieces with
  different reasons to change stay apart, however alike they read.
- **The owner is the module that owns the concern.** Across crates it is the
  crate lower in the layering (`ihe-iti` and `nl-generic-functions` below
  `ferropix-server`, `ferropix-operator-api` for what the server and the
  console both read). Within a crate it is the module whose part of the
  domain the code serves. It is never a `util`, `utils`, `helpers` or
  `common` module, whose name says nothing about what it holds, and never a
  function that takes a flag to choose between two behaviours.
- **One copy is fine, two are looked at, three are extracted.**
- **The ratchet measures it.** `scripts/checks/duplication.sh` runs jscpd
  (pinned by version and image digest in `docs/VERSIONS.md`, the
  duplication guard) over `app/*/src/`, `crates/*/src/` and `tools/*/src/`
  in the `duplication` CI job and holds the total of duplicated lines to the
  count in `scripts/checks/duplication.count`. A rise fails and names both
  places of each new block. A fall passes with a one-line note, and your
  pull request leaves the count alone: the `Ratchet tighten` workflow runs
  `--update` on `main` each day and opens one pull request with the lower
  count and `scripts/checks/duplication-baseline.txt`, the guard's own key
  of every counted block (its two paths and the sha256 of the lines at
  each, with no line number). `--update` never raises the count; a rise
  that has to stay is a hand edit of the file in a change that says why.
  Locally the guard takes a `jscpd` on `PATH` of the pinned version, then
  the pinned image through docker or podman, then the release binary
  `scripts/checks/duplication.sh --install` puts under `target/tools/`.
- **The threshold is 50 tokens over at least 5 lines**, jscpd's own
  defaults. The scan leaves out the `use` declarations, a file that opens
  with `// @generated`, a root `.gitattributes` marks generated, a file from
  its first column-one `#[cfg(test)]` line on (the inline test module), and
  `tests/`. Measured on 2026-11-01 at `7c231e8` with those exclusions, over
  260 files and 71,588 lines:

  | Threshold | Blocks | Duplicated lines | Blocks across two files |
  | --- | --- | --- | --- |
  | 50 tokens, 5 lines | 127 | 1,364 | 89 |
  | 70 tokens, 5 lines | 57 | 740 | 39 |
  | 100 tokens, 5 lines | 17 | 298 | 12 |

  The largest share sits in `ihe-iti` (66 blocks, 771 lines), then
  `ferropix-server` (42 blocks, 383 lines) and `ferropix-console` (19
  blocks, 210 lines). Raising the threshold drops the short blocks (two
  match arms, two error variants written side by side), yet each is code
  written twice as well, so 50 it is. Raising it is a change to this section
  and to the guard together, with the table measured again.

## Comments and documentation

The comment and doc-comment discipline lives in **`comments.md`** (RFC 505
plus RFC 1574): line comments only, `// TODO(#NNNN):` / `// NOTE:` as the only
markers, a NOTE as a citation plus one sentence (at most 3 lines), plain `//`
runs at most 8 lines, the summary-line convention for doc comments.

## Default values live in the struct's `Default` impl (RFC 3681 shape)

One `impl Default` per struct, every default value inline, and
container-level `#[serde(default, deny_unknown_fields)]` on a configuration
section. The per-field `#[serde(default = "path")]` form, a zero-argument
`fn default_x()`, and a single-reader `const DEFAULT_X` are all banned: each
puts a field's default somewhere other than its own struct. A `const` with
more than one consumer stays a constant.

## RFC-grounded conventions

| RFC | The rule | Enforcement |
|---|---|---|
| 0505 + 1574 | comment and doc conventions | `comments.md` + `too_long_first_doc_paragraph` |
| 3681 | a field's default lives inline in its `Default` impl | §Default values above |
| 3107 | `#[derive(Default)]` + `#[default]` where the default is a variant | `clippy::derivable_impls` (deny) |
| 0199 / 0344 / 0430 | `as_`/`to_`/`into_` cost conventions, naming | `clippy::wrong_self_convention` |
| 1940 | `#[must_use]` on functions whose result is the point | `clippy::must_use_candidate` (pedantic) |
| 2383 | every suppression carries `reason = "…"` | `clippy::allow_attributes_without_reason` (deny) |
| 1946 | intra-doc links over bare paths | `rustdoc::broken_intra_doc_links` (deny) |
| 0201 | an error carries its cause (`Error::source`) | `#[source]`/`#[from]` on every wrapping variant; review-enforced |

## Type and error conventions

- `thiserror` error enums, `#[non_exhaustive]` where the API will grow; no
  `unwrap`/`expect` outside `#[cfg(test)]`; `todo!()` and `unimplemented!()`
  are banned. An error a caller branches on is a variant, never a substring.
- A refusal on the FHIR wire is an `OperationOutcome` with the status the
  transaction names, and on the v2 wire the acknowledgment code ITI TF-2
  names; each is a typed variant, never a string built at the call site.
- An identifier carries its assigning authority: a patient identifier is a
  typed pair of the domain (the `system` or the CX-4) and the value, never a
  bare string. Demographics and identifiers never reach a log or a `Debug`
  rendering; every type that holds them hand-writes a `Debug` that redacts.
- Model closed sets as Rust `enum`s (the transaction, the link type, the
  acknowledgment code, the match grade).
- **No `use X as Y` import renaming.** Fix the name at its definition, or
  qualify one at the use site. (A trait import as `use Trait as _;` is not a
  rename.)
- `std::sync::LazyLock` for statics. Edition 2024, resolver 3, `cargo fmt`
  clean.
- Suppressions are `#[expect(lint, reason = "…")]` scoped to the smallest
  item (`reliability.md`).

## HTTP statuses are compared as types

An HTTP status is a `StatusCode`, and it is compared as one, never as a
number. Rendering the number stays legal.

## Documentation (`missing_docs` is enforced workspace-wide)

Every public item carries a doc comment; intra-doc links resolve in the scope
of the module where the item is defined; literal square brackets are escaped.

## Edition-2024 standing guidance

- `if let` scrutinee temporaries drop before `else` (`reliability.md`).
- RPIT captures every in-scope lifetime and type parameter by default; say
  `use<…>` when a returned `impl Trait` must not.
- `Future` and `IntoFuture` are in the prelude.

## Naming

A name says what the thing is in its domain. Specification-defined things
keep the specification's names verbatim (`targetSystem`, `sourceIdentifier`,
`$ihe-pix`, `$match`); a configuration key follows the family's shape
(`timeout_seconds`, `password_file`). Modules are named for their
contents.

## No Python, anywhere

The tooling languages are bash and Rust. Python is banned in scripts and
embedded in shell; reach for `jq`, `awk`, `sed`, `grep`, or a Rust tool.

## What not to do

- Do not move code from FerroFED piecemeal: the move is one milestone
  (v0.0.2), copied with its history.
- Do not write a second HL7 v2 parser, a second FHIR model or a second audit
  sender: the family's crates are the model.
- Do not store identifying data in the clinical domain: FerroPIX holds the
  identity, the clinical systems hold pseudonymous identifiers.
