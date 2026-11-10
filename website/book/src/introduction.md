<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Introduction

FerroPIX is the Master Patient Index of the
[FerroHEALTH](https://ferrohealth.eu/) family, in pure Rust. Every
organisation issues its own identifiers for the same person. FerroPIX holds
the cross-reference between those identifiers in PostgreSQL and answers the
question every other system starts with: who is this patient, and which
identifier does each system know them by. It also mints the pseudonyms a
clinical store names a patient by, so identifying data stays outside the
clinical record.

It answers over the IHE profiles integrators already use, all on one
cross-reference:

- the [PIXm Manager](integrate/pixm-manager.md) (ITI-83, ITI-104) and the
  [PDQm Supplier](integrate/pdqm-supplier.md) (ITI-78, ITI-119), on HL7 FHIR
  R4;
- the [PMIR Registry](integrate/pmir-registry.md) (ITI-93, ITI-94);
- the [HL7 v2 PIX and PDQ transactions](integrate/hl7v2-face.md) over MLLP
  (ITI-8, ITI-9, ITI-10, ITI-21);
- the [XCPD Responding Gateway](integrate/xcpd-gateway.md) (ITI-55).

A data steward resolves the pairs the matcher is unsure of in the
[operator console](operate/console.md). FerroPIX ships as a server image,
a console image and a Helm chart, and publishes the libraries of the
identity specifications it owns, [`ihe-iti`](crates/ihe-iti.md) and
[`nl-generic-functions`](crates/nl-generic-functions.md), on crates.io.

This book documents `main`, which is FerroPIX {{version:ferropix}} when the
book is built. The [changelog](changelog.md) says what each release
changed.

## How this book is organised

- **Evaluate:** what FerroPIX is and where it sits in the family, the
  specification versions it follows, and the licence.
- **Integrate:** the [quickstart](integrate/getting-started.md), then every
  transaction a system sends FerroPIX, with what it reads, what it answers
  and the section of the profile behind each answer, and the published
  crates.
- **Operate:** configuration, deployment by image, compose and chart,
  matching, the console and its API, security and audit, operations, and
  the release and support policy.
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
