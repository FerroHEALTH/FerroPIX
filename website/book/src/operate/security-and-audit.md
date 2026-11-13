<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Security and audit

A patient index handles the most identifying data in a health system, so
the server and both crates are built to keep it out of anywhere it was not
sent. This page says who may call the server, how identifying data is kept
out of logs, errors and URLs, and what the audit trail records.

## Who may call the server

Every request to `{base}/fhir` and `{base}/operator` carries a bearer access
token that one of the `[[auth.issuer]]` entries signed. At start, `serve`
reads each issuer's RFC 8414 metadata and key set, and refuses to start
when one cannot be read. A token is checked for its signature, issuer,
audience and lifetime under RFC 9068. The scopes each caller needs:

| Caller | Scope | Routes |
|---|---|---|
| PIXm or PDQm Consumer | `system/Patient.rs` | `$ihe-pix` by `GET` and `POST` (ITI-83), the Patient search and read (ITI-78), `$match` (ITI-119) |
| Patient Identity Source | `system/Patient.cud`, and a machine's token whose `client_id` equals the domain's `source_client_id` | the conditional update and delete of `Patient` (ITI-104) |
| PMIR subscriber | `system/Subscription.cruds` | `Subscription` (ITI-94) |
| Data steward | `auth.product_scope` (default `__ferropix.steward`) | the operator API |
| Administrator | `operator.admin_scope` (default `__ferropix.admin`) | the writes of identifier and pseudonymisation domains, and the reads beside them |

Each IHE transaction also admits the scope its profile defines for an IUA
grouping, the transaction's name (`ITI-83`, `ITI-104`, `ITI-78`,
`ITI-119`). A `patient/` scope is refused: it reaches one patient's
records, and a patient index answers about every patient. A Source's
`client_id` counts only when the issuer that signed its token lists it in
`clients`, so a Source of one domain cannot change another (PIXm 3.1.0
§2:3.104.5).

A Source is a machine. Its token comes from the client credentials grant
(RFC 6749 §4.4), and the issuer's profile classifies it as app-only: under
the `rfc9068` profile, its `sub` is its `client_id`. A person's token
through the same client is refused, even when the issuer lists that client
in `first_party_clients` for a login. To upgrade a Source that fed with a
person's token, have it request a client credentials token for its
`source_client_id` and send that.

The answers:

- No valid token: `401` with a `WWW-Authenticate: Bearer` challenge and an
  `OperationOutcome` with a `security` issue on the FHIR face, or the
  problem [`unauthenticated`](problems.md#unauthenticated) on the operator
  API.
- A token without the scope: `403` with `error="insufficient_scope"` and a
  `security` issue, or the problem
  [`not-permitted`](problems.md#not-permitted).
- A Source changing a domain it does not feed: `403` with a `forbidden`
  issue.
- A person's token through a Source's client: `403` with a `forbidden`
  issue whose diagnostics say a Source sends a client credentials token.

No refusal names a claim, a scope, the token or a value of the request.
`GET {base}/fhir/metadata` and `GET {base}/fhir/.well-known/smart-configuration`
need no token: the capability statement names the SMART service, the token
endpoint and the scopes, and the SMART discovery document lists the same
scopes as `scopes_supported`. FerroPIX never forwards a caller's token.

The PIXm, PDQm and PMIR routes and the operator API are served
([The PIXm Manager](../integrate/pixm-manager.md),
[The PDQm Supplier](../integrate/pdqm-supplier.md),
[The PMIR Registry](../integrate/pmir-registry.md),
[The operator API](operator-api.md)), and the server refuses to start when a
row of the guard's table has no handler.

Two faces carry no bearer token, because their transactions define none.
The HL7 v2 face admits a message by its sending application and facility
(MSH-3, MSH-4), each listed in `[[hl7v2.senders]]`, and the XCPD gateway
answers only the initiating communities `[xcpd]` lists. The mutual TLS an
ATNA Secure Node asks of both is the deployment's, at a sidecar or ingress
in front of FerroPIX ([The HL7 v2 face](../integrate/hl7v2-face.md),
[The XCPD Responding Gateway](../integrate/xcpd-gateway.md)).

Basic users are for a run on your own machine: they need
`profile = "development"` and the `[dev]` table. A Basic user is a person,
a steward or an administrator, and never a Source or another machine: a
`[basic_grants]` table granting a `system/` scope refuses the start, naming
the user, and a machine signs in with a client credentials token (RFC 6749 §4.4) of an
`[[auth.issuer]]`.

## No identifying data where it was not sent

- Every identifier, every demographic criterion, a pseudonymised BSN, a SAML
  assertion and a token travels as a `secrecy::SecretString` or in a type
  that redacts its content in `Debug` and has no `Display`.
- No error carries a value, a request URL or body, or the free text the
  other server wrote, so an error you log cannot leak one.
- A client's `Debug` shows its URLs with the userinfo replaced by `***` and
  whether it carries a credential, never the credential.
- Every client is built from the `OutboundClient` of `ferrohealth-smart`,
  which follows no redirect: the request carries the identifier, and a
  redirect would send it on.
- **Use `POST` for every lookup by a patient identifier.** A PDQm query
  sends its criteria in the request body. Call a PIXm Manager by `POST`
  with a `Parameters` body (`ihe_iti::pixm::Invocation::Post`, FHIR R4
  Operations §3.2.0.1), as every FerroHEALTH client does, so a BSN never
  appears in a URL. FerroPIX's PIX Manager accepts `$ihe-pix` by `GET`
  (ITI-83) and by `POST` with the same answers, and its request log, spans
  and errors hold neither the query nor the body
  ([The PIXm Manager](../integrate/pixm-manager.md)). A proxy in front of
  any Manager must not log query strings, because a client that uses the
  `GET` puts the identifier there.
- The PMIR feed drops every demographic as it reads an ITI-93 message.
- The server's store seals every identifier of a sensitive domain at rest
  and finds it by a keyed HMAC, so a database dump shows no identifier
  ([The store](configuration.md#the-store)). Its errors keep the SQLSTATE,
  the constraint and the message of a refusal and drop the detail
  PostgreSQL fills with the row, so no identifier or name reaches a log line
  through them.

## Access tokens

Every client of `ihe-iti` and the NVI and Mitz clients of
`nl-generic-functions` send the credential of the endpoint table their
`OutboundClient` is built from: HTTP Basic, or an OAuth 2.0 client
credentials token incorporated as IUA ITI-72 §3.72.4.2 asks, with a `DPoP`
proof of each request for a bound token. After a `401` the refused token is
dropped and the request sent once more with a new one (§3.72.4.3), never
more than twice in all. A `DPoP` proof names the request URL without its
query, so no identifier or search criterion reaches it, and a request no
token can be had for is not sent.

## The server's audit trail

The server writes an `AuditEvent` for every transaction it answers or
sends, on every face, and for every steward action:

| Transaction | Audit record |
|---|---|
| ITI-83 `$ihe-pix` | PIXm Query Manager (PIXm 3.1.0 §2:3.83.5.1.2) |
| ITI-104 add, revise, remove | PIXm Feed Create, Update and Delete Manager (§2:3.104.5.1.2) |
| ITI-78 search and read | PDQm Query Supplier (PDQm 3.2.0 §2:3.78.5.1.2) |
| ITI-119 `$match` | PDQm Match Supplier (§2:3.119.5.1.2) |
| ITI-93 received | PMIR Feed audit, one patient entity per change |
| ITI-94 create, read, delete | PMIR Subscription Create, Read and Delete audit; an enable or disable is a BALP Update |
| ITI-8 | the Patient Record event: `C` for an add, `U` for a revision, and for an A40 `U` for the survivor and `D` for the subsumed identifier (ITI TF-2 §3.8.5) |
| ITI-9 and ITI-21 | the Query event with the whole query message (ITI TF-2 §3.9.5.1.2) |
| ITI-10 delivered | the Manager's Patient Record read, the consumer as its destination (ITI TF-2 §3.10.5.1.1) |
| ITI-55 | the Responding Gateway's Query event (ITI TF-2 §3.55.5.1.2) |
| A steward's view or decision | BALP Patient Read or Patient Update |
| A steward's search | BALP Patient Query per person found, or Query when none was |
| A steward's reveal of an identifier or a pseudonym | BALP Privacy Disclosure at the source, with the reason |
| A pseudonym minted, on an ITI-83 ask or by a steward | BALP Create, naming the patient and the domain, never the value |
| A steward's reverse resolution of a pseudonym | BALP Privacy Disclosure at the source, with the reason |
| An export of the steward trail | an Export event (DICOM `110106`) |

Each record names the patient as its subject and, for a search, carries the
raw request base64-encoded in `entity.query` (BALP 1.1.4 §1:52.4.1.4,
§1:52.4.1.6): the URL of a `GET`, or the request line, media type and body of
a `POST`. A search or a match that answers several people writes one record
per person in the answer, and one that answers none writes one record naming
no patient; an ITI-78 read writes the PDQm Query Supplier record of the one
person it answers. Every face stores its records before it sends the answer:
the FHIR face answers `503` with a `transient` issue when they cannot be
stored, the HL7 v2 face `AR`, and the XCPD gateway Case 5. The audit record is the only place outside the store where the
server writes a patient identifier.

The records wait in a spool under `audit.spool_dir`, one file readable by
the server's user alone per record in a directory only that user may enter,
and are sent to `audit.endpoint.url` over the FHIR Feed. A delivered record is
removed; one the repository refuses is moved to the `quarantine`
subdirectory for you to inspect. Before a transaction runs, the server
takes its record's place in the spool: when the spool is full, the request
is refused with `503` and a `transient` issue before it changes anything.
`GET {base}/health/readiness` reports `audit_spool` down when less than a
tenth of either bound is left, and `GET {base}/operator/dependencies` shows
whether the last delivery to the repository went through.

## The crates' audit trail

The audit trail is [`ihe-atna`](https://crates.io/crates/ihe-atna), the
family's audit crate from FerroAUDIT: ATNA ITI-20 with its syslog sender,
spool and forwarder, and the BALP FHIR feed. `ihe-iti` writes the record
each profile defines for its own transactions on it:

- **`balp`:** every PIXm, PDQm, mCSD and PMIR client takes an
  `ihe_atna::balp::AuditRecorder` through `.audited(…)` and records the BALP
  `AuditEvent` of each transaction.
- **`atna`:** `XcpdClient::audited` hands the ITI-55 Initiating Gateway audit
  message of every exchange to a recorder, written as the DICOM audit message
  in `ihe-atna`'s ATNA model.

The client waits for the recorder before it returns the answer, the
exchange's timeout bounds the record too, and a record the recorder refuses
fails the exchange as an audit failure, so no answer is used without its
audit. An audited exchange's record names the user it was made for.

## Reporting a vulnerability

Report a vulnerability privately through GitHub's
[private vulnerability reporting](https://github.com/FerroHEALTH/FerroPIX/security/advisories/new),
never in a public issue. The supported versions and the response times are
in
[`SECURITY.md`](https://github.com/FerroHEALTH/FerroPIX/blob/main/SECURITY.md),
and the manufacturer's vulnerability handling, disclosure and advisories are
in the family book's [security page](https://ferrohealth.eu/docs/security.html).
