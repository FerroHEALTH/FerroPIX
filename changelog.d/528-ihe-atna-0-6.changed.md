- `ihe-iti` requires `ihe-atna` 0.6 and the server pins `ihe-atna` 0.6.0.
  None of the APIs they use change: the syslog `Repository` now takes its
  TLS configuration from the caller and ITI-81 reads through `fhir-search`,
  and neither crate builds a syslog repository or reads ITI-81 (#528).
