- Breaking for a crate user: `ihe-iti` 0.0.76 requires `ferrohealth-smart`
  0.9 (caret `0.9.0`) and `ihe-atna` 0.4 (caret `0.4.0`), and
  `nl-generic-functions` 0.0.43 requires `ferrohealth-smart` 0.9 and
  `ihe-iti` 0.0.76. The `smart` feature's
  `authorizer::smart::TokenAuthorizer` takes a `TokenSource` of the 0.9
  line, and the `xcpd` ITI-55 audit message writes the `ihe-atna` 0.4
  message shape. Upgrade: move `ferrohealth-smart` to 0.9.0 and `ihe-atna`
  to 0.4.0 in the same change as these crates, or the build holds two
  copies of each.
- The server and the console move to `ferrohealth-smart` 0.9.0,
  `ferrohealth-session` 0.3.0 and `ihe-atna` 0.4.0, which writes every
  string of a BALP `AuditEvent` with its control and bidirectional
  formatting characters escaped as code points.
