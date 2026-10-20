<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Testing discipline

Test discipline is non-negotiable (a standing hard rule; see `CLAUDE.md`). It
applies to every crate.

## The hard rule

- **Never** silently weaken, skip, or delete an existing test to make a build
  pass.
- **Never** edit a test to route around a bug it exposes. If a test fails and
  the fix is unclear, leave it failing and record a `// TODO(#NNNN):` naming
  its issue.
- A conformance test asserts the specification: cite the section it encodes
  (the ITI TF-2 transaction, the PIXm, PDQm or PMIR page, the FHIR R4 page),
  and never adjust an expectation to match a defect. The tests that move from
  FerroFED keep every case; a difference between their pinned behaviour and
  the specification is settled on the tracker, never by editing the test.

## Tooling

- **Runner:** `cargo-nextest` (`cargo nextest run --workspace`, with and
  without `--all-features`).
- **Upstreams:** every system FerroPIX calls (a Patient Identity Source, a
  subscriber, an Audit Record Repository, a terminology server) is a
  `wiremock` stub or an in-process MLLP peer. No test reaches the network.
- **The store:** tests run against the real store engine, never a mock of
  it; an end-to-end lane behind its own gate starts the pinned containers.
- **The audit trail** is read back through the stub repository, so a case
  asserts the BALP `AuditEvent` it expects and the absence of every
  identifying value the log must not carry.

## Coverage is a mandate, not just pass rate

- Every transaction's success path and every refusal the transaction text
  names (an unknown source domain, an unknown target domain, a malformed
  identifier, a missing required parameter) is its own case with its own
  asserted status and `OperationOutcome` or acknowledgment.
- Linking and merge: every link, unlink and merge outcome, and the
  cross-reference read back after each.
- Pseudonyms: stable for one patient and one pseudonymisation domain,
  distinct across domains, never derivable from the identifier alone.
- **Coverage only ratchets up.** Cases are added, never removed to go green.
- **One behaviour per case.**

## Where tests live

Unit tests live beside the code they test (`#[cfg(test)] mod tests`); a test
that drives the public API lives in the crate's one integration binary,
`tests/it/main.rs` plus one `mod` per topic. Dedicated test files under `src/`
are banned.

## Test shapes

- `Result`-returning tests where plumbing can fail; assertions carry a
  message where the values alone do not say what failed.
- Doctests are copy-paste templates, never `ignore`; `no_run` for an
  example that would open a socket.

## No patient data, no credential, ever

Every name, birth date, address and identifier the tests show is invented for
them, in an identifier domain made up for the test, and every key is
generated when the case runs. A real national identifier never enters the
tree, not even a published test number, unless its issuer publishes it as a
test value and the fixture cites that publication.
