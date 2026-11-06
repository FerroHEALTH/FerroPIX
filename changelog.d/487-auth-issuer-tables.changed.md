- Breaking for an operator: `[auth]` takes the `ferrohealth-smart` 0.9.0
  shape. Every issuer is an `[[auth.issuer]]` table with a required `kind`
  (`"persons"`, `"machines"`, `"patients"` or `"federation"`) and a profile
  (`profile = "rfc9068"` by default, or `"entra"`, `"keycloak"`, `"okta"`,
  `"nl"`), and holds every setting of its tokens itself. A configuration in
  the old shape refuses the start, naming what replaces it. Upgrade steps:
  write a flat `auth.issuer = "…"` as an `[[auth.issuer]]` table; move
  `audience`, `algorithms`, `clock_skew_seconds`, `jwks_max_age_seconds`,
  `jwks_refresh_interval_seconds`, `jwks_file`, `timeout_ms`, `mode`,
  `role_claims` and `[auth.introspection]` from `[auth]` onto each issuer
  table (`[auth.issuer.introspection]`); add `kind` to each issuer, with
  `kind = "machines"` for an issuer that signs only the client credentials
  tokens of Sources, consumers and subscribers, or `kind = "persons"` with
  the console's client in `first_party_clients` for one that also signs
  the stewards' logins; replace `token_profile = "jwt"` with
  `token_types = ["JWT"]` and the `required_claims` the issuer's tokens
  carry, and drop `token_profile = "rfc9068"`; and write each
  `scopeless_subjects` entry as `{ issuer = "…", sub = "…" }`.
- Breaking for a development configuration: a Basic user is a person, a
  steward or an administrator, and never a Source or another machine. An
  `[[auth.basic.users]]` entry granting a `system/` scope refuses the start,
  naming the user, and a Basic user-id no longer counts as a domain's
  `source_client_id`. Upgrade: move each machine on Basic to a client
  credentials token (RFC 6749 §4.4) of an `[[auth.issuer]]`, list its
  client under that issuer's `[[trust.issuer]] clients`, and keep on Basic
  only the people who sign in by hand. `GET /operator/sources` no longer
  lists Basic users, and the operator API's `Admission` has no `basic_user`
  kind.
