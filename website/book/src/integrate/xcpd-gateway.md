<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The XCPD Responding Gateway

FerroPIX answers ITI-55 Cross Gateway Patient Discovery as an XCPD
Responding Gateway, on the same cross-reference as the FHIR and HL7 v2
faces. An Initiating Gateway of another community asks whether your
community knows a patient, by a shared identifier or by demographics, and
the answer names the patient's identifiers in your domains. The messages are
HL7 V3 `PRPA_IN201305UV02` and `PRPA_IN201306UV02` in SOAP 1.2 with
WS-Addressing, as the IHE ITI Technical Framework Volume 2, §3.55, prescribes
them. The gateway answers synchronously.

Turn it on with `[xcpd] enabled = true`
([Configuration](../operate/configuration.md#xcpd)):

```toml
[xcpd]
enabled = true
home_community_id = "urn:oid:2.999.60.1"
device = "2.999.60.2"
domains = ["urn:oid:2.999.1.1", "urn:oid:2.999.1.2"]
initiating_communities = ["urn:oid:2.999.70.1"]
```

The gateway listens at `POST {base}/xcpd` on the HTTP server. The SOAP
endpoint carries no bearer token: the mutual TLS that ITI-55 asks of an ATNA
Secure Node is the deployment's, at a sidecar or ingress in front of
FerroPIX, as it is for the MLLP listener.

## How a query is answered

- **A shared identifier:** an identifier of the query in a domain the store
  holds is looked up in the cross-reference. The person found is Case 1,
  and identifiers that find more than one person are Case 2. An identifier
  carried by the records of two persons is Case 5 with `AnswerNotAvailable`,
  since a data steward has to resolve it first.
- **Demographics:** when no identifier finds a person, the name, birth time,
  gender, address, telecom and mother's maiden name are scored by the
  matcher, as an ITI-119 `$match` input is
  ([Matching](../operate/matching.md)). One `certain` or `probable` person is
  Case 1 and more are Case 2. When only `possible` persons are found, the
  answer is Case 3: it asks for the scored attributes the query left out
  (gender, address, telecom, mother's maiden name). No person is Case 4. A
  `MinimumDegreeMatch` drops a person whose score, as a percentage, is
  below it.
- **The answer:** each person is answered with its identifiers in the
  `[xcpd] domains`, in their order. The first one the person holds is the
  `Patient.id` an XCA Cross Gateway Query uses, and the others are
  `OtherIDs`. A person who holds none of them is left out. Every
  `RegistrationEvent` names `home_community_id` as its custodian.
- **Refusals:** a query from an initiating community not listed in
  `initiating_communities` is Case 5 with `AnswerNotAvailable`. A query
  targeted at another community is Case 5. A Deferred request
  (`responsePriorityCode` `D`) is answered with the `NS250` "Unsupported
  processing mode" acknowledgement. A store that cannot be read is Case 5
  with `InternalError`. A request that is no ITI-55 query is answered with a
  SOAP 1.2 fault, `400` for a `Sender` fault.

## Audit

Every query writes the Responding Gateway audit record of §3.55.5.1.2 before
its answer leaves, as a BALP `AuditEvent` in the spool: the DICOM `Query`
event with the `ITI-55` subtype, the initiating gateway as the source, the
`QueryByParameter` as the query entity with the initiating community as its
`ihe:homeCommunityID` detail, and one patient entity per patient answered.
An answer whose record cannot be stored is replaced by Case 5. The log
lines name the case and the refusal, never an identifier or a name.
