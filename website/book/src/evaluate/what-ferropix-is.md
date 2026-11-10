<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# What FerroPIX is

FerroPIX is a Master Patient Index. It holds the patient identity
cross-reference of a group of systems: which identifier each identifier
domain (a hospital's patient number, a national number such as the BSN, a
regional index) uses for one person. A system that knows a patient by one
identifier asks FerroPIX for the others, or finds a patient by their
demographics, and each domain's one Patient Identity Source keeps its own
identifiers current.

Records become one person in three tiers: an identifier in an
`authoritative` domain links two records on its own, a probabilistic scorer
grades the remaining pairs, and a data steward decides the pairs the scorer
is unsure of ([Matching](../operate/matching.md)). Every merge can be
reverted, and the store keeps the history of every record.

FerroPIX also mints pseudonyms: one per patient and pseudonymisation
domain, so a clinical store names a patient by a value that identifies no
one on its own, and only the steward resolves it back
([Pseudonyms](../integrate/pseudonyms.md)).

## What it implements

Each row is served by the server on `main`. The section is the one of the
pinned specification that governs the transaction
([Conformance and specification pins](conformance.md)).

| Capability | Transactions | Specification | Page |
|---|---|---|---|
| PIXm Patient Identifier Cross-reference Manager | ITI-83 by `GET` and by `POST` of a `Parameters` body, ITI-104 with the Remove Patient Option | PIXm 3.1.0 §2:3.83, §2:3.104 | [The PIXm Manager](../integrate/pixm-manager.md) |
| PDQm Patient Demographics Supplier, with the Match Operation Option | ITI-78 search and read, ITI-119 `$match` | PDQm 3.2.0 §2:3.78, §2:3.119 | [The PDQm Supplier](../integrate/pdqm-supplier.md) |
| PMIR Patient Identity Registry | ITI-93 received and sent, ITI-94 | PMIR 1.6.0 §2:3.93, §2:3.94 | [The PMIR Registry](../integrate/pmir-registry.md) |
| HL7 v2 Patient Identifier Cross-reference Manager and Patient Demographics Supplier | ITI-8, ITI-9, ITI-10 and ITI-21 over MLLP | ITI TF-2 §3.8, §3.9, §3.10, §3.21 | [The HL7 v2 face](../integrate/hl7v2-face.md) |
| XCPD Responding Gateway | ITI-55, synchronous and asynchronous | ITI TF-2 §3.55 | [The XCPD Responding Gateway](../integrate/xcpd-gateway.md) |
| Pseudonym minting | a UUID per patient and pseudonymisation domain, answered through ITI-83 | no specification governs the pseudonym: FerroPIX's own design | [Pseudonyms](../integrate/pseudonyms.md) |
| The BSN and the pseudonymised BSN | identifier domains under the naming systems of the Dutch Generic Functions | the Netherlands Generic Functions guide, `fhir.nl.gf` | [The Dutch identifiers](../integrate/dutch-identifiers.md) |
| Authorization | SMART and OAuth 2.0 access tokens on every request, with the IUA scope of each transaction | IUA ITI-72 §3.72, RFC 9068 | [Security and audit](../operate/security-and-audit.md) |
| Audit | a BALP `AuditEvent` for every transaction and every steward action, sent over the FHIR feed | BALP 1.1.4, each profile's audit section | [Security and audit](../operate/security-and-audit.md#the-servers-audit-trail) |
| The data steward's tools | the review queue, search, the patient record, merge and unmerge, domains, sources and the audit trail | no specification governs them: FerroPIX's own design | [The operator console](../operate/console.md), [The operator API](../operate/operator-api.md) |

The HL7 v2 face does not answer ITI-22, the demographics and visit query:
FerroPIX keeps no visit data. The XCPD gateway does not offer the Deferred
Response Option. Each page says what its transaction refuses.

FerroPIX also publishes the client sides of these profiles, and of mCSD
(ITI-90, ITI-91), as the crate [`ihe-iti`](../crates/ihe-iti.md), and the
Dutch Generic Functions (NVI localization, Mitz consent, LRZa addressing
and Nuts authentication) as [`nl-generic-functions`](../crates/nl-generic-functions.md).
The server is built on both.

## What you run

| Part | What it is |
|---|---|
| The server, `ghcr.io/ferrohealth/ferropix` | One static Linux binary, `ferropix`, on a distroless base, for amd64 and arm64. It serves the FHIR face, the operator API and the XCPD gateway over HTTP, and the HL7 v2 face over MLLP. |
| The console, `ghcr.io/ferrohealth/ferropix-console` | The data steward's web console, its own binary and image, a client of the server's operator API that keeps no patient data. |
| The Helm chart, `oci://ghcr.io/ferrohealth/charts/ferropix` | The server and, when you switch it on, the console, on Kubernetes. |
| `compose.yaml` | The quickstart: the server beside PostgreSQL and a development issuer, with the console and a terminology server as profiles. |

Beside them you run PostgreSQL 18, an OAuth 2.0 authorization server whose
tokens FerroPIX reads, an audit repository that takes the BALP records,
and, for the HL7 v2 face, a FHIR terminology server
([Deployment](../operate/deployment.md)). Releases are built at SLSA Build
Level 3 with signed provenance and SBOMs.

## Where it sits in the family

The FerroHEALTH products reach each other only over published protocols,
and FerroPIX is the one that owns patient identity:

- **FerroEHR**, the openEHR clinical data repository, finds a patient's EHR
  through a PIXm Manager when its subject index is `pixm`: its
  `EHR_STATUS` names the patient by the pseudonym FerroPIX minted in the
  clinical domain. Its compose quickstart and its Helm chart run FerroPIX.
- **FerroFED**, the federation gateway, resolves the patients of a
  cross-site query through a PIXm Manager, and takes `ihe-iti` and
  `nl-generic-functions` from crates.io. Its quickstart runs FerroPIX.
- **FerroBRIDGE** provides the HL7 v2 parser, MLLP codec and v2-to-FHIR
  maps the HL7 v2 face reads with, and **FerroTERM** is the terminology
  server those maps translate table values on.
- **FerroAUTH** provides `ferrohealth-smart`, the resource server that
  checks every token, and **FerroAUDIT** provides `ihe-atna`, the audit
  trail the BALP records travel on.

## What FerroPIX is not

- It holds no clinical data. Compositions, observations and documents stay
  in the clinical repository; FerroPIX holds identifiers, the demographics
  a match needs, and the links between them.
- It is no audit repository. It writes its records and sends them to the
  repository you run.
- It is no authorization server. Access tokens come from the issuer you
  run, and FerroPIX never forwards a caller's token.
- It calls no BSNk pseudonymisation service. A pseudonymised BSN is fed to
  it like any other identifier
  ([The Dutch identifiers](../integrate/dutch-identifiers.md)).
