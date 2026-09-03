# NVIDIA and latency-first model qualification

Status: development evidence, not a release or production entitlement  
Measured: 2026-08-30 UTC  
Official provider/model status last verified: 2026-08-30 UTC  
Credential handling: native key source only; no value was printed, copied into
the repository, persisted by the probe, or included in an artifact.

## One NVIDIA Developer API key: observed result

| Role | Exact route/model | Bounded live result | Product decision |
| --- | --- | --- | --- |
| Discovery | `GET integrate.api.nvidia.com/v1/models` | HTTP 200; 83 records; 116.6–122.9 ms | Credential and catalog access proven. Availability remains refreshable metadata. |
| LLM | `nvidia/nemotron-3.5-lightning-30b-a3b`, streaming chat | HTTP 200; headers 251.2 ms; first content 1084.1 ms | Qualified development option. Preserve exact ID and measure inside the real turn. |
| Embedding | `nvidia/nemotron-3-embed-1b` | HTTP 200; 2048 finite dimensions; 338.6 ms | Qualified development option for explicitly authorized text egress. |
| Vision | `meta/llama-3.2-11b-vision-instruct` | HTTP 200; synthetic red fixture classified correctly; 515.6 ms | Endpoint access proven, but generic game identity should stay local/explicit by default. |
| TTS | Magpie multilingual NVCF function; stock `EN-US.Aria`; concrete fixed-origin Riva gRPC transport | HTTP discovery/synthesis produced valid 22.05-kHz mono WAV; the authorized gRPC run measured TLS 105 ms, discovery 261 ms/86 voices, first audio 816 ms, 948 ms total, and 59,392 non-silent unclipped PCM bytes | Provider transport and stock-voice development option qualified. Normal app-turn selection, physical speaker delivery, reliability, and game-load behavior remain separate gates. No cloning or uploaded prompt audio. |
| STT | Nemotron streaming ASR gRPC function | Official same-key route reached, but 12.0 s and 30.7 s bounded calls both timed out | Visible but disabled until the real app's latency/accuracy test passes. Do not silently substitute HTTP. |
| Lip-sync | Current NVIDIA LipSync AR SDK model | A public model card exists, but access requires the Private Access Program and a separately entitled download; there is no generally available hosted function for an ordinary Developer key | Ordinary one-key setup does not cover this product today. Do not make it the default. |

No numeric quota, remaining-credit, reset, or `Retry-After` headers appeared in
the successful discovery/probe responses. That absence is not an unlimited-use
signal. Sections 1.1–1.4 and 8 of NVIDIA's [API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf) allow NVIDIA-defined access-instance, duration, time, usage, credit, availability, and rate limits, restrict use to trial purposes, require a subscription after trial credits or for production, and allow change, deprecation, slowdown, or discontinuation. The current [General NIM FAQ](https://docs.api.nvidia.com/nim/docs/product) and [Run NIM Anywhere](https://docs.api.nvidia.com/nim/docs/run-anywhere) describe free Developer Program access for prototyping/research/development/testing and a separately licensed NVIDIA AI Enterprise production path. The UI therefore says “single-key evaluation with model-specific limits,” never “unlimited usage.”

The Magpie gRPC measurement is a bounded authorized provider-transport
qualification, not a release latency benchmark. It proves the checked-in client
can connect to the fixed NVIDIA Riva authority, discover provider stock voices,
receive live PCM, and reject silence/clipping in that run. It does not prove that
the Response Console has selected Magpie for a normal turn, that audio reached a
physical endpoint, or that the route remains available under another account,
region, load, or entitlement.

Official route and terms references:

- [General NIM FAQ](https://docs.api.nvidia.com/nim/docs/product)
- [Run NIM Anywhere](https://docs.api.nvidia.com/nim/docs/run-anywhere)
- [Deployment FAQ](https://docs.api.nvidia.com/nim/docs/deployment)
- [NVIDIA API Trial Terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf)
- [API Catalog quickstart](https://docs.api.nvidia.com/nim/re/docs/api-quickstart)
- [NVCF gRPC invocation metadata](https://docs.nvidia.com/nvcf/dev/g-rpc-function-invocation)
- [Magpie multilingual API](https://build.nvidia.com/nvidia/magpie-tts-multilingual/api)
- [Nemotron streaming ASR API](https://build.nvidia.com/nvidia/nemotron-asr-streaming/api)
- [Current LipSync access](https://docs.nvidia.com/nim/maxine/lipsync/latest/getting-started.html)

## Single-key hosted scope and per-model entitlement

The [deployment FAQ](https://docs.api.nvidia.com/nim/docs/deployment) describes
the NVIDIA/NGC key as unique, account-bound, and usable for hosted endpoints and
entitled NIM pulls. Current official catalog surfaces cover [LLM/chat](https://docs.api.nvidia.com/nim/reference/llm-apis), [retrieval](https://docs.api.nvidia.com/nim/reference/retrieval-apis), [vision](https://build.nvidia.com/explore/vision), and [ASR/TTS](https://build.nvidia.com/explore/speech). This supports a single-key NVIDIA evaluation profile across available LLM, embeddings/retrieval, vision, ASR, and TTS.

It does not create provider-wide entitlement. Availability remains account-,
model-, region-, and time-specific; private access and deprecation still apply.
NVCF gRPC also requires the exact model function ID alongside the bearer key.
For example, the official pages currently publish Magpie function ID
`877104f7-e885-42b9-8de8-f6e4c6303969` and Nemotron ASR function ID
`bb0837de-8c7b-481f-9ec8-ef5663e9c1fa`. Those IDs are route metadata, not a
signal that either service shares another model's readiness or license.

## Stock-voice discovery boundary

NVIDIA's [current voice documentation](https://docs.nvidia.com/nim/speech/latest/tts/voices.html) defines runtime `list_voices` discovery, model/locale/speaker names, and voice-specific emotional suffixes. Magpie Multilingual currently documents built-in speakers across twelve locales; emotional variants differ by speaker/locale, so the app must consume discovery rather than assume a universal matrix. NVIDIA also documents zero-shot prompt cloning separately. This project chooses discovered stock voices only, disables audio-prompt cloning, and retains deterministic character-to-voice binding.

## Local-first candidates for an advanced profile

These are qualification targets, not bundled or downloadable packs yet.

| Role | Candidate | Intended device | Why |
| --- | --- | --- | --- |
| LLM | Qwen3-4B-Instruct-2507 Q4_K_M through llama.cpp | CPU-first; partial GPU layers only after live admission | Keeps the game's VRAM available. A community benchmark reported 2.32 GiB and 21.66 tok/s on six CPU threads, but every PC must remeasure. |
| STT | Moonshine v2 Tiny/Small streaming | CPU | Official Apple M3 time-to-first-token results are 13.5/65.1 ms; Windows accuracy and latency remain unmeasured here. |
| TTS | Kokoro-82M v1.0 INT8 ONNX through sherpa-onnx | CPU | Small multilingual-capable stock-voice route with broad ONNX deployment; exact Windows RTF and voice assets still need qualification. |
| Retrieval | BGE-small-en-v1.5 INT8/FP32 | CPU | Small local semantic option; FTS5 remains the zero-model fallback. |
| Lip-sync | Current-frame landmark warp driven by TTS visemes/causal audio | GPU only for an optional tiny residual | Deterministic frame identity, queue-depth-one cancellation, and lowest game contention. |
| Animation signal | Audio2Face-3D SDK + regression Mark v2.3 | Windows GPU, subject to game-first admission | First NVIDIA low-latency coefficient/geometry spike; requires a project-owned mapper/compositor and has not been installed, run, or measured here. |

A safe 12 GB gaming preset keeps STT, TTS, and embeddings on CPU. The game and
lip-sync get first claim on GPU memory; the LLM stays CPU-first unless live DXGI
budget/usage plus measured p99 envelopes prove headroom.

## Whole-loadout admission and residency

Activation requires:

`game reserve + resident model VRAM + p99 workspace VRAM + safety margin <= GPU budget`

and:

`resident model RAM + safety margin <= system RAM budget`

Every selected role must have device/model/runtime-hash-bound measurements.
Unknown values fail closed. The resource broker prioritizes game presentation,
deadline-bound lip-sync/audio, STT, LLM, TTS, then background retrieval. Stale
visual work is dropped; it is never queued to play late.

Recommended residency policy:

1. Keep a repeatedly used local LLM warm on CPU; do not reload it every turn.
2. Keep STT/TTS/embeddings CPU-resident when their measured RAM fits.
3. Use a separate worker process per heavy model so termination really reclaims
   allocator/runtime state.
4. On GPU pressure, remove LLM GPU layers first, then evict optional vision and
   visual residuals; preserve audio/subtitles.
5. Re-admit after resolution, scene, driver, model revision, runtime, or device
   changes. A cached fit from another machine is not authority.

Primary model/runtime references:

- [Qwen3-4B-Instruct-2507](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507)
- [llama.cpp](https://github.com/ggml-org/llama.cpp)
- [Moonshine](https://github.com/usefulsensors/moonshine)
- [Kokoro](https://huggingface.co/hexgrad/Kokoro-82M)
- [sherpa-onnx Kokoro voices](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html)
- [BGE-small-en-v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5)
- [DXGI video-memory budgeting](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/nf-dxgi1_4-idxgiadapter3-queryvideomemoryinfo)

## Lip-sync decision

The shipping direction is a tracked current-frame mouth warp with an optional
tiny residual, not a talking-head video generator. Each job carries generation
epoch, actor track, source frame, capture QPC, ROI transform, occlusion state,
audio sample range, and playback-clock origin. Queue depth is one per actor. An
interrupt increments the epoch and restores the untouched current frame on the
next refresh.

MuseTalk 1.5 remains the first neural residual spike because its one-step 256²
editor is more plausible than diffusion alternatives, but its stock avatar
preparation, centered/noncausal audio context, identity/jitter limitations, and
consumer-GPU evidence prevent a product claim. NVIDIA LipSync NIM is an
enterprise/offline benchmark rather than a one-key hosted default. The current
[LipSync model card](https://build.nvidia.com/nvidia/lipsync/modelcard) requires
private access and describes a downloadable 613M-parameter AR SDK model taking
one complete human-face image plus 16-kHz mono speech for content localization.
It makes no arbitrary stylized-game actor-tracking, temporal-stability,
current-frame-compositor, or beside-a-game latency claim. The H4M variant uses
a [30-frame look-ahead](https://docs.nvidia.com/nim/maxine/lipsync-h4m/latest/limitations-and-known-behaviors.html), about one second at 30 fps.

[Audio2Face-2D](https://build.nvidia.com/nvidia/audio2face-2d/deploy) is currently
deprecated/downloadable and portrait-oriented. The old hosted
[Audio2Face-3D endpoint](https://build.nvidia.com/nvidia/audio2face-3d/api) is
also deprecated, while the current self-hosted [Audio2Face-3D NIM](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html)
continues separately.

NVIDIA now publishes the [Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK)
source under MIT for Windows x64 and Linux. Its Windows build requires CUDA
`>=12.8,<13.0` with 12.9 recommended and TensorRT `>=10.13,<11.0`. The official
[Audio2Face-3D collection](https://github.com/NVIDIA/Audio2Face-3D) distinguishes
that SDK from the proprietary NIM container and lists regression v2.3 and
diffusion v3.0 pretrained models. The regression [Mark v2.3 model](https://huggingface.co/nvidia/Audio2Face-3D-v2.3-Mark)
uses the NVIDIA Open Model License and is the first A2F3D low-latency signal
candidate for a bounded Windows spike.

That decision is not a pack qualification. Audio2Face-3D produces animation
coefficients/geometry for a prepared character and renderer, not a finished
mouth-pixel patch for an arbitrary captured game frame. The product would still
need a tracked actor/frame mapper and current-frame compositor, immutable source
preservation, queue-depth-one cancellation, exact source/model hashes, install
and load timing, p50/p95 latency, RAM/VRAM and p99 transient workspace, output
stability, rendered quality, and game-impact evidence. NVIDIA advertises
faster-than-60-FPS generation for the SDK; no SDK/model run was performed in
this research task, so that vendor result is unverified on the target RTX 4080
beside a running game and is not a product benchmark. The generic framebuffer
product therefore keeps the local tracked current-frame residual/compositor as
its baseline.

Key lip-sync references:

- [NVIDIA LipSync overview](https://docs.nvidia.com/nim/maxine/lipsync/latest/overview.html)
- [NVIDIA LipSync performance](https://docs.nvidia.com/nim/maxine/lipsync/latest/performance-results.html)
- [NVIDIA H4M limitations](https://docs.nvidia.com/nim/maxine/lipsync-h4m/latest/limitations-and-known-behaviors.html)
- [Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK)
- [Audio2Face-3D GitHub collection](https://github.com/NVIDIA/Audio2Face-3D)
- [Audio2Face-3D Mark v2.3 model](https://huggingface.co/nvidia/Audio2Face-3D-v2.3-Mark)
- [Audio2Face-3D NIM overview](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html)
- [MuseTalk 1.5](https://github.com/TMElyralab/MuseTalk)
- [EfficientSync](https://arxiv.org/abs/2608.18832)
- [FlashLips](https://arxiv.org/abs/2512.20033)
- [Azure Speech visemes](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/how-to-speech-synthesis-viseme)
- [Amazon Polly speech marks](https://docs.aws.amazon.com/polly/latest/dg/output.html)
- [MediaPipe Face Landmarker](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker)

## Container and model distribution boundary

The installer must not bundle Developer Program NIM containers or runtimes.
Section F of NVIDIA's [Product Specific Terms for AI Products](https://www.nvidia.com/en-us/agreements/enterprise-software/product-specific-terms-for-ai-products/)
limits Developer Program Enterprise Product software to internal,
non-production evaluation/development/testing and says it cannot be included in
a customer product. Users may acquire entitled components directly from NVIDIA
after accepting current terms, or a future distribution must obtain an
appropriate enterprise/distribution license.

Container/runtime, SDK-code and model rights are separate. The public
Audio2Face-3D SDK is MIT, while Mark v2.3 weights use NVIDIA's [Open Model License](https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/).
Neither license automatically grants redistribution of the proprietary NIM
runtime or third-party CUDA/TensorRT dependencies. Trial service, container,
SDK, exact model and transitive dependency terms and notices are recorded
independently; unknown or changed status fails closed.
