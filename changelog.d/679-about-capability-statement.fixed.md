- The book's About screenshot of the operator console draws the server's
  `CapabilityStatement` with the server and FHIR versions it declares. The
  journeys' stand-in for the server now answers `GET /fhir/metadata` with
  the statement the server serves, and a journey fails when About draws a
  refusal there.
