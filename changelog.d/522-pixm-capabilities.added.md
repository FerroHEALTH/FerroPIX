- `ihe-iti`: `PixmClient::capabilities` reads the PIX Manager's
  `CapabilityStatement` with `GET [base]/metadata` (FHIR R4 http.html
  §capabilities) through the client's own transport and authorizer, and
  answers every other outcome as a typed `CapabilitiesError`: refused with
  the status, not a FHIR server, timeout, transport or unauthenticated.
  `nl-generic-functions` follows `ihe-iti` 0.0.79.
