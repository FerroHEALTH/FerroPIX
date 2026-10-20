<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Code generation: the family's generated layers are consumed, never copied

FerroPIX generates nothing today. The machine-readable models it needs are
generated elsewhere in the family and consumed as published crates: the FHIR
model (`fhir-types`) and the HL7 v2 tables (`hl7v2-types`), both emitted by
FerroBRIDGE's generator from the vendored HL7 inputs. FerroPIX serves HL7 v2
on FerroBRIDGE's v2 parsing, never a second parser.

## The rules

- **Never hand-edit a `// @generated` file**, here or in a dependency. Every
  generated file starts with a `// @generated … DO NOT EDIT` banner. A doc
  defect, a wrong field type, or a missing variant is an emitter fix in the
  generator's own repository plus a new published version, never an edit of
  the output.
- **Fix the emitter, never the consumer.** When FerroPIX hits a shape in a
  generated crate that is wrong or insufficient versus the specification, the
  fix is filed on the generator's tracker and lands there. A shadow type, a
  duplicate model, an adapter layer, a placeholder value, or a "temporary"
  local representation here is forbidden: it silently forks the model. On
  discovering an existing workaround, register its removal.
- **Should FerroPIX ever generate a layer of its own**, the family discipline
  applies in the same change: a declared root set emitted complete,
  per-version output by construction, byte-deterministic output from ordered
  structures, a drift check in CI that regenerates and fails on any diff, a
  `.gitattributes` `linguist-generated` entry, and the inputs vendored
  verbatim with provenance (`vendored-inputs.md`).

## What is never generated

The index itself: the cross-reference and its store, the transaction
handlers, the linking and merge rules, the pseudonym minting and the server
surface. That is hand-written idiomatic Rust of our own design
(`rust-style.md`), consuming the generated types directly, and it is the
product.
