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
  `frame_limit_bytes` or does not complete within `frame_timeout_ms` is
  answered `AR` when its header can be read, and the connection closes. A
  connection idle past `idle_timeout_ms` closes.
- **Character sets:** MSH-18 names the set a message is read in; a message
  with an empty MSH-18 is read in `default_charset`. The answer is written
  in the set the message came in.
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
  messages it is answering within `[server] drain_timeout_ms`, and closes.
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
name, after the HL7 v2-to-FHIR maps) and applies it through the same feed
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
| `AE`, ERR code `204` | An A40 names a survivor FerroPIX does not hold. |
| `AR`, ERR code `200` | Any other message type or trigger event. |
| `AR`, ERR code `207` | The store or the audit spool cannot take the message; send it again. |

A message sent again with the same MSH-10 from the same sender is
acknowledged with the code it got the first time and is not applied again.

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
demographics. A QPD-8 domain FerroPIX does not know answers `AE` with an
ERR at `QPD^1^8^n`, and a field FerroPIX does not search on answers `AE`.

RCP-2 (`2^RD`) asks for increments of that many records. An answer that
holds fewer than match ends with a DSC whose pointer you send back in a DSC
segment for the next increment. A pointer lives ten minutes, is used once,
and is given up by QCN^J01; a pointer unknown or past its lifetime answers
`AE` with an ERR at `DSC^1^1`.

## Audit

Every message writes its audit record before the answer leaves: the Patient
Record event of ITI-8 (`C`, `U`, and for an A40 `U` for the survivor and `D`
for the subsumed identifier) and the Query event of ITI-9 and ITI-21 with
the whole query message, each with the sender as its source at the
connection's own network address
([Security and audit](../operate/security-and-audit.md)). An audit spool
without room refuses the message with `AR`.

ITI-10, the PIX update notification, is planned
([#113](https://github.com/FerroHEALTH/FerroPIX/issues/113)).
