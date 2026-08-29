# Providers and models reference

## Hosted provider targets

| Stage | Targets |
| --- | --- |
| LLM | OpenAI, Gemini, Anthropic, Groq, Cohere Chat, NVIDIA NIM Chat, configurable OpenAI-compatible endpoint |
| STT | Deepgram, AssemblyAI, ElevenLabs, OpenAI; NVIDIA Nemotron Streaming ASR adapter implemented but selection blocked pending live qualification |
| TTS | Cartesia, ElevenLabs, Inworld, Deepgram; NVIDIA Magpie experimental/manual with live gRPC qualification warning |
| Retrieval | NVIDIA Nemotron 3 Embed 1B implemented/experimental; NVIDIA NIM rerank catalog-only because tested hosted routes were unavailable |

Catalog entries describe capabilities; only adapter, fixture, integration, and endpoint evidence establish current support. Cohere Chat has an implemented hosted LLM adapter with streaming, cancellation, model discovery, and structured-output handling. Cohere Embed and Rerank appear only as non-selectable `catalog_only` metadata; no runtime adapters exist for them. NVIDIA's exact `nvidia/nemotron-3-embed-1b` route has implemented transport/runtime integration and a successful synthetic hosted endpoint probe, but remains experimental, explicit, non-default, and non-fallback. NVIDIA rerank transport/fixtures exist, but the tested hosted routes were unavailable, so rerank remains non-selectable `catalog_only` metadata. Queries and passages leave the PC and no cross-provider fallback occurs. Cohere and NVIDIA trial credentials remain evaluation-only; production readiness also requires an appropriate production credential and current provider entitlement.

The checked-in provider catalog is revision **6** with canonical digest `c32a6136ced1f1ce6923a7c922e6812c2ec25a3b9f644c3d39826bb3ebabba19`. All automatic-fallback candidate flags are false. A fallback can exist only inside a named loadout as `ManualOnly` plus `user_authorized`, and using it requires a deliberate user switch.

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
fixed NVCF HTTP/gRPC adapter tests plus green authorized HTTP stock-voice/audio
evidence, so it is implemented but experimental/manual; live gRPC streaming is
still replay-tested only and must be disclosed. The route exposes 30 eligible
EN-US stock voices from 86 discovered voices, structurally forbids cloning, and
does not accept arbitrary provider options. Other Speech NIMs remain
unqualified: appearing in documentation or the API Catalog does not prove that
a free hosted endpoint exists.

Developer Program access is a one-account experimentation path: one Developer
API key can authenticate available API Catalog model endpoints for that account,
but availability and unpublished/dynamic rate limits are model-specific. Free
hosted endpoints are for prototyping, development, testing, and evaluation—not
production or commercial use. Production requires a separate eligible NVIDIA
or partner entitlement.

Onboarding may recommend NIM as a convenience for users experimenting with
their own key across available preview modalities. It must still require
explicit egress consent and explicit model selection. NIM is never a silent
route, default provider, or automatic fallback, and the recommendation is not a
promise of permanent, unlimited, production, or commercial access.

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

| Candidate | Policy |
| --- | --- |
| Tracked viseme/mouth-warp | Low-resource generic baseline; no model availability claim and still requires rendered tracking/quality evidence |
| MuseTalk 1.5 | Public experimental comparator; an attested standalone Windows run produced 39 frames for 1.579 s of synthetic stock-voice audio, but its ~102 s batch-path wall time fails live-latency admission. It remains blocked pending a persistent runner, exact license/security review, quality, and game-impact evidence. |
| NVIDIA AR SDK LipSync/private NGC package | Conditional candidate; a normal NIM API key does not unlock it. Private access, Windows/Ada path, image + 16 kHz mono input, region/tracking, fixed 14-frame pre-roll, license, size/VRAM, quality, and impact must be qualified |
| Ditto | Deferred until leading candidates and the baseline are resolved |
| LatentSync | Offline comparison only; rejected as a live-game route |
| Native rigs/Audio2Face, Wav2Lip, SadTalker, LivePortrait | Rejected as live product paths |

Conversation is API-first: there are no product download routes for local LLM, STT, TTS, or embedding models. The list above is not a redistribution or availability promise. Every eligible lip-sync runtime/weight needs an immutable source, hash, ABI, download/installed size, RAM/VRAM envelope, signed pack manifest, runtime self-test, explicit license approval, quality/game-impact report, and experimental disclosure. The checked-in catalog has no `installed_qualified` lip-sync route and therefore exposes no selectable or default local third-party model.

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
