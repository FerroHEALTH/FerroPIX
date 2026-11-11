<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Deployment

The server ships as a container image, `ghcr.io/ferrohealth/ferropix`, for
`linux/amd64` and `linux/arm64`, and as a static Linux binary per
architecture attached to each GitHub release. The operator console has an
image of its own, `ghcr.io/ferrohealth/ferropix-console`, published by the
same release ([The operator console](console.md#the-image)), and the Helm
chart, `oci://ghcr.io/ferrohealth/charts/ferropix`, runs both on Kubernetes
([The Helm chart](#the-helm-chart)). Every release publishes all of them,
built at SLSA Build Level 3 ([Verifying an image](#verifying-an-image)).

The two crates deploy inside your own application: they build with the
Rust toolchain your application uses, at or above the minimum supported
Rust version in
[Conformance and specification pins](../evaluate/conformance.md#the-toolchain),
and need nothing at run time beyond the servers they call.

## What the server runs beside

- **the clinical systems that feed and ask it,** FerroEHR and FerroFED
  among them ([What FerroPIX is](../evaluate/what-ferropix-is.md));
- **a PostgreSQL 18 database** that holds the cross-reference, reached as a
  role that is no superuser (the server refuses one at boot, because a
  superuser reads every schema);
- **an authorization server** whose access tokens it reads, and whose
  metadata it reads once at start;
- **an ATNA audit repository** that receives its BALP records; until one is
  configured the records wait in the spool;
- **a FHIR R4 terminology server**, such as FerroTERM, when the HL7 v2
  face is on: it translates every v2 table value
  ([The HL7 v2 face](../integrate/hl7v2-face.md#the-terminology-server));
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

## Disk latency

Every audited request, a read included, answers only once its BALP record
is on disk. The spool writes each record to a file, flushes it with
`fsync`, renames it and flushes the directory, and it writes one record at
a time. A request that changes the cross-reference also waits for
PostgreSQL's commit to reach its disk. So the server's request latency
follows the flush latency of two disks: the volume that holds the spool
(`/var/lib/ferropix/audit`, `[audit] spool_dir`) and PostgreSQL's. A flush
that takes 100 ms holds every request whose record is queued behind it,
on every route at once, while the median stays where it was. A record not
stored within `[audit] spool_write_timeout` (2 seconds) refuses its
request with `503`.

Put the spool on a disk whose flushes are short and steady, such as a local
SSD. A network disk whose flush latency spikes shows up as a long tail on
every route at the same moment
([FerroPIX #511](https://github.com/FerroHEALTH/FerroPIX/issues/511)).
Do not put the spool on `tmpfs`: a record must survive a restart until the
repository has it (ITI TF-2 §3.20.4.1.1).

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

`compose.yaml` runs the published image beside PostgreSQL and a
development OpenID Provider. It is one self-contained file: the server's
configuration and the database role are inlined as Compose configs, so it
needs Docker Compose 2.23.1 or newer and no checkout, and every release
attaches it with its `.sha256sum`. The
[quickstart](../integrate/getting-started.md) walks through it, from the
download and the secret files to a first feed and query; CI runs those
steps over the repository's `compose.yaml`, the file a release attaches, on
every change. `FERROPIX_VERSION` picks the image tag, the release the file
names by default.

| Profile | What it adds |
|---|---|
| none | the server, PostgreSQL 18 and the development issuer |
| `console` | the operator console on `http://localhost:3000/` |
| `hl7v2` | FerroTERM, the terminology server the HL7 v2 face needs, reading the three packages under `./terminology` (`FERROPIX_TERMINOLOGY_DIR`), which the header of `compose.yaml` fetches; set `FERROPIX__HL7V2__ENABLED=true` and `FERROPIX__TERMINOLOGY__ENDPOINT__URL=http://localhost:8280/r4` to switch the face on |

- **Hardening.** The server runs with a read-only root filesystem, every
  capability dropped, `no-new-privileges`, a `tmpfs` for `/tmp`, and its
  state on a named volume.
- **Health.** `compose.yaml` declares the healthcheck of the server and of
  the console on the service, the same check their images carry, so
  `podman compose up --wait` waits for the same readiness `docker compose`
  does: Podman runs the published images without their own healthcheck.
- **Secrets.** All four are files under `./secrets`, mounted under
  `/run/secrets/`, never environment variables. PostgreSQL creates the
  `ferropix` role from `ferropix-db-password` on its first start
  (the `ferropix-role` config, a copy of `docker/postgres/10-ferropix-role.sh`).
- **Ports.** Every port binds `127.0.0.1`; set `FERROPIX_BIND_HOST` to
  publish on another interface.
- **The issuer.** The server accepts a plain `http` issuer only on
  loopback, so the development provider owns the network namespace and the
  server joins it. The server, your shell and your browser all reach it at
  `http://localhost:8180/ferropix`. It signs any client credentials
  request, and its login form signs in any user name you type as a data
  steward: a development issuer, never for real data. For a deployment,
  name your own `https` issuer in `[[auth.issuer]]`.
- **The console.** The `console` profile runs the operator console beside
  the server on `http://localhost:3000/`, in the same network namespace,
  signed in through the development issuer; it needs a fifth secret,
  `secrets/console-oidc-client-secret`. The quickstart mounts the operator
  API the console reads.
- **The usage report** is on and states `compose`; set
  `FERROPIX__USAGE_REPORT__ENABLED=false` in your shell or a `.env` file to
  switch it off ([Configuration](configuration.md#usage_report)).

## The Helm chart

`deploy/helm/ferropix` installs the server and, with `console.enabled`, the
operator console, against a PostgreSQL 18 you run yourself. Install it from
a checkout:

```sh
kubectl create secret generic ferropix-db \
  --from-literal=url='postgresql://ferropix:…@postgres:5432/ferropix'
kubectl create secret generic ferropix-data-key \
  --from-literal=data-key="$(openssl rand -base64 32)"
helm install ferropix deploy/helm/ferropix \
  --set database.existingSecret=ferropix-db \
  --set dataKey.existingSecret=ferropix-data-key \
  -f my-values.yaml
```

Both containers carry a memory and an ephemeral-storage limit. The server's
ephemeral storage, 256 MiB by default (`resources`), holds the audit spool on
its default `emptyDir` (`[audit] spool_max_size`, 64 MiB), `/tmp` and the
container log: raise it with the spool, or keep the spool on a
PersistentVolumeClaim (`state.existingClaim`).

The release lane publishes the chart as an OCI artifact,
`oci://ghcr.io/ferrohealth/charts/ferropix`, with the release as its app
version and both images pinned by the digests it scanned, and attests its
build provenance. A published chart version is never published again with
other files, so each release that changes the chart, its app version included,
raises the chart version: FerroPIX 0.0.28 is chart 0.3.21. Install a published
chart by its version and verify it first:

```sh
gh attestation verify oci://ghcr.io/ferrohealth/charts/ferropix:<chart version> \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/build-chart.yml
helm install ferropix oci://ghcr.io/ferrohealth/charts/ferropix \
  --version <chart version> -f my-values.yaml
```

The server's Service is `<release>-ferropix` on port `8080`, and PIXm and
PDQm answer under its `/fhir`. A chart that takes this one as a dependency
reads that base from the named template `ferropix.fhirBaseUrl`. The chart's
README lists the addresses and shows how a domain and its Source client are
declared.

The server needs at least one issuer, so the chart refuses to render
without a `config.auth.issuer` entry. Each entry is one `[[auth.issuer]]`
table ([`[auth]`](configuration.md#auth)) with its `kind` and its own
`audience`:

```yaml
config:
  auth:
    issuer:
      - issuer: "https://idp.example.org/realms/care"
        kind: machines
        audience: ["https://pix.example.org/fhir"]
```

The server reads the audience of each issuer from its entry. CI renders the chart over every
value set under `deploy/helm/ci`, the defaults among them, and runs each
image's `config check` over the configuration it renders
(`deploy/helm/config-check.sh`); the release does the same with the images
it pins before it publishes the chart.

No credential is a value of the chart. The PostgreSQL URL, the sealing key
and the console's OIDC client secret are Secrets you create; the chart
mounts each as a file and the configuration names it by its `_file` key, so
a chart without them refuses to render.

| Value | Default | What it sets |
|---|---|---|
| `config` | a production profile on `https://pix.example.org` | The server's configuration file as a map, rendered as TOML; every key of [Configuration](configuration.md) but the secret files, which the chart sets. |
| `database.existingSecret`, `database.key` | none, `url` | The Secret and key that hold the PostgreSQL URL. |
| `dataKey.existingSecret`, `dataKey.key` | none, `data-key` | The Secret and key that hold the sealing key. |
| `usageReport.enabled` | `true` | The installation report, stating `helm`; `false` stops it. |
| `image.tag`, `image.digest` | the chart's app version, none | The server image; a digest wins over the tag. |
| `state.existingClaim` | none | A PersistentVolumeClaim for the audit spool, which is an `emptyDir` without one. |
| `service.port`, `service.mllpPort` | `8080`, `2575` | The HTTP Service and, with `config.hl7v2.enabled`, the MLLP Service. |
| `networkPolicy.enabled`, `networkPolicy.ingressFrom` | `true`, every source | The NetworkPolicy of the serving ports and the sources it admits. |
| `podDisruptionBudget.enabled` | `true` | A PodDisruptionBudget of one pod unavailable at a time. |
| `extraContainers` | none | Containers beside the server, such as a TLS proxy. |
| `hostUsers` | `false` | Whether the server's and the console's pods share the node's user namespace. `false` gives each pod its own, which needs nodes whose container runtime and kernel support user namespaces; a pod on a node without them does not start. |
| `supplementalGroupsPolicy` | `Strict` | `Strict` gives a container only the groups the pod spec names, never those of the image's `/etc/group`; `Merge` is the Kubernetes default, and an empty value sets no field. |
| `console.enabled`, `console.config`, `console.clientSecret.existingSecret` | `false`, an OIDC example, none | The console, its configuration as a map (the server's Service is its default `pix.base_url`), and the Secret of its client secret. |

Every container runs as user 65532 with a read-only root filesystem, no
privilege escalation, every capability dropped and the `RuntimeDefault`
seccomp profile, the Restricted Pod Security Standard. Both pods set
`enableServiceLinks: false`, so no Service of the namespace reaches a
container as environment variables. Before Kubernetes 1.33 the feature gates
of `hostUsers` and `supplementalGroupsPolicy` are off by default, and the API
server drops both fields. The server's startup
and readiness probes ask `/health/readiness` and its liveness probe
`/health/liveness`; the console's ask `/health/readiness` and
`/health/liveness`. `deploy/helm/validate.sh` lints and renders the chart and holds
the manifests to the Kubernetes schemas with `kubeconform`, and
`deploy/helm/kind-check.sh` installs it on a throwaway kind cluster with
the published images and waits until its health routes answer.

## Building the image locally

`scripts/release/stage-dist.sh <version>` downloads the musl binaries of a
published release, checks each against its published checksum and stages
them with the state tree under `dist/`. To package a build of your own,
stage its binary instead:

```sh
cargo build --release --locked -p ferropix-server --target x86_64-unknown-linux-musl
mkdir -p dist/linux_amd64
install -m 0555 target/x86_64-unknown-linux-musl/release/ferropix dist/linux_amd64/
scripts/release/stage-dist.sh tree
docker buildx build -f docker/Dockerfile --platform linux/amd64 \
  -t ghcr.io/ferrohealth/ferropix:<version> --load .
```
