- `ihe-iti` 0.22.0 requires `ihe-atna` 0.27 and builds on `fhir-types` 0.2,
  and `nl-generic-functions` 0.22.0 requires `ihe-iti` 0.22.0. Both are
  breaking for a consumer that names types from those crates: move
  `ihe-atna` to 0.27 and `fhir-types` to 0.2 together with them. The server
  takes FerroBRIDGE's crates on `fhir-types` 0.2 (`ferrobridge-hl7v2` 0.28,
  `ferrobridge-validator` 0.20, `ferrobridge-term` 0.19).
