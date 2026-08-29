# Providers and credentials

## Hosted integrations

- LLM: OpenAI, Google Gemini, Anthropic, Groq, Cohere, NVIDIA NIM, and a best-effort configurable OpenAI-compatible endpoint
- STT: Deepgram, AssemblyAI, ElevenLabs, OpenAI, and an implemented but not yet live-qualified NVIDIA Nemotron streaming-ASR route
- TTS: Cartesia, ElevenLabs, Inworld, Deepgram, and experimental NVIDIA Magpie stock voices; its HTTP audio path has a synthetic smoke result while live gRPC streaming remains pending
- Retrieval: experimental NVIDIA NIM embeddings; NVIDIA hosted reranking is currently non-selectable because the tested routes were unavailable

An entry in the catalog means a contract is modeled; [CHANGELOG.md](../../CHANGELOG.md) and runtime diagnostics determine whether an adapter is implemented and verified in the current build.

The checked-in development catalog is `catalog/v1/catalog.json`; its trust and discovery rules are documented in `catalog/README.md`. It is intentionally unsigned and cannot be treated as production update metadata.

## Configure a route

Choose a provider/model independently for STT, LLM, retrieval, and TTS. Review the UI's transmitted-data, cost, region, streaming, cancellation, language, codec, and schema-capability summary before enabling it. A provider account covering several services does not make every route equally ready.

Select **Add key** for the provider. The Tauri shell opens a native Windows credential prompt; the user enters their own key there, and the value is written to Windows Credential Manager. The application stores only a credential reference in settings. The Response Console WebView, game profile, logs, diagnostics, and inference workers must never receive the value.

Do not use `apikeys.json`, `.env` files checked into the repository, command-line keys, or character/profile files for production credentials.

For NVIDIA's one-account experimentation path, follow [Trying NVIDIA NIM](nvidia-nim.md). The app may recommend NIM as a convenient first provider to try, but it never chooses NIM, contacts it, or enables another NIM modality without explicit user selection and egress consent.

## Validate without exposing the key

Use the provider connection test. It should report credential presence, authentication outcome, endpoint, model discovery/capabilities, and sanitized provider request ID. It must not echo the key or send game/dialogue content for a credential-only test.

## Fallback policy

A route is explicit. Local-to-cloud, one provider to another, or one region to another cannot happen silently. If a pre-authorized fallback exists, the UI shows its data/cost boundary and the runtime records only the route identifier and outcome.

The same rule applies within a multi-service account: selecting NVIDIA chat does not enable NVIDIA embeddings, ASR, TTS, reranking, or animation. Each route has independent consent and readiness.

## OpenAI-compatible endpoints

Compatibility is best-effort because servers differ in streaming framing, schema/tool support, cancellation, tokenization, usage reporting, and authentication. Configure endpoint/model explicitly and run simulation before using a game. Never assume OpenAI-compatible means identical behavior.

## Remove or rotate

Remove the credential in Settings and then revoke/rotate it at the provider. Removing the local reference cannot cancel provider billing or invalidate the upstream secret.
