<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Provenance: the FerroPULSE report envelope v2

- Source: <https://github.com/FerroHEALTH/FerroPULSE> (private; Cadasto B.V., the Licensor
  of this repository too)
- Commit: 208262ad71f6addbb97c4b8025fcb9c95b3751d0
- Schema sha256: e609f17a5bfa78c6c08bdd3a28025889e20e0fde5c72fa881e7ed47b6abc8bc7
- Fetched: 2026-10-24 by `scripts/vendor/ferropulse-schemas.sh`
- Licence: upstream declares `LicenseRef-Proprietary` in its `Cargo.toml`
  and ships no LICENSE file; Cadasto B.V. holds the copyright of both
  repositories and publishes this copy under this repository's licence
- Contents: `schemas/report-envelope.v2.json` (JSON Schema 2020-12) and
  `schemas/examples/report-envelope.v2/` (the contract's own valid and
  invalid examples), 68 files, copied verbatim

The usage report tests in `app/ferropix-server/tests/it/usage_report.rs`
check the validator against the examples, then validate every report the
server builds against the schema. Never hand-edit these files; change the pin
in `docs/VERSIONS.md` and re-run the script.
