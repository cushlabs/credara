{{/* Chart name */}}
{{- define "creda.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* Fully qualified app name */}}
{{- define "creda.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name (include "creda.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{/* Common labels */}}
{{- define "creda.labels" -}}
app.kubernetes.io/name: {{ include "creda.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end -}}

{{/* Selector labels */}}
{{- define "creda.selectorLabels" -}}
app.kubernetes.io/name: {{ include "creda.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/* ServiceAccount name */}}
{{- define "creda.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "creda.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/* Image refs (tag defaults to appVersion) */}}
{{- define "creda.coreImage" -}}
{{- printf "%s:%s" .Values.image.core.repository (default .Chart.AppVersion .Values.image.core.tag) -}}
{{- end -}}
{{- define "creda.bridgeImage" -}}
{{- printf "%s:%s" .Values.image.bridge.repository (default .Chart.AppVersion .Values.image.bridge.tag) -}}
{{- end -}}
