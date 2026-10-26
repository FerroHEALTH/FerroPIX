<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Operations

This page covers the server's health checks and request log, and what
operating the crates involves. The installation report the server sends
is set under [`[usage_report]`](configuration.md#usage_report), the span
export under [`[telemetry]`](configuration.md#telemetry), and an upgrade
of the server applies its migrations with `db migrate` or at the start of
`serve` ([Deployment](deployment.md)).

## Health checks

| Route | Token | Answers |
|---|---|---|
| `GET {base}/health` | none | `200` while the process is up. |
| `GET {base}/health/readiness` | none | `200` while every readiness indicator is up, `503` while one is down, with a JSON document naming each indicator, its state and, when it is down, why. |
| `GET {base}/operator/dependencies` | `auth.steward_scope` | `200` with the readiness document and the last outcome of every upstream the server sends to. |

The `database` indicator is down while no statement reaches PostgreSQL,
and the `audit_spool` indicator is down when less than a tenth of the
spool's record or byte bound is left. The `outbox` indicator is down while
a due outbound message, an XCPD answer, an ITI-93 message or an ITI-10
notification, has waited past the sender's lease and two idle wakes, which
a running sender never lets happen; a subscriber or consumer that refuses
its messages leaves the server ready. The dependency report names every
PMIR subscriber and every HL7 v2 consumer, the consumer as `mllp://` and its
`host:port`, with the outcome of its latest message. With the HL7 v2 face on, the `hl7v2_listener` indicator is down
while the MLLP listener is not accepting. Each check
gets two seconds; one that takes longer counts as down. The dependency
report sits behind the steward scope so an anonymous reader learns no
endpoint. `ferropix healthcheck` asks the readiness route of the server on
the same host and exits 1 on anything but `200`: use it as the container
health check.

## The request log

The server writes one line per request under the `tracing` target
`ferropix_server::request`, with these fields:

| Field | What it holds |
|---|---|
| `request_id` | The `X-Request-Id` the client sent, or the one the server minted. |
| `method` | The HTTP method. |
| `route` | The matched route template, such as `/fhir/Patient/$ihe-pix`, or `<unmatched>`. |
| `status` | The status answered. |
| `latency_ms` | The time to the response, in milliseconds. |
| `caller` | The token's `client_id`, or its subject when it names none, or `-` when no credential held. |
| `domain` | The identifier domains a feed wrote to, or `-`. |

The line never holds the path as sent, a query string, a body or a header
value. A PIXm Consumer that calls `$ihe-pix` by `GET` puts the patient's
identifier in the query, and the line still names only the route template.
A proxy in front of the server must keep query strings out of its own logs
for the same reason. The gate of `ferrohealth-smart` writes its own audit
line per guarded request under the target `ferrohealth_smart::audit`, with
the same discipline.

## Releases and support

- **Only the newest release is supported.** When a release is published,
  every earlier one stops receiving fixes: there are no maintenance
  branches and no backports. Security fixes keep being published for at
  least five years after each release, in the newest release
  ([`SECURITY.md`](https://github.com/FerroHEALTH/FerroPIX/blob/main/SECURITY.md)).
- **A FerroPIX release is cut from its milestone.** The release workflow
  refuses a tag whose milestone still holds an open issue, and closes the
  milestone once every leg has published. The
  [milestones](https://github.com/FerroHEALTH/FerroPIX/milestones) are the
  roadmap.
- **The crates are published between releases.** A crate's version moves
  with any change to what it ships, so a fix reaches crates.io without
  waiting for a product release.

## Upgrading a crate

Read the crate's entries in the [changelog](../changelog.md) for the versions
you skip. A change that breaks the API is marked **API break** with what to
change. The family pins its crates by exact version, so `ihe-iti` moves
together with the `ihe-atna` version it names.

## Reporting a defect

[Open an issue](https://github.com/FerroHEALTH/FerroPIX/issues/new/choose)
when a transaction answers differently from what its specification names, or
identifying data reaches a log line. Never paste patient data or a real
identifier. The family's
[post-market procedure](https://ferrohealth.eu/docs/post-market.html) says
how complaints and incidents are handled.
