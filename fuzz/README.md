<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Fuzz targets

`cargo fuzz` (libFuzzer) over the request readers of the `pixm-server` and
`pdqm-server` features of `ihe-iti`, the bytes a caller controls before
anything else reads them (#79). This crate sits outside the workspace, with
its own lockfile, because cargo-fuzz needs a nightly toolchain for its
sanitizer flags; the nightly is pinned by date in `docs/VERSIONS.md`, and
nothing built here ships.

| Target | Entry point | Input | Seeds |
|---|---|---|---|
| `pix_query` | `PixQuery::from_query` (ITI-83 `GET`) | the query string | the PIXm request examples as queries |
| `pix_parameters` | `PixQuery::from_body` (ITI-83 `POST`) | the `Parameters` body | the PIXm request examples |
| `pix_feed` | `Removal::from_delete` and `Feed::from_update` (ITI-104) | the query, a NUL byte, the `Patient` body | the PIXm Patient examples behind their `identifier` condition |
| `pdq_search` | `PatientSearch::from_query` and `from_form` (ITI-78) | the query string, also read as a form body | the demographics of the PIXm and PDQm Patient examples |
| `pdq_match` | `MatchRequest::from_request` (ITI-119) | the query, a NUL byte, the body | the PDQm Match Input examples |
| `format` | `Format::of_content_type` and `Format::negotiate` | `Content-Type`, `_format` and `Accept`, separated by NUL bytes | the media types of the IHE CapabilityStatements |

A body is read as FHIR XML when its first byte past whitespace is `<`, and
as FHIR JSON otherwise. The token parser of `ihe_iti::server` is internal to
the crate, so the targets reach it through the ITI-83, ITI-104 and ITI-78
readers.

A finding is a panic, an abort or a hang. An `Err` is the reader doing its
job and is never a finding. Every target also asserts two properties:

- A refusal quotes nothing from the request. The error, each error in its
  source chain, its `Debug` and the encoded `OperationOutcome` hold only the
  characters the crate's own static texts use, so a reader that copied a
  request value into an error would show the arbitrary bytes the fuzzer
  sent it.
- What a reader accepts survives a writer. `pix_query` and `pix_parameters`
  post the request back as the Query Parameters In `Parameters`, `pix_feed`
  and `pdq_match` write the Patient they read in both FHIR encodings, and
  `pdq_search` follows its own `self` link; each must read the same again.
  `format` checks that the format chosen names itself through its own media
  type.

## Seeds

`scripts/fuzz/seeds.sh` writes every `fuzz/seeds/<target>/gen-*` file from
the examples of the vendored PIXm and PDQm packages, and
`scripts/fuzz/seeds.sh --check` fails when they are out of date. Never
hand-edit a `gen-*` file. A regression seed committed after a finding
carries any other name, so the generator leaves it alone.

## Running a target

```sh
cargo +nightly-2026-10-19 fuzz run pdq_search fuzz/corpus/pdq_search fuzz/seeds/pdq_search -- -max_total_time=60
```

The lane is `.github/workflows/fuzz.yml`: weekly and on dispatch for five
minutes per target, and for one minute on a pull request that touches what a
target reads. A reproducing input is uploaded as a run artifact and becomes
a `Bug` issue with the input attached; the fix lands in `ihe-iti` with a
regression test.
