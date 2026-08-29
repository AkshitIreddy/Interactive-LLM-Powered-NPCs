# Trying NVIDIA NIM

NVIDIA NIM is the most convenient **single-account experimentation option** currently modeled by this project: one user-owned NVIDIA Developer API key can authenticate the preview endpoints that NVIDIA makes available to that account. The app may recommend it during onboarding for that convenience. It is never automatically selected, never contacted without consent, and never used as a silent default or fallback.

NVIDIA describes free hosted APIs as an unlimited-prototyping path for Developer Program members, but the governing trial terms still allow model-specific limits, credits, throttling, changes, or termination. Treat this as free development preview access, not a promise of permanently unlimited usage and not a production or commercial entitlement. Review the current [NIM developer page](https://developer.nvidia.com/nim), [API Catalog](https://build.nvidia.com/), [API quickstart](https://docs.api.nvidia.com/nim/docs/api-quickstart), [NIM FAQ and rate-limit guidance](https://forums.developer.nvidia.com/t/nvidia-nim-faq/300317), and [API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf) before using it.

## Current project evidence

| Route | Current state | What the evidence means |
| --- | --- | --- |
| Chat/LLM | Implemented, explicit, experimental | A bounded synthetic live request succeeded. This proves basic endpoint compatibility, not latency, reliability, game-load behavior, production readiness, or permanent model availability. |
| `nvidia/nemotron-3-embed-1b` | Implemented, explicit, experimental | A bounded synthetic live request returned a 2048-dimensional embedding. Retrieval text leaves the PC. |
| Magpie stock-voice TTS | Implemented, manual, experimental | A bounded HTTP smoke generated valid stock-voice audio without cloning or an audio prompt. Live gRPC streaming is still pending, so this is not yet a generally selectable speech route. See NVIDIA's [Magpie multilingual model page](https://build.nvidia.com/nvidia/magpie-tts-multilingual). |
| Nemotron streaming ASR | Adapter implemented; selection blocked | Fixtures and transport integration pass, but live gRPC audio qualification is still pending. Do not interpret the adapter as a working hosted microphone path yet. See NVIDIA's [Nemotron ASR model page](https://build.nvidia.com/nvidia/nemotron-asr-streaming). |
| Reranking | Unavailable/non-selectable | The tested hosted rerank endpoints were unavailable. Transport or catalog metadata alone does not establish a usable route. |
| Face/lip animation | Not a hosted one-key route | The application does not expose hosted Audio2Face or another NVIDIA animation service through this key. Lip-sync, if qualified, is a separate optional generic local screen-space pack rather than a game-rig integration. |

These smoke checks used synthetic text or audio and are not release benchmarks. No provider key or raw provider response belongs in tracked source, logs, screenshots, diagnostics, or committed artifacts.

## Get and add a key

1. Sign in to the [NVIDIA API Catalog](https://build.nvidia.com/) with the NVIDIA account you want to use.
2. Open an eligible model and choose **Get API Key**. NVIDIA's [quickstart](https://docs.api.nvidia.com/nim/docs/api-quickstart) documents the current account and key flow.
3. In the Response Console, open **Settings → Providers → NVIDIA NIM** and choose **Add key**.
4. Enter the key only in the native Windows prompt. The Tauri shell stores it in Windows Credential Manager; the WebView receives presence/cancel status, not the secret value.
5. Run the credential-only connection test. It must not include dialogue, game context, audio, or the key in its result.
6. Explicitly select each route you want to test and review its egress disclosure. Start with chat; enable embeddings or an experimental speech route only when its current readiness label permits it.

The project does not need or accept a shared maintainer key from end users. Each person supplies and controls their own provider credential on their own Windows account.

## Privacy and restricted data

Hosted NIM calls leave the PC. Depending on the selected route, transmitted material can include a player transcript, NPC prompt, selected game context, retrieval query or passage, response text, or audio. Do not send confidential, controlled, restricted, sensitive, personal, identifying, or secret game/mod data. Review NVIDIA's current service, model, privacy, retention, and regional terms for every selected endpoint; a single account does not imply one uniform license or data policy across models.

NIM is not compatible with the application's deny-all-network Offline mode. Cross-provider fallback remains disabled unless the user explicitly pre-authorizes the exact route.

## Voices and animation

The experimental Magpie integration is intentionally limited to provider stock voices with deterministic character-to-voice binding. It rejects zero-shot voice cloning/audio-prompt payloads; the project does not imitate game actors or clone game audio.

Speech generation and facial animation are separate systems. A working Magpie voice does not animate a face. The product does not pursue per-game native-rig or mod integration. Any future lip-sync choice must be a user-selected generic local screen-space pack with explicit model, size, VRAM/RAM, license, quality, and experimental disclosures. No such pack is bundled or claimed available until its full qualification gates pass. Audio and subtitles continue when animation is unavailable.
