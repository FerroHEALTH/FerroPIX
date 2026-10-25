#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# Installs the chart on a throwaway kind cluster and waits until it serves:
# the PostgreSQL of ci/kind/, the published server and console images (pulled,
# never built), every Secret written fresh, the install waited on, the
# readiness and liveness routes asked through the Service, then the release
# uninstalled and the cluster deleted. It needs kind, kubectl and helm, and a
# container engine kind runs on (KIND_EXPERIMENTAL_PROVIDER=podman for
# Podman). No specification governs this: our own design.
#
#   deploy/helm/kind-check.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cluster="${FERROPIX_KIND_CLUSTER:-ferropix-chart}"
namespace=ferropix

for tool in kind kubectl helm openssl curl; do
  command -v "$tool" > /dev/null || { echo "kind-check: $tool is not on PATH" >&2; exit 2; }
done

work="$(mktemp -d)"
cleanup() {
  rm -rf -- "$work"
  helm uninstall ferropix --namespace "$namespace" > /dev/null 2>&1 || true
  kind delete cluster --name "$cluster" > /dev/null 2>&1 || true
}
trap cleanup EXIT

kind create cluster --name "$cluster" --wait 120s
kubectl create namespace "$namespace"

db_password="$(openssl rand -hex 16)"
kubectl --namespace "$namespace" create secret generic postgres-passwords \
  --from-literal=postgres-password="$(openssl rand -hex 16)" \
  --from-literal=ferropix-db-password="$db_password"
kubectl --namespace "$namespace" create configmap postgres-init \
  --from-file=10-ferropix-role.sh="$root/docker/postgres/10-ferropix-role.sh"
kubectl --namespace "$namespace" apply -f "$here/ci/kind/postgres.yaml"
kubectl --namespace "$namespace" rollout status deployment/postgres --timeout=300s

kubectl --namespace "$namespace" create secret generic ferropix-db \
  --from-literal=url="postgresql://ferropix:${db_password}@postgres:5432/ferropix"
kubectl --namespace "$namespace" create secret generic ferropix-data-key \
  --from-literal=data-key="$(openssl rand -base64 32)"
kubectl --namespace "$namespace" create secret generic ferropix-console-oidc \
  --from-literal=client-secret="$(openssl rand -hex 16)"

helm lint "$here/ferropix" -f "$here/ci/kind-values.yaml"
# Prints the state and the last log lines of every pod, for a failed check.
dump() {
  kubectl --namespace "$namespace" get pods -o wide || true
  local pod
  for pod in $(kubectl --namespace "$namespace" get pods -o name); do
    kubectl --namespace "$namespace" describe "$pod" | tail -20 || true
    kubectl --namespace "$namespace" logs "$pod" --all-containers --tail=40 || true
  done
}

if ! helm install ferropix "$here/ferropix" --namespace "$namespace" \
  -f "$here/ci/kind-values.yaml" --set usageReport.enabled=false \
  --wait --timeout 5m; then
  dump
  echo "kind-check: the release did not become ready" >&2
  exit 1
fi

kubectl --namespace "$namespace" port-forward svc/ferropix 18080:8080 > "$work/forward.log" 2>&1 &
forward=$!
kubectl --namespace "$namespace" port-forward svc/ferropix-console 13000:3000 > "$work/console.log" 2>&1 &
console=$!
sleep 3
status=0
curl --fail --silent --show-error http://127.0.0.1:18080/health > /dev/null || status=1
curl --fail --silent --show-error http://127.0.0.1:18080/health/readiness || status=1
echo
curl --fail --silent --show-error http://127.0.0.1:13000/health > /dev/null || status=1
kill "$forward" "$console" 2> /dev/null || true
kubectl --namespace "$namespace" get pods
if [[ "$status" -ne 0 ]]; then
  dump
  echo "kind-check: a health route did not answer" >&2
  exit 1
fi
echo "kind-check: the chart installed, served its health routes and is uninstalled"
