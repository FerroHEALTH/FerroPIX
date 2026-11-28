- The server and the console run on `ferrohealth-server` 0.14,
  `ferrohealth-config` 0.5, `ferrohealth-telemetry` 0.6,
  `ferrohealth-usage-report` 0.6, `ferrohealth-smart` 0.38.2,
  `ferrohealth-session` 0.36, `ihe-atna` 0.32 and `fhir-types` 0.5, on the
  path crates `ihe-iti` and `nl-generic-functions` 0.30.1, which require
  `ferrohealth-smart` 0.38.2, the release that keeps an outbound client's
  endpoint cleared for revocation. Every outbound `tls` table verifies a peer
  by its root: a chain to a root of `trust_roots_file` connects only once a
  revocation check cleared its endpoint, at the start and every 4 minutes,
  failing closed when no CRL or OCSP source answers, and the XCPD delivery
  keeps its reply endpoints cleared; a root of the new
  `trust_roots_without_revocation_file` is trusted with no revocation check
  and named at the start.
