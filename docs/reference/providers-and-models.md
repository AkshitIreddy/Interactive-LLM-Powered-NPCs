# Providers and models reference

Official provider/model status last verified: **2026-09-01**

## Hosted provider targets

| Stage | Targets |
| --- | --- |
| LLM | OpenAI, Gemini, Anthropic, Groq, Cohere Chat, NVIDIA NIM Chat, configurable OpenAI-compatible endpoint |
| STT | Deepgram, AssemblyAI, ElevenLabs, OpenAI; NVIDIA Nemotron Streaming ASR adapter implemented but selection blocked pending live qualification |
| TTS | ElevenLabs; NVIDIA Magpie experimental/manual with live-qualified fixed-origin Riva gRPC transport. These are the only hosted TTS providers the ordinary selected-route runtime constructs. Cartesia, Inworld, and Deepgram are catalog-only/qualification-required. |
| Retrieval | NVIDIA Nemotron 3 Embed 1B implemented/experimental; NVIDIA NIM rerank catalog-only because tested hosted routes were unavailable |

Catalog entries describe capabilities; only adapter, fixture, integration, and endpoint evidence establish current support. The Cartesia, Deepgram, and Inworld TTS crates contain tested protocol/command adapter cores, but the ordinary runtime has no credential resolver or live socket/provider builder for them, native loadout persistence rejects them, and they are not selectable product routes. Cohere Chat has an implemented hosted LLM adapter with streaming, cancellation, model discovery, and structured-output handling. Cohere Embed and Rerank appear only as non-selectable `catalog_only` metadata; no runtime adapters exist for them. NVIDIA's exact `nvidia/nemotron-3-embed-1b` route has implemented transport/runtime integration and a successful synthetic hosted endpoint probe, but remains experimental, explicit, non-default, and non-fallback. NVIDIA rerank transport/fixtures exist, but the tested hosted routes were unavailable, so rerank remains non-selectable `catalog_only` metadata. Queries and passages leave the PC and no cross-provider fallback occurs. Cohere and NVIDIA trial credentials remain evaluation-only; production readiness also requires an appropriate production credential and current provider entitlement.

The checked-in provider catalog is revision **8** with canonical digest `f672ca123ad8ebf7815a6789a5638e638aaecc9e74e9b1da45104b0656a68ae3`. All automatic-fallback candidate flags are false. A fallback can exist only inside a named loadout as `ManualOnly` plus `user_authorized`, and using it requires a deliberate user switch.

## NVIDIA NIM API Catalog

NVIDIA NIM is represented as one multi-service hosted provider. Chat,
embedding, rerank, ASR, and TTS are separate route records so one service cannot
borrow another service's readiness. NVIDIA NIM Chat has a dedicated hosted
adapter and fixture tests, but remains experimental, explicitly selected,
non-default, and non-fallback. The exact Nemotron 3 Embed 1B route has an
implemented adapter, runtime integration, and a successful synthetic endpoint
probe, but remains experimental and explicitly selected. Rerank remains
`catalog_only` because tested hosted routes were unavailable despite transport
code and fixtures. Nemotron Streaming ASR has a fixed NVCF/Riva gRPC adapter and
green integration fixtures; it is experimental/manual because live audio
qualification is still pending and therefore blocks selection. Magpie TTS has
fixed NVCF HTTP/gRPC adapter tests, authorized HTTP stock-voice/audio evidence,
and a live-qualified stock Aria stream through the concrete fixed-origin Riva
gRPC transport. The ordinary selected-route runtime can construct Magpie, verify
the requested stock voice, and submit its PCM through authenticated broker
playback leases. It remains experimental/manual because an authorized
end-to-end normal-turn run, reliability, physical endpoint, and game-load
qualification are separate evidence gates. The route exposes 30 eligible
EN-US stock voices from 86 discovered voices, structurally forbids cloning, and
does not accept arbitrary provider options. Other Speech NIMs remain
unqualified: appearing in documentation or the API Catalog does not prove that
a free hosted endpoint exists.

Developer Program access is a one-account evaluation path: the current
[deployment FAQ](https://docs.api.nvidia.com/nim/docs/deployment) describes the
key as unique and account-bound for hosted endpoints and entitled NIM pulls.
Official catalog surfaces cover [LLM/chat](https://docs.api.nvidia.com/nim/reference/llm-apis),
[retrieval/embedding/reranking](https://docs.api.nvidia.com/nim/reference/retrieval-apis),
[vision](https://build.nvidia.com/explore/vision), and [ASR/TTS](https://build.nvidia.com/explore/speech).
One key can therefore authenticate the available routes across those modalities,
but it does not make every catalog entry hosted/free, unlock private access,
survive deprecation, or remove per-model terms and function IDs.

For NVCF gRPC, NVIDIA requires both `authorization: Bearer <key>` and the exact
`function-id`; see the [gRPC invocation contract](https://docs.nvidia.com/nvcf/dev/g-rpc-function-invocation).
The currently published [Magpie API](https://build.nvidia.com/nvidia/magpie-tts-multilingual/api)
uses function ID `877104f7-e885-42b9-8de8-f6e4c6303969`, while the
[Nemotron ASR API](https://build.nvidia.com/nvidia/nemotron-asr-streaming/api)
uses `bb0837de-8c7b-481f-9ec8-ef5663e9c1fa`. Function IDs are exact route
metadata; they do not broaden provider entitlement or fallback policy.

Free hosted endpoints are for prototyping, development, testing, and evaluation,
not unlimited use. NVIDIA's [API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf)
allow access-instance, duration, time, usage, credit, availability, and rate
limits plus service change/deprecation. The current [General NIM FAQ](https://docs.api.nvidia.com/nim/docs/product)
and [Run NIM Anywhere](https://docs.api.nvidia.com/nim/docs/run-anywhere) require
a separately licensed NVIDIA AI Enterprise or eligible partner path for
production.

Onboarding may recommend NIM as a convenience for users experimenting with
their own key across available preview modalities. It must still require
explicit egress consent and explicit model selection. NIM is never a silent
route, default provider, or automatic fallback, and the recommendation is not a
promise of permanent, unlimited, production, or commercial access.

Magpie voices are discovered at runtime. NVIDIA's [voice reference](https://docs.nvidia.com/nim/speech/latest/tts/voices.html)
documents model/locale/speaker names and optional emotional suffixes that vary
by voice. The catalog exposes only discovered eligible stock voices. NVIDIA's
separate zero-shot prompt-cloning capability is not enabled by this product.

Before enabling NVIDIA NIM, the UI must disclose that text, selected game
context, or audio leaves the PC. Do not send confidential, controlled or
sensitive, personal, or secret game data. NVIDIA's trial terms generally say
ordinary user/generated content is not stored after a session unless a
service-specific disclosure applies. The same governing terms disclose
product/model-improvement collection plus security/fraud/abuse logging, and
special services may retain data. Exact model licenses and service terms apply.
Review the current [NIM developer access page](https://developer.nvidia.com/nim),
[API Catalog quickstart](https://docs.api.nvidia.com/nim/re/docs/api-quickstart),
[NIM FAQ and rate-limit guidance](https://forums.developer.nvidia.com/t/nvidia-nim-faq/300317),
[LLM API reference](https://docs.nvidia.com/nim/large-language-models/1.12.0/api-reference.html),
and [API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf).

## Optional local lip-sync targets

The current Windows-local runtime research is recorded in the [Windows/NVIDIA AI runtime selection](../research/windows-nvidia-ai-runtime-selection-2026-08-30.md) and the separate [Windows/NVIDIA local LLM runtime selection](../research/windows-nvidia-llm-runtime-selection-2026-08-30.md). Both are qualification plans, not pack-admission or availability evidence.

| Candidate | Policy |
| --- | --- |
| Public Audio2Face-3D SDK + regression Mark v2.3 → project-owned mapper/compositor | Local low-latency coefficient/geometry candidate only. The SDK source is MIT and supports Windows x64, with CUDA `>=12.8,<13.0` (12.9 recommended) and TensorRT `>=10.13,<11.0`; Mark weights use the NVIDIA Open Model License. It produces animation data for a prepared character/renderer, not arbitrary captured-game pixels. NVIDIA's faster-than-60-FPS statement is not reproduced on this RTX 4080 beside a game. |
| NVIDIA LipSync AR SDK model | Separate NVIDIA AI for Media private-access downloadable experiment for one complete human face plus PCM speech. The current model card lists Windows 10/11 and same-resolution RGB output up to 4K, but a hosted NIM key alone does not grant access. Arbitrary stylized-game tracking, temporal stability, cancellation, current-frame compositing, latency, resources, and game impact remain unqualified. |
| Audio2Face-2D | Deprecated portrait-image generator; not a current generic captured-NPC path. |
| MuseTalk 1.5 | Offline comparator only. The latest realistic Mara Venn Windows run produced 39 frames for 1.56 s of audio in 87.873 s and peaked at 7,836 MiB GPU memory. Its visual result proves compatibility with that realistic human fixture, but the batch path fails live-latency admission and does not qualify a persistent runner, app integration or pack. |
| EfficientSync; FlashLips | Paper watchlist only; code, weights, license, Windows runtime, cancellation, resources and rendered behavior remain unqualified. |
| Ditto | Deferred until leading candidates and the baseline are resolved |
| LatentSync | Offline comparison only; rejected as a live-game route |
| Native game-rig integration, Wav2Lip, SadTalker, LivePortrait | Rejected as live product paths |

Conversation is API-first: there are no product download routes for local LLM, STT, TTS, or embedding models. The list above is not a redistribution or availability promise. Every eligible lip-sync runtime/weight needs an immutable source, hash, ABI, download/installed size, RAM/VRAM envelope, signed pack manifest, runtime self-test, explicit license approval, quality/game-impact report, and experimental disclosure. The checked-in catalog has no `installed_qualified` lip-sync route and therefore exposes no selectable or default local third-party model.

There is no generally available hosted NVIDIA NIM that performs end-to-end
lip-sync for arbitrary captured game NPCs. The [LipSync model](https://build.nvidia.com/nvidia/lipsync/modelcard)
is private-access/downloadable; [Audio2Face-2D](https://build.nvidia.com/nvidia/audio2face-2d/deploy)
is deprecated and portrait-oriented; and the old hosted [Audio2Face-3D endpoint](https://build.nvidia.com/nvidia/audio2face-3d/api)
is deprecated while the current [self-hosted A2F3D NIM](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html)
continues separately. NVIDIA also publishes the [Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK)
under MIT and the regression [Mark v2.3 weights](https://huggingface.co/nvidia/Audio2Face-3D-v2.3-Mark)
under the NVIDIA Open Model License. This makes A2F3D a credible local motion-
signal candidate, not a hosted one-key path or a pixel editor. It still needs a
project-owned tracked ROI mapper/compositor, cancellation, resource and rendered
quality qualification. The generic framebuffer path therefore remains the local
tracked current-frame residual/compositor.

Developer Program NIM containers are not installer assets. Section F of the
current [AI Product Terms](https://www.nvidia.com/en-us/agreements/enterprise-software/product-specific-terms-for-ai-products/)
limits that software to internal non-production evaluation/development/testing
and says it cannot be included in a customer product. Users acquire entitled
components directly from NVIDIA, or a future release obtains separate
enterprise/distribution rights. This does not erase the separate rights in the
public MIT SDK or model weights covered by the [Open Model License](https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/),
and those rights do not automatically grant redistribution of the proprietary
NIM container/runtime or third-party CUDA/TensorRT dependencies. Every component's
exact terms, notices, hashes and qualification remain independent.

The source game frame is immutable. Visual candidates may return only a bounded residual keyed to an exact actor/frame/timestamp/cancellation generation; invalid or stale output is discarded and presentation falls open to the untouched current frame. Current resource admission permits one optional local visual lease and uses live game reserve plus measured p99 workspace—not total VRAM. Any future local conversation-model scope requires a separate decision and co-residency matrix; it cannot silently share the lease, move devices or switch providers.

`ModelPackManifestV1` is implemented by `crates/model-manager/src/manifest.rs`. The example/schema boundary under `packaging/model-packs/` is illustrative until it is generated from or checked against the authoritative Rust contract.

The checked-in development catalog lives at `catalog/v1/catalog.json` and the loader at `crates/provider-catalog`. Route placeholders such as `$user_selected` intentionally avoid silently tracking changing hosted aliases. Production callers require a trusted signed catalog; the checked-in catalog is development-only and unsigned.

## Route readiness states

| State | Meaning | Selectable? |
| --- | --- | --- |
| `implemented_adapter` | Runtime adapter and fixture conformance evidence exist | Yes unless `live_qualification` is pending/unavailable; credentials and policy still apply |
| `catalog_only` | Metadata only; no runtime adapter | No |
| `pack_candidate_unqualified` | Local candidate without complete installation qualification | No |
| `installed_qualified` | Signed manifest, self-test, attestation, license approval, and benchmark evidence attached by the model manager | Only after explicit user activation; never a default/dependency/migration/game/fallback activation |

Generic screen-space lip-sync has no routable provider/model entry in this catalog. The animation rows above are research directions only, not implemented or selectable model claims. Native-rig/per-game animation is outside the product architecture.

`live_qualification` is independent of adapter readiness. `pending` and
`endpoint_unavailable` block selection and all automatic routing. A passed
synthetic probe records functional evidence only; it is never latency,
reliability, quality, or release-benchmark evidence.

## Provider capability contract

The catalog currently exposes route-level streaming, structured-output, latency, language, timestamp/alignment, lifecycle, egress, and readiness metadata. Adapter-specific contracts can expose additional transport details. The UI must use route-level capability/readiness fields—not a provider-wide capability union—to decide what is selectable. Unsupported means unsupported; the runtime does not emulate a capability silently.

## Structured effects

Spoken text streams through the speech lane. `NpcEffectsV1` is independently validated and can become neutral/no-op without delaying or retracting speech. A provider lacking native schema support gets at most the runtime's documented repair behavior; game-action requests are outside the contract.

## Externally managed OpenAI-compatible server

Advanced users may connect an externally managed compatible API endpoint. It is not treated as an installed pack, may not expose complete discovery/cancellation/usage semantics, and must pass simulation. “Compatible” does not mean behaviorally identical.
