- FerroPIX moves to `ferrohealth-smart` 0.18, `ferrohealth-session` 0.12,
  `ihe-atna` 0.12 and `ferrobridge-term` 0.5. A configured issuer is a
  `SecretUrl` now, so a password in an issuer's userinfo appears in no
  `config check` output, no written-back configuration and no `Debug`
  rendering. The start still refuses such an issuer, naming it masked.
- Breaking for crate users: `ihe-iti` 0.4.0 requires `ferrohealth-smart`
  0.18 and `ihe-atna` 0.12, and `nl-generic-functions` 0.4.0 requires
  `ferrohealth-smart` 0.18 and `ihe-iti` 0.4. Their public API carries the
  types of those crates (the outbound client, the endpoint table and the
  audit user), so a consumer moves to the same lines. Where an
  issuer is read, use `.expose()`, and build one with `SecretUrl::new`.
