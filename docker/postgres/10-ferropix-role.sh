#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The quickstart's database role, run once by the official PostgreSQL image
# when it initialises an empty data directory (compose.yaml mounts this file
# under /docker-entrypoint-initdb.d/). The server refuses a superuser at boot,
# because a superuser reads every schema, so it connects as `ferropix`, a
# login role that may create the `pix` schema in the `ferropix` database and
# owns nothing else. The password is the file secret ferropix-db-password,
# which psql reads itself, so it reaches neither the environment nor a
# command line; the URL in the ferropix-db-url secret carries the same one.
# No specification governs this: our own design.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<'SQL'
\set password `cat /run/secrets/ferropix-db-password`
CREATE ROLE ferropix LOGIN PASSWORD :'password';
GRANT CREATE ON DATABASE ferropix TO ferropix;
SQL
