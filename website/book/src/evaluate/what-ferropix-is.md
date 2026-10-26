<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# What FerroPIX is

FerroPIX is a Master Patient Index. It holds the patient identity
cross-reference of a group of systems: which identifier each identifier
domain (a hospital's patient number, a national number, a regional index)
uses for one person. A system that knows a patient by one identifier asks
FerroPIX for the others, or finds a patient by their demographics.

Two siblings are planned to resolve patients through the FerroPIX server once
it runs: FerroEHR, as the provider of its EHR index
([FerroHEALTH/FerroEHR#3380](https://github.com/FerroHEALTH/FerroEHR/issues/3380)),
and FerroFED, whose identity binding uses a PIX Manager fake from its own
tests until then
([FerroHEALTH/FerroFED#367](https://github.com/FerroHEALTH/FerroFED/issues/367)).
FerroFED already takes both published crates from crates.io.

A patient index keeps identifying data outside the clinical record, which is
where the openEHR Platform Service Model puts the EHR Index. FerroPIX adds
pseudonyms: one per patient and pseudonymisation domain, so a clinical store
names a patient by a value that identifies no one on its own.

## What is built and what is planned

The table gives the status of each capability. "Client, in `ihe-iti`" means
the published crate can call another system that offers the transaction;
the server side, where FerroPIX answers the transaction itself, is the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).

| Capability | Transactions | Status |
|---|---|---|
| PIXm, the identifier cross-reference query | ITI-83 by `GET`, or by `POST` of a `Parameters` body | Client, in `ihe-iti` (`pixm`), and the server's PIXm Manager ([The PIXm Manager](../integrate/pixm-manager.md)). |
| PIXm, the FHIR identity feed | ITI-104 | The server's PIXm Manager, over the store's feed transaction. |
| PDQm, the demographics query and match | ITI-78 search and read, ITI-119 `$match` | Client, in `ihe-iti` (`pdqm`), and the server's PDQm Supplier with the Match Operation Option ([The PDQm Supplier](../integrate/pdqm-supplier.md)). |
| PMIR, the master identity feed and subscription | ITI-93, ITI-94 | Subscriber and feed Consumer, in `ihe-iti` (`pmir`); the Registry side in `ihe-iti` (`pmir-server`); the server's PMIR Registry receives ITI-93, answers ITI-94 and sends ITI-93 to each subscriber ([The PMIR Registry](../integrate/pmir-registry.md)). |
| mCSD, the care services directory | ITI-90, ITI-91 | Client and replica, in `ihe-iti` (`mcsd`). |
| XCPD, cross-community patient discovery | ITI-55 | Initiating Gateway and the Responding Gateway's messages, in `ihe-iti` (`xcpd`), and the server's Responding Gateway behind `[xcpd]`, synchronous and asynchronous ([The XCPD Responding Gateway](../integrate/xcpd-gateway.md)). The Deferred Response Option is not offered. |
| IUA, access tokens on each request | ITI-72 | The authorizer seam of the `ihe-iti` clients. |
| The persisted cross-reference | identifier domains, one source per domain, linking and merge | Planned. |
| Pseudonym minting | a UUID per patient and pseudonymisation domain | The server mints on the consumer's first ITI-83 ask, resolves through merges, and audits the steward's reverse resolution ([Pseudonyms](../integrate/pseudonyms.md)). |
| HL7 v2 PIX and PDQ | ITI-8 (A01, A04, A05, A08, A40), ITI-9 and ITI-21 with the continuation protocol | The server's MLLP listener, on FerroBRIDGE's HL7 v2 parser and MLLP codec ([The HL7 v2 face](../integrate/hl7v2-face.md)). ITI-10 and ITI-22 are planned. |
| Audit of the Manager side | BALP `AuditEvent` through `ihe-atna` | The PIXm Manager records every ITI-83 and ITI-104, and the PDQm Supplier every ITI-78 and ITI-119. The clients' audit records are built (`balp`, `atna`). |
| Dutch Generic Functions: localization (NVI) | the NVI Localization Service search | Client, in `nl-generic-functions` (`nvi`). |
| Dutch Generic Functions: consent (Mitz) | the closed authorization question | Client, in `nl-generic-functions` (`mitz`). |
| Dutch Generic Functions: addressing (LRZa) | the URA of an organisation | In `nl-generic-functions` (`lrza`). |
| Dutch Generic Functions: authentication (Nuts) | the Nuts RFC021 access token request | Client, in `nl-generic-functions` (`nuts-auth`). |
| BSN and the pseudonymised BSN | the identifier systems the functions share | The naming systems and a redacting pseudonym type, in `nl-generic-functions`. |
| The server's container image | `ghcr.io/ferrohealth/ferropix`, on distroless static pinned by digest, SLSA Build Level 3 | Built from source today, with the compose quickstart beside PostgreSQL; the release lane publishes it from the first tagged release ([Deployment](../operate/deployment.md)). |
| The operator console | the data steward's tool: search, the patient record, the review queue, merge and unmerge, the audit trail | Built on the FerroHEALTH component library, each page saying it cannot be shown until the server's operator API ([#90](https://github.com/FerroHEALTH/FerroPIX/issues/90)) exists; see [The operator console](../operate/console.md). Its image, `ghcr.io/ferrohealth/ferropix-console`, is published by the release lane from the first tagged release. |

The BSNk polymorphic pseudonyms are in the scope of `nl-generic-functions`
and are not built: the crate does not call the BSNk pseudonym service today.

## The server, as planned

The [v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3)
names what the server will do. The PIXm Manager
([The PIXm Manager](../integrate/pixm-manager.md)) and the PDQm Supplier
([The PDQm Supplier](../integrate/pdqm-supplier.md)) run today; the rest of
this list is planned:

- **The cross-reference:** identifier domains, one Patient Identity Source
  per domain, linking and merge with their full history, persisted in
  PostgreSQL.
- **HL7 v2:** ITI-10, the PIX update notification, beside the ITI-8, ITI-9
  and ITI-21 the server answers today.
- **Audit:** the Manager side's BALP records through `ihe-atna`, the audit
  crate of FerroAUDIT.
- **The server's and the console's container images**, published by the
  release lane from the first tagged release.

## What FerroPIX is not

These are the limits of its scope, for the crates today and for the server
as the [v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3)
plans it.

- It holds no clinical data. Compositions, observations and documents stay
  in the clinical repository; FerroPIX holds identifiers, the demographics a
  match needs, and the links between them.
- It is no audit repository. The audit trail is the `ihe-atna` crate, and
  the records go to the ATNA repository you run.
- It is no authorization server. Access tokens come from the issuer you run.
  The crates take a token through an authorizer you supply, and the server
  takes its authorization from `ferrohealth-smart`, the family's SMART and
  OAuth 2.0 crate.
