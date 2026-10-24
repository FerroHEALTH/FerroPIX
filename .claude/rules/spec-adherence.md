<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Spec adherence (the published specifications are the oracle)

FerroPIX answers to these sources, never to memory and never to how another
implementation behaves. Each is vendored under `docs/specs/` at the version
`docs/VERSIONS.md` pins, and every spec-facing decision cites them section
by section. The identity corpora arrived with the move from FerroFED. The
ITI TF-2 texts of ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22 are vendored
cache only by `scripts/vendor/ihe-iti-tf.sh` (their HL7 v2 tables keep them
out of the tree; `docs/specs/ihe-iti-tf-hl7v2/PROVENANCE.md`), so run the
script before reading or citing one:

1. **The IHE IT Infrastructure (ITI) Technical Framework**: Volume 1 for the
   PIX, PDQ and PMIR profiles, their actors and options, and Volume 2 for
   each transaction's trigger events, message semantics and expected
   actions (ITI-8 Patient Identity Feed, ITI-9 PIX Query, ITI-10 PIX Update
   Notification, ITI-21 Patient Demographics Query, ITI-22 Patient
   Demographics and Visit Query, and the HL7 v3 and v2.5 variants the
   texts name where they are in scope).
2. **The IHE PIXm implementation guide** (ITI-83 Mobile Patient Identifier
   Cross-reference Query, ITI-104 Patient Identity Feed FHIR) and **the IHE
   PDQm implementation guide** (ITI-78 Mobile Patient Demographics Query,
   ITI-119 Patient Demographics Match), with **the IHE PMIR implementation
   guide** (ITI-93, ITI-94) for the master identity feed. The FHIR packages
   of each guide are the machine-readable form.
3. **HL7 FHIR R4 (4.0.1)**: the resources, search and the RESTful API the
   IHE guides profile.
4. **HL7 v2**, at the version each PIX or PDQ transaction names in ITI TF-2,
   for the segments and fields of the v2 messages, where the v2 transactions
   are in scope.
5. **The docs.rs pages of the pinned crates**, the Rust book and reference,
   for every question about the tools.

## Hard rules

- **Read the governing section before implementing or reviewing** a
  transaction, a query parameter, a linking rule, a status or an
  acknowledgment, and cite it (document and section) in the `// NOTE:` or the
  pull request.
- **NEVER LAX. Strictness is a hard rule.** FerroPIX accepts exactly what the
  governing specification admits. Everything a specification refuses, we
  refuse, and every refusal is an asserted negative test, so a silently
  loosened check is a failing build rather than quiet drift. A tolerance
  outside the specification is recorded on the tracker with the owner's
  decision, flagged "no specification governs this: our own design", and
  never widened without a new decision.
- **Cite only durable references**: the sources above or official external
  documentation. Never cite an internal markdown file as a design authority.
  Where no specification governs a decision (the store, the pseudonym
  derivation, the process model, the configuration), flag it: "no
  specification governs this: our own design".
- **FerroFED's identity code and harness are prior art**, never the oracle.
  Where they and a specification disagree, the specification wins, and the
  difference is settled on the tracker, never by editing a test.
- **Never claim a property no test or citation shows.**
- **A defect in a published specification is recorded as an
  `upstream-report` issue**, with what the specification says, what this
  implementation does and the resolution an upstream would need. The issue
  is the record and stays here.
