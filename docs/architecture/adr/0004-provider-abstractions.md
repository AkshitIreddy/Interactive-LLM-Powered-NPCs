# ADR-0004: Capability-driven provider abstractions

Status: Accepted  
Date: 2026-08-28

## Context

Hosted provider model IDs, limits, prices and capabilities change frequently. API routes must be replaceable without pretending their privacy, streaming, schema and audio semantics are identical. Version 1 hard-codes Cohere/Google recognition and embeds credentials/file conventions throughout helpers.

## Decision

Define three repository-owned async interfaces:

- `StreamingRecognizer`: input formats, languages, partial/final transcript events, endpointing, word timing, keyword bias, retention/region metadata, cancellation and readiness.
- `LanguageModelProvider`: model discovery, streaming text, structured-schema/tool capability, context/output limits, usage, retention/region metadata, cancellation and normalized errors.
- `TtsSession`: voice discovery/traits, input and streaming output formats, sample rate, alignment/visemes, style controls, determinism, retention/region metadata, cancellation and readiness.

Initial hosted adapters: OpenAI, Google Gemini, Anthropic and Groq for LLM; Deepgram, AssemblyAI, ElevenLabs and OpenAI for STT; Cartesia, ElevenLabs, Inworld and Deepgram for TTS. A configurable OpenAI-compatible LLM endpoint is explicitly best-effort because compatibility is not uniform.

Provider discovery is augmented by a signed curated catalog, but the UI preserves the exact model ID and capabilities returned/selected. No permanent alias silently remaps a user. Defaults are chosen from release-candidate benchmarks and can change only through visible catalog/config migration.

## Routing policy

- Provider routing and performance preset are separate.
- Named provider/model loadouts may group explicit LLM/STT/TTS/retrieval choices and use global/game/character inheritance. Switching a loadout is a visible user action. Every saved fallback is `ManualOnly` plus `user_authorized`; the catalog contains no automatic fallback candidate.
- Every request resolves one explicit route before data leaves the runtime.
- Fallback requires pre-authorization of the exact route and data categories.
- Adapter normalization never fabricates unsupported schema, timing, viseme or privacy guarantees.
- Provider-specific fields live in an extension map outside core session semantics.

## Consequences

- New providers do not alter turn state or UI data models.
- Capability matrices and fixture servers become mandatory; “OpenAI compatible” is tested per feature.
- Pricing/model facts are volatile and must be refreshed/signed rather than frozen in prose.
- Provider errors normalize to authentication, quota/rate, invalid request, unavailable, timeout, cancelled and policy-blocked while retaining a redacted provider code.

## Evidence

- OpenAI model/API documentation: <https://developers.openai.com/api/docs/models>
- OpenAI realtime transcription: <https://developers.openai.com/api/docs/guides/realtime-transcription>
- Gemini models: <https://ai.google.dev/gemini-api/docs/models>
- Anthropic models: <https://platform.claude.com/docs/en/about-claude/models/overview>
- Groq text streaming/model API: <https://console.groq.com/docs/text-chat>
- Deepgram docs: <https://developers.deepgram.com/docs>
- AssemblyAI streaming STT: <https://www.assemblyai.com/docs/speech-to-text/streaming>
- ElevenLabs TTS: <https://elevenlabs.io/docs/overview/capabilities/text-to-speech>
