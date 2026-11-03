- The release vulnerability record, the scan before the image tags and the
  weekly published-image scan install Trivy from its GitHub release tarball,
  checked against a sha256 pinned per version. `aquasecurity/setup-trivy`
  downloaded it from `get.trivy.dev`, which stopped resolving on 2026-11-03,
  so every one of those lanes failed.
