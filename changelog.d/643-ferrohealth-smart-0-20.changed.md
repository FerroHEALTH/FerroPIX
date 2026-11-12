- FerroPIX moves to `ferrohealth-smart` 0.20, `ferrohealth-session` 0.15,
  `ihe-atna` 0.14, `ferrobridge-term` 0.7 and `ferrobridge-hl7v2` 0.10. A
  refusal of a role issuer (`ConfigError::RoleIssuer`) renders the issuer
  with its userinfo masked.
- Breaking for crate users: `ihe-iti` 0.6.0 requires `ferrohealth-smart`
  0.20 and `ihe-atna` 0.14, and `nl-generic-functions` 0.6.0 requires
  `ferrohealth-smart` 0.20 and `ihe-iti` 0.6. Their public API carries the
  types of those crates (the outbound client, the endpoint table and the
  audit user), so a consumer moves to the same lines.
