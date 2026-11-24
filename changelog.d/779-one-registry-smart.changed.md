- Breaking: FerroPIX takes `ferrohealth-smart` 0.34 and `ferrohealth-session`
  0.29, and trusts a SMART authorization server, FerroSMART among them, with
  an `[auth.issuer.<name>]` table that names only its `issuer`. The caller is
  read from the token's standard claims: `typ` is `at+jwt`, `scope` is
  required, a token whose `sub` is its `client_id` is a machine's, and a user
  is named by their `fhirUser`. The authorization server's registry is the
  one list of clients, so a token whose `aud` names FerroPIX admits its
  client. The compose quickstart's development issuer signs `at+jwt` tokens.
- Breaking: `ihe-iti` 0.21.0 and `nl-generic-functions` 0.21.0 take
  `ferrohealth-smart` 0.34, whose `OutboundClient`, `EndpointSection` and
  `ServerTls` their public API exposes: a product moves to
  `ferrohealth-smart` 0.34 with them.
