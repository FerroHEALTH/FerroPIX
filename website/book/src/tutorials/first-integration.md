<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Your first integration

**Audience:** integrators who connect a hospital system to FerroPIX for the
first time, as its Patient Identity Source and as a consumer of the
cross-reference.

In this tutorial you act as a hospital that registers the same patient
twice by mistake. You feed both records, watch FerroPIX link them through
the patient's BSN, resolve the duplicate as the hospital, and ask for the
patient by identifier and by demographics. Along the way you see the answer
to a BSN that fails its check.

A test runs every request on this page against the server and PostgreSQL in
CI, so what you read here is what the server answers.

<!-- toc -->

## Before you start

Run the [quickstart](../integrate/getting-started.md) up to "Get a token":
its stack is the server you talk to here, with three identifier domains:

- `urn:oid:2.999.1.1`, the synthetic hospital, a `local` domain;
- `http://fhir.nl/fhir/NamingSystem/bsn`, the BSN, an `authoritative`
  domain;
- `http://fhir.nl/fhir/NamingSystem/pseudo-bsn`, the pseudonymised BSN.

One client, `quickstart-source`, is the Patient Identity Source of all
three. Every value on this page is synthetic: the BSNs come from a range no
person holds.

## 1. Get a token as the hospital's Source

A Source is a machine, so it asks the issuer for a client credentials token
(RFC 6749 §4.4). The scopes let it feed (`system/Patient.cud`) and query
(`system/Patient.rs`):

```sh
{{#include ../../../../scripts/quickstart/tour.sh:token}}
```

Each request below is written as the HTTP message you send. With `curl`,
send one like this, the body saved in `body.json`:

```sh
curl -s -X PUT 'http://localhost:8080/fhir/Patient?identifier=urn:oid:2.999.1.1|H-200001' \
  -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/fhir+json' \
  --data-binary @body.json
```

## 2. Register the patient

The hospital registers Mila de Vries as `H-200001`, with her BSN. A
conditional update on the hospital's identifier is the ITI-104 Add or
Revise Patient (PIXm 3.1.0 §2:3.104.4.1):

<!-- book-request: register expect=201 holds=/resourceType=OperationOutcome holds=/issue/0/severity=information -->

```http
PUT /fhir/Patient?identifier=urn:oid:2.999.1.1|H-200001 HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Patient",
  "identifier": [
    { "system": "urn:oid:2.999.1.1", "value": "H-200001" },
    { "system": "http://fhir.nl/fhir/NamingSystem/bsn", "value": "999990019" }
  ],
  "name": [{ "family": "de Vries", "given": ["Mila"] }],
  "gender": "female",
  "birthDate": "1987-09-23"
}
```

FerroPIX answers `201 Created`: the identifier is new, and the record is a
new person. Its `Location` names that person.

## 3. Register her again by mistake

At another desk, the hospital registers her a second time as `H-200002`,
with the same BSN:

<!-- book-request: register-again expect=201 holds=/issue/0/severity=information -->

```http
PUT /fhir/Patient?identifier=urn:oid:2.999.1.1|H-200002 HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Patient",
  "identifier": [
    { "system": "urn:oid:2.999.1.1", "value": "H-200002" },
    { "system": "http://fhir.nl/fhir/NamingSystem/bsn", "value": "999990019" }
  ],
  "name": [{ "family": "de Vries", "given": ["Mila", "Johanna"] }],
  "gender": "female",
  "birthDate": "1987-09-23"
}
```

The identifier `H-200002` is new, so this is `201` too. The BSN is in an
`authoritative` domain, so FerroPIX links the two records to one person in
the same transaction, with no steward involved
([Matching](../operate/matching.md)).

## 4. Ask which hospital numbers the BSN has

A consumer that knows only the BSN asks the PIXm Manager for the hospital's
identifiers (ITI-83). It posts the parameters, so the BSN sits in no URL:

<!-- book-request: by-bsn expect=200 holds=/resourceType=Parameters contains=H-200001 contains=H-200002 -->

```http
POST /fhir/Patient/$ihe-pix HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Parameters",
  "parameter": [
    { "name": "sourceIdentifier", "valueString": "http://fhir.nl/fhir/NamingSystem/bsn|999990019" },
    { "name": "targetSystem", "valueString": "urn:oid:2.999.1.1" }
  ]
}
```

The answer holds one `targetIdentifier` for each hospital number, `H-200001`
and `H-200002`, and one `targetId` naming the person
(PIXm 3.1.0 §2:3.83.4.2.2.1). The BSN was never fed as a record of its own:
FerroPIX answers by it because the hospital's records carry it.

## 5. Resolve the duplicate

The hospital finds the double registration and keeps `H-200001`. It tells
FerroPIX with the same conditional update on `H-200002`, now inactive and
with a `replaced-by` link to the survivor, the ITI-104 Resolve Duplicate
Patient (PIXm 3.1.0 §2:3.104.4.2):

<!-- book-request: resolve expect=200 holds=/issue/0/severity=information -->

```http
PUT /fhir/Patient?identifier=urn:oid:2.999.1.1|H-200002 HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Patient",
  "identifier": [
    { "system": "urn:oid:2.999.1.1", "value": "H-200002" }
  ],
  "active": false,
  "name": [{ "family": "de Vries", "given": ["Mila", "Johanna"] }],
  "gender": "female",
  "birthDate": "1987-09-23",
  "link": [
    {
      "other": { "identifier": { "system": "urn:oid:2.999.1.1", "value": "H-200001" } },
      "type": "replaced-by"
    }
  ]
}
```

FerroPIX answers `200`: `H-200002` is known, and from now on it stands for
the survivor.

## 6. Ask by the subsumed number

A system that still holds the old number `H-200002` asks for the patient's
identifiers in every domain, naming no `targetSystem`:

<!-- book-request: subsumed expect=200 contains=H-200001 contains=999990019 -->

```http
POST /fhir/Patient/$ihe-pix HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Parameters",
  "parameter": [
    { "name": "sourceIdentifier", "valueString": "urn:oid:2.999.1.1|H-200002" }
  ]
}
```

The answer is the survivor's identifiers, `H-200001` and the BSN
(PIXm 3.1.0 §2:3.104.4.2.3), so the old number keeps working while the
hospital's other systems catch up.

## 7. Find her by demographics

A consumer that has no identifier at all describes the patient and asks
the PDQm Supplier to score her against everyone FerroPIX holds, the ITI-119
`$match` (PDQm 3.2.0 §2:3.119):

<!-- book-request: match expect=200 holds=/resourceType=Bundle holds=/type=searchset contains=certain contains=H-200001 -->

```http
POST /fhir/Patient/$match HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Parameters",
  "parameter": [
    {
      "name": "resource",
      "resource": {
        "resourceType": "Patient",
        "name": [{ "family": "de Vries", "given": ["Mila"] }],
        "gender": "female",
        "birthDate": "1987-09-23"
      }
    }
  ]
}
```

The answer is a `searchset` Bundle with one entry per person, most likely
first. Her entry carries a `search.score` and the `match-grade` extension
`certain`: the family name, the given name, the birth date and the gender
all agree. Each grade is explained on the [Matching](../operate/matching.md)
page.

## 8. Send a BSN that fails its check

A BSN passes the eleven-proof. The hospital mistypes one digit for a new
patient:

<!-- book-request: bad-bsn expect=422 holds=/resourceType=OperationOutcome holds=/issue/0/code=business-rule lacks=999990018 -->

```http
PUT /fhir/Patient?identifier=urn:oid:2.999.1.1|H-200003 HTTP/1.1
Host: localhost:8080
Authorization: Bearer $token
Content-Type: application/fhir+json

{
  "resourceType": "Patient",
  "identifier": [
    { "system": "urn:oid:2.999.1.1", "value": "H-200003" },
    { "system": "http://fhir.nl/fhir/NamingSystem/bsn", "value": "999990018" }
  ],
  "name": [{ "family": "Bakker", "given": ["Sem"] }],
  "gender": "male",
  "birthDate": "2001-02-14"
}
```

FerroPIX refuses the whole feed with `422` and a `business-rule` issue, and
stores nothing: a wrong BSN would link the record to a stranger. The issue
names the rule and never the value you sent.

## What you did

- You fed two records as a domain's one Patient Identity Source, and
  FerroPIX linked them through an `authoritative` identifier.
- You resolved a duplicate the way the Source does, and saw the old number
  answer for the survivor.
- You found the patient by an identifier she was never fed under, and by
  her demographics alone.

[How FerroPIX manages patient identity](../explanation/patient-identity.md)
explains the model behind each step. The pages under Integrate list every
case of each transaction, and [Security and audit](../operate/security-and-audit.md)
says which scopes each caller needs.
