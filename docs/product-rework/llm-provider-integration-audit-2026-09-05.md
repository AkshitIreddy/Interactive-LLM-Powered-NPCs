# Hosted LLM integration audit — 2026-09-05

## Decision

Expose only named routes with fixed service endpoints, exact Windows Credential Manager targets, an executable runtime factory, catalog admission, and measured request/model behavior. The selected additions are Mistral and OpenRouter. Existing Gemini, Groq, and Cohere routes receive the model-specific corrections found by bounded live qualification.

The generic `openai-compatible` transport remains reusable code, but its catalog route is not selectable because the native host has no trusted endpoint registry. A WebView-provided URL or credential target must never become a network destination.

Cloudflare Workers AI remains unavailable. Its route needs a separately persisted trusted account ID, a dedicated top-level `response` SSE parser, and a product decision about the current conflict between streaming and schema-backed JSON mode. A composite credential field or user-supplied URL would hide those missing boundaries rather than solve them.

## Evidence-driven route set

| Provider | Curated model | Native request rule | Qualification result |
|---|---|---|---|
| Groq | `openai/gpt-oss-20b` | Standard fixed Groq chat-completions route | Strict schema and character-purpose checks passed 2/2 |
| Groq | `qwen/qwen3.6-27b` | Exact model only: disable and hide reasoning | Generic payload produced no dialogue; corrected payload passed 2/2 |
| Mistral | `ministral-8b-2512` | Fixed Mistral endpoint and vault target | Strict schema passed 2/2; faster measured Mistral option |
| Mistral | `ministral-3b-2512` | Fixed Mistral endpoint and vault target | Strict schema passed 2/2 |
| Gemini | `gemini-3.1-flash-lite` | Translate JSON Schema `const` to singleton `enum` | Strict schema and character-purpose checks passed 2/2 |
| OpenRouter | `liquid/lfm-2.5-2.6b:free` | Disable fallbacks and require requested parameters | One quota-bounded strict schema and character-purpose check passed |
| Cohere | `command-a-plus-05-2026` | Exact model only: `thinking.type=disabled` | Generic payload produced no dialogue; corrected payload passed |

OpenRouter free inventory is volatile. The exact `:free` route remains a manual choice, never an automatic fallback, and the request forbids provider substitution. A stale earlier free model returned 404 and is not curated.

## Normal-turn contract

Direct adapter probes are insufficient if normal game turns omit the schema. `RuntimeBridge` therefore attaches the portable provider schema when route metadata selects `structured_v1` or `structured_speech_first_v1`. The schema requires both `schema_version` and `spoken_response`, uses a singleton enum for the fixed version, and omits provider-unsupported string-length keywords. Native response validation remains authoritative for content bounds and safety.

The speech-first runtime mode must release only a fully decoded and validated `spoken_response` object after the exact schema version is also present. It accepts either top-level field order and buffers complete values until both are available. Effects, actions, and memory proposals remain unavailable until the complete response validates. It must never speak a partial JSON fragment.

## Acceptance

- Named provider factories use fixed HTTPS endpoints and `providers/<provider-id>` vault references.
- UI provider/model choices match exact admitted catalog routes.
- Groq Qwen, Cohere Command A+, Gemini schema translation, and OpenRouter routing guards have captured fixture tests.
- Normal structured turns include the portable schema, and speech release does not depend on provider property order.
- Generic compatible and Cloudflare routes remain non-selectable until their missing trust/transport contracts exist.
- No live request runs as part of ordinary tests, and no credential value appears in source, logs, docs, or fixtures.
