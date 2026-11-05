#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# Installs the pinned Trivy on a Linux x86_64 runner from its GitHub release
# tarball, verified against the sha256 below, and puts it on GITHUB_PATH.
#
#   scripts/release/install-trivy.sh <version> <bindir>
#   scripts/release/install-trivy.sh --sha256 <version>   # print the pin, or fail
#   scripts/release/install-trivy.sh --self-test
#
# Trivy's own install script, which aquasecurity/setup-trivy runs, fetches
# the tarball from get.trivy.dev, which stopped resolving on 2026-11-03
# (#449), so the lanes take the release asset directly. A version with no row
# below is refused: a bump adds its row, read from the release's
# trivy_<version>_checksums.txt. No specification governs this: our own
# design, FerroBRIDGE's.
set -euo pipefail

# One "version<TAB>sha256 of trivy_<version>_Linux-64bit.tar.gz" row per line.
readonly CHECKSUMS="\
0.76.1	c7cbe3137fb8a48a17574bede3dd6beb16742d1e9772929940fc90082c39d9c0"

self="${BASH_SOURCE[0]}"

die() {
  printf 'install-trivy: %s\n' "$1" >&2
  exit 1
}

pinned_sha256() {
  local version="$1"
  awk -F'\t' -v want="$version" '$1 == want { print $2; found = 1 } END { exit !found }' \
    <<<"$CHECKSUMS"
}

# The self-test drives the script against a stub uname and a stub curl, so
# it proves every refusal without the network.
self_test() {
  local work failed=0 n=0 pinned
  work="$(mktemp -d)"
  mkdir -p "$work/bin"
  # shellcheck disable=SC2016 # the stubs are literal scripts, never expanded here
  printf '#!/usr/bin/env bash\ncase "$1" in -s) echo "${STUB_OS:-Linux}" ;; -m) echo "${STUB_ARCH:-x86_64}" ;; esac\n' >"$work/bin/uname"
  # shellcheck disable=SC2016 # the stubs are literal scripts, never expanded here
  printf '#!/usr/bin/env bash\nwhile [[ $# -gt 0 ]]; do if [[ "$1" == --output ]]; then printf "not the tarball" >"$2"; shift; fi; shift; done\n' >"$work/bin/curl"
  chmod +x "$work/bin/uname" "$work/bin/curl"
  # expect WANT NAME ARGS...: the script with ARGS exits WANT.
  expect() {
    local want="$1" name="$2" status=0
    shift 2
    n=$((n + 1))
    PATH="$work/bin:$PATH" GITHUB_PATH="$work/github-path" bash "$self" "$@" >/dev/null 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "install-trivy: self-test failed: $name exited $status, wanted $want." >&2
      failed=1
    fi
  }
  pinned="$(head -n 1 <<<"$CHECKSUMS" | cut -f1)"
  expect 0 "the sha256 of a pinned version" --sha256 "$pinned"
  if ! [[ "$(bash "$self" --sha256 "$pinned")" =~ ^[0-9a-f]{64}$ ]]; then
    echo "install-trivy: self-test failed: the pin of $pinned is no sha256." >&2
    failed=1
  fi
  expect 1 "the sha256 of a version with no row" --sha256 0.0.0
  expect 1 "--sha256 without a version" --sha256
  expect 1 "an install without a bindir" "$pinned"
  expect 1 "an install of a version with no row" 0.0.0 "$work/out"
  STUB_OS=Darwin expect 1 "an install on macOS" "$pinned" "$work/out"
  STUB_ARCH=aarch64 expect 1 "an install on arm64" "$pinned" "$work/out"
  expect 1 "a tarball whose sha256 differs from the pin" "$pinned" "$work/out"
  if [[ -e "$work/out/trivy" || -e "$work/github-path" ]]; then
    echo "install-trivy: self-test failed: a refused tarball was installed." >&2
    failed=1
  fi
  rm -r "$work"
  [[ "$failed" -eq 0 ]] || exit 1
  echo "install-trivy: self-test OK ($n cases)."
}

case "${1:-}" in
  --self-test)
    self_test
    exit 0
    ;;
  --sha256)
    [[ $# -eq 2 ]] || die "usage: $0 --sha256 <version>"
    pinned_sha256 "$2" || die "no sha256 is pinned for Trivy $2"
    exit 0
    ;;
  *) ;;
esac

[[ $# -eq 2 ]] || die "usage: $0 <version> <bindir> | --sha256 <version> | --self-test"
version="$1"
bindir="$2"

want="$(pinned_sha256 "$version")" || die "no sha256 is pinned for Trivy $version"
[[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]] ||
  die "only the Linux x86_64 tarball is pinned, and this is $(uname -s) $(uname -m)"

tarball="trivy_${version}_Linux-64bit.tar.gz"
url="https://github.com/aquasecurity/trivy/releases/download/v${version}/${tarball}"
work="$(mktemp -d)"
trap 'rm -r "$work"' EXIT

curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
  --retry 3 --output "$work/$tarball" "$url"
got="$(sha256sum "$work/$tarball" | cut -d' ' -f1)"
[[ "$got" == "$want" ]] || die "$tarball has sha256 $got, and $want is pinned"

mkdir -p "$bindir"
tar -xzf "$work/$tarball" -C "$bindir" trivy
"$bindir/trivy" --version
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s\n' "$bindir" >>"$GITHUB_PATH"
fi
