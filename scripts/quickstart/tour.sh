#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# tour.sh: the compose quickstart the book walks through, as one script (no
# specification governs this: our own design).
#
#   scripts/quickstart/tour.sh               run the tour against compose.yaml
#   scripts/quickstart/tour.sh --self-test   check the anchors the book and
#                                            the README read
#
# The book's quickstart page shows the commands between each `ANCHOR:` and
# `ANCHOR_END:` line through mdBook's anchored {{#include}}, and the
# `quickstart` job of ci.yml runs this script against the images it builds,
# so a command the book shows is a command CI ran. GitHub renders no include,
# so README.md carries a copy of every anchor, each fenced block under a
# `<!-- tour:<anchor> -->` line, and --self-test refuses a copy that differs
# from the script by a byte. Every step fails the
# script when its answer is not the one the book describes: curl refuses a
# status of 400 or above, and each `jq -e` refuses an answer without the
# value it selects.
#
# It writes the secrets under ./secrets, starts the server, PostgreSQL, the
# development issuer and the console, feeds one synthetic patient (a BSN
# from the 99999 range no real person holds) and queries it through PIXm and
# PDQm. Run it from a checkout with docker compose (or podman compose), curl,
# jq and openssl. `docker compose down --volumes` removes what it started.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly SELF=scripts/quickstart/tour.sh

# anchor_body NAME: the lines between `# ANCHOR: NAME` and its end.
anchor_body() {
  awk -v name="$1" '$0 == "# ANCHOR_END: " name { p = 0 } p { print } $0 == "# ANCHOR: " name { p = 1 }' "$SELF"
}

# readme_body NAME: the fenced block under README.md's `<!-- tour:NAME -->`.
readme_body() {
  awk -v name="$1" '
    $0 == "<!-- tour:" name " -->" { want = 1; next }
    want && $0 == "```sh" { want = 0; p = 1; next }
    p && $0 == "```" { exit }
    p { print }' README.md
}

# self_test: every anchor opens and closes once, every anchor the book
# includes from this script exists, and README.md shows every anchor as the
# script holds it.
self_test() {
  cd "$root"
  bash -n "$SELF" || { echo "tour: self-test failed: $SELF does not parse." >&2; exit 1; }
  local opened closed included failed=0 name
  opened="$(sed -n 's/^# ANCHOR: \([a-z-]*\)$/\1/p' "$SELF" | sort)"
  closed="$(sed -n 's/^# ANCHOR_END: \([a-z-]*\)$/\1/p' "$SELF" | sort)"
  if [[ -z "$opened" || "$opened" != "$closed" ]]; then
    echo "tour: self-test failed: the ANCHOR and ANCHOR_END lines do not pair up." >&2
    failed=1
  fi
  if [[ "$(uniq -d <<< "$opened")" != "" ]]; then
    echo "tour: self-test failed: an anchor is opened twice." >&2
    failed=1
  fi
  included="$(grep -rhoE '\{\{#include [./]*scripts/quickstart/tour\.sh:[a-z-]+\}\}' website/book/src \
    | sed 's/.*tour\.sh:\([a-z-]*\)}}/\1/' | sort -u)"
  if [[ -z "$included" ]]; then
    echo "tour: self-test failed: no page of the book includes $SELF." >&2
    failed=1
  fi
  while IFS= read -r name; do
    [[ -z "$name" ]] && continue
    if ! grep -qx "$name" <<< "$opened"; then
      echo "tour: self-test failed: the book includes the anchor '$name', which $SELF lacks." >&2
      failed=1
    fi
  done <<< "$included"
  while IFS= read -r name; do
    [[ -z "$name" ]] && continue
    if ! grep -qx "<!-- tour:$name -->" README.md; then
      echo "tour: self-test failed: README.md shows no copy of the anchor '$name'." >&2
      failed=1
    elif [[ "$(readme_body "$name")" != "$(anchor_body "$name")" ]]; then
      echo "tour: self-test failed: README.md's copy of the anchor '$name' differs from $SELF." >&2
      failed=1
    fi
  done <<< "$opened"
  while IFS= read -r name; do
    [[ -z "$name" ]] && continue
    if ! grep -qx "$name" <<< "$opened"; then
      echo "tour: self-test failed: README.md shows the anchor '$name', which $SELF lacks." >&2
      failed=1
    fi
  done < <(sed -n 's/^<!-- tour:\([a-z-]*\) -->$/\1/p' README.md)
  [[ "$failed" == 0 ]] || exit 1
  echo "tour: self-test OK."
}

if [[ "${1:-}" == --self-test ]]; then
  self_test
  exit 0
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--self-test]" >&2
  exit 2
fi

cd "$root"

# A tour is a throwaway run on synthetic data, never an installation, so it
# sends no usage report; `docker compose up` by hand leaves it on.
export FERROPIX__USAGE_REPORT__ENABLED=false

# ANCHOR: secrets
mkdir -p secrets && umask 077
openssl rand -base64 32 > secrets/ferropix-data-key
openssl rand -hex 16 > secrets/postgres-password
openssl rand -hex 16 > secrets/ferropix-db-password
printf 'postgresql://ferropix:%s@postgres:5432/ferropix' \
  "$(cat secrets/ferropix-db-password)" > secrets/ferropix-db-url
openssl rand -hex 16 > secrets/console-oidc-client-secret
# ANCHOR_END: secrets

# ANCHOR: start
docker compose up --wait
curl --fail-with-body -s http://localhost:8080/health/readiness
# ANCHOR_END: start
echo

# ANCHOR: token
token="$(curl --fail-with-body -s http://localhost:8180/ferropix/token \
  -d grant_type=client_credentials -d client_id=quickstart-source \
  -d client_secret=any -d 'scope=system/Patient.cud system/Patient.rs' \
  | jq -er .access_token)"
# ANCHOR_END: token

# ANCHOR: feed
curl --fail-with-body -s -X PUT \
  'http://localhost:8080/fhir/Patient?identifier=urn:oid:2.999.1.1|H-100001' \
  -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/fhir+json' \
  --data-binary @- <<'EOF' | jq -e '.issue[0].severity == "information"'
{
  "resourceType": "Patient",
  "identifier": [
    { "system": "urn:oid:2.999.1.1", "value": "H-100001" },
    { "system": "http://fhir.nl/fhir/NamingSystem/bsn", "value": "999990007" }
  ],
  "name": [{ "family": "Proefsma", "given": ["Ada"] }],
  "gender": "female",
  "birthDate": "1961-04-12"
}
EOF
# ANCHOR_END: feed

# The `$` names a FHIR operation, never a shell expansion.
# shellcheck disable=SC2016
# ANCHOR: identifiers
curl --fail-with-body -s -X POST 'http://localhost:8080/fhir/Patient/$ihe-pix' \
  -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/fhir+json' \
  --data-binary @- <<'EOF' | jq -e '.parameter[] | select(.name == "targetIdentifier") | .valueIdentifier'
{
  "resourceType": "Parameters",
  "parameter": [
    { "name": "sourceIdentifier", "valueString": "urn:oid:2.999.1.1|H-100001" },
    { "name": "targetSystem", "valueString": "http://fhir.nl/fhir/NamingSystem/bsn" }
  ]
}
EOF
# ANCHOR_END: identifiers

# ANCHOR: demographics
curl --fail-with-body -s -X POST 'http://localhost:8080/fhir/Patient/_search' \
  -H "Authorization: Bearer $token" \
  -H 'Accept: application/fhir+json' \
  -d family=proefsma -d birthdate=1961-04-12 \
  | jq -e '.total == 1 and .entry[0].resource.name[0].family == "Proefsma"'
# ANCHOR_END: demographics

# ANCHOR: console
docker compose --profile console up --wait
# ANCHOR_END: console
curl --fail-with-body -s -o /dev/null http://localhost:3000/health/readiness
echo "tour: every step answered as the book describes."
