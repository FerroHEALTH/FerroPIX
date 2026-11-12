- FerroPIX moves to `ferrohealth-smart` 0.21, `ferrohealth-session` 0.16,
  `ihe-atna` 0.15, `ferrobridge-term` 0.8 and `ferrobridge-hl7v2` 0.12, the
  releases on the smart line that chooses the described SMART issuer by one
  shared rule and adds the optional `described_issuer` key to `AuthSection`.
- Breaking for crate users: `ihe-iti` 0.7.0 requires `ferrohealth-smart`
  0.21 and `ihe-atna` 0.15, and `nl-generic-functions` 0.7.0 requires
  `ferrohealth-smart` 0.21 and `ihe-iti` 0.7. Their public API carries the
  types of those crates (the outbound client, the endpoint table and the
  audit user), so a consumer moves to the same lines.
