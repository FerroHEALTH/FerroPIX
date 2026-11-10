#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# Builds the quickstart bundle a release attaches: a reader downloads it from
# the latest release and runs the quickstart from it, so the compose file,
# every file it mounts and the images it runs come from one release, and the
# reader never clones the repository (owner decision, 2026-11-10; #619). No
# specification governs this file: our own design.
#
#   scripts/release/quickstart-bundle.sh OUT [--image NAME=REF]...
#       Writes OUT/ferropix-quickstart/ (compose.yaml, every repository file
#       it mounts at its own path, and scripts/quickstart/tour.sh),
#       OUT/quickstart.tar.gz holding that directory, and OUT/compose.yaml.
#       Each --image replaces the image line of the compose service NAME
#       (ferropix, console) with REF: the release pins
#       ghcr.io/ferrohealth/ferropix:<version>@sha256:<digest>, CI the image
#       it built. A ./ path compose.yaml mounts that the bundle lacks fails
#       the build; the secrets and the terminology directory are the reader's.
#   scripts/release/quickstart-bundle.sh --self-test
#       Builds the bundle from this checkout and proves the image pinning and
#       the refusal of a mount the bundle would lack.
#
# Exit 0 on a bundle, 1 naming what is wrong, 2 on a usage error.
set -euo pipefail

readonly DIR=ferropix-quickstart
# The repository files the bundle carries beside compose.yaml.
readonly FILES=(
  app/ferropix-server/quickstart.toml
  docker/postgres/10-ferropix-role.sh
  scripts/quickstart/tour.sh
)
# The mounted paths the reader makes: the secrets the quickstart writes and
# the optional terminology packages of the HL7 v2 face.
readonly READER_PATHS='^\./(secrets|terminology)(/|$)'

usage() {
  echo "usage: $0 OUT [--image NAME=REF]... | --self-test" >&2
  exit 2
}

# mounts COMPOSE: every ./ path COMPOSE mounts or reads, one per line.
mounts() {
  sed -E 's/#.*//' "$1" | grep -oE '(^|[[:space:]:-])\./[A-Za-z0-9._/-]+' | sed -E 's/^[[:space:]:-]//' | sort -u
}

# build ROOT OUT IMAGE...: the bundle of the checkout at ROOT into OUT.
build() {
  local root=$1 out=$2 spec name ref path bundle
  shift 2
  bundle="$out/$DIR"
  rm -rf "$bundle"
  mkdir -p "$bundle"
  cp "$root/compose.yaml" "$bundle/compose.yaml"
  for path in "${FILES[@]}"; do
    mkdir -p "$bundle/$(dirname "$path")"
    cp -p "$root/$path" "$bundle/$path"
  done
  for spec in "$@"; do
    name="${spec%%=*}"
    ref="${spec#*=}"
    [[ -n "$name" && -n "$ref" && "$name" != "$spec" ]] || usage
    # The image line of service NAME: the first `image:` after `  NAME:`.
    NAME="$name" REF="$ref" perl -0pi -e \
      'BEGIN { $n = quotemeta $ENV{NAME}; $r = $ENV{REF} } s/(\n  $n:(?:(?!\n  \S).)*?\n    image: )[^\n]+/$1$r/s or die "no image line under service $ENV{NAME}\n"' \
      "$bundle/compose.yaml" || {
      echo "quickstart-bundle: compose.yaml has no service $name with an image" >&2
      return 1
    }
  done
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    [[ "$path" =~ $READER_PATHS ]] && continue
    if [[ ! -e "$bundle/$path" ]]; then
      echo "quickstart-bundle: compose.yaml mounts $path, which the bundle lacks; add it to FILES" >&2
      return 1
    fi
  done < <(mounts "$bundle/compose.yaml")
  tar -C "$out" -czf "$out/quickstart.tar.gz" "$DIR"
  cp "$bundle/compose.yaml" "$out/compose.yaml"
}

self_test() {
  local root work fails=0
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' RETURN
  if ! build "$root" "$work/a" "ferropix=ghcr.io/ferrohealth/ferropix:9.9.9@sha256:aa" "console=ghcr.io/ferrohealth/ferropix-console:9.9.9@sha256:bb" 2> "$work/err"; then
    echo "quickstart-bundle: self-test failed: the checkout's bundle did not build: $(cat "$work/err")" >&2
    fails=1
  else
    grep -qx '    image: ghcr.io/ferrohealth/ferropix:9.9.9@sha256:aa' "$work/a/$DIR/compose.yaml" ||
      { echo "quickstart-bundle: self-test failed: the server image is not pinned." >&2; fails=1; }
    grep -qx '    image: ghcr.io/ferrohealth/ferropix-console:9.9.9@sha256:bb' "$work/a/$DIR/compose.yaml" ||
      { echo "quickstart-bundle: self-test failed: the console image is not pinned." >&2; fails=1; }
    tar -tzf "$work/a/quickstart.tar.gz" | grep -qx "$DIR/app/ferropix-server/quickstart.toml" ||
      { echo "quickstart-bundle: self-test failed: the archive lacks the configuration." >&2; fails=1; }
  fi
  mkdir -p "$work/fixture/app/ferropix-server" "$work/fixture/docker/postgres" "$work/fixture/scripts/quickstart"
  for path in "${FILES[@]}"; do : > "$work/fixture/$path"; done
  printf 'services:\n  ferropix:\n    image: x\n    volumes:\n      - ./extra.toml:/etc/extra.toml:ro\n' > "$work/fixture/compose.yaml"
  if build "$work/fixture" "$work/b" 2> /dev/null; then
    echo "quickstart-bundle: self-test failed: a mount the bundle lacks passed." >&2
    fails=1
  fi
  if build "$work/fixture" "$work/c" "nosuch=ref" 2> /dev/null; then
    echo "quickstart-bundle: self-test failed: an image for a missing service passed." >&2
    fails=1
  fi
  [[ "$fails" == 0 ]] || exit 1
  echo "quickstart-bundle: self-test OK."
}

[[ $# -ge 1 ]] || usage
if [[ "$1" == --self-test ]]; then
  self_test
  exit 0
fi
out=$1
shift
images=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --image)
      images+=("${2:?usage: --image NAME=REF}")
      shift 2
      ;;
    *) usage ;;
  esac
done
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
build "$root" "$out" ${images[@]+"${images[@]}"}
echo "quickstart-bundle: $out/quickstart.tar.gz and $out/compose.yaml"
