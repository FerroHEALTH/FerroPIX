---
name: spec-lookup
description: Look up the authoritative IHE ITI (PIX, PIXm, PDQ, PDQm, PMIR), HL7 FHIR R4 or HL7 v2 requirement for any spec-facing behaviour, in the correct oracle order. Use before implementing or reviewing index behaviour, or to settle a "what does the spec say" question.
allowed-tools: Read, Grep, Glob, WebFetch
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Spec lookup

Answer a "what does the specification say" question about a FerroPIX
transaction, and cite the source (document plus section). Never resolve a
spec-facing question from memory or from another implementation's behaviour
alone (`.claude/rules/spec-adherence.md`).

## Route by surface, then cite

| The question is about | The authority |
|---|---|
| an actor, an option, a transaction's trigger, message semantics or expected actions | the IHE ITI Technical Framework, Volume 1 (profiles) and Volume 2 (transactions) |
| the FHIR form of the cross-reference query or the feed (ITI-83, ITI-104) | the IHE PIXm implementation guide |
| the FHIR demographics query or match (ITI-78, ITI-119) | the IHE PDQm implementation guide |
| the patient master identity feed and subscription (ITI-93, ITI-94) | the IHE PMIR implementation guide |
| a FHIR resource shape, a search parameter or a REST interaction | HL7 FHIR R4 (4.0.1) |
| an HL7 v2 PIX or PDQ message (ITI-8, ITI-9, ITI-10, ITI-21, ITI-22) | ITI TF-2 for the transaction, then the HL7 v2 version it names |

Within a surface, the machine-readable artifact (a FHIR `StructureDefinition`,
`CapabilityStatement` or `OperationDefinition` from the IHE package) outranks
a remembered summary, and the normative prose gives it meaning. Read both
where both exist.

Other implementations (FerroFED's harness PIX Manager, any public MPI) are
behavioural evidence for spec-silent edge cases only. Cite them as such and
record the decision; never treat one as authority.

## Where to look

- **Vendored specifications:** `docs/specs/`, each tree with a
  `PROVENANCE.md`, at the versions `docs/VERSIONS.md` pins. Grep there first,
  because it is the exact pinned text this project implements. The corpora
  arrive with the move from FerroFED (v0.0.2) and the ITI TF-2 v2 texts before
  the v2 transactions (v0.0.3); until a corpus is vendored, the lookup goes to
  the published source below.
- **Published sources (fetch to confirm):**
  - IHE ITI Technical Framework: <https://profiles.ihe.net/ITI/TF/index.html>
  - IHE PIXm: <https://profiles.ihe.net/ITI/PIXm/>
  - IHE PDQm: <https://profiles.ihe.net/ITI/PDQm/>
  - IHE PMIR: <https://profiles.ihe.net/ITI/PMIR/>
  - HL7 FHIR R4: <https://hl7.org/fhir/R4/>

## How to answer

State the requirement, quote the decisive sentence, and cite the document plus
section (and the resource or profile name where the machine-readable form
carries it). If the sources are silent, say so and name the behaviour you
would match; flag it as a spec-silent decision to record on the tracker, never
as a spec fact. If the question needs a version pin that is not made yet, say
that too: an answer that assumes a version is worse than no answer.
