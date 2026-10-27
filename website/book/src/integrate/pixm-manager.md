<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The PIXm Manager

FerroPIX serves the IHE PIXm 3.1.0 Patient Identifier Cross-reference
Manager under `{base}/fhir`, in FHIR R4 JSON and XML: ITI-83 answers which
identifiers the cross-reference holds for a patient, and ITI-104 lets each
domain's Patient Identity Source feed it. `[pixm] enabled` switches both
on (the default) and `[pixm] remove_patient` adds the Remove Patient
Option ([Configuration](../operate/configuration.md)).

Every request needs an access token or, on a development surface, a Basic
user ([Security and audit](../operate/security-and-audit.md)). A Consumer
holds `system/Patient.rs` or the IUA scope `ITI-83`; a Source holds
`system/Patient.cud` or `ITI-104` and must be its domain's
`source_client_id`.

## Ask for a patient's identifiers (ITI-83)

The Manager accepts `$ihe-pix` in two forms that answer the same:

- `POST {base}/fhir/Patient/$ihe-pix` with a `Parameters` body carrying
  `sourceIdentifier` (one `system|value`) and any number of `targetSystem`,
  each a `valueString` (FHIR R4 Operations, "Executing an Operation"). The
  identifier stays out of the URL, so no browser history, proxy log or trace
  holds it. Use this form: it is what `ihe_iti::pixm::Invocation::Post`
  sends, and every FerroHEALTH client calls the Manager this way
  ([Getting started](getting-started.md)).
- `GET {base}/fhir/Patient/$ihe-pix?sourceIdentifier=system|value{&targetSystem=…}`,
  the form PIXm prescribes (§2:3.83.4.1.2), for a Consumer built strictly
  to IHE. The identifier then sits in the query string; FerroPIX's request
  log never records a query, but a proxy in front of it might.

The answer is a `Parameters` resource with one `targetIdentifier` per
identifier the person holds in the domains asked for (every domain when no
`targetSystem` is given), leaving out the identifier asked about, and one
`targetId` naming `Patient/{id}`, the person (§2:3.83.4.2.2.1). The
identifiers are the records each Source fed and every identifier in an
`authoritative` domain that those records carry and no Source feeds, such
as the BSN on a hospital's `Patient`; you can ask by that BSN too. A
pseudonymisation domain is a `targetSystem` too: the answer carries the
person's pseudonym in it, minted on the first ask of the domain's consumer
and answered to that consumer alone ([Pseudonyms](pseudonyms.md)).

| Case | Answer |
|---|---|
| found | `200`, the `Parameters` |
| the identifier was subsumed by a resolved duplicate, or its person merged | `200`, the survivor's identifiers (§2:3.104.4.2.3) |
| the domain is known, the identifier is not, or was removed | `404`, `not-found`, "sourceIdentifier Patient Identifier not found" (§2:3.83.4.2.2.2) |
| the source domain is unknown or retired | `400`, `code-invalid`, "sourceIdentifier Assigning Authority not found" (§2:3.83.4.2.2.3) |
| a target domain is unknown or retired | `403`, `code-invalid`, "targetSystem not found" (§2:3.83.4.2.2.4) |
| a carried identifier sits in two persons the steward has not resolved | `409`, `multiple-matches` |
| a malformed request: no `sourceIdentifier`, two, no system, an unknown parameter | `400`, with the issue naming the fault |

The merged-identifier and `409` rows are FerroPIX's own policy: PIXm
leaves the merged case to the Manager (§2:3.83.4.2.2.5) and names no
conflict case.

## Feed the cross-reference (ITI-104)

- `PUT {base}/fhir/Patient?identifier=system|value` with a `Patient` body
  is Add or Revise Patient (§2:3.104.4.1): `201` when the identifier is
  new, `200` when it is known, with a `Location` naming the person. The
  body must carry the identifier of the condition and is read up to the
  PDQm Patient profile.
- The same `PUT` whose `Patient` has a `link` of type `replaced-by` to
  another identifier of the domain is Resolve Duplicate Patient
  (§2:3.104.4.2): `200`, and from then on a query of the subsumed
  identifier answers the survivor's set.
- `DELETE {base}/fhir/Patient?identifier=system|value` is Remove Patient,
  served only with `[pixm] remove_patient = true` (§2:3.104.4.3): `200`,
  and the identifier is not found afterwards. Removing an identifier the
  cross-reference does not hold also answers `200` (FHIR R4 RESTful API,
  delete).

A new cross-reference is visible to the next query once the feed commits.
Each answer is an `OperationOutcome` with one `information` issue on
success, or one `error` issue:

| Case | Answer |
|---|---|
| the caller is not the domain's Source | `403`, `forbidden` (§2:3.104.5) |
| the condition is missing, names two identifiers or no domain | `400` |
| the body is no `Patient`, breaks the PDQm Patient profile or lacks the condition's identifier | `400` |
| the condition's domain is unknown or retired | `400`, `code-invalid`, "identifier Assigning Authority not found" |
| a value the domain's rule refuses (a BSN failing the eleven-proof) | `422`, `business-rule` |
| a revision of an identifier the Source subsumed | `422`, `business-rule` |
| a survivor that names no active record of the domain | `422`, `business-rule` |

## Every route

- `_format` (`json`, `xml` or a FHIR media type) chooses the answer's
  encoding over `Accept`; a request that accepts neither answers `406`
  with a `not-supported` issue, and a body in another media type `415`.
- A store that cannot be reached answers `503` with a `transient` issue,
  and so does a request whose audit record has no room in the spool, before
  anything is read or changed.
- Every query and every feed writes its PIXm BALP record
  ([Security and audit](../operate/security-and-audit.md#the-servers-audit-trail)). The
  request log records the matched route and never the query, the body or
  the identifier ([Operations](../operate/operations.md)).

`GET {base}/fhir/metadata` serves the `CapabilityStatement`, in JSON or XML,
that instantiates the PIXm Manager actor statement: the `Patient` entry
with the PIXm Patient profile, the `update` interaction with
`conditionalUpdate`, the `delete` interaction with `conditionalDelete` only
while the Remove Patient Option is on, and the `ihe-pix` operation, next to
the security service and the scopes the permission table enforces.
