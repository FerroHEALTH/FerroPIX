<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Deployment

The server ships as a container image, `ghcr.io/ferrohealth/ferropix`, for
`linux/amd64` and `linux/arm64`, and as a static Linux binary per
architecture attached to each GitHub release. The release lane that
publishes them exists; the first tagged release of the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3)
publishes the first image, so until then you build the image from source
(see [Building the image locally](#building-the-image-locally)). The
operator console has an image of its own, `ghcr.io/ferrohealth/ferropix-console`,
published by the same release ([The operator console](console.md#the-image)).
A Helm chart is planned
([#100](https://github.com/FerroHEALTH/FerroPIX/issues/100)).

The two crates deploy inside your own application: they build with the
Rust toolchain your application uses, at or above the minimum supported
Rust version in
[Conformance and specification pins](../evaluate/conformance.md#the-toolchain),
and need nothing at run time beyond the servers they call.

## What the server runs beside

- **the clinical systems that ask it,** FerroEHR among them, which is
  planned to resolve its `ehr_id` through PIXm and PDQm
  ([FerroHEALTH/FerroEHR#3380](https://github.com/FerroHEALTH/FerroEHR/issues/3380));
- **a PostgreSQL 18 database** that holds the cross-reference, reached as a
  role that is no superuser (the server refuses one at boot, because a
  superuser reads every schema);
- **an authorization server** whose access tokens it reads, and whose
  metadata it reads once at start;
- **an ATNA audit repository** that receives its BALP records; until one is
  configured the records wait in the spool;
- **the operator console**, its own image, which reads the server's
  operator API with a signed-in steward's token.

## The image

| Property | Value |
|---|---|
| Base | `gcr.io/distroless/static-debian13:nonroot`, pinned by digest: no shell, no package manager |
| Entrypoint | `/usr/local/bin/ferropix`, default job `serve` |
| User | `65532:65532` |
| Ports | `8080/tcp` (HTTP, `FERROPIX__SERVER__LISTEN=0.0.0.0:8080`), `2575/tcp` (the MLLP listener of `[hl7v2]`, when you turn it on) |
| State | `/var/lib/ferropix`, owned by `65532`: the audit spool under `audit/` (mode `0700`) and the usage report's instance id |
| Health | `HEALTHCHECK` runs `ferropix healthcheck`, which asks `GET /health/readiness` over loopback and exits 0 on `200` |

Nothing is compiled inside the image: the release lane builds the binary
and the image copies it. Configure the server with a mounted TOML file
named by `FERROPIX_CONFIG` and with `FERROPIX__<SECTION>__<KEY>` variables
([Configuration](configuration.md)); every secret is a file its `_file`
sibling names. Run the root filesystem read-only and mount a volume at
`/var/lib/ferropix`. A Kubernetes Job runs `db migrate` by setting `args:`
alone.

Tags: `<version>` for every release, and `<major>.<minor>` and `latest`
for a release that is not a pre-release. A tag is applied only after the
image passed the release's vulnerability gate, and tags move, so pin the
digest the release names.

## Verifying an image

Each image index and both platform manifests carry SLSA provenance, and
each manifest an SPDX SBOM, as attestations signed through Sigstore by the
reusable release workflow. Verify an image against that workflow:

```sh
gh attestation verify oci://ghcr.io/ferrohealth/ferropix:<version> \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/release-image.yml
```

A release tarball verifies the same way against `release-build.yml`, and
the `.sha256sum` beside it detects a damaged download. Each release also
attaches its vulnerability record: every finding of a scan of the image and
the binaries, at every severity, with the OpenVEX statement that judges it.

## The compose quickstart

`compose.yaml` at the root of the repository runs the published image
beside PostgreSQL and a development OpenID Provider. It mounts
`app/ferropix-server/quickstart.toml`, so run it from a checkout:

```sh
mkdir -p secrets && umask 077
openssl rand -base64 32 > secrets/ferropix-data-key
openssl rand -hex 16 > secrets/postgres-password
openssl rand -hex 16 > secrets/ferropix-db-password
printf 'postgresql://ferropix:%s@postgres:5432/ferropix' \
  "$(cat secrets/ferropix-db-password)" > secrets/ferropix-db-url
docker compose up --wait
curl http://localhost:8080/health/readiness
```

- **Hardening.** The server runs with a read-only root filesystem, every
  capability dropped, `no-new-privileges`, a `tmpfs` for `/tmp`, and its
  state on a named volume.
- **Secrets.** All four are files under `./secrets`, mounted under
  `/run/secrets/`, never environment variables. PostgreSQL creates the
  `ferropix` role from `ferropix-db-password` on its first start
  (`docker/postgres/10-ferropix-role.sh`).
- **Ports.** Every port binds `127.0.0.1`; set `FERROPIX_BIND_HOST` to
  publish on another interface.
- **The issuer.** The server accepts a plain `http` issuer only on
  loopback, so the development provider owns the network namespace and the
  server joins it. The server, your shell and your browser all reach it at
  `http://localhost:8180/ferropix`. It signs any client credentials
  request: a development issuer, never for real data. For a deployment,
  name your own `https` issuer in `[[auth.issuer]]`.
- **The console.** The `console` profile runs the operator console beside
  the server on `http://localhost:3000/`, in the same network namespace,
  signed in through the development issuer; it needs a fifth secret,
  `secrets/console-oidc-client-secret`.
- **The usage report** is on and states `compose`; set
  `FERROPIX__USAGE_REPORT__ENABLED=false` in your shell or a `.env` file to
  switch it off ([Configuration](configuration.md#usage_report)).

A token of the development issuer, and an ITI-83 query with it:

```sh
token="$(curl -s -d grant_type=client_credentials \
  -d client_id=quickstart-source -d client_secret=any \
  -d scope=system/Patient.rs http://localhost:8180/ferropix/token | jq -r .access_token)"
curl -H "Authorization: Bearer $token" \
  'http://localhost:8080/fhir/Patient/$ihe-pix?sourceIdentifier=urn:oid:2.999.1.1|1'
```

## Building the image locally

`scripts/release/stage-dist.sh <version>` downloads the musl binaries of a
published release, checks each against its published checksum and stages
them with the state tree under `dist/`. Before the first release, stage a
binary of your own build instead:

```sh
cargo build --release --locked -p ferropix-server --target x86_64-unknown-linux-musl
mkdir -p dist/linux_amd64
install -m 0555 target/x86_64-unknown-linux-musl/release/ferropix dist/linux_amd64/
scripts/release/stage-dist.sh tree
docker buildx build -f docker/Dockerfile --platform linux/amd64 \
  -t ghcr.io/ferrohealth/ferropix:<version> --load .
```
