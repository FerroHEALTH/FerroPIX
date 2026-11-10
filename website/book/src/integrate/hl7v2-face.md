<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The HL7 v2 face

FerroPIX answers the HL7 v2 PIX and PDQ transactions on one MLLP listener,
beside the FHIR face and on the same cross-reference: ITI-8 Patient
Identity Feed, ITI-9 PIX Query and ITI-21 Patient Demographics Query, as
the IHE ITI Technical Framework Volume 2 texts prescribe them. A v2 feed
and a FHIR feed of the same patient land in one person, and a v2 query and
a FHIR query answer the same cross-reference. The face reads messages with
FerroBRIDGE's HL7 v2 parser and MLLP codec, so the family has one parser.

Turn it on with `[hl7v2] enabled = true` and list every sender
([Configuration](../operate/configuration.md#hl7v2)):

```toml
[hl7v2]
enabled = true
listen = "0.0.0.0:2575"

[[hl7v2.senders]]
application = "ADT"
facility = "EAST"
domain = "urn:oid:2.999.1.1"

[[hl7v2.senders]]
application = "PORTAL"
facility = "NORTH"

[hl7v2.namespaces]
"urn:oid:2.999.1.1" = "HOSP"
```

## The listener

- **Framing:** MLLP Release 1, one message per frame. A frame that does not
  open with the start block, holds a start block inside it, runs past
  `frame_limit` or does not complete within `frame_timeout` is
  answered `AR` when its header can be read, and the connection closes. A
  connection idle past `idle_timeout` closes.
- **Character sets:** MSH-18 names the set a message is read in; a message
  with an empty MSH-18 is read in `default_charset`. The answer is written
  in the set the message came in, by `ferrobridge-hl7v2`'s writer: the
  acknowledgment by `ack::render`, and the RSP^K23 checked against its
  structure before it leaves, so a response that does not conform is
  answered `AE` instead.
- **Senders:** each `[[hl7v2.senders]]` entry names a sending application
  (MSH-3) and facility (MSH-4), matched by namespace id or universal id.
  A message from a sender not listed is answered `AR` and not applied. A
  sender with a `domain` is that domain's one Patient Identity Source and
  may feed it; a sender without one may only query.
- **Assigning authorities:** a domain is the `system` of a FHIR identifier.
  An assigning authority with a universal id of type `ISO` names the domain
  `urn:oid:<oid>`, one of type `URI` names the domain by that URI, and a
  namespace id names the domain `[hl7v2.namespaces]` gives it. Every
  identifier in an answer carries all three subcomponents; the namespace id
  is empty for a domain the configuration names none for.
- **The drain:** on `SIGTERM` the listener stops accepting, finishes the
  messages it is answering within `[server] shutdown_timeout`, and closes.
  Readiness carries the `hl7v2_listener` indicator, down once the listener
  stops accepting.

No byte of a message reaches a log line: the server logs each message by
MSH-10, its type and the acknowledgment code.

## ITI-8, the identity feed

ADT^A01, A04, A05 and A08 add or revise a patient, and ADT^A40 merges the
MRG-1 identifier into the PID-3 identifier. The identifier fed is the PID-3
repetition in the sender's domain; a repetition with no assigning authority
takes the sender's domain. The other repetitions in domains FerroPIX holds,
a BSN among them, are carried as a FHIR feed carries them.

The face reads PID as the `Patient` a FHIR Source would send for the same
patient (name, birth date, gender, address, telecom and the mother's maiden
name, through the HL7 v2-to-FHIR structure maps that FerroBRIDGE's
`ferrobridge-hl7v2` is built with, every table value translated on the
[terminology server](#the-terminology-server)) and applies it through the same feed
transaction as ITI-104, so the version written, the deterministic match and
the merge are the same. An A40 is the Resolve Duplicate message: the
subsumed record is replaced by the survivor, its person merged into the
survivor's.

The acknowledgment is in original mode:

| Answer | When |
|---|---|
| `AA` | The message is applied. |
| `AE`, ERR `PID^1^3^1^4` code `204` | PID-3 carries no identifier in the sender's domain. |
| `AE`, ERR code `101` | PID-3 or MRG-1 is missing. |
| `AE`, ERR `MRG^1^1^1^4` code `204` | MRG-1 is not in the sender's domain. |
| `AE`, ERR `PID^1^7` code `102` | PID-7 is no date. |
| `AE`, ERR code `204` | An A40 names a survivor FerroPIX does not hold. |
| `AR`, ERR code `200` | Any other message type or trigger event. |
| `AR`, ERR code `207` | The store, the audit spool or the terminology server cannot take the message; send it again. |

A message sent again with the same MSH-10 from the same sender is
acknowledged with the code it got the first time and is not applied again.

## The terminology server

The face maps PID to a `Patient`, and a `Patient` back into the PID of an
ITI-21 answer, through the structure maps of the HL7 v2-to-FHIR 1.0.0 guide
(`hl7.fhir.uv.v2mappings`) with FerroBRIDGE's corrections, which
`ferrobridge-hl7v2` is built with. A table value (PID-8 by table 0001, the
name type by 0200, the address type by 0190, the telecommunication use and
equipment type by 0201 and 0202) is a code of a terminology, so the face
translates each one by `ConceptMap/$translate` on the FHIR R4 terminology
server `[terminology]` names, and holds no copy of a table. FerroTERM is
the family's terminology server.

Load three FHIR packages into it, as FerroBRIDGE's book says for its own
HL7 v2 face: HL7 Terminology (`hl7.terminology`, which holds the v2 tables'
code systems and the value sets the table maps name), the guide's package
`hl7.fhir.uv.v2mappings` 1.0.0 (both from the FHIR package registry), and
FerroBRIDGE's value package, `terminology/hl7v2-values/` of the FerroBRIDGE
release whose `ferrobridge-hl7v2` FerroPIX runs. For FerroTERM, name the
three directories in its `terminology.code_systems`:

```toml
[terminology]
code_systems = [
  "/srv/hl7.terminology/package",
  "/srv/hl7.fhir.uv.v2mappings/package",
  "/srv/ferrobridge-hl7v2-values",
]
```

Then point FerroPIX's [`[terminology.endpoint]`](../operate/configuration.md#terminology)
`url` at the server's R4 base. The server refuses to start with the HL7
v2 face on and no terminology server. A call the terminology server refuses
or does not answer is answered `AR`, never mapped without the value, so
the sender sends the message again; a code its map translates to nothing
is left out of the `Patient`.

## ITI-9, the PIX query

QBP^Q23 names one identifier in QPD-3 and, in QPD-4, the domains whose
identifiers it wants. The answer, RSP^K23, holds what ITI-83 answers for the
same identifier: one PID whose PID-3 lists every identifier of the person
except the one asked, filtered to the QPD-4 domains, with a pseudo-name in
PID-5. A merged identifier answers the survivor's identifiers and a removed
one is not found, as on the FHIR face.

| MSA-1, QAK-2 | When |
|---|---|
| `AA`, `OK` | Identifiers are found. |
| `AA`, `NF` | The person holds no identifier in the requested domains. |
| `AE`, `AE`, ERR `QPD^1^3^1^1` code `204` | The identifier is not known. |
| `AE`, `AE`, ERR `QPD^1^3^1^4` code `204` | The identifier's domain is not known. |
| `AE`, `AE`, ERR `QPD^1^4^n` code `204` | The domain of QPD-4 repetition `n` is not known. |

## ITI-21, the demographics query

QBP^Q22 names its criteria as QPD-3 parameters and the domains whose
identifiers it wants in QPD-8. Each parameter is searched as its ITI-78
parameter, so the persons found are the ones a PDQm search finds:

| QPD-3 field | ITI-78 parameter |
|---|---|
| `@PID.3.1` with `@PID.3.4` | `identifier` |
| `@PID.5.1.1` | `family` |
| `@PID.5.2` | `given` |
| `@PID.6.1.1` | `mothersMaidenName` |
| `@PID.7` | `birthdate` |
| `@PID.8` | `gender` |
| `@PID.11.1` to `@PID.11.6` | `address`, `address-city`, `address-state`, `address-postalcode`, `address-country` |
| `@PID.13.1` | `telecom` |
| `@PID.18.1` with `@PID.18.4` | `identifier` |

The answer, RSP^K22, holds one PID per person with its identifiers and
demographics, written back through the same maps and checked against
RSP_K21 as IHE ITI TF-2 Table 3.21-6 constrains it (QRI optional): an
answer that does not conform, such as a PID with no name, is answered `AE`.
A QPD-8 domain FerroPIX does not know answers `AE` with an ERR at
`QPD^1^8^n`, and a field FerroPIX does not search on answers `AE`. When
the terminology server does not translate, the query is answered `AR`.

RCP-2 (`2^RD`) asks for increments of that many records. An answer that
holds fewer than match ends with a DSC whose pointer you send back in a DSC
segment for the next increment. A pointer lives ten minutes, is used once,
and is given up by QCN^J01; a pointer unknown or past its lifetime answers
`AE` with an ERR at `DSC^1^1`.

## ITI-22, the demographics and visit query

FerroPIX keeps no visit data, so it does not answer ITI-22 (QBP^ZV1): the
transaction returns a PV1 segment for every patient and searches visit
fields. A QBP^ZV1 is acknowledged `AR` with ERR code `200`. Query with
ITI-21, which searches the same demographics.

## Audit

Every message writes its audit record before the answer leaves: the Patient
Record event of ITI-8 (`C`, `U`, and for an A40 `U` for the survivor and `D`
for the subsumed identifier) and the Query event of ITI-9 and ITI-21 with
the whole query message, each with the sender as its source at the
connection's own network address
([Security and audit](../operate/security-and-audit.md)). An audit spool
without room refuses the message with `AR`.

## ITI-10, the PIX update notification

After every change to the cross-reference (a feed that creates or links a
record, a merge, a revert, a removal), FerroPIX notifies each consumer
`[[hl7v2.consumers]]` names whose domains of interest the changed patient
holds an identifier in; a consumer with no `domains` is interested in every
domain. Each notification is an ADT^A31 in HL7 v2.5: MSH-9
`ADT^A31^ADT_A05`, an EVN, one PID whose PID-3 lists the patient's
identifiers in the consumer's domains with all three subcomponents of each
assigning authority and whose PID-5 is a single space, and a PV1 with
PV1-2 `N` (ITI TF-2 3.10.4.1.2), checked against the 2.5 `ADT_A05`
structure when it is written. A merge sends the survivor's identifiers;
a patient whose last record was removed holds none, so it sends nothing.

The notification is written to the outbox in the transaction that made the
change, and the outbox sender delivers each consumer's notifications in
order, one at a time, over MLLP. An acknowledgment `AA` whose MSA-2 names
the notification is a delivery; another code, a reply to another message,
a refused connection or a timeout is retried within
`[hl7v2.notification]`, after which the notification is abandoned and the
next one goes. Each notification that reaches the consumer writes the
Manager's audit record of ITI TF-2 3.10.5.1.1: a Patient Record read with
the consumer as the destination, a patient per identifier of PID-3 and
MSH-10 as a detail. The dependency report shows each consumer's latest
outcome ([Operations](../operate/operations.md)).
