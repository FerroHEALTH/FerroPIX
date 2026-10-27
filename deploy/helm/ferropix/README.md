<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The FerroPIX chart

FerroPIX is the FerroHEALTH Master Patient Index over IHE PIXm, PDQm, PMIR
and the HL7 v2 identity transactions. This chart runs its server against an
external PostgreSQL 18, and its operator console when you switch it on.

Each release publishes the chart as an OCI artifact, with its images pinned
by digest and a build provenance attestation:

```sh
helm install ferropix oci://ghcr.io/ferrohealth/charts/ferropix --version <chart version>
gh attestation verify oci://ghcr.io/ferrohealth/charts/ferropix:<chart version> \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/build-chart.yml
```

The configuration reference is <https://ferropix.eu/docs/operate/configuration.html>,
and deployment is described at <https://ferropix.eu/docs/operate/deployment.html>.

## Before you install

Create the Secrets the values name. No credential is a value of the chart:

- `database.existingSecret`: the PostgreSQL connection URL under
  `database.key` (default `url`), for example
  `postgresql://ferropix:…@postgres:5432/ferropix`.
- `dataKey.existingSecret`: the sealing key of the identifier values under
  `dataKey.key` (default `data-key`), the base64 of 32 random bytes
  (`openssl rand -base64 32`).
- With the console on, `console.clientSecret.existingSecret`: the console's
  OIDC client secret.

## The Service that serves PIXm and PDQm

The server's Service is named after the release, `<release>-ferropix`, or
`<release>` when the release name already contains `ferropix`. It listens on
`service.port` (default `8080`), and the FHIR face is under `/fhir` below
`config.server.base_path`. In the cluster:

| Transaction | Address |
|---|---|
| PIXm ITI-83 | `http://<service>.<namespace>.svc:8080/fhir/Patient/$ihe-pix` |
| PIXm ITI-104 | `http://<service>.<namespace>.svc:8080/fhir/Patient` (`PUT`, `DELETE`) |
| PDQm ITI-78, ITI-119 | `http://<service>.<namespace>.svc:8080/fhir/Patient`, `/fhir/Patient/$match` |
| PMIR ITI-93, ITI-94 | `http://<service>.<namespace>.svc:8080/fhir`, `/fhir/Subscription` |

A chart that takes this one as a dependency reads that base from the named
template `ferropix.fhirBaseUrl`, rendered in the subchart's scope:

```yaml
# In the parent chart's template:
value: {{ include "ferropix.fhirBaseUrl" (dict "Values" .Values.ferropix "Release" .Release "Chart" (dict "Name" "ferropix")) | quote }}
```

With `config.hl7v2.enabled`, the MLLP listener has its own Service,
`<service>-mllp`, on `service.mllpPort` (default `2575`). With
`console.enabled`, the console's Service is `<service>-console` on
`console.service.port` (default `3000`).

## Declaring a domain and its Source

An identifier domain and the one client that feeds it are values under
`config`. The issuer that signs the client's tokens lists the client in the
`clients` of its `trust.issuer` entry, and the domain names it as its
`source_client_id`:

```yaml
config:
  public_base_url: "https://pix.example.org"
  auth:
    audience:
      - "https://pix.example.org/fhir"
    issuer:
      - issuer: "https://idp.example.org/realms/care"
  trust:
    issuer:
      - issuer: "https://idp.example.org/realms/care"
        clients: ["ferroehr"]
  domain:
    - system: "urn:oid:2.999.1.1"
      name: "Clinical domain"
      kind: local
      source_client_id: ferroehr
```

The client then calls PIXm and PDQm with a client-credentials token from
that issuer: `system/Patient.rs` to read, and `system/Patient.cud` to feed
its own domain. A domain's `kind` is `local`, `authoritative` (a national
number such as the BSN, which links records across domains) or `master`, the
PMIR Master Patient Identification Domain, whose system is
`config.public_base_url`. The seed adds a domain the store lacks and
leaves one it holds as it is; the operator console changes a domain after
that.

## Values

`values.yaml` documents every value. The ones you set for every install are
`config.public_base_url`, `config.auth`, `config.domain`,
`database.existingSecret` and `dataKey.existingSecret`. The installation
report to FerroPULSE is on by default; set `usageReport.enabled=false` to
stop it.

Both pods set `enableServiceLinks: false` and, by default, run in their own
user namespace (`hostUsers: false`) with the `Strict` supplemental groups
policy. Set `hostUsers: true` when your nodes do not support user
namespaces, or the pods do not start.

Both containers set a memory and an ephemeral-storage limit (`resources`,
`console.resources`). The server's ephemeral storage, 256 MiB by default,
covers the audit spool on its default `emptyDir` (`[audit] spool_max_bytes`,
64 MiB), `/tmp` and the container log: raise it with the spool, or keep the
spool on a PersistentVolumeClaim (`state.existingClaim`).
