- FerroPIX moves to `ferrohealth-smart` 0.19, `ferrohealth-session` 0.14,
  `ihe-atna` 0.13, `ferrobridge-term` 0.6 and `ferrobridge-hl7v2` 0.9. The
  issuer `[[auth.attributes]]`, `[[auth.federation]]`, `[[auth.party]]` and
  `auth.scopeless_subjects` name is a `SecretUrl` now, so a password in its
  userinfo appears in no `config check` output, no written-back
  configuration and no `Debug` rendering.
- Breaking for crate users: `ihe-iti` 0.5.0 requires `ferrohealth-smart`
  0.19 and `ihe-atna` 0.13, and `nl-generic-functions` 0.5.0 requires
  `ferrohealth-smart` 0.19 and `ihe-iti` 0.5. Their public API carries the
  types of those crates (the outbound client, the endpoint table and the
  audit user), so a consumer moves to the same lines.
