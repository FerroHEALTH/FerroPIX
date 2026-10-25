<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Introduction

FerroPIX is the Master Patient Index of the
[FerroHEALTH](https://ferrohealth.eu/) family, in pure Rust. Every
organisation issues its own identifiers for the same person. A patient index
holds the cross-reference between those identifiers and answers the question
every other component starts with: who is this patient, and which identifier
does each system know them by. FerroPIX also mints the pseudonymous
identifiers the clinical domain uses, so a clinical record carries a
pseudonym and identifying data stays outside it.

FerroPIX speaks the IHE profiles integrators already expect: PIX and PDQ, in
their FHIR forms PIXm and PDQm, the PMIR identity feed, and the HL7 v2 PIX and
PDQ transactions beside them.

## Where the project is

FerroPIX is built in the order of its milestones, and this book says which
parts exist today.

- **Built and published:** the two libraries of the identity specifications
  FerroPIX owns, [`ihe-iti`](crates/ihe-iti.md) (the IHE identity profiles,
  a feature each) and [`nl-generic-functions`](crates/nl-generic-functions.md)
  (the Dutch Generic Functions). They hold the client side of each profile
  and are on crates.io for any product to use. FerroFED takes both from
  there.
- **Being built:** the server, the
  [v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).
  Its cross-reference in PostgreSQL, the
  [PIXm Manager](integrate/pixm-manager.md) and the
  [PDQm Supplier](integrate/pdqm-supplier.md) run today, as does the
  [HL7 v2 face](integrate/hl7v2-face.md) with ITI-8, ITI-9 and ITI-21; the
  PMIR Registry and pseudonym minting are planned, and the pages under Operate say which is which.

[What FerroPIX is](evaluate/what-ferropix-is.md) lists every capability with
its status.

## How this book is organised

- **Evaluate:** what FerroPIX is, which specification versions it follows,
  and the licence.
- **Integrate:** how to use the published crates today, feature by feature.
- **Operate:** configuration, deployment, security and audit, and
  operations, each with what holds today and what the server adds.
- **Contribute:** how the work is organised and how this book stays in step
  with the code.

This book covers FerroPIX alone. What every FerroHEALTH product shares is in
the [family book](https://ferrohealth.eu/docs/):
[how the products fit together](https://ferrohealth.eu/docs/architecture.html),
[the manufacturer](https://ferrohealth.eu/docs/manufacturer.html), Cadasto
B.V., with its [security policy](https://ferrohealth.eu/docs/security.html),
[post-market procedure](https://ferrohealth.eu/docs/post-market.html) and the
[Cyber Resilience Act](https://ferrohealth.eu/docs/cra.html) from the
manufacturer's side, and
[licensing and trademarks](https://ferrohealth.eu/docs/licensing.html).
