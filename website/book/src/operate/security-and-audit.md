<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Security

**Audience:** operators and security reviewers.

A patient index handles the most identifying data in a health system, so
the server and both crates are built to keep it out of anywhere it was not
sent. This page says who may call the server and how identifying data is kept
out of logs, errors and URLs; [The audit trail](audit-trail.md) says what
FerroPIX records.

## Who may call the server

Every request to `{base}/fhir` and `{base}/operator` carries a bearer access
token that one of the `[auth.issuer.<name>]` issuers signed. At start, `serve`
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
(RFC 6749 §4.4), so its `sub` is its `client_id` (RFC 9068 §2.2). A
person's token through the same client is refused. To upgrade a Source that fed with a
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
`[auth.issuer.<name>]` issuer.

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

## Post-quantum key exchange

The server and console listeners, every client FerroPIX builds and the
PostgreSQL connection offer the hybrid group X25519MLKEM768 (RFC 10024)
first in the TLS 1.3 key exchange, then X25519, P-256 and P-384 for a peer
that lacks it. A session recorded today stays confidential against a later
quantum computer, which a patient's identity needs: the EU coordinated
implementation roadmap asks a high-risk use to complete the move by the end
of 2030. Certificates and token signatures stay classical until their
post-quantum forms are in use. The
[post-quantum inventory](https://github.com/FerroHEALTH/FerroPIX/blob/main/docs/post-quantum.md)
lists every cryptographic use with its algorithm and whether it is safe.

## Reporting a vulnerability

Report a vulnerability privately through GitHub's
[private vulnerability reporting](https://github.com/FerroHEALTH/FerroPIX/security/advisories/new),
never in a public issue. The supported versions and the response times are
in
[`SECURITY.md`](https://github.com/FerroHEALTH/FerroPIX/blob/main/SECURITY.md),
and the manufacturer's vulnerability handling, disclosure and advisories are
in the family book's [security page](https://ferrohealth.eu/docs/security.html).
