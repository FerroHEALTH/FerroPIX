<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The operator API

The operator API is what the operator console reads: the dashboard, the two
searches, a person with everything the index holds about them, the
identifier domains, the sources, the matching configuration and the audit
trail. It is
mounted under `{base}/operator` when `[operator] enabled = true`, and every
route needs a bearer token with a product scope
([Security and audit](security-and-audit.md)). No specification governs the
API; its shape is FerroPIX's own design.

The documents are JSON, defined once in the `ferropix-operator-api` crate
that the server writes and the console reads, so the two cannot drift apart.
Every answer carries `Cache-Control: no-store`. A refusal is a FHIR
`OperationOutcome` whose diagnostics never quote the request.

## Routes

| Route | Scope | Answers |
|---|---|---|
| `GET /operator/overview` | steward | The dashboard: the open queue, the records created and linked in the last 24 hours, the versions fed today, the subscriptions in error, the queue depth over 30 days, the feeds per hour by channel over 24 hours and the five oldest candidates. |
| `GET /operator/dependencies` | steward | The readiness document with the last outcome of every upstream ([Operations](operations.md)). |
| `POST /operator/search/identifier` | steward | The persons one identifier is cross-referenced to. |
| `POST /operator/search/demographics` | steward | The persons the scorer finds for the fields you know, the most likely first, each with its score and grade. |
| `GET /operator/persons/{id}` | steward | One person: the identifiers per domain, masked; the source records with their demographics; the history; the pseudonyms, masked. |
| `POST /operator/persons/{id}/reveal` | steward | One identifier or pseudonym of the person unmasked, with your reason: a privacy disclosure. |
| `GET /operator/domains` | steward or administrator | A page of identifier domains by system, with their record count and last feed. |
| `GET /operator/domains/{id}` | steward or administrator | One domain with its last 20 feeds: time, channel, and whether the feed created or revised a record. |
| `POST /operator/domains` | administrator | A new identifier domain. |
| `PATCH /operator/domains/{id}` | administrator | A domain renamed, given another Source, or retired. |
| `GET /operator/sources` | steward or administrator | Every system allowed to feed: each FHIR client an issuer lists in its `clients`, each Basic user of the development profile and each `[[hl7v2.senders]]` entry, with its channel, how it is admitted, the domains that name it and its last activity. |
| `GET /operator/sources/{id}` | steward or administrator | One source with its last 20 messages: time, channel, what it asked and its outcome (`applied`, or the HL7 v2 acknowledgment code). An HL7 v2 sender's id is its facility and application joined by `.`. |
| `GET /operator/matching` | steward or administrator | The thresholds, automatic linking, the blocking keys and every field's weights in effect, the configured ones marked. |
| `GET /operator/configuration` | steward or administrator | The matching configuration in file mode: the thresholds, the weights, the blocking keys and the built-in deterministic rules, each setting with where its value came from (`file`, `environment`, `default`, `built_in`) and the file named once. |
| `GET /operator/actions` | steward | A page of the audit trail, the newest first. |
| `POST /operator/actions/export` | steward | The same filter answered as CSV. |

The review queue, the decisions, the merges and the pseudonymisation domains
follow with the [v0.0.5 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/5).
Until then those routes answer `501`.

## Identifiers stay off the URL and out of the answer

A search that carries an identifier is a `POST` with a JSON body, so the
value never sits in a URL, a proxy log or a browser history; a `GET` on a
search route answers `405`. A body is read only as `application/json`
(`415` otherwise), and a body that is not the route's document answers `400`
without saying which value failed.

An identifier in an answer is masked: a `•` for every character but the last
three, and never more than half of the value shown (`••••••123`). The server
opens a sealed identifier only to mask it, and the unmasked value does not
leave the server. The one identifier a URL carries is the person id FerroPIX
minted.

## Revealing an identifier

`POST /operator/persons/{id}/reveal` takes the handle the person document
gave the identifier or pseudonym, your reason and how long the console
should show the value:

```json
{"target": {"record": "0192..."}, "reason": "the patient asked at the desk", "seconds": 15}
```

The target is `{"record": id}` for a record's own identifier, `{"carried":
id}` for an authoritative identifier the person's records carry, or
`{"pseudonym": domain-id}` for the person's pseudonym in a
pseudonymisation domain. It must belong to the person: any other answers
`404`. The reason is required, at most 500 characters (`400` otherwise).

A reveal is a privacy disclosure. The server writes a `reveal` steward
action, naming you, the person, the record and the domain but never the
value, and a BALP Privacy Disclosure record (BALP 1.1.4 §3:5.7.7) with the
person as its subject and your reason as a detail, in one transaction.
Only once both are stored does it answer the value, once, with
`Cache-Control: no-store`, beside the domain's system and the seconds to
show it: 15 by default, never more than 30. Looking again is another
reveal. The value never reaches a log line.

## Writing identifier domains

Behind the administrator scope, `POST /operator/domains` adds a domain:

```json
{"system": "urn:oid:2.999.1.9", "name": "Clinic", "kind": "local", "source_client_id": "clinic-feed", "reason": "a new clinic"}
```

The system is an absolute URI no other domain holds (`409` otherwise), the
kind is `local` or `authoritative`, `sensitive` defaults to `true`, and the
Source must be a client an `[[auth.issuer]]` lists in its `clients`, or a
Basic user (`422` otherwise). The answer is `201` with the domain.

`PATCH /operator/domains/{id}` takes `name`, `source_client_id` and
`retire`, any of them, and a required `reason`. A retired domain answers as
unknown everywhere and stays retired: a change to it answers `409`. Each
write is a `domain-create` or `domain-update` steward action with a BALP
record, and the guard takes the new Source the moment the write commits:
the new Source can feed at once, and the old one is refused.

## Searching

`POST /operator/search/identifier` takes `{"system": "...", "value": "..."}`.
The value is normalised by its domain's rule and found by its HMAC, as a PIX
query finds it, so no sealed value is opened to search. An authoritative
identifier that no Source feeds, such as a BSN the records carry, finds the
persons whose current records carry it; when two persons carry it, both are
listed. A domain the index does not know, or has retired, answers `422`.

`POST /operator/search/demographics` takes any of `family`, `given`,
`birth_date`, `gender` and `postal_code`, at least one, and runs the scorer
over the blocking keys as `$match` does ([Matching](matching.md)).

Both take `offset` and `limit` (20 by default, at most 50) and answer
`total`, `offset`, `next` when more persons follow, and the persons of the
page.

## Paging

Every list is bounded. The domains page by system: pass the `next` of one
page as `after` for the following one (`limit` 50 by default, at most 200).
The trail pages the same way by action id (50 by default, at most 200).
An export answers at most 10,000 actions and, when more follow, names the
`after` of the rest in its `x-ferropix-next` header.

## The audit trail

`GET /operator/actions` filters by `actor`, `kind`, `person` (a person id),
`from` and `to` (RFC 3339 instants), each in the query; the actor is a
steward or a Source's client id, never a patient value. Each action names
its kind, who acted, when, the person, record, candidate or merge it
concerns, the reason given and the request id. The kinds the feed records
are `feed`, `link`, `merge`, `candidate`, `remove` and `resolve-duplicate`;
the operator API adds `view`, `reveal`, `export`, `domain-create` and
`domain-update`. An action names the identifier domain it wrote or whose
identifier it revealed, and the pseudonymisation domain whose pseudonym it
revealed.

`POST /operator/actions/export` takes the same filter as a JSON body and
answers `text/csv` as an attachment. A cell a spreadsheet would read as a
formula starts with `'`.

## What each route records

| Route | Steward action | BALP record |
|---|---|---|
| `GET /operator/persons/{id}` | `view` | Patient Read |
| either search | none | one Patient Query record per person found, or one Query record when none was, with the raw request |
| `POST /operator/persons/{id}/reveal` | `reveal` | Privacy Disclosure at the source |
| `POST /operator/actions/export` | `export` | an Export event |
| `POST` and `PATCH /operator/domains` | `domain-create`, `domain-update` | a create or update of the domain, naming no patient |

The server stores the BALP record before it sends the answer, and answers
`503` with a `transient` issue when it cannot. A search writes no steward
action: its BALP record is the trail. The dashboard, the domain and source
reads, the matching configuration and the trail name no patient and record
nothing.

## Sources and configuration are set in the file

FerroPIX keeps no configuration store. A source is admitted by the file:
an issuer's `clients`, a Basic user, or an `[[hl7v2.senders]]` entry. The
matching settings are read from `[matching]` at start. So the operator API
shows both read-only, and you change them in the file; a change takes effect
at the next start. An environment variable `FERROPIX__…` wins over the file,
and the configuration document says which setting it set.
