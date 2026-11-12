- A `private_key_jwt` client assertion the server signs, for an OAuth 2.0
  client of an outbound lane or of `[auth.issuer.introspection]`, is valid
  for 60 seconds, down from 300, so an authorization server that caps the
  assertion lifetime below SMART App Launch 2.2.0's five-minute ceiling
  accepts it. FerroPIX moves to `ferrohealth-smart` 0.19.1,
  `ferrohealth-secret` 0.2.1, `ferrohealth-config` 0.4.4,
  `ferrohealth-telemetry` 0.4.1, `ferrohealth-server` 0.8.4 and
  `ferrohealth-usage-report` 0.4.1; `ihe-iti` and `nl-generic-functions`
  move to 0.5.1 and take the same lines.
