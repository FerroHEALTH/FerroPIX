#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# Installs the browser the operator console's journeys drive, Chrome for
# Testing and its matching chromedriver, at the release the `journeys` job of
# ci.yml pins, each zip checked against its pinned SHA-256 before it is
# unpacked (no specification governs this: our own design, after
# FerroBRIDGE's script of the same name).
#
# The download URLs come from the Chrome for Testing JSON API, for the one
# pinned version and this platform. The version and digests below are the
# rows of docs/VERSIONS.md, which scripts/checks/versions.sh holds them and
# the chrome-version of ci.yml to. The journeys read DIR at
# target/journey-tools unless FERROPIX_CHROMEDRIVER and FERROPIX_CHROME name
# other binaries.
#
# Usage:
#   scripts/release/journey-tools.sh DIR   install both into DIR, as
#                                          DIR/chrome-<platform>/ and
#                                          DIR/chromedriver-<platform>/
set -Eeuo pipefail

chrome_version="156.0.8078.80"
chrome_linux64_sha256="b711c720f774be355197f32edf37e0651418ba036d92446ee2da19ac017540a7"
chrome_mac_arm64_sha256="8bc590d1b1acac042a000234c768f31830f25a3983a83ed5e067114c87c4cad1"
chromedriver_linux64_sha256="cae58bbd262ba5c8a0ce2540f44254b6e789efc4fe9b18bbb4a00487a9f5859e"
chromedriver_mac_arm64_sha256="b5fe5abe5a82ca3ac49b3f2ea9e177b63f82fc3a4227883e0eaa9894a6bf3b7f"

api="https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json"

die() {
  echo "journey-tools: $*" >&2
  exit 1
}

# Prints the SHA-256 of FILE, with the tool the platform has.
digest_of() {
  if command -v sha256sum > /dev/null; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Checks FILE against the SHA-256 DIGEST.
verify() {
  local file="$1" digest="$2" found
  found="$(digest_of "$file")"
  [ "$found" = "$digest" ] || die "$file has SHA-256 $found, the pin is $digest"
}

# Downloads URL into FILE.
fetch() {
  curl --fail --silent --show-error --location --retry 3 --output "$2" "$1"
}

# Prints the URL the API lists for the pinned version of PRODUCT (chrome or
# chromedriver) on PLATFORM, from the API answer in FILE.
url_of() {
  jq -r --arg version "$chrome_version" --arg product "$1" --arg platform "$2" \
    '.versions[] | select(.version == $version) | .downloads[$product][]?
      | select(.platform == $platform) | .url' "$3"
}

# Fetches PRODUCT for PLATFORM into DIR, checks it against DIGEST, and
# unpacks it, replacing an earlier copy.
install_one() {
  local product="$1" platform="$2" digest="$3" dir="$4" url zip
  url="$(url_of "$product" "$platform" "$dir/download/versions.json")"
  [ -n "$url" ] || die "the Chrome for Testing API lists no $product $chrome_version for $platform"
  zip="$dir/download/$product-$platform.zip"
  fetch "$url" "$zip"
  verify "$zip" "$digest"
  rm -rf "${dir:?}/$product-$platform"
  unzip -q "$zip" -d "$dir"
}

install() {
  local dir="$1" platform chrome_sha driver_sha chrome said
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
      platform="linux64"
      chrome_sha="$chrome_linux64_sha256"
      driver_sha="$chromedriver_linux64_sha256"
      chrome="$dir/chrome-linux64/chrome"
      ;;
    Darwin-arm64)
      platform="mac-arm64"
      chrome_sha="$chrome_mac_arm64_sha256"
      driver_sha="$chromedriver_mac_arm64_sha256"
      chrome="$dir/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
      ;;
    *) die "no pinned digest for $(uname -s) $(uname -m); install Chrome for Testing $chrome_version by hand" ;;
  esac
  command -v jq > /dev/null || die "jq reads the Chrome for Testing API; install it first"
  command -v unzip > /dev/null || die "unzip unpacks the downloads; install it first"
  mkdir -p "$dir/download"
  fetch "$api" "$dir/download/versions.json"
  install_one chrome "$platform" "$chrome_sha" "$dir"
  install_one chromedriver "$platform" "$driver_sha" "$dir"
  said="$("$dir/chromedriver-$platform/chromedriver" --version)"
  case "$said" in
    *"ChromeDriver $chrome_version "*) ;;
    *) die "the unpacked chromedriver says '$said', not $chrome_version" ;;
  esac
  [ -x "$chrome" ] || die "the Chrome zip holds no $chrome"
  echo "journey-tools: Chrome for Testing and chromedriver $chrome_version are in $dir"
}

case "${1:-}" in
  "" | -*)
    echo "usage: scripts/release/journey-tools.sh DIR" >&2
    exit 2
    ;;
  *)
    mkdir -p "$1"
    install "$(cd "$1" && pwd)"
    ;;
esac
