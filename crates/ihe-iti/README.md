<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ihe-iti

The IHE IT Infrastructure (ITI) identity profiles in Rust: one crate for the
technical framework, with a feature per profile. It is published from
[FerroPIX](https://github.com/FerroHEALTH/FerroPIX), the FerroHEALTH Master
Patient Index, which adds the server sides of the profiles as features.

| Feature | Profile | Transactions |
|---|---|---|
| `pixm` | Patient Identifier Cross-reference for Mobile | ITI-83 |
| `pdqm` | Patient Demographics Query for Mobile | ITI-78, ITI-119 |
| `mcsd` | Mobile Care Services Discovery | ITI-90, ITI-91 |
| `pmir` | Patient Master Identity Registry | ITI-93, ITI-94 |
| `xcpd` | Cross-Community Patient Discovery | ITI-55 |
| `balp` | The BALP `AuditEvent` each FHIR profile above defines for its transactions, recorded on the audit trail of [`ihe-atna`](https://crates.io/crates/ihe-atna) | ITI-83, ITI-78, ITI-119, ITI-90, ITI-91, ITI-93, ITI-94 |
| `atna` | The ITI-55 DICOM audit message of `xcpd`, in `ihe-atna`'s ATNA model, with no FHIR model | ITI-55 |
| `pixm-server` | The Patient Identifier Cross-reference Manager side of PIXm, typed, with no store and no HTTP server | ITI-83, ITI-104 |
| `pdqm-server` | The Patient Demographics Supplier side of PDQm, typed, with no store and no HTTP server | ITI-78, ITI-119 |
| `pmir-server` | The Patient Identity Registry side of PMIR, typed, with no store and no HTTP server | ITI-93, ITI-94 |

The crate depends on no application, so a federation gateway, a master patient
index or any other caller can use it as it is. Only the `xcpd` feature may carry
the SOAP 1.2, HL7 v3 and SAML XUA stack; a build without it compiles none of it.
The profiles are published at <https://profiles.ihe.net/ITI/>.

## Access tokens (IUA ITI-72)

The PIXm, PDQm, mCSD and PMIR clients send the credential their
`reqwest::Client` carries in its default headers, or, built
`.with_authorizer(…)`, ask an `ihe_iti::authorizer::Authorizer` for the
headers of every request: an OAuth 2.0 access token incorporated as IUA
ITI-72 §3.72.4.2 asks, which the authorizer refreshes before it expires. The
authorizer reads every answer, and a request it asks for again after a `401`
is sent once more with new headers (§3.72.4.3), never more than twice in all.
The authorizer is handed the request URL without its query, so no patient
identifier or search criterion reaches it, and a request it cannot
authenticate is not sent.

## PIXm (`pixm`)

`ihe_iti::pixm::PixmClient` is the Patient Identifier Cross-reference
Consumer of ITI-83 (PIXm 3.1.0): `GET [base]/Patient/$ihe-pix` with one
`sourceIdentifier` and any number of `targetSystem` domains, read into a
cross-reference, the profile's not-found answer, or a typed error. A client
built with `.invoked_by(Invocation::Post)` posts the same parameters in a
`Parameters` body to `[base]/Patient/$ihe-pix` instead (FHIR R4 Operations
§3.2.0.1), so the source identifier is in no request URL; ITI-83 prescribes
the `GET`, the default, so post only to a Manager that accepts it. A `404` is
"patient unknown" only when its `OperationOutcome` carries a `not-found` issue,
so an outage or a misrouted request is never mistaken for a patient with no
identifiers. Every answer is held to the `$ihe-pix` `OperationDefinition`: the
two out parameters, an assigning authority on each identifier, identifiers only
from the domains asked about, and never the source identifier itself.

Identifier values travel in `secrecy::SecretString`, with redacted `Debug` and
no `Display`, and no error carries a value, the request URL or body, or the
Manager's free text. Build the `reqwest::Client` you pass in with
`redirect::Policy::none()`: the request holds the source identifier. The
client's `Debug` shows its URL with the userinfo replaced by `***` and leaves
out the `reqwest::Client`, whose default headers may hold a credential.

## PDQm (`pdqm`)

`ihe_iti::pdqm::PdqmClient` is the Patient Demographics Consumer of ITI-78
(PDQm 3.2.0): a `POST [base]/Patient/_search` with the criteria of a
`PatientQuery` (every Patient search parameter ITI-78 names, with `:exact` on
the string ones), read into a page of matching Patients with the `total`, each
match's `fullUrl`, `search.score` and `match-grade`, the warnings of any
`OperationOutcome` entry, and the `next` link that `next_page` follows on the
Supplier's own origin. No match is a `total` of `0`; a `404` is "identifier
domain not recognised" only when the query names a domain and the
`OperationOutcome` carries a `not-found` issue. Every answer is held to the
Query Patient Resource Response Message profile: a `searchset` with a
`total`, and a `fullUrl` on every entry. The Patients are decoded as FHIR R4,
not held to the PDQm Patient profile, as the profile asks of a Consumer.

The same client is the Consumer of ITI-119, Patient Demographics Match:
`match_patient` posts a `MatchInput` (the input Patient's identifiers, with
`onlyCertainMatches` and `count` when set) in a `Parameters` resource to
`[base]/Patient/$match`, and reads the Match Output Bundle into the matched
Patients, most likely first, each with its `fullUrl`, its score between 0
and 1 and its `match-grade`. No match is an answer with no Patient. An
answer is refused when a Patient entry lacks its search mode `match`, its
score or its grade, or when a `200` carries an `OperationOutcome` of
`error` or `fatal` severity.

The criteria travel in the request body, so no URL carries them. They, every
matched Patient and every page link redact their content in `Debug`, with no
`Display`, and no error carries a value, a URL or the Supplier's free text.
Build the `reqwest::Client` you pass in with `redirect::Policy::none()`. The
client's `Debug` shows its URLs as the PIXm client's does.

## mCSD (`mcsd`)

`ihe_iti::mcsd::directory::Directory` reads the `Organization` and `Endpoint`
resources a care services directory publishes (mCSD 4.0.0) from one FHIR R4
JSON `Bundle` of type `collection` or `searchset`. It resolves the references
between them inside the Bundle as FHIR R4 §2.36.4.1 does: a relative
`[type]/[id]` against the root of the referring entry's REST `fullUrl`, an
absolute reference against an entry's `fullUrl`. A reference it cannot
resolve is reported as outside the Bundle, never guessed. It refuses an entry
of another resource type, a resource with a `modifierExtension`, and a
repeated `fullUrl` or logical id. Each resource stays as `fhir-types` decodes
it, with accessors for what addressing reads; what a caller accepts as a
connection type, a status or an identifier is the caller's policy. The
`Debug` of an entry shows its `fullUrl`, and an endpoint's `address`, with the
userinfo and the query replaced by `***`, and leaves out an endpoint's
`header` list.

`ihe_iti::mcsd::client::McsdClient` asks a directory over HTTP. `find` is
ITI-90, Find Matching Care Services: `GET [base]/Organization` or
`GET [base]/Endpoint` with the search parameters you give, answered with a
`searchset` Bundle. `updates` is ITI-91, Request Care Services Updates:
`GET [base]/[type]/_history?_since=[instant]`, answered with a `history`
Bundle, newest version first, a deletion as a `DELETE` entry. Both follow
every `next` link on the directory's own origin and return the `Date` the
directory stamped its first page with, as written. Every walk draws on a
`budget::Budget`: one deadline for the whole walk, and caps on the pages,
the answer bytes and the entries it reads (by default 200 pages, 64 MiB and
50,000 entries). Running out of any of them is an error, never a shorter
answer. A status other than `200`, an answer that is not the interaction's
Bundle, a match of another type and a page link to another origin are errors
that carry no URL and no text of the directory's. Build the
`reqwest::Client` you pass in with `redirect::Policy::none()`.

`ihe_iti::mcsd::replica::Replica` holds a directory's `Organization`s and
`Endpoint`s in a `Scope` (every resource, or those carrying an identifier in
a system you name), read with ITI-90 and kept in step with ITI-91 from an
instant you choose, usually from the `Date` it keeps of its last read. The
newest version of each resource wins, a deletion or a version that leaves
the scope removes it, and a refresh with no instant reads everything again.
A refresh returns a new replica and leaves the old one as it was, and
`directory` answers the content as a `Directory`.

## PMIR (`pmir`)

The Patient Identity Subscriber and Patient Identity Consumer sides of PMIR
1.6.0. `ihe_iti::pmir::PmirSubscriber` is ITI-94, Subscribe to Patient
Updates: it creates a `Subscription` at the Patient Identity Registry from a
`SubscriptionRequest` (the criteria the Registry must accept, and your
channel endpoint), reads its status and deletes it. `ihe_iti::pmir::feed::Feed::read`
is the Consumer side of ITI-93, Mobile Patient Identity Feed: it reads a
message the Registry sends to your channel into typed creates, updates,
deletes and merges, refuses one that departs from the PMIR profiles, and
`acknowledgement` writes the response message. The feed keeps the resource
ids and the identifiers only, each value in a `secrecy::SecretString`, and
drops every demographic as it reads; no type has `Display`, and no error
carries a value, a URL or the Registry's text. PMIR leaves the
authentication of the Registry to your channel to the two parties, so your
code authenticates each message before it reads it. Build the
`reqwest::Client` you pass in with `redirect::Policy::none()`.

## The server sides (`pixm-server`, `pdqm-server`, `pmir-server`)

Each server side is the profile's request readers and answer writers, with
no store, no matching and no HTTP framework, so any server can put them on
its own stack: you hand a reader the query string, the media type and the
body, and send the `ihe_iti::server::answer::Answer` it or a writer returns,
its status and its resource encoded by `encode`. Every server reads and
writes FHIR JSON and FHIR XML, as the profiles require;
`ihe_iti::server::format::Format` names the encoding of a body from its
`Content-Type` and of the answer from `_format` and `Accept`. Every refusal
is an `Answer<OperationOutcome>` whose diagnostics are fixed text, so no
outcome quotes the request. Identifiers and demographics travel in the
client sides' redacting types, and no type has `Display`.

- **`pixm-server`, the PIX Manager:** `pixm::server::query::PixQuery` reads
  `$ihe-pix` from the query of the `GET` ITI-83 prescribes or from the
  posted `Parameters` (FHIR R4 Operations), refusing a second
  `sourceIdentifier`, a token without a system and a parameter the
  operation does not define; `pixm::server::answer::cross_references`
  writes the `Parameters` answer, leaving out the source identifier and the
  domains not asked for; `pixm::server::Refusal` writes the three failures
  ITI-83 names. `pixm::server::feed::Feed` reads the ITI-104 conditional
  update on `Patient?identifier=system|value`, held to the PDQm Patient
  profile and to the identifier of the condition, as an add or revise or,
  with a `replaced-by` link, a duplicate resolved; `feed::Removal` reads the
  conditional delete.
- **`pdqm-server`, the Patient Demographics Supplier:**
  `pdqm::server::search::PatientSearch` reads every ITI-78 search parameter,
  with `:exact`, AND and OR, and the domain filter, drops a parameter or a
  modifier it does not support and says so, and writes the `self` link of
  what it respected; `pdqm::server::searchset::searchset` writes the
  `searchset` Bundle with its `total` and paging links, the identifiers
  pruned to the domains asked for. `pdqm::server::matching::MatchRequest`
  reads the `$match` input `Parameters` or a bare `Patient`, and
  `matching::match_set` writes the Match Output Bundle, most likely first,
  each entry with its `search.score` and `match-grade`, honouring
  `onlyCertainMatches` and `count`. `pdqm::server::Refusal` writes the
  unrecognised domain of Case 4 and the missing Patient of a retrieve.
- **`pmir-server`, the Patient Identity Registry:**
  `pmir::server::feed::FeedMessage` reads an ITI-93 feed message in FHIR
  JSON or XML, held to the PMIR Bundle, `MessageHeader`, history Bundle and
  merged Patient profiles, into creates, updates, deletes and merges that
  keep their Patients; `feed::response` writes the response message, `ok`
  or `fatal-error` with a contained `OperationOutcome` correlating each
  refused entry, such as the `405` of an unmerge.
  `pmir::server::subscription::RequestedSubscription` reads an ITI-94
  `Subscription` with one of the four criteria, and `Criteria::matches`
  says whether a Patient falls under it. `pmir::server::notification::message`
  builds the ITI-93 message for a subscriber, which `pmir::feed::Feed` reads.

## XCPD (`xcpd`)

`ihe_iti::xcpd::XcpdClient` is the Initiating Gateway of ITI-55, Cross
Gateway Patient Discovery (ITI TF-2 §3.55, Revision 20.1): a
`PRPA_IN201305UV02` in a SOAP 1.2 envelope with the WS-Addressing headers of
Appendix V, posted to one Responding Gateway, which asks by the shared
patient identifier alone (the identifier mode of §3.55.1, no demographics).
The answer reads into a `Discovery` for Cases 1 to 4 of §3.55.4.2.3, each
match with the `homeCommunityId` that holds it, or a typed error for Case 5,
a SOAP fault, a status with no answer, a timeout, and any answer that does not
hold to the transaction: a `RelatesTo` naming another request, a match with no
community, a `NF` with matches, a document type declaration. Only the
synchronous exchange with an immediate response is offered.

A `RespondingGateway` is `https` only, since the request carries the
patient identifier and any XUA assertion; `unencrypted_for_development`
admits `http` for development and tests. The `reqwest::Client` you pass in
carries the mutual TLS of the ATNA Secure Node the actor is grouped with;
build it with `redirect::Policy::none()`.

The crate signs nothing. `XuaAssertion` holds a SAML 2.0 assertion your
identity provider signed, checked to be exactly one `saml2:Assertion`
element that declares every prefix it uses, and the client sends its bytes
unchanged in a WS-Security header. The patient identifier, every identifier
an answer returns, and the assertion redact their content in `Debug`, and no
error carries a value or the gateway's free text.

`XcpdClient::audited` hands the ITI-55 Initiating Gateway audit message of
every exchange (§3.55.5.1.1) to an `AuditRecorder` you route to your ATNA
audit repository; the client awaits `record` before it returns the answer.
The message carries the query parameters, which name the patient, as a
`SecretString`. A message the recorder refuses fails the discovery with
`XcpdError::Audit`, so no answer is used without its audit. With `atna`,
`AuditEvent::message` writes it as the DICOM PS3.15 audit message the
Initiating Gateway's table fills, in `ihe-atna`'s ATNA model.

`ihe_iti::xcpd::responding` is the Responding Gateway's side of ITI-55.
`responding::query::read` reads a `PRPA_IN201305UV02` from its SOAP 1.2
envelope in any of the three modes of §3.55.1: the WS-Addressing
`MessageID`, a non-anonymous `ReplyTo` (an asynchronous exchange), the
`CorrelationTimeToLive`, the sender's and the target community, the
reverse-query assigning authority, the `responsePriorityCode` and every
parameter of Table 3.55.4.1.2.2-1. It refuses a request that names neither an
identifier nor a name and a birth time, with a typed `Refusal` that quotes
nothing. `responding::answer::envelope` writes the `PRPA_IN201306UV02` of
the case your matcher decided (Cases 1 to 5 of §3.55.4.2.3): one
`RegistrationEvent` per match with the custodian's `homeCommunityId` and
the Health Data Locator code, the `DetectedIssueEvent` of a request for more
attributes or of an error, the `RelatesTo` of the request and the copy of its
query. `answer::deferred_refused` writes the `NS250` acknowledgement of a
Deferred request you do not take, and `answer::fault` a SOAP 1.2 fault.
`responding::audit::RespondingAuditEvent` is the Responding Gateway audit
message (§3.55.5.1.2); with `atna` its `message` writes the DICOM audit
message. Matching, and the asynchronous exchange on the `ReplyTo`, are the
deployment's.

The XML is read and written with `quick-xml`, which the FHIR features
compile already through `fhir-types`; `xcpd` adds only `uuid` for the
message ids and `jiff` for the creation time, and no other feature but
`balp` and `atna`, which write timestamps too, compiles either.

## Whom an exchange is made for

`PixmClient::cross_reference`, the `PdqmClient` searches, pages and
matches, and `XcpdClient::discover` each take an `ihe_atna::user::OnBehalfOf`,
the user model the audit trail shares:
the `User` an OAuth 2.0 access token names (its `iss`, `sub`, `client_id`,
`aud` and purposes of use), or `System` for an exchange the client's own
system makes. An audited exchange's record names the user: the `AuditEvent`
of the `balp` feature carries the BALP `agent:user` and an Application agent
with the `client_id` (PIXm §2:3.83.5.2.1, BALP 1.1.4 §3:5.7.5.4), and the
ITI-55 message carries the Human Requestor, with `UserName` written
`aud<sub@iss>` (ITI TF-2 §3.55.5.1.1, IUA ITI TF-2 §3.72.5.1). A `System`
exchange names no user. `User`'s `Debug` shows none of its values. The mCSD
and PMIR exchanges are always the system's own.

## BALP (`balp`)

With `balp`, every PIXm, PDQm, mCSD and PMIR client takes an
`ihe_atna::balp::AuditRecorder` through `.audited(…)` and records the BALP
`AuditEvent` its profile defines for each transaction: the PIXm Query
Consumer of ITI-83, the PDQm Query and Match Consumers of ITI-78 and
ITI-119, the mCSD Query and Updates Clients of ITI-90 and ITI-91, and the
PMIR Feed Consumer and Subscriber of ITI-93 and ITI-94. The client awaits
the recorder before it returns, its `timeout` bounds the record too
(`ihe_atna::balp::recording::Late`), and a record the recorder refuses fails the exchange
as an audit failure, so no answer is used without its audit. The audit
trail itself, ATNA ITI-20 with its syslog sender, spool and forwarder and
the ATX: FHIR Feed, is the [`ihe-atna`](https://crates.io/crates/ihe-atna)
crate of [FerroAUDIT](https://github.com/FerroHEALTH/FerroAUDIT).

## Licence

Business Source License 1.1 (`LICENSE`): free for every non-production use and
for non-commercial production use; a commercial licence for other production
use; Apache License 2.0 four years after each version.
