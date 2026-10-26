<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The PMIR Registry

FerroPIX is the Patient Identity Registry of IHE PMIR 1.6.0. It receives
ITI-93, the Mobile Patient Identity Feed, from the Source of the master
domain, answers ITI-94, Subscribe to Patient Updates, and sends ITI-93 to
each subscriber.

Turn PMIR on with `[pmir] enabled = true`
([Configuration](../operate/configuration.md)).

## The feed a Source sends (ITI-93)

The master domain is the `[[domain]]` of kind `master`, whose system is the
server's `public_base_url`. Only its Source may send the feed: its client id
is the domain's `source_client_id`, and it holds `system/Patient.cud`. Any
other client is answered `403` with a `forbidden` issue, and with no master
domain every client is.

The Source sends `POST {base}/fhir` with a PMIR message Bundle in FHIR JSON
or XML. FerroPIX applies each change of the history Bundle in order, each in
its own transaction:

- **Create:** the Patient's id must be a UUID, written in lowercase with
  hyphens. FerroPIX creates the patient with that id, so the id the Source
  chose is the id every face serves. A create of a patient FerroPIX holds
  already updates it.
- **Update:** the patient's demographics and identifiers are revised.
- **Merge:** a Patient sent with `active` false and a `replaced-by` link is
  merged into the patient the link names, as a merge by the Source. The
  merged patient then reads `inactive` with its `replaced-by` link, and its
  identifiers resolve to the survivor's in PIXm.
- **Delete:** the patient's master identifier is removed and answers as
  unknown from then on.

The response is a message Bundle with one `MessageHeader`. When every change
applied its code is `ok`. Otherwise it is `fatal-error`, and its `details`
point to an `OperationOutcome` with one issue per refused change, naming the
change's history entry and its status:

- `400` `invalid` for a Patient id that is not a UUID;
- `404` `not-found` for an update, merge or delete of a patient FerroPIX
  does not hold;
- `405` `not-supported` for an update of a merged patient, which would undo
  the merge. Unmerge goes through the operator console, not the feed.

The changes before a refused one stay applied. When the store fails the
answer is `503` and the Source sends the message again. Each message writes
one BALP record, following PMIR's Feed audit profile, before its answer
leaves.

## The feed FerroPIX sends (ITI-93)

Every committed change to a patient is an ITI-93 message to each active
subscription whose criteria match. A message is written in the same
transaction as the change, so a change is never left unannounced, and it is
written in the format the subscription's `channel.payload` names. A
merged patient is sent as the deprecated `Patient`, `active` false with its
`replaced-by` link.

The sender posts a subscription's messages to its channel endpoint one at a
time and in order, with the headers the subscription's `channel.header`
lists. It follows no redirect. A failed post is tried again with a doubling
wait, within `[pmir.feed]`. Once the attempts are spent, the subscription's
`status` becomes `error` and its `error` says why. Its later messages wait,
and an enable (`PUT` with `status` `requested`) resumes from the next one.
The operator API's dependency report names each subscriber with the outcome
of its latest message ([Operations](../operate/operations.md)).

## Subscriptions (ITI-94)

A subscriber holds the `system/Subscription.cruds` scope.

- **Create:** `POST {base}/fhir/Subscription` with a `Subscription` in FHIR
  JSON or XML. It must hold to the PMIR Subscription profile: a `message`
  channel, a payload media type, every contact with its system and value,
  and one of the four criteria listed below. FerroPIX activates the
  subscription at once. It answers `201` with the `Location` and the stored
  `Subscription`, now `active`.
- **Read:** `GET {base}/fhir/Subscription/{id}` answers the subscription as
  it stands, with `status` `error` and the error text once a delivery has
  failed for good.
- **Enable and disable:** `PUT {base}/fhir/Subscription/{id}` with `status`
  `off` disables it, and with `requested` enables it again. A disabled
  subscription is sent nothing. Any other status is refused.
- **Delete:** `DELETE {base}/fhir/Subscription/{id}` answers `204`. The
  subscription is gone for every later request, and FerroPIX sends it
  nothing.

The four criteria are:

- `Patient`: every patient;
- `Patient?_id=X`: one person;
- `Patient?organization=X`: the patients of one Organization;
- `Patient?identifier=X`: a FHIR token, by system, value or both.

The channel endpoint must be `https`. Plain `http` on a loopback host is
accepted only under the development profile's `[dev]` table. A criteria
outside the four, a missing channel and any other endpoint are refused with
`400` and an `OperationOutcome`.

Every request writes its BALP record, following PMIR's subscription audit
profiles, before its answer leaves. The operator dashboard counts the
subscriptions in error.
