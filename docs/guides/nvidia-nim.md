# Trying NVIDIA NIM

Official-provider facts last verified: **2026-09-01**

NVIDIA NIM is the most convenient **single-account evaluation option** currently modeled by this project: one user-owned, account-bound NVIDIA API key can authenticate the hosted endpoints and downloadable NIMs that account is entitled to use. The app may recommend it during onboarding for that convenience. It is never automatically selected, never contacted without consent, and never used as a silent default or fallback.

NVIDIA's hosted endpoints are free for prototyping, not a promise of unrestricted production usage. The current [API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf), exact published revision **v. September 19, 2025**, allow NVIDIA to impose access-instance, duration, time, usage, credit, availability, and rate limits; change, slow, deprecate, or discontinue the service; and restrict the trial to evaluation rather than production. The current [NIM FAQ](https://docs.api.nvidia.com/nim/docs/product) and [Run NIM Anywhere](https://docs.api.nvidia.com/nim/docs/run-anywhere) distinguish Developer Program research/development/testing from production, which requires a separately licensed NVIDIA AI Enterprise or eligible partner deployment. NVIDIA's developer page currently markets “unlimited prototyping,” but that phrase does not override the governing limits or grant production entitlement. The app therefore never presents the route as unlimited.

The native acknowledgement is provider-wide, not Magpie-only. Its exact current revision identifier is `nvidia-api-trial-terms-2025-09-19-private-evaluation-v1`, its provider is `nvidia-nim`, and it is valid only in the isolated debug/review application namespaces with promotion and publication set to false. It gates hosted NVIDIA LLM, embedding, and Magpie activation. A persisted acknowledgement using the older Magpie-only provider or revision is stale and must be shown again before any NVIDIA trial route can activate. Public/production namespaces reject these trial routes unless a separate production entitlement is proven.

## What one key does—and does not—mean

The current [deployment FAQ](https://docs.api.nvidia.com/nim/docs/deployment) says the same account-bound key is used for hosted API access and entitled NIM downloads. The [API Catalog quickstart](https://docs.api.nvidia.com/nim/re/docs/api-quickstart) applies that key to available catalog models across several modalities:

| Modality | Current official catalog surface | Product rule |
| --- | --- | --- |
| LLM/chat | [LLM APIs](https://docs.api.nvidia.com/nim/reference/llm-apis) | Select an exact hosted model; do not infer permanent availability from the provider account. |
| Retrieval | [Retrieval APIs](https://docs.api.nvidia.com/nim/reference/retrieval-apis) and [retrieval catalog](https://build.nvidia.com/explore/retrieval) | Embedding and reranking are distinct routes with separate readiness and text-egress consent. |
| Vision | [Vision catalog](https://build.nvidia.com/explore/vision) | Image egress is separately authorized; one key does not make hosted vision the identity default. |
| ASR and TTS | [Speech catalog](https://build.nvidia.com/explore/speech) | Each speech model has its own endpoint, entitlement, codec, language, and qualification state. |

One key does **not** unlock every catalog entry, private-access model, deprecated endpoint, or downloadable container. For NVCF gRPC, NVIDIA requires the bearer key plus the exact model `function-id`; see the [gRPC invocation contract](https://docs.nvidia.com/nvcf/dev/g-rpc-function-invocation), [Magpie API](https://build.nvidia.com/nvidia/magpie-tts-multilingual/api), and [Nemotron ASR API](https://build.nvidia.com/nvidia/nemotron-asr-streaming/api). Per-model function IDs and terms are immutable route metadata, not secrets or provider-wide defaults.

## Current project evidence

| Route | Current state | What the evidence means |
| --- | --- | --- |
| Chat/LLM | Implemented, explicit, experimental | A bounded synthetic live request succeeded. This proves basic endpoint compatibility, not latency, reliability, game-load behavior, production readiness, or permanent model availability. |
| `nvidia/nemotron-3-embed-1b` | Implemented, explicit, experimental | A bounded synthetic live request returned a 2048-dimensional embedding. Retrieval text leaves the PC. |
| Magpie stock-voice TTS | Fixed-origin Riva gRPC transport live-qualified; private-evaluation-only activation | Authorized `EN-US.Aria` synthesis passed through the concrete Riva gRPC route: TLS connection 104 ms, HTTP stock-voice discovery 314 ms for 86 voices, first audio 627 ms, 755 ms total, and 57,344 non-silent unclipped 22.05-kHz mono PCM bytes. This proves the provider transport and stock-voice policy, not automatic route selection, speaker delivery, game-load performance, or production entitlement. Release compilation does not change this boundary: native current-terms acknowledgement, a user-owned credential, and an exact discovered stock voice are required; public/production activation, promotion, and publication remain unavailable. See NVIDIA's [Magpie multilingual model page](https://build.nvidia.com/nvidia/magpie-tts-multilingual). |
| Nemotron streaming ASR | Adapter implemented; selection blocked | The official same-key gRPC route was reached, but bounded live calls at 12.0 and 30.7 seconds both timed out. Keep it disabled until the real app's latency/accuracy test passes; do not silently substitute a different transport. See NVIDIA's [Nemotron ASR model page](https://build.nvidia.com/nvidia/nemotron-asr-streaming). |
| Reranking | Unavailable/non-selectable | The tested hosted rerank endpoints were unavailable. Transport or catalog metadata alone does not establish a usable route. |
| Face/lip animation | Private local route, not a hosted one-key endpoint | The current NVIDIA LipSync build page offers a downloadable/private AI for Media path and its model card includes Windows 10/11. It accepts one complete human face plus PCM audio and returns same-resolution RGB frames. Access entitlement, SDK delivery, exact latency, resources, temporal behavior and game-load qualification are still unresolved. |

These smoke checks used synthetic text or audio and are not release benchmarks. No provider key or raw provider response belongs in tracked source, logs, screenshots, diagnostics, or committed artifacts.

## Get and add a key

1. Sign in to the [NVIDIA API Catalog](https://build.nvidia.com/) with the NVIDIA account you want to use.
2. Open an eligible model and choose **Get API Key**. NVIDIA's [quickstart](https://docs.api.nvidia.com/nim/re/docs/api-quickstart) documents the current account and key flow.
3. In the Response Console, open **Settings → Providers → NVIDIA NIM** and choose **Add key**.
4. Enter the key only in the native Windows prompt. The Tauri shell stores it in Windows Credential Manager; the WebView receives presence/cancel status, not the secret value.
5. Run the credential-only connection test. It must not include dialogue, game context, audio, or the key in its result.
6. Explicitly select each route you want to test and review its egress disclosure. Start with chat; enable embeddings or an experimental speech route only when its current readiness label permits it.

The project does not need or accept a shared maintainer key from end users. Each person supplies and controls their own provider credential on their own Windows account.

## Privacy and restricted data

Hosted NIM calls leave the PC. Depending on the selected route, transmitted material can include a player transcript, NPC prompt, selected game context, retrieval query or passage, response text, or audio. Do not send confidential, controlled, restricted, sensitive, personal, identifying, or secret game/mod data. Review NVIDIA's current service, model, privacy, retention, and regional terms for every selected endpoint; a single account does not imply one uniform license or data policy across models.

The trial terms place responsibility for input rights/consents on the submitter. They generally say ordinary session content is not stored after the session unless a service-specific disclosure says otherwise, while also allowing security/fraud usage logs and specified product/model-improvement use. Fine-tuning and individually disclosed services can have different retention. Treat the exact current endpoint disclosure and model license as authoritative.

NIM is not compatible with the application's deny-all-network Offline mode. Cross-provider fallback remains disabled unless the user explicitly pre-authorizes the exact route.

## Voices, animation, and redistribution

The experimental Magpie integration is intentionally limited to provider stock voices with deterministic character-to-voice binding. NVIDIA documents runtime discovery through `list_voices`, model/locale/speaker names, and voice-specific emotional suffixes in [Voices and Emotional Styles](https://docs.nvidia.com/nim/speech/latest/tts/voices.html). The live-qualified fixed-origin transport discovered 86 voices and used `Magpie-Multilingual.EN-US.Aria`; it does not assume every voice supports every emotion. Although NVIDIA separately documents zero-shot prompt cloning, this project rejects audio-prompt cloning and does not imitate game actors. Review/debug builds may select this route only after the provider-wide acknowledgement, credential, and authenticated-stock-voice gates pass. The transport result is not a claim of public/production entitlement, physical-speaker delivery, or lip-sync.

Speech generation and facial animation are separate systems. A working Magpie voice does not animate a face. NVIDIA's current animation surfaces have different boundaries:

- [LipSync](https://build.nvidia.com/nvidia/lipsync/modelcard) is a private-access downloadable AR SDK model for one complete human face plus PCM speech, positioned for content localization. The current [deploy page](https://build.nvidia.com/nvidia/lipsync/deploy) routes access through NVIDIA AI for Media/private deployment rather than the ordinary hosted API-key flow. The model card lists Windows 10/11 and same-resolution RGB output up to 4K, but NVIDIA does not establish arbitrary stylized-game tracking, multi-face actor authority, current-frame compositing, or beside-a-game latency for this product.
- [Audio2Face-2D](https://build.nvidia.com/nvidia/audio2face-2d/deploy) is deprecated and animates a portrait image from audio.
- The old hosted [Audio2Face-3D catalog endpoint](https://build.nvidia.com/nvidia/audio2face-3d/api) is deprecated, while the current self-hosted [Audio2Face-3D NIM](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html) continues separately.
- NVIDIA now publishes the [Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK) source under MIT for Windows x64 and Linux. Its Windows build requires CUDA `>=12.8,<13.0` (12.9 recommended) and TensorRT `>=10.13,<11.0`. The regression [Mark v2.3 model](https://huggingface.co/nvidia/Audio2Face-3D-v2.3-Mark) is a separately licensed NVIDIA Open Model candidate for a future low-latency local spike.

Audio2Face-3D produces animation coefficients/geometry for a prepared character and renderer; it does not directly modify an arbitrary captured game frame. A project-owned mapper and compositor would still have to turn that motion signal into a tracked mouth-region residual. NVIDIA advertises faster-than-60-FPS generation for the SDK, but this project has not reproduced that result on the target RTX 4080 beside a running game. Mark v2.3 is therefore a candidate, not an installed, selectable, qualified, or default pack. The generic product path remains a separately qualified local current-frame mouth residual, not an NVIDIA hosted default or a per-game rig integration.

Do not bundle Developer Program NIM containers or runtimes in this app's installer. Section F of NVIDIA's current [AI Product Terms](https://www.nvidia.com/en-us/agreements/enterprise-software/product-specific-terms-for-ai-products/) limits Developer Program Enterprise Product software to internal non-production evaluation/development/testing and says it cannot be included in a customer product. Users must acquire entitled components directly from NVIDIA after accepting the applicable terms, or a future release must obtain an appropriate enterprise/distribution license. That NIM-container boundary is distinct from the public MIT Audio2Face-3D SDK and from model weights covered by NVIDIA's [Open Model License](https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/). It is not blanket permission to redistribute a complete pack: SDK notices, exact model terms, CUDA/TensorRT dependencies, hashes, and every transitive component still require separate review and validation. Unknown status fails closed.
