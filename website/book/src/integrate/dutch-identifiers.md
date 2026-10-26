<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The Dutch identifiers

Patients in the Netherlands are identified by their BSN, and the Dutch
Generic Functions name a patient toward the national index by a
pseudonymised BSN. FerroPIX holds both as identifier domains.

## The BSN

The BSN is an `authoritative` and `sensitive` identifier domain, under one of
the naming systems the Generic Functions list
(`http://fhir.nl/fhir/NamingSystem/bsn`, or the BSN's OID). Being
authoritative, a BSN that two records carry links them to one patient.
Being sensitive, it is sealed at rest. A value must be nine digits that pass
the eleven-proof: the feed drops spaces, dots and dashes, and restores a
leading zero dropped from the nine. A BSN that fails the proof is refused at
the feed with `422` and a `business-rule` issue.

## The pseudonymised BSN

The pseudonymised BSN (`http://fhir.nl/fhir/NamingSystem/pseudo-bsn`) is an
identifier domain of its own, also `authoritative` and `sensitive`. The
system that obtained a pseudonymised BSN feeds it like any identifier, and
FerroPIX holds it beside the patient's local identifiers. A data holder can
then derive, from the patient it knows, the key the national index wants.
FerroPIX keeps the value as it was fed, sealed, and never writes it to a
log.

## BSNk

FerroPIX calls no BSNk pseudonymisation service. The Generic Functions leave
BSNk outside their current version, so FerroPIX neither derives nor
transforms a pseudonymised BSN. Once the guide specifies BSNk, the work is
filed on the [tracker](https://github.com/FerroHEALTH/FerroPIX/issues).

## In the quickstart

The quickstart configuration seeds both domains beside its synthetic
hospital ([Configuration](../operate/configuration.md)):

```toml
[[domain]]
system = "http://fhir.nl/fhir/NamingSystem/bsn"
name = "BSN"
kind = "authoritative"
source_client_id = "quickstart-source"
sensitive = true

[[domain]]
system = "http://fhir.nl/fhir/NamingSystem/pseudo-bsn"
name = "Pseudonymised BSN"
kind = "authoritative"
source_client_id = "quickstart-source"
sensitive = true
```
