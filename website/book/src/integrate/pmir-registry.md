<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The PMIR Registry

FerroPIX is building the Patient Identity Registry of IHE PMIR 1.6.0. It
already answers ITI-94, Subscribe to Patient Updates. ITI-93, the Mobile
Patient Identity Feed, is being built: the feed a Source sends, and the feed
FerroPIX sends each subscriber.

Turn PMIR on with `[pmir] enabled = true`
([Configuration](../operate/configuration.md)).

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
