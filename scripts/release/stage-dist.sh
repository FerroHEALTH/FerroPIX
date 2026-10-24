#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# Stages the build context docker/Dockerfile copies from: the musl binary of
# each platform under dist/<os>_<arch>/ferropix, and the server's state tree
# under dist/state/ (the audit spool directory, mode 0700). The base image has
# no shell to create a directory, and git keeps no empty one, so the tree is
# made here, with the mode the spool refuses to start without.
#
#   scripts/release/stage-dist.sh tree
#       Stages the state tree alone. release-image.yml runs it after it has
#       verified and unpacked the tarballs of its own run, and the CI dry run
#       runs it beside placeholder binaries.
#   scripts/release/stage-dist.sh <version>
#       Downloads the published musl tarballs of release v<version>, checks
#       each against the .sha256sum published beside it before a byte is
#       unpacked, stages both binaries and the state tree, so the image can be
#       built locally from the bytes the release attached:
#         docker buildx build -f docker/Dockerfile --platform linux/arm64 \
#           -t ghcr.io/ferrohealth/ferropix:<version> --load .
#   scripts/release/stage-dist.sh --self-test
#       Proves the staging, the checksum refusal and the tree offline, with
#       a stub `gh` serving fixture tarballs.
#
# Needs the GitHub CLI (`gh`) and `sha256sum` or `shasum` for a release. No
# specification governs this file: our own design, after FerroFED's script of
# the same name.
set -euo pipefail

readonly REPO=FerroHEALTH/FerroPIX
# The Docker platform suffix each published musl target is staged under.
readonly TARGETS="x86_64-unknown-linux-musl|linux_amd64
aarch64-unknown-linux-musl|linux_arm64"

die() {
  echo "stage-dist: $*" >&2
  exit 1
}

usage() {
  echo "usage: scripts/release/stage-dist.sh tree | <version> | --self-test" >&2
  exit 2
}

sha() {
  local file="$1"
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum > /dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    die "neither sha256sum nor shasum is installed"
  fi
}

# tree DIST: the state tree under DIST/state, the audit spool private to the
# runtime user (docker/Dockerfile chowns it to 65532).
tree() {
  local dist="$1"
  mkdir -p "$dist/state"
  chmod 0700 "$dist/state"
  mkdir -p "$dist/state/audit"
  chmod 0700 "$dist/state/audit"
  echo "staged $dist/state/audit (0700)"
}

# release VERSION DIST: the binaries of release vVERSION under DIST, each
# checked against its published checksum first, and the state tree.
release() {
  local version="$1" dist="$2" work target platform asset want got
  command -v gh > /dev/null 2>&1 || die "the GitHub CLI (gh) is not installed"
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now, while it is in scope
  trap "rm -r -- '$work'" RETURN
  while IFS='|' read -r target platform; do
    asset="ferropix-v${version}-${target}.tar.gz"
    gh release download "v${version}" --repo "$REPO" \
      --pattern "$asset" --pattern "$asset.sha256sum" --dir "$work" --clobber ||
      die "release v${version} has no $asset"
    want="$(awk '{print $1; exit}' "$work/$asset.sha256sum")"
    got="$(sha "$work/$asset")"
    [[ -n "$want" && "$want" = "$got" ]] ||
      die "$asset: sha256 $got does not match the published $want"
    mkdir -p "$dist/$platform"
    tar -xzf "$work/$asset" -C "$dist/$platform" ferropix
    chmod 0555 "$dist/$platform/ferropix"
    echo "staged $dist/$platform/ferropix from $asset ($got)"
  done <<< "$TARGETS"
  tree "$dist"
}

self_test() {
  local tmp failures=0 mode
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $tmp now, while it is in scope
  trap "chmod -R u+w '$tmp'; rm -r -- '$tmp'" RETURN
  mkdir -p "$tmp/bin" "$tmp/assets" "$tmp/pkg"
  printf '#!/bin/sh\necho fixture\n' > "$tmp/pkg/ferropix"
  local target
  while IFS='|' read -r target _; do
    tar -C "$tmp/pkg" -czf "$tmp/assets/ferropix-v9.9.9-${target}.tar.gz" ferropix
    (cd "$tmp/assets" && sha "ferropix-v9.9.9-${target}.tar.gz" | awk -v f="ferropix-v9.9.9-${target}.tar.gz" '{print $1 "  " f}' \
      > "ferropix-v9.9.9-${target}.tar.gz.sha256sum")
  done <<< "$TARGETS"
  # A stub gh: `release download <tag> --repo R --pattern P... --dir D --clobber`
  # copies each pattern from the fixture directory, and fails on a missing one.
  cat > "$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
dir="" patterns=()
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --pattern) patterns+=("$2"); shift 2 ;;
    --dir) dir="$2"; shift 2 ;;
    *) shift ;;
  esac
done
for p in "${patterns[@]}"; do cp "$STAGE_FIXTURES/$p" "$dir/$p"; done
STUB
  chmod +x "$tmp/bin/gh"

  if ! (PATH="$tmp/bin:$PATH" STAGE_FIXTURES="$tmp/assets" release 9.9.9 "$tmp/dist") > /dev/null; then
    echo "self-test: a release with matching checksums was refused" >&2
    failures=1
  fi
  for platform in linux_amd64 linux_arm64; do
    [[ -x "$tmp/dist/$platform/ferropix" ]] || {
      echo "self-test: $platform was not staged" >&2
      failures=1
    }
  done
  mode="$(stat -c '%a' "$tmp/dist/state/audit" 2> /dev/null || stat -f '%Lp' "$tmp/dist/state/audit")"
  [[ "$mode" = "700" ]] || {
    echo "self-test: the audit spool was staged $mode, not 700" >&2
    failures=1
  }

  printf '%064d  ferropix-v9.9.9-x86_64-unknown-linux-musl.tar.gz\n' 0 \
    > "$tmp/assets/ferropix-v9.9.9-x86_64-unknown-linux-musl.tar.gz.sha256sum"
  if (PATH="$tmp/bin:$PATH" STAGE_FIXTURES="$tmp/assets" release 9.9.9 "$tmp/refused") > /dev/null 2>&1; then
    echo "self-test: a tarball whose checksum differs was staged" >&2
    failures=1
  fi
  [[ ! -e "$tmp/refused/linux_amd64/ferropix" ]] || {
    echo "self-test: a refused tarball left a binary behind" >&2
    failures=1
  }
  [[ "$failures" -eq 0 ]] || return 1
  echo "stage-dist: self-test OK (both platforms and the 0700 spool staged, a checksum mismatch refused)."
}

case "${1:-}" in
  --self-test)
    [[ "$#" -eq 1 ]] || usage
    self_test
    ;;
  tree)
    [[ "$#" -eq 1 ]] || usage
    cd "$(dirname "${BASH_SOURCE[0]}")/../.."
    tree dist
    ;;
  "") usage ;;
  *)
    [[ "$#" -eq 1 ]] || usage
    version="${1#v}"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "'$1' is not a version"
    cd "$(dirname "${BASH_SOURCE[0]}")/../.."
    release "$version" dist
    ;;
esac
