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
| `xcpd` | Cross-Community Patient Discovery, XCPD | ITI-55 | Initiating Gateway, and the Responding Gateway's messages with no HTTP server |
| `balp` | The BALP `AuditEvent` of each FHIR profile above | ITI-83, ITI-78, ITI-119, ITI-90, ITI-91, ITI-93, ITI-94 | the audit record of the client side |
| `atna` | The DICOM audit message of `xcpd`, with no FHIR model | ITI-55 | the audit record of either gateway |
| `smart` | The authorizer over the client-credentials token of `ferrohealth-smart` | ITI-72 | the client's access token |
| `pixm-server` | The server side of PIXm, with no store and no HTTP server | ITI-83, ITI-104 | Patient Identifier Cross-reference Manager |
| `pdqm-server` | The server side of PDQm, with no store and no HTTP server | ITI-78, ITI-119 | Patient Demographics Supplier |
| `pmir-server` | The server side of PMIR, with no store and no HTTP server | ITI-93, ITI-94 | Patient Identity Registry |

Only `xcpd` carries the SOAP 1.2, HL7 v3 and SAML XUA stack; a build without
it compiles none of it. The FHIR features decode FHIR R4 through
[`fhir-types`](https://crates.io/crates/fhir-types), the model FerroBRIDGE
generates from the HL7 packages.

## PIXm (`pixm`)

`ihe_iti::pixm::PixmClient` asks a PIX Manager for the identifiers other
domains hold for a patient (ITI-83, PIXm 3.1.0): `GET
[base]/Patient/$ihe-pix` with one `sourceIdentifier` and any number of
`targetSystem` domains. Built with `.invoked_by(Invocation::Post)`, it posts
the same parameters in a `Parameters` body to `[base]/Patient/$ihe-pix`
instead (FHIR R4 Operations §3.2.0.1), so the source identifier is in no
request URL.

### Lookups by a patient identifier use POST

ITI-83 prescribes the `GET`, which puts the patient identifier, a BSN for a
Dutch patient, in the query string. FerroHEALTH therefore settles it this
way:

- **The FerroPIX PIX Manager, planned,** is to accept `$ihe-pix` by `GET`
  (ITI-83) and by `POST` with a `Parameters` body (FHIR R4 Operations
  §3.2.0.1), and to keep no URL query in a request log ([FerroHEALTH/FerroPIX#52](https://github.com/FerroHEALTH/FerroPIX/issues/52)).
- **FerroHEALTH clients call it by `POST`**
  (`ihe_iti::pixm::Invocation::Post`), so a BSN never appears in a URL.
  Against another Manager, post only when it accepts the `POST` form.
- **A proxy in front of a Manager must not log query strings,** since a
  client that uses the `GET` puts the identifier there.

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

The criteria travel in the request body, never the URL, so a PDQm search
needs no choice of invocation: like a PIXm lookup from a FerroHEALTH client,
it keeps every identifier and demographic out of the URL. The planned PDQm
Supplier of FerroPIX answers the same `POST` ([FerroHEALTH/FerroPIX#52](https://github.com/FerroHEALTH/FerroPIX/issues/52) covers its request
log). An answer that
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

## The server sides (`pixm-server`, `pdqm-server`, `pmir-server`)

A server side is the profile's request readers and answer writers. It holds
no store and no HTTP framework: your server hands a reader the query string,
the media type and the body, and sends the `Answer` it or a writer returns,
a status and a resource encoded as FHIR JSON or FHIR XML, both of which the
profiles require of a server. Every refusal is an `OperationOutcome` whose
text is fixed, so it never quotes the request. A FHIR JSON body whose XML
form does not read back, such as a Patient whose narrative is no XHTML, is
refused, so your server never stores a resource it could not answer with in
XML.

- **`pixm-server`** reads `$ihe-pix` from the `GET` query ITI-83 prescribes
  or from a posted `Parameters` body into one request, and refuses a second
  `sourceIdentifier`, a token without a system, a query value no FHIR
  `string` holds and a parameter the operation does not define. It writes the `Parameters` answer, without the source
  identifier and without the domains not asked for, and the three failures
  of ITI-83. It reads the ITI-104 conditional update on
  `Patient?identifier=system|value`, refusing a Patient that does not carry
  that identifier, and tells an add or revise from a duplicate resolved by a
  `replaced-by` link; it reads the conditional delete of a removal.
- **`pdqm-server`** reads every ITI-78 search parameter, with `:exact`, AND,
  OR and the `identifier=system|` domain filter. A parameter or modifier it
  does not support is dropped, reported, and left out of the `self` link, as
  PDQm allows; following the `self` link reads the same search. It writes the `searchset` Bundle with its `total` and paging
  links, each Patient's identifiers pruned to the domains asked for. It
  reads the `$match` input as `Parameters` or as a bare `Patient`, refusing
  a bare `Patient` posted with `onlyCertainMatches` or `count`, and writes
  the Match Output Bundle, most likely first, each entry with its score and
  `match-grade`.
- **`pmir-server`** reads an ITI-93 feed message in FHIR JSON or XML into
  creates, updates, deletes and merges that keep their Patients, refusing a
  message whose first entry is no `MessageHeader` and a merge that departs
  from the merged Patient profile. It writes the response message, with a
  correlated refusal for each entry the Registry did not apply, such as the
  `405` of an unmerge. It reads an ITI-94 `Subscription` with one of the four
  criteria, refusing any other, says whether a Patient falls under the
  criteria, and builds the ITI-93 message a subscriber receives.

The FerroPIX server answers with them as the PIXm Manager and the PDQm
Supplier ([The PIXm Manager](../integrate/pixm-manager.md),
[The PDQm Supplier](../integrate/pdqm-supplier.md)).

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
profile defines. With `smart`, `authorizer::smart::TokenAuthorizer` is that
authorizer over `ferrohealth-smart`'s client-credentials `TokenSource`: a
bearer token on each request, and after a `401` the refused token dropped
and the request sent once more with a new one. [Security and audit](../operate/security-and-audit.md)
describes both.
