<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The PDQm Supplier

FerroPIX serves the IHE PDQm 3.2.0 Patient Demographics Supplier, with the
Match Operation Option, under `{base}/fhir` in FHIR R4 JSON and XML: ITI-78
finds patients by their demographics and identifiers and reads one by its
id, and ITI-119 `$match` scores a patient you describe against every patient
the cross-reference holds. `[pdqm] enabled` switches both on (the default)
and `[pdqm] deprecated` chooses how a merged patient is answered
([Configuration](../operate/configuration.md)).

Every request needs an access token or, on a development surface, a Basic
user ([Security and audit](../operate/security-and-audit.md)). A Consumer
holds `system/Patient.rs`, or the IUA scope `ITI-78` for the search and the
read and `ITI-119` for `$match`. A read of one patient needs the `r`
permission, a search the `s` permission.

## The Patient you get back

Each `Patient` is a person of the cross-reference, the same resource the
PIXm Manager names in `targetId`:

- `id` is the person's id, and the first identifier is that id in the
  Master Patient Identification Domain, whose system is
  `[server] public_base_url`.
- The other identifiers are the identifier of every record a Source fed for
  the person, each in its domain's system, opened from its sealed form for
  the answer alone.
- The demographics are the current version of one of the person's records:
  a record of an `authoritative` domain first, then the most recently
  received.
- A merged person reads `active = false`, carries no demographics and has a
  `link` of type `replaced-by` to the person it was merged into (FHIR R4
  Patient, "Patient records that have been merged").
- The mother's maiden name, when a Source sent it, stays in the
  `patient-mothersMaidenName` extension (PDQm 3.2.0 §2:3.78.4.2.2.2).

Every `Patient` claims the PDQm Patient profile in `meta.profile`.

## Search (ITI-78)

`GET {base}/fhir/Patient?…` and `POST {base}/fhir/Patient/_search` answer
alike (§2:3.78.4.1.2). A `POST` sends the parameters as an
`application/x-www-form-urlencoded` body, which keeps them out of the URL;
parameters in its URL join those of the body.

| Parameter | Matches |
|---|---|
| `_id` | the person's id, exactly |
| `active` | `true` for an active person, `false` for a merged one |
| `family`, `given` | a name part that starts with the value |
| `identifier` | `system\|value` in that domain, or `value` in any domain, found by the value's HMAC; the master domain's value is the person's id |
| `telecom` | a contact point's value, in the `ContactPoint.system` the token names |
| `birthdate` | the birth date, with the FHIR prefixes `eq`, `ne`, `gt`, `lt`, `ge`, `le`, `sa`, `eb` and `ap`, each value at its own precision |
| `address` | any part of an address that starts with the value: a line, the city, the district, the state, the postal code, the country or the text |
| `address-city`, `address-country`, `address-postalcode`, `address-state` | that part of an address |
| `gender` | one administrative gender code |
| `mothersMaidenName` | the mother's maiden name extension |

A string parameter matches a value that starts with yours once both are
folded to their base letters and lowercased, so `proef` finds `Próefsma`;
`:exact` matches the value as written (§2:3.78.4.1.2.2). Repeating a
parameter asks for both values (AND) and a comma between values for either
(OR). Every combination of parameters is supported, the two PDQm requires
included (§2:3.78.4.1.2.1.1). A parameter FerroPIX does not read, or another
modifier, is ignored and left out of the `self` link, which names exactly
what the search respected.

A search reads the demographics each `Patient` shows. A merged person holds
no records, since a merge moves them to the survivor, so only `_id` and
`active` find it.

### The domain filter

`identifier=system|` with no value names a domain whose identifiers you
want back, and repeats or takes a comma for several (§2:3.78.4.1.2.3). Every
returned `Patient` then holds only the identifiers of those domains.

### Paging

`_count` sets the page size: 20 when you name none, at most 100 whatever
you ask, and `0` answers only the `total`. The `searchset` Bundle carries
the `total` of every match, a `self` and a `first` link, and a `next` link
while matches follow. Pages run in the order of the persons' ids and the
`next` link names the last person of the page, so a person created or
merged while you page does not move another person onto two pages or off
all of them. Follow the links as the server writes them; the paging
parameter is FerroPIX's own.

### The answers

| Case | Answer |
|---|---|
| matches found | `200`, a `searchset` Bundle with its `total` and one `match` entry per person (§2:3.78.4.1.3, Case 1) |
| a domain filter of known domains | `200`, each `Patient` holding only those domains' identifiers (Case 2) |
| no match | `200`, `total` 0 and no entry (Case 3) |
| a domain filter naming a domain FerroPIX does not hold | `404`, `warning`, `not-found`, "targetSystem not found" (Case 4, the preferred response) |
| a `_format` or `Accept` that names neither JSON nor XML | `406`, `not-supported` (Case 5) |
| a merged person matched | `[pdqm] deprecated = "inactive"` (the default): the person with `active = false` and its `replaced-by` link, the survivor included as an `include` entry; `"omit"`: the person left out (Case 6) |
| a malformed value: an empty one, a gender that is no code, a date that is no FHIR date | `400`, with the issue naming the fault |

## Read one patient (ITI-78)

`GET {base}/fhir/Patient/{id}` answers `200` with the `Patient`, or `404`
with an `error` `not-found` issue when no person has the id
(§2:3.78.4.3.3). A merged person answers `200` with `active = false` under
`"inactive"` and `404` under `"omit"` (Case 3). A format the read cannot
write answers `400` (§2:3.78.4.4.2).

## Match (ITI-119)

`POST {base}/fhir/Patient/$match` takes the PDQm Match Input `Parameters`,
with the `Patient` in `resource` and the optional `onlyCertainMatches` and
`count`, or, when you use neither, the bare `Patient` (§2:3.119.4.1.2). The
mother's maiden name goes in its extension on that `Patient`
(§2:3.119.4.1.2.1).

FerroPIX reads your `Patient` as if a Source had fed it, finds the records
that share a blocking key with it, scores each with the scorer of the
[Matching](../operate/matching.md) page, and grades each person by the best
score of its records. Your `Patient`'s identifiers in `local` domains are
compared by their HMAC. The answer is the PDQm Match Output Bundle:

- one entry per person, ordered from the most to the least likely, each with
  `search.score` in `[0, 1]` and the `match-grade` extension, `certain`,
  `probable` or `possible`, from the `[matching]` thresholds
  (§2:3.119.4.2.2.4); a person graded `certainly-not` is no match and is
  left out;
- with `onlyCertainMatches`, only `certain` entries, and none at all when
  more than one person is certain (Cases 3 to 5);
- with `count`, at most that many entries (Case 6);
- a `warning` `OperationOutcome` entry when your `Patient` states fewer than
  three of the scorer's fields, so the match rests on little (Case 10).

| Case | Answer |
|---|---|
| one or more matches | `200`, the entries as above (Cases 1, 2, 3 and 6) |
| one match that is not `certain`, with `onlyCertainMatches` | `200`, no entry (Case 4) |
| several `certain` persons, with `onlyCertainMatches` | `200`, no entry (Case 5) |
| no match | `200`, no `Patient` entry (Case 7) |
| a merged person | never returned: it holds no records, so its survivor is matched (Case 8) |
| a body that is no Match Input: no `Patient`, an unknown parameter, a bare `Patient` with `count` in the query | `400`, with the issue naming the fault (Case 9) |
| a body that is neither FHIR JSON nor FHIR XML | `415`, `not-supported` |
| a `_format` or `Accept` that names neither JSON nor XML | `406`, `not-supported` |

## Every route

- `_format` (`json`, `xml` or a FHIR media type) chooses the answer's
  encoding over `Accept`.
- A store that cannot be reached answers `503` with a `transient` issue,
  and so does a request whose audit record has no room in the spool.
- Every search, read and match writes the PDQm Supplier BALP record, one per
  person in the answer, or one naming no patient when the answer names none,
  before the answer is sent
  ([Security and audit](../operate/security-and-audit.md#the-servers-audit-trail)).
  The request log records the matched route and never the query, the body,
  a name or an identifier ([Operations](../operate/operations.md)).

`GET {base}/fhir/metadata` declares the Supplier on the one `Patient` entry
of its `CapabilityStatement`: it instantiates the PDQm Supplier statement
and the statement of the Match Operation Option, and names the PDQm Patient
profile, the `read` and `search-type` interactions, every search parameter
above with its `:exact` form, and the `match` operation. `_lastUpdated`,
which the PDQm Supplier statement lists, is not read: the person has no
version of its own to date.
