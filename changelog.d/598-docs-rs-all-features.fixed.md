- docs.rs documents every module of `ihe-iti` and `nl-generic-functions`.
  Both crates turn no feature on by default, and docs.rs built them with
  their default features, so the published API reference listed no item.
  Each manifest now carries `[package.metadata.docs.rs] all-features = true`,
  from `ihe-iti` 0.3.1 and `nl-generic-functions` 0.3.1, and CI builds each
  crate's docs the way docs.rs does.
