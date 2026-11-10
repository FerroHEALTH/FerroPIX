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
#      configuration names every secret by a `_file` path, judged leaf by
#      leaf, so `[db] url` is a credential, an endpoint table's `url` is one
#      only when it carries userinfo, and a password, client secret, client
#      identity or data key is one under any table;
#   6. the chart refuses to render without the Secrets it needs;
#   7. every FerroPIX container sets a memory and an ephemeral-storage limit,
#      so one pod cannot take its node's memory or disk.
#   8. the chart refuses to render without an issuer, and its values schema
#      refuses the removed config.auth.audience naming [[auth.issuer]].
# deploy/helm/kind-check.sh installs the chart on a cluster for real, and
# deploy/helm/config-check.sh runs the images' config check over the renders.
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
  local chart="$1" values="$2"
  helm template ferropix "$chart" --namespace ferropix -f "$values"
}

# Prints each credential the rendered configuration on standard input sets
# as a plain value, as its dotted key: judged by the leaf and its table.
plain_credentials() {
  awk '
    match($0, /^[[:space:]]*\[+[^]]+\]+[[:space:]]*$/) {
      table = $0; gsub(/[][[:space:]]/, "", table); next
    }
    match($0, /^[[:space:]]*[a-z_]+ = "/) {
      key = $1; value = $0; sub(/^[^"]*"/, "", value)
      if (key ~ /^(password|password_hash|client_secret|client_identity|data_key)$/ ||
          (key == "url" && table == "db") ||
          (key == "url" && value ~ /^[a-z][a-z0-9+.-]*:\/\/[^\/@"]+@/)) {
        print (table == "" ? key : table "." key)
      }
    }
  '
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
    local bounded
    bounded="$(awk '
      /^ +limits:$/ { depth = index($0, "l"); memory = 0; storage = 0; open = 1; next }
      open && match($0, /^ +/) && RLENGTH >= depth {
        if ($1 == "memory:") memory = 1
        if ($1 == "ephemeral-storage:") storage = 1
        next
      }
      open { if (memory && storage) n++; open = 0 }
      END { if (open && memory && storage) n++; print n + 0 }
    ' <<< "$rendered")"
    if [[ "$bounded" -lt "$containers" ]]; then
      echo "validate: $(basename "$values") renders a container without a memory and an ephemeral-storage limit"
      faults=1
    fi
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
    local plain
    plain="$(plain_credentials <<< "$rendered")"
    if [[ -n "$plain" ]]; then
      echo "validate: $(basename "$values") renders a credential as a plain value: ${plain//$'\n'/, }"
      faults=1
    fi
  done
  if helm template ferropix "$target" > /dev/null 2>&1; then
    echo "validate: the chart renders without the Secrets it needs"
    faults=1
  fi
  local defaults="$here/ci/default-values.yaml" refusal
  # Chart 0.3.18 refuses the removed config.auth.audience by name, for that
  # one chart version (#585).
  refusal="$(helm template ferropix "$target" -f "$defaults" --set 'config.auth.audience[0]=https://pix.example.org/fhir' 2>&1 || true)"
  if ! grep -qF '[[auth.issuer]]' <<< "$refusal"; then
    echo "validate: config.auth.audience renders, or its refusal does not name [[auth.issuer]]"
    faults=1
  fi
  if helm template ferropix "$target" -f "$defaults" --set-json 'config.auth.issuer=[]' > /dev/null 2>&1; then
    echo "validate: the chart renders without an issuer"
    faults=1
  fi
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
  cp -R "$chart/." "$work/ferropix"
  perl -0pi -e 's/^(    memory: 512Mi\n)    ephemeral-storage: 256Mi\n/$1/m' "$work/ferropix/values.yaml"
  if check "$work/ferropix" > "$work/out" 2>&1; then
    echo "validate: self-test failed: a container without an ephemeral-storage limit passed" >&2
    exit 1
  fi
  grep -q "ephemeral-storage limit" "$work/out" || { echo "validate: self-test failed: the refusal did not say why" >&2; cat "$work/out" >&2; exit 1; }
  for case in '[db]|url = "postgresql://pix@db/pix"|db.url' \
    '[terminology.endpoint.credentials]|password = "s3cret"|terminology.endpoint.credentials.password' \
    '[audit.endpoint]|url = "https://user:pw@arr.example.org/fhir"|audit.endpoint.url'; do
    IFS='|' read -r table line want <<< "$case"
    got="$(printf '%s\n  %s\n' "$table" "$line" | plain_credentials)"
    [[ "$got" == "$want" ]] || { echo "validate: self-test failed: $want passed as no credential ($got)" >&2; exit 1; }
  done
  for case in '[terminology.endpoint]|url = "https://tx.example.org/fhir/r4"' '[audit.endpoint]|url = ""' \
    '[db]|url_file = "/run/secrets/ferropix/db-url"'; do
    IFS='|' read -r table line <<< "$case"
    got="$(printf '%s\n  %s\n' "$table" "$line" | plain_credentials)"
    [[ -z "$got" ]] || { echo "validate: self-test failed: $table $line was judged a credential" >&2; exit 1; }
  done
  echo "validate: self-test passed."
  exit 0
fi

check "$chart"
echo "validate: the chart lints, renders valid manifests on Kubernetes ${versions[*]}, holds the Restricted fields, bounds every container and renders no credential."
