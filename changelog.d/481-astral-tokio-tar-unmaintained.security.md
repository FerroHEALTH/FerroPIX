- RUSTSEC-2026-0440 (`astral-tokio-tar` is unmaintained, with no patched
  version) does not affect FerroPIX: `testcontainers` reaches the crate only
  in the test harness, and the published `ihe-iti` and `nl-generic-functions` crates do not contain it.
  `deny.toml` accepts the advisory until a `testcontainers` release drops
  the crate (`security/vex/rust-advisories.openvex.json`).
