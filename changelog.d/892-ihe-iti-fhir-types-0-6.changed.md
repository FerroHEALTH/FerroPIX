- `ihe-iti` 0.31.0 builds on `fhir-types` 0.6 and `ihe-atna` 0.33, so the
  FHIR resources it hands a caller and its audit records come from those
  lines, and `nl-generic-functions` 0.31.0 builds on `fhir-types` 0.6 and
  `ihe-iti` 0.31. The server stays on the published 0.30 line until
  FerroBRIDGE's v2 and terminology crates move to `fhir-types` 0.6 (#891).
