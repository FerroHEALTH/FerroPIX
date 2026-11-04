- The server, the console and the testkit run on the newest FerroHEALTH
  family releases: `ferrobridge-hl7v2` 0.1.177, `ferrohealth-config`
  0.2.8, which masks a refused configuration value of any type in its
  message and names a key inside an array of tables by its index,
  `ferrohealth-session` 0.2.2, `ferrohealth-telemetry` 0.2.4, whose panic
  line carries the `request_id` of the request it broke, and `ihe-atna`
  0.3.3, whose BALP record names the data of an access by
  `what.identifier` when its id is no FHIR literal reference.
