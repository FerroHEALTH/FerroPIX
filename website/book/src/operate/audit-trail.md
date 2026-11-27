<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The audit trail

**Audience:** operators and auditors who need to know what FerroPIX
records, where each record waits and how it reaches the audit repository.

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

A write's record (an ITI-104, ITI-93, ITI-8 or ITI-94 write, a pseudonym
minted on an ITI-83 ask, a steward's decision, merge, reveal or
configuration change) is kept in PostgreSQL, in the table
`pix.audit_outbox`, inserted in the same transaction as the change. The
change and its record commit together or not at all: a crash before the
commit leaves neither, and a committed change always has its record. The
record is compressed and sealed under the store's key, like an identifier.
A write refused before it changed anything keeps the record of its refusal
there too.

A read's record waits in a spool under `audit.spool_dir`, in files readable
by the server's user alone in a directory only that user may enter. The
records that arrive together are written as one segment file and reach the
disk with one flush, and each read answers only once its record is on the
disk. Before a read runs, the server takes its record's place in the
spool: when the spool is full, the request is refused with `503` and a
`transient` issue. A record the spool does not store within
`audit.spool_write_timeout` is withdrawn and the record of the read
refused, with the outcome `8`, takes its place; the read answers `503`, so
the trail says what the client got. A spool directory an older FerroPIX
wrote holds one `.msg` file per record, which this server refuses at
start: let the older server deliver them, or move them out, before you
upgrade.

Both are sent to `audit.endpoint.url` over the FHIR Feed, several at once
as one batch Bundle, each judged by its entry of the answer. A delivered
record is removed. One the repository refuses is moved aside for you to
inspect: a spooled record to the `quarantine` subdirectory, a write's record
to `pix.audit_quarantine`, logged by its id. `GET {base}/health/readiness`
reports `audit_spool` down when less than a tenth of either bound is left
and `audit_outbox` down while `pix.audit_quarantine` holds a record, and
`GET {base}/operator/dependencies` shows whether the last delivery to the
repository went through.

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
