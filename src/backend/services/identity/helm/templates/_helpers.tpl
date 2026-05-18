{{- define "insight-identity.fullname" -}}
{{ .Release.Name }}-identity
{{- end }}

{{- define "insight-identity.labels" -}}
app.kubernetes.io/name: identity
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "insight-identity.selectorLabels" -}}
app.kubernetes.io/name: identity
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}
