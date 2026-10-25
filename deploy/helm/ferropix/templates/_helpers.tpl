{{/*
SPDX-FileCopyrightText: Cadasto B.V.
SPDX-License-Identifier: BUSL-1.1
*/}}

{{/* The release's base name, at most 63 characters. */}}
{{- define "ferropix.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{/* The console's name. */}}
{{- define "ferropix.console" -}}
{{- printf "%s-console" (include "ferropix.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* The labels every object carries. */}}
{{- define "ferropix.labels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{/* The selector of the server's pods. */}}
{{- define "ferropix.selector" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: server
{{- end -}}

{{/* The selector of the console's pods. */}}
{{- define "ferropix.consoleSelector" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: console
{{- end -}}

{{/* An image reference: the digest when set, else the tag, else appVersion. */}}
{{- define "ferropix.image" -}}
{{- $image := index . 0 -}}
{{- $chart := index . 1 -}}
{{- if $image.digest -}}
{{- printf "%s@%s" $image.repository $image.digest -}}
{{- else -}}
{{- printf "%s:%s" $image.repository (default $chart.AppVersion $image.tag) -}}
{{- end -}}
{{- end -}}

{{/* The service account's name. */}}
{{- define "ferropix.serviceAccount" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "ferropix.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/* The Pod Security Standards "Restricted" container context, with a read-only root. */}}
{{- define "ferropix.containerSecurity" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
runAsNonRoot: true
runAsUser: 65532
runAsGroup: 65532
capabilities:
  drop: ["ALL"]
seccompProfile:
  type: RuntimeDefault
{{- end -}}

{{/* The server's configuration: the values' map with the chart's own keys. */}}
{{- define "ferropix.config" -}}
{{- $config := deepCopy .Values.config -}}
{{- $db := default dict (get $config "db") -}}
{{- $_ := set $db "url_file" "/run/secrets/ferropix/db-url" -}}
{{- $_ := set $db "data_key_file" "/run/secrets/ferropix/data-key" -}}
{{- $_ := set $config "db" $db -}}
{{- $report := default dict (get $config "usage_report") -}}
{{- $_ := set $report "enabled" .Values.usageReport.enabled -}}
{{- $_ := set $report "deployment" "helm" -}}
{{- $_ := set $report "state_dir" "/var/lib/ferropix" -}}
{{- $_ := set $config "usage_report" $report -}}
{{- toToml $config -}}
{{- end -}}
