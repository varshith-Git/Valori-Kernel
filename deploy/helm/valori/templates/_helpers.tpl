{{/*
Expand the name of the chart.
*/}}
{{- define "valori.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "valori.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "valori.labels" -}}
helm.sh/chart: {{ include "valori.chart" . }}
{{ include "valori.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "valori.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "valori.selectorLabels" -}}
app.kubernetes.io/name: {{ include "valori.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Build the VALORI_CLUSTER_MEMBERS string from the StatefulSet replicas.
Format: 1=<pod-0-dns>:3100/<pod-0-dns>:3000,2=...
*/}}
{{- define "valori.clusterMembers" -}}
{{- $fullname := include "valori.fullname" . -}}
{{- $ns := .Release.Namespace -}}
{{- $raftPort := .Values.raftPort -}}
{{- $apiPort := .Values.apiPort -}}
{{- $count := int .Values.replicaCount -}}
{{- $parts := list -}}
{{- range $i := until $count -}}
{{- $id := add1 $i -}}
{{- $dns := printf "%s-%d.%s-headless.%s.svc.cluster.local" $fullname $i $fullname $ns -}}
{{- $entry := printf "%d=%s:%d/%s:%d" $id $dns $raftPort $dns $apiPort -}}
{{- $parts = append $parts $entry -}}
{{- end -}}
{{- join "," $parts -}}
{{- end }}
