<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The Helm chart

**Audience:** operators who run FerroPIX on Kubernetes.

`deploy/helm/ferropix` installs the server and, with `console.enabled`, the
operator console, against a PostgreSQL 19 you run yourself. Install it from
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
raises the chart version, so the chart version and the app version differ. Install a published
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
without a `config.auth.issuer` entry. Each entry is one
`[auth.issuer.<name>]` table ([`[auth]`](configuration.md#auth)) by its
name, with its `issuer`, and `kind: federation` only for another party's
authorization server:

```yaml
config:
  auth:
    issuer:
      care:
        issuer: "https://idp.example.org/realms/care"
```

A token's audience is the FHIR base, `{public_base_url}/fhir`, unless the
entry sets its own `audience`. CI renders the chart over every
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
