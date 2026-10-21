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
and FerroFED, whose identity binding uses a PIX Manager of its own test
harness until then
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
| PIXm, the identifier cross-reference query | ITI-83 by `GET`, or by `POST` of a `Parameters` body | Client, in `ihe-iti` (`pixm`). The PIXm Manager is planned. |
| PIXm, the FHIR identity feed | ITI-104 | Planned, with the PIXm Manager. |
| PDQm, the demographics query and match | ITI-78, ITI-119 | Client, in `ihe-iti` (`pdqm`). The PDQm Supplier is planned. |
| PMIR, the master identity feed and subscription | ITI-93, ITI-94 | Subscriber and feed Consumer, in `ihe-iti` (`pmir`). The PMIR Registry is planned. |
| mCSD, the care services directory | ITI-90, ITI-91 | Client and replica, in `ihe-iti` (`mcsd`). |
| XCPD, cross-community patient discovery | ITI-55 | Initiating Gateway, in `ihe-iti` (`xcpd`). |
| IUA, access tokens on each request | ITI-72 | The authorizer seam of the `ihe-iti` clients. |
| The persisted cross-reference | identifier domains, one source per domain, linking and merge | Planned. |
| Pseudonym minting | a UUID per patient and pseudonymisation domain | Planned. |
| HL7 v2 PIX and PDQ | ITI-8, ITI-9, ITI-21, ITI-22, and ITI-44 to ITI-47 as the ITI Technical Framework texts decide | Planned, on FerroBRIDGE's HL7 v2 parser and MLLP codec. |
| Audit of the Manager side | BALP `AuditEvent` through `ihe-atna` | Planned. The clients' audit records are built (`balp`, `atna`). |
| Dutch Generic Functions: localization (NVI) | the NVI Localization Service search | Client, in `nl-generic-functions` (`nvi`). |
| Dutch Generic Functions: consent (Mitz) | the closed authorization question | Client, in `nl-generic-functions` (`mitz`). |
| Dutch Generic Functions: addressing (LRZa) | the URA of an organisation | In `nl-generic-functions` (`lrza`). |
| Dutch Generic Functions: authentication (Nuts) | the Nuts RFC021 access token request | Client, in `nl-generic-functions` (`nuts-auth`). |
| BSN and the pseudonymised BSN | the identifier systems the functions share | The naming systems and a redacting pseudonym type, in `nl-generic-functions`. |
| A container image | | Planned, pinned by digest. |

The BSNk polymorphic pseudonyms are in the scope of `nl-generic-functions`
and are not built: the crate does not call the BSNk pseudonym service today.

## The server, as planned

The v0.0.3 milestone names what the server will do. Until it lands, none of
this runs, and this book describes it only as planned:

- **The PIXm Manager:** ITI-83 by `GET` and by `POST`, with several
  `targetSystem` domains in one query, and the ITI-104 identity feed.
- **The PDQm Supplier:** ITI-78 and the ITI-119 match.
- **The PMIR Registry:** ITI-93 and ITI-94.
- **The cross-reference:** identifier domains, one Patient Identity Source
  per domain, linking and merge, persisted.
- **Pseudonyms:** a UUID per patient and pseudonymisation domain, for
  FerroEHR's subject namespaces.
- **HL7 v2:** the PIX and PDQ transactions on the HL7 v2 parsing FerroBRIDGE
  publishes, so the family has one parser.
- **Audit:** the Manager side's BALP records through `ihe-atna`, the audit
  crate of FerroAUDIT.
- **A container image**, published and pinned by digest.

## What FerroPIX is not

These are the limits of its scope, for the crates today and for the server
as planned.

- It holds no clinical data. Compositions, observations and documents stay
  in the clinical repository; FerroPIX holds identifiers, the demographics a
  match needs, and the links between them.
- It is no audit repository. The audit trail is the `ihe-atna` crate, and
  the records go to the ATNA repository you run.
- It is no authorization server. Access tokens come from the issuer you run.
  The crates take a token through an authorizer you supply, and the server
  takes its authorization from `ferrohealth-smart`, the family's SMART and
  OAuth 2.0 crate.
