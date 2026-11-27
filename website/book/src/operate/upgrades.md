<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Upgrading the server

**Audience:** operators who move a running FerroPIX to a newer release.

Only the newest release is supported
([Releases, upgrades and support](releases.md#support)), so plan to follow
each release. [What FerroPIX stores](deployment.md#what-ferropix-stores)
lists the state an upgrade carries across.

## Before the upgrade

1. **Read the changelog** for every version you skip. A change that asks
   something of you, such as a configuration key renamed or removed, says
   what to change ([Changelog](../changelog.md)).
2. **Check the configuration with the new binary.** Run `ferropix config
   check` of the new release over your configuration
   ([Commands](configuration.md#commands)). A key a release
   renamed or removed is refused by name in that one release, and as an
   unknown key from the next release on, so upgrade one release at a time
   when you skip several, or read each skipped section's Removed entries.
3. **Back up the store.** Dump schema `pix` of the PostgreSQL database
   (`pg_dump --schema=pix`), and keep the file `db.data_key_file` names
   beside the dump in your secret store: the identifiers in the dump open
   only with that key. The migrations only move forward, so the backup is
   your way back.

## The rollout

- **Migrations.** `serve` applies the pending migrations of its release
  when it starts, under the migrator's advisory lock, so servers that start
  together apply each file once
  ([The store](configuration.md#the-store)). With `db.migrate = false`, run
  `ferropix db migrate` of the new release first, as the role that may
  create in the schema, then start the new servers; a server that does not
  migrate refuses to start while a migration of its release is missing.
- **Scoring fields.** When a release changes how the matcher normalises a
  field, the same step rewrites the stored scoring fields and blocking keys
  of every record, a thousand at a time, before the server takes traffic.
  A large store takes longer to start on such a release.
- **The images.** With the chart, `helm upgrade` to the chart version of the
  release ([The Helm chart](helm-chart.md)); with compose, download the
  release's `compose.yaml` or set `FERROPIX_VERSION`. Upgrade the console to
  the same release: it reads the operator documents of its own release.
- **The audit spool** keeps its records across the restart, and the new
  server delivers them. Stop the old server with `SIGTERM`, so the requests
  in flight finish within `server.shutdown_timeout`.

## Rolling back

Restore the backup and start the release it was taken under. Do not start
an older release against a store a newer one migrated: with `db.migrate =
true` it refuses the migrations it does not know, and with `db.migrate =
false` it would run against tables its code was not written for.

## After the upgrade

- `GET {base}/health/readiness` answers `200`, and
  `GET {base}/operator/dependencies` shows each upstream's last outcome
  ([Operations](operations.md)).
- `ferropix config check` prints the configuration the server runs on,
  with the source of each value.
- The sealing key is untouched by an upgrade;
  [rotating it](configuration.md#the-store) is its own step.
