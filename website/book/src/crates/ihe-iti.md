<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ihe-iti

The IHE IT Infrastructure (ITI) identity profiles in Rust: one crate for the
technical framework, with a feature per profile. The API reference is on
[docs.rs/ihe-iti](https://docs.rs/ihe-iti); this page says what each feature
does and what it checks. The version on `main` is {{version:ihe-iti}}.

## Features

| Feature | Profile | Transactions | Actor |
|---|---|---|---|
| `pixm` | Patient Identifier Cross-reference for Mobile, PIXm | ITI-83 | Patient Identifier Cross-reference Consumer |
| `pdqm` | Patient Demographics Query for Mobile, PDQm | ITI-78, ITI-119 | Patient Demographics Consumer |
| `pmir` | Patient Master Identity Registry, PMIR | ITI-93, ITI-94 | Patient Identity Subscriber and Consumer |
| `mcsd` | Mobile Care Services Discovery, mCSD | ITI-90, ITI-91 | Query Client and Update Client |
| `xcpd` | Cross-Community Patient Discovery, XCPD | ITI-55 | Initiating Gateway |
| `balp` | The BALP `AuditEvent` of each FHIR profile above | ITI-83, ITI-78, ITI-119, ITI-90, ITI-91, ITI-93, ITI-94 | the audit record of the client side |
| `atna` | The DICOM audit message of `xcpd`, with no FHIR model | ITI-55 | the audit record of the Initiating Gateway |

Only `xcpd` carries the SOAP 1.2, HL7 v3 and SAML XUA stack; a build without
it compiles none of it. The FHIR features decode FHIR R4 through
[`fhir-types`](https://crates.io/crates/fhir-types), the model FerroBRIDGE
generates from the HL7 packages.

## PIXm (`pixm`)

`ihe_iti::pixm::PixmClient` asks a PIX Manager for the identifiers other
domains hold for a patient (ITI-83, PIXm 3.1.0): `GET
[base]/Patient/$ihe-pix` with one `sourceIdentifier` and any number of
`targetSystem` domains. Built with `.invoked_by(Invocation::Post)`, it posts
the same parameters in a `Parameters` body instead, so the source
identifier is in no request URL; ITI-83 prescribes the `GET`, so post only to
a Manager that accepts it.

Every answer is held to the `$ihe-pix` `OperationDefinition`: the two out
parameters, an assigning authority on each identifier, identifiers only
from the domains asked about, and never the source identifier itself. A
`404` is "patient unknown" only when its `OperationOutcome` carries a
`not-found` issue.

## PDQm (`pdqm`)

`ihe_iti::pdqm::PdqmClient` is the Patient Demographics Consumer:

- **ITI-78, the demographics query:** a `POST [base]/Patient/_search` with
  the criteria of a `PatientQuery` (every Patient search parameter ITI-78
  names), read into a page of matching Patients with the `total`, each
  match's score and grade, and the `next` link, which `next_page` follows on
  the Supplier's own origin only.
- **ITI-119, the demographics match:** `match_patient` posts a `MatchInput`
  to `[base]/Patient/$match` and reads the matched Patients, most likely
  first, each with its score between 0 and 1 and its `match-grade`.

The criteria travel in the request body, never the URL. An answer that
departs from the profile, such as a match entry without its score or grade,
is refused.

## PMIR (`pmir`)

The Patient Identity Subscriber and Consumer sides of PMIR 1.6.0:

- **ITI-94, Subscribe to Patient Updates:** `ihe_iti::pmir::PmirSubscriber`
  creates a `Subscription` at the Patient Identity Registry, reads its status
  and deletes it.
- **ITI-93, Mobile Patient Identity Feed:** `ihe_iti::pmir::feed::Feed::read`
  reads a message the Registry sends to the subscription's channel into
  typed creates, updates, deletes and merges, and `acknowledgement` writes the
  response. The feed keeps the resource ids and the identifiers and drops
  every demographic as it reads.

The specification leaves the authentication of the Registry to your channel
to the two parties, so your code authenticates each message before it reads
it.

## mCSD (`mcsd`)

- `ihe_iti::mcsd::directory::Directory` reads the `Organization` and
  `Endpoint` resources of one directory Bundle and resolves the references
  between them as FHIR R4 does. A reference it cannot resolve is reported,
  never guessed.
- `ihe_iti::mcsd::client::McsdClient` asks a directory over HTTP: `find` is
  ITI-90 and `updates` is ITI-91. Every walk over the pages draws on one
  budget of time, pages, bytes and entries, and running out of any of them is
  an error, never a shorter answer.
- `ihe_iti::mcsd::replica::Replica` keeps a copy of a directory in step with
  ITI-90 and ITI-91.

## XCPD (`xcpd`)

`ihe_iti::xcpd::XcpdClient` is the Initiating Gateway of ITI-55 (ITI TF-2
§3.55, Revision 20.1): a `PRPA_IN201305UV02` in a SOAP 1.2 envelope, posted to
one Responding Gateway, asking by the shared patient identifier alone. The
answer reads into a `Discovery` for Cases 1 to 4 of the transaction, each
match with the community that holds it, or a typed error. A Responding
Gateway is `https` only; the `reqwest::Client` you pass in carries the mutual
TLS of the ATNA Secure Node.

The crate signs nothing. You supply a SAML 2.0 assertion your identity
provider signed, and the client sends it unchanged in a WS-Security header.

## Whom an exchange is made for

`PixmClient::cross_reference`, the `PdqmClient` searches and matches, and
`XcpdClient::discover` each take an `ihe_atna::user::OnBehalfOf`: the `User`
an OAuth 2.0 access token names, or `System` for an exchange the client's own
system makes. An audited exchange's record names that user. The mCSD and
PMIR exchanges are always the system's own.

## Access tokens and audit

Each client can ask an authorizer you supply for the headers of every
request (IUA ITI-72), and with `balp` or `atna` records the audit record its
profile defines. [Security and audit](../operate/security-and-audit.md)
describes both.
