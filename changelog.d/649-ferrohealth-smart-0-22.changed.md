- FerroPIX moves to `ferrohealth-smart` 0.22, `ferrohealth-session` 0.17,
  `ihe-atna` 0.16, `ferrobridge-term` 0.9 and `ferrobridge-hl7v2` 0.13, the
  releases on the smart line where `DpopSection` no longer carries
  `external_url`. FerroPIX sets no `[auth.dpop]`, so no configuration key
  changes.
- Breaking for crate users: `ihe-iti` 0.8.0 requires `ferrohealth-smart`
  0.22 and `ihe-atna` 0.16, and `nl-generic-functions` 0.8.0 requires
  `ferrohealth-smart` 0.22 and `ihe-iti` 0.8. Their public API carries the
  types of those crates (the outbound client, the endpoint table and the
  audit user), so a consumer moves to the same lines.
