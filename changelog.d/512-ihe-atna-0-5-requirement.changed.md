- `ihe-iti` 0.0.77 requires `ihe-atna` 0.5 (caret `0.5.1`), and
  `nl-generic-functions` 0.0.44 requires `ihe-iti` 0.0.77. `ihe-iti`'s
  public API names `ihe-atna` types (the PIXm consumer's `OnBehalfOf`, the
  `xcpd` audit message), so a product on `ihe-atna` 0.5 needs this release.
  `ihe-atna`'s API did not change between 0.4.0 and 0.5.1, so no `ihe-iti`
  signature moves. The server pins `ihe-atna` 0.5.1. Upgrade: move
  `ihe-atna` to 0.5.1 beside `ihe-iti` 0.0.77.
