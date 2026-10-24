<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The operator API

The operator API is what the operator console reads: the dashboard, the two
searches, a person with everything the index holds about them, the
identifier domains, the matching configuration and the audit trail. It is
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
| `GET /operator/domains` | steward or administrator | A page of identifier domains by system, with their record count and last feed. |
| `GET /operator/domains/{id}` | steward or administrator | One domain with its last 20 feeds: time, channel, and whether the feed created or revised a record. |
| `GET /operator/matching` | steward or administrator | The thresholds, automatic linking, the blocking keys and every field's weights in effect, the configured ones marked. |
| `GET /operator/actions` | steward | A page of the audit trail, the newest first. |
| `POST /operator/actions/export` | steward | The same filter answered as CSV. |

The reveal of one identifier and the writes of identifier domains are
planned in [#90](https://github.com/FerroHEALTH/FerroPIX/issues/90); the
review queue, the decisions, the merges and the pseudonymisation domains
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
the operator API adds `view` and `export`.

`POST /operator/actions/export` takes the same filter as a JSON body and
answers `text/csv` as an attachment. A cell a spreadsheet would read as a
formula starts with `'`.

## What each route records

| Route | Steward action | BALP record |
|---|---|---|
| `GET /operator/persons/{id}` | `view` | Patient Read |
| either search | none | one Patient Query record per person found, or one Query record when none was, with the raw request |
| `POST /operator/actions/export` | `export` | an Export event |

The server stores the BALP record before it sends the answer, and answers
`503` with a `transient` issue when it cannot. A search writes no steward
action: its BALP record is the trail. The dashboard, the domains, the
matching configuration and the trail name no patient and record nothing.
