- `ihe-iti` 0.0.78 and `nl-generic-functions` 0.0.45 require
  `ferrohealth-smart` 0.10 (caret `0.10.0`), and `nl-generic-functions`
  0.0.45 requires `ihe-iti` 0.0.78. Both name `ferrohealth-smart` types in
  their public API (`ihe-iti`'s `authorizer::smart::TokenAuthorizer` over
  `client::cache::TokenSource`, the Nuts grant's issuer rule), so a product
  on `ferrohealth-smart` 0.10 needs this release. No signature of either
  crate moves: the one 0.10.0 break is the `cache` field of
  `config::onward::OnwardSection`, which neither builds. Upgrade: move
  `ferrohealth-smart` to 0.10.0 beside `ihe-iti` 0.0.78 and
  `nl-generic-functions` 0.0.45.
