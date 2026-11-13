- Breaking: a Basic user's scopes move out of `[[auth.basic.users]]`,
  which holds the credential alone (`user`, `password_hash`,
  `password_hash_file`), into FerroPIX's own `[basic_grants.<user-id>]`
  table with `scopes` and `roles`. A `scopes` or `roles` key in
  `[[auth.basic.users]]` now refuses the start as an unknown key, and so
  does a `[basic_grants]` table naming a user `[[auth.basic.users]]` does
  not list. A granted `system/` scope or role is refused as before, now
  naming `basic_grants.<user-id>`. Upgrade: move each user's `scopes` line
  into a `[basic_grants.<user>]` table named by its `user`.
- FerroPIX moves to `ferrohealth-smart` 0.24, `ferrohealth-session` 0.19,
  `ihe-atna` 0.18, `ferrobridge-term` 0.10 and `ferrobridge-hl7v2` 0.14,
  the releases on the smart line where the caller classification asks each
  issuer's standing source. FerroPIX keeps its client lists in `[auth]`,
  the default source, so no further configuration key changes.
- Breaking for crate users: `ihe-iti` 0.9.0 requires `ferrohealth-smart`
  0.24 and `ihe-atna` 0.18, and `nl-generic-functions` 0.9.0 requires
  `ferrohealth-smart` 0.24 and `ihe-iti` 0.9. Their public API carries the
  types of those crates (the outbound client, the endpoint table and the
  audit user), so a consumer moves to the same lines.
