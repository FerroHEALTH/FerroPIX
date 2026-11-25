<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the MyHealth@EU NCP API package

Vendored verbatim by `scripts/vendor/myhealtheu-ncp-api.sh`. Never edit a
file here: change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://build-fhir.ehdsi.eu/ncp-api/package.tgz>, the
  continuous-integration build of the MyHealth@EU NCPeH API implementation
  guide (<https://build-fhir.ehdsi.eu/ncp-api/>)
- Pin: package `myhealth.eu.fhir.ncp-api` version `9.1.1-ci` (built
  2026-09-08), tarball sha256
  `acf655481026decd6395ffc2f66c5b0b7b3316a6aaed9520db0922d727ae8fef`
- Fetched: 2026-11-24
- Upstream licence: Creative Commons Zero (`CC0-1.0`, the `license` of the
  package manifest; <https://creativecommons.org/publicdomain/zero/1.0/>).
- FHIR version: 4.0.1
- Layout: the package tarball as published, unchanged:
  `myhealth.eu.fhir.ncp-api-9.1.1-ci.tgz`
- Read by: the conformance corpus `ncp-patient-minimal` of
  `app/ferropix-server`, which validates the PDQm answers against the
  profile `http://fhir.ehdsi.eu/ncp-api/StructureDefinition/Patient-minimal-myhealtheu`
  with `ferrobridge-validator`

## Why the tarball

The build has no copy in the FHIR package registry, and the validator reads
a package with its manifest, so the tarball is kept whole.

## The closure, fetched and never committed

`scripts/vendor/myhealtheu-ncp-api.sh --closure` fetches the packages this
package and `ihe.iti.pdqm` 3.2.0 depend on, 19 tarballs of about 66 MB, from
<https://packages.fhir.org> into the ignored `closure/` beside this file,
each held to the registry's sha1 the script pins.
