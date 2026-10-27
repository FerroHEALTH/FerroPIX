#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# Validates the ferropix chart with no cluster:
#   1. helm lint over every value set under ci/;
#   2. helm template of each, every manifest held by kubeconform to the
#      Kubernetes schemas of the chart's floor and of the newest release;
#   3. every FerroPIX container of every workload holds the Pod Security Standards
#      "Restricted" fields (runAsNonRoot, allowPrivilegeEscalation false,
#      drop ALL, seccomp RuntimeDefault) and a read-only root filesystem
#      (https://kubernetes.io/docs/concepts/security/pod-security-standards/);
#   4. every pod spec turns the Service environment variables off and, by
#      default, runs in its own user namespace with the Strict supplemental
#      groups policy; hostUsers true renders no field, and a value outside
#      each field's set refuses to render;
#   5. no Secret is rendered and no credential is a plain value: the
#      configuration names every secret by a `_file` path;
#   6. the chart refuses to render without the Secrets it needs.
# deploy/helm/kind-check.sh installs the chart on a cluster for real.
# No specification governs this: our own design, after FerroEHR's.
#
#   deploy/helm/validate.sh [--self-test]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
chart="$here/ferropix"
versions=(1.31.0 1.34.0)

for tool in helm kubeconform; do
  command -v "$tool" > /dev/null || { echo "validate: $tool is not on PATH" >&2; exit 2; }
done

# Renders `chart` with the values file `values`.
render() {
  helm template ferropix "$1" --namespace ferropix -f "$2"
}

# Checks the chart at `$1`; prints each fault and returns 1 on any.
check() {
  local target=$1 values rendered faults=0
  for values in "$here"/ci/*-values.yaml; do
    helm lint --quiet "$target" -f "$values" > /dev/null || { echo "validate: helm lint refuses $(basename "$values")"; faults=1; continue; }
    rendered="$(render "$target" "$values")"
    for version in "${versions[@]}"; do
      kubeconform -strict -summary=false -kubernetes-version "$version" <<< "$rendered" \
        || { echo "validate: kubeconform $version refuses $(basename "$values")"; faults=1; }
    done
    local containers restricted
    containers="$(grep -c 'image: "ghcr.io/ferrohealth/' <<< "$rendered" || true)"
    for field in 'allowPrivilegeEscalation: false' 'readOnlyRootFilesystem: true' 'runAsNonRoot: true' 'drop: \["ALL"\]' 'type: RuntimeDefault'; do
      restricted="$(grep -cE "^ {12}${field}|^ {14}${field}" <<< "$rendered" || true)"
      if [[ "$restricted" -lt "$containers" ]]; then
        echo "validate: $(basename "$values") renders a container without '$field'"
        faults=1
      fi
    done
    [[ "$containers" -ge 1 ]] || { echo "validate: $(basename "$values") renders no FerroPIX container"; faults=1; }
    # The user namespace and the groups policy are held at their defaults in
    # every value set that leaves them alone; the kind check's sets hostUsers.
    local pods isolated fields=('enableServiceLinks: false')
    if ! grep -qE '^(hostUsers|supplementalGroupsPolicy):' "$values"; then
      fields+=('hostUsers: false' '  supplementalGroupsPolicy: Strict')
    fi
    pods="$(grep -c '^kind: Deployment$' <<< "$rendered" || true)"
    for field in "${fields[@]}"; do
      isolated="$(grep -cE "^ {6}${field}$" <<< "$rendered" || true)"
      if [[ "$isolated" -lt "$pods" ]]; then
        echo "validate: $(basename "$values") renders a pod without '${field#"${field%%[! ]*}"}'"
        faults=1
      fi
    done
    if grep -qE '^kind: Secret$' <<< "$rendered"; then
      echo "validate: $(basename "$values") renders a Secret; a credential is the deployer's own Secret"
      faults=1
    fi
    if grep -qE '^[[:space:]]*(password|client_secret|data_key|url) = "' <<< "$rendered"; then
      echo "validate: $(basename "$values") renders a credential as a plain value"
      faults=1
    fi
  done
  if helm template ferropix "$target" > /dev/null 2>&1; then
    echo "validate: the chart renders without the Secrets it needs"
    faults=1
  fi
  local defaults="$here/ci/default-values.yaml"
  if helm template ferropix "$target" -f "$defaults" --set hostUsers=true | grep -q 'hostUsers:'; then
    echo "validate: hostUsers=true renders the field, which is the API default"
    faults=1
  fi
  if helm template ferropix "$target" -f "$defaults" --set-string hostUsers=no > /dev/null 2>&1; then
    echo "validate: the chart renders with hostUsers=no"
    faults=1
  fi
  if helm template ferropix "$target" -f "$defaults" --set supplementalGroupsPolicy=Loose > /dev/null 2>&1; then
    echo "validate: the chart renders with supplementalGroupsPolicy=Loose"
    faults=1
  fi
  if helm template ferropix "$target" -f "$defaults" --set supplementalGroupsPolicy= | grep -q 'supplementalGroupsPolicy:'; then
    echo "validate: an empty supplementalGroupsPolicy renders the field"
    faults=1
  fi
  return "$faults"
}

if [[ "${1:-}" = "--self-test" ]]; then
  work="$(mktemp -d)"
  trap 'rm -rf -- "$work"' EXIT
  cp -R "$chart" "$work/ferropix"
  perl -0pi -e 's/readOnlyRootFilesystem: true/readOnlyRootFilesystem: false/g' "$work/ferropix/templates/_helpers.tpl"
  if check "$work/ferropix" > "$work/out" 2>&1; then
    echo "validate: self-test failed: a writable root filesystem passed" >&2
    exit 1
  fi
  grep -q "readOnlyRootFilesystem: true" "$work/out" || { echo "validate: self-test failed: the refusal did not say why" >&2; cat "$work/out" >&2; exit 1; }
  cp -R "$chart/." "$work/ferropix"
  perl -0pi -e 's/^enableServiceLinks: false\n//m' "$work/ferropix/templates/_helpers.tpl"
  if check "$work/ferropix" > "$work/out" 2>&1; then
    echo "validate: self-test failed: a pod with Service links passed" >&2
    exit 1
  fi
  grep -q "enableServiceLinks: false" "$work/out" || { echo "validate: self-test failed: the refusal did not say why" >&2; cat "$work/out" >&2; exit 1; }
  echo "validate: self-test passed."
  exit 0
fi

check "$chart"
echo "validate: the chart lints, renders valid manifests on Kubernetes ${versions[*]}, holds the Restricted fields and renders no credential."
