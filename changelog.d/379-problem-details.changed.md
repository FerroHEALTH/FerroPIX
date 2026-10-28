- The operator API, and the refusals the HTTP stack writes outside the FHIR
  face and the XCPD gateway, answer every error as RFC 9457 problem details,
  `application/problem+json`, where they answered a FHIR `OperationOutcome`.
  The status codes are unchanged. FerroPIX's problem types are defined once,
  each `type` a section of the book's Problem types page
  (<https://ferropix.eu/docs/operate/problems.html>), and a problem quotes
  nothing from the request. The FHIR face and the XCPD gateway keep their
  `OperationOutcome` and SOAP answers. Breaking for a client of the operator
  API that read the `issue` array: read `type`, `status` and `detail`
  instead. The console reads the new body.
- `ferrohealth-server` 0.3.8: a `[server.tls]` table that sets
  `client_ca_file` while `client_auth` is `"none"`, the default included, is
  refused at load. Set `client_auth = "required"` (or `"optional"`) beside
  the CA, or remove it.
