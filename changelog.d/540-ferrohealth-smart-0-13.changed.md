- `ihe-iti` 0.0.83 requires `ferrohealth-smart` 0.13 and `ihe-atna` 0.8,
  and `nl-generic-functions` 0.0.50 requires `ferrohealth-smart` 0.13. The
  server and the operator console pin `ferrohealth-smart` 0.13.0,
  `ferrohealth-session` 0.7.0 and `ihe-atna` 0.8.0, so the lock holds one
  `ferrohealth-smart`. The 0.13 line refuses a malformed JSON document
  without quoting it (GHSA-mhv7-3975-6xq3) and bounds the token endpoint's
  answer; none of the APIs these crates use change (#540).
