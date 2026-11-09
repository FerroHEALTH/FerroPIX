- `ihe-iti` 0.0.81 and `nl-generic-functions` 0.0.48 require
  `ferrohealth-smart` 0.12 (caret `0.12.0`), and `nl-generic-functions`
  0.0.48 requires `ihe-iti` 0.0.81. Both name `ferrohealth-smart` types in
  their public API, so a product on `ferrohealth-smart` 0.12 needs this
  release. No signature of either crate moves. Upgrade: move
  `ferrohealth-smart` to 0.12.0 beside `ihe-iti` 0.0.81 and
  `nl-generic-functions` 0.0.48 (#530).
- `ihe_iti::authorizer::smart::TokenAuthorizer` proves each request when
  its `TokenSource` binds tokens to a `DPoP` key
  (`ClientCredentials::with_dpop`): the token goes as `Authorization: DPoP`
  with a `DPoP` proof of the request's method and URL and the token's
  `ath` (RFC 9449 §4.2, §7.1). A `401` whose `DPoP` challenge demands a
  nonce keeps the token and sends the request once more with a proof
  carrying the nonce (§9); any other `401` still drops the token and
  resends once with a new one. A bearer source is sent as before (#530).
