{{/*
h.token — emit a workflow-engine string-interpolation token, e.g. {{params.env}}.
Built with printf so the engine's {{...}} delimiters never collide with Go-template delimiters.
*/}}
{{- define "h.token" -}}
{{- printf "{{%s}}" . -}}
{{- end }}

{{/*
h.outputContractEpilogue — the per-step INSTANCE of the output contract, rendered from the
template's declared schema so instruction and contract cannot drift. The declaring template
appends it to its final agent step's task, sets the SAME schema as that step's outputContract
input, and emits it top-level as outputs:.
*/}}
{{- define "h.outputContractEpilogue" -}}
===OUTPUT CONTRACT===
End your final message with a fenced ```json code block containing a single JSON
object matching this schema. The block is machine-validated: a missing or
mismatching block fails this step. Nothing may follow the block.
{{ . | toPrettyJson }}
{{- end }}
