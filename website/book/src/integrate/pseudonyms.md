<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Pseudonyms

A clinical store such as FerroEHR names a patient by a value that identifies
no one on its own: a UUID per patient and pseudonymisation domain. FerroPIX
mints that pseudonym and keeps it, so identifying data stays outside the
clinical record.

## Pseudonymisation domains

A pseudonymisation domain is a namespace: an absolute URI, the system the
pseudonyms carry. It names the one client its pseudonyms are minted for, the
domain's consumer. Declare one in the configuration:

```toml
[[pseudonym_domain]]
namespace = "urn:oid:2.999.7.1"
name = "Clinical domain"
consumer_client_id = "ferroehr"
```

The server adds a domain the store lacks at every start and leaves one it
holds as it is ([Configuration](../operate/configuration.md)). An
administrator adds, renames, gives another consumer or retires a domain
through the operator API ([The operator API](../operate/operator-api.md)).
A namespace that an identifier domain holds is refused. A retired domain
mints nothing and answers as unknown.

## Minting on the first ask

The consumer asks for a person's pseudonym through PIXm ITI-83, naming the
namespace as `targetSystem`:

```text
GET {base}/fhir/Patient/$ihe-pix?sourceIdentifier=urn:oid:2.999.1.1|H-1&targetSystem=urn:oid:2.999.7.1
```

When the person has no pseudonym in the domain yet, FerroPIX mints one in
that request and answers it as a `targetIdentifier` whose system is the
namespace. Every later ask answers the same value, by `GET` and by `POST`.
Two first asks at once answer one value. Each mint writes its own BALP
Create record, which names the patient and the domain and never the value.

These calls never mint:

- **Another client** that names the namespace gets the pseudonym already
  minted, or none.
- **A query that names no `targetSystem`** answers the pseudonyms the
  person already has.

A steward can also mint through `POST {base}/operator/pseudonyms`.

## Merges

A pseudonym never changes and stays attached to the person it was minted
for. While a person is merged, its identifiers answer the survivor's
pseudonym, and its own pseudonym resolves to the survivor. After the merge
is reverted, each person answers its own pseudonym again.

## The reverse resolution

From a pseudonym back to a person is a steward's action:
`POST {base}/operator/pseudonyms/resolve` with the namespace, the pseudonym
and a reason. It answers the person the pseudonym resolves to and is
recorded as a steward action and a BALP Privacy Disclosure. PIXm does not
resolve a pseudonym: a `sourceIdentifier` in a pseudonymisation domain
answers `400`, "sourceIdentifier Assigning Authority not found".

## Why the clinical domain never resolves back

A pseudonym is pseudonymised data in the sense of the GDPR: it can no
longer be attributed to a person "without the use of additional
information, provided that such additional information is kept
separately" (Regulation (EU) 2016/679, Art. 4(5)). FerroPIX is where that
additional information is kept. A clinical store holds only the pseudonym
of its own domain: it translates a real identifier forward through ITI-83
with its namespace as `targetSystem`, and it has no call that translates a
pseudonym back. The one way back is the steward's reverse resolution above,
with a reason and a disclosure record, which keeps the separation the
GDPR asks for when pseudonymisation is used as a measure of data
protection by design (Art. 25(1)).

## The console

The console's Pseudonymisation domains page lists the domains with their
counts and is the one place a steward resolves a pseudonym: the domain,
the pseudonym and a reason, answered with the patient it belongs to. An
administrator adds, renames or retires a domain there. A patient's
Pseudonyms tab shows each domain and whether the patient has a pseudonym
in it, masked, with its audited reveal
([The operator console](../operate/console.md)).

## The Dutch identifiers

The pseudonymised BSN of the Dutch Generic Functions is an identifier
domain of its own, not a pseudonymisation domain
([The Dutch identifiers](dutch-identifiers.md)).
