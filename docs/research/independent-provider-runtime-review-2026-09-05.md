# Independent provider, retrieval, and Windows runtime review

**Review date:** 2026-09-05
**Compared implementations:** `v1.0.0`, `main`, and `feat/2.0-overhaul`
**Evidence policy:** primary vendor documentation, official model cards, official repositories, and direct repository inspection only

Snapshot notice: this research intentionally made no credentialed calls, so its
catalog-versus-runtime findings describe the inspected September 5 baseline.
Follow-up code added fixed-origin Mistral/OpenRouter LLM and
Cartesia/Deepgram/Inworld TTS construction. A bounded production Groq Qwen →
Cartesia component-chain call reached bridge PCM in 492.964 ms. This does not
qualify microphone/STT, `HostState`, native playback, a physical speaker, or a
provider-wide latency guarantee. See
[the reconciled review record](../product-rework/local-review-2026-09-05.md).

## Scope and evidence boundary

This review covers hosted and local LLM, speech-to-text, text-to-speech, embedding, retrieval, Windows execution, and resource scheduling choices. Lip-sync model selection is deliberately excluded because it has a separate review. The hosted-provider review did not use credentials or make billable calls. Local model packs were not downloaded. Therefore this document establishes architectural fit and qualification work; it does not claim live-provider compatibility, end-to-end latency, transcription accuracy, speech quality, or model admission.

Fifty distinct official sources were opened and assessed. Vendor benchmark numbers below are kept separate from repository measurements. Marketing availability is not treated as a durable API contract.

## Executive verdict

The 2.0 design is a large improvement over v1 in security boundaries, cataloging, supervision, evidence handling, deterministic retrieval, and resource accounting. Its main defect is a mismatch between declared capability and executable capability. The catalog presents a broad provider and model surface while the runtime bridges implement a much smaller, partly simulated subset. The product must derive availability from registered executable adapters plus current discovery, not from catalog status alone.

The best direction is to preserve the current typed bridges, provider registry, signed-catalog production boundary, FTS5 plus reciprocal-rank fusion, supervised loopback workers, and conservative loadout admission. Replace hard-coded endpoint/model assumptions with capability negotiation and discovery. Wire the existing `ResourceBroker` into actual runtime job dispatch while retaining `ResourceGovernorV1` as the loadout preflight gate.

| Area | Decision | Reason |
|---|---|---|
| Hosted LLM | **Keep the adapter model; update protocols and discovery** | The bridge abstraction is sound, but endpoint generations and model IDs drift. Gemini should gain the GA Interactions route; Groq can use its current Responses-compatible route; Anthropic SSE parsing must tolerate ping, error, and unknown events. |
| Hosted STT | **Replace the single AssemblyAI route with a real adapter set** | The catalog calls OpenAI, Deepgram, AssemblyAI, and ElevenLabs stable while normal execution only constructs AssemblyAI. |
| Hosted TTS | **Finish the normal execution path before calling any provider stable** | At the September 5 source inspection, ElevenLabs and NVIDIA credentials could resolve while Cartesia, Inworld, and Deepgram were unresolved. The snapshot notice records the later fixed-origin transports and component qualification; physical output remains open. |
| NVIDIA NIM | **Hosted optional tier; never advertise “unlimited”** | The trial terms restrict unlicensed use to evaluation and allow service changes. Local Magpie TTS is incompatible with the target Windows/WSL footprint. Discover voices at runtime. |
| Local LLM | **Keep llama.cpp as the dependable cross-Windows path; qualify a current Qwen candidate** | The frozen Qwen3 pack has only stale, one-sample non-admissible evidence. TensorRT-LLM is a poor ordinary Windows desktop dependency. Foundry Local/Windows ML deserves an experimental Win11 lane. |
| Local STT | **Keep whisper.cpp fallback; update Moonshine source; evaluate sherpa-onnx** | Moonshine now officially supports Windows streaming. sherpa-onnx could consolidate VAD and streaming ASR, but every model license and artifact must be pinned. |
| Local TTS | **Use Kokoro as the small stock-voice candidate; evaluate Chatterbox Nano as a second tier** | Kokoro is small and Apache-2.0. Chatterbox is promising but the catalog entry is too vague and some examples require voice prompts. Qwen3-TTS is a quality tier with a larger GPU collision domain. |
| Local embedding | **Keep NVIDIA Nemotron-3 hosted; add a small offline lane** | The current NVIDIA model still exists and correctly uses fixed 2048 dimensions. EmbeddingGemma is a strong offline candidate but its access terms and conversion provenance require qualification. |
| Retrieval | **Keep FTS5 + deterministic RRF; make ANN optional** | Current exact semantic scan is correct for small stores but is `O(ND)`. sqlite-vec is useful only behind a pinned, rebuildable, pre-v1 adapter. LanceDB/Qdrant add unjustified operational weight at current scale. |
| Scheduling | **Separate preflight from live dispatch and wire both** | `ResourceGovernorV1` answers whether a loadout may be selected. `ResourceBroker` answers whether a particular job may run now. The latter appears unconstructed outside its own module. |

## What v1 actually did

The `v1.0.0` tag and old `main` are the same architectural era; their diff is mostly documentation and screen-grab work rather than a provider/runtime redesign. Useful product intent should survive, while the implementation methods should not.

| v1 behavior | Keep | Replace |
|---|---|---|
| One voice per character | Character-level voice intent and a deterministic fallback | Fixed Edge TTS subprocess invocation and generated `temp.py` execution. Use an in-process/supervised adapter with an explicit voice identity and streamed audio. |
| Public and character-specific knowledge stores | Separate visibility scopes | LangChain/Chroma coupling and top-1 retrieval based on recent conversation. Use the 2.0 typed memory store, exact scope filtering, hybrid retrieval, and provenance. |
| Recent conversation context and periodic summary | Bounded context and summary intent | Response-wide serialization, state commit before audio delivery, and a fixed 500-token heuristic. Commit only accepted turns and define interruption/partial-delivery semantics. |
| Cohere-generated dialogue | Provider-independent dialogue generation | Legacy `command`, non-streaming `.run`, high temperature, and a random plaintext API key selected from `apikeys.json`. |
| Google speech recognition through `speech_recognition` | A low-friction hosted STT option | Opaque consumer recognizer behavior and no endpoint/partial/final contract. |
| Edge TTS | It may remain a clearly labeled OS/network fallback if licensing and availability are acceptable | It must not be the core TTS path or spawn arbitrary generated Python. |

The 2.0 code correctly moves secrets into credential references, workers behind loopback supervision, catalogs into a schema, and memory into a first-party store. Reintroducing a LangChain-style universal wrapper would weaken those gains because it would obscure provider-specific streaming, retention, interruption, and deprecation behavior.

## Catalog claims versus executable truth

The current catalog contains 20 providers, 30 model entries, and eight pack templates. It is explicitly an unsigned development artifact; production requires a valid signature. That boundary should remain. The statuses need another dimension: a model may be cataloged and even “stable” while no normal runtime route can execute it.

| Capability | Catalog presentation | Executable 2.0 path found | Decision |
|---|---|---|---|
| LLM | OpenAI, Anthropic, Gemini, Groq, Cohere, NVIDIA NIM and `openai-compatible` entries | Fixed cloud adapters exist for OpenAI Responses, Anthropic Messages, Gemini `v1beta:generateContent`, Groq Chat Completions, Cohere Chat, and NVIDIA NIM. The ordinary bridge has no generic `openai-compatible` route. | Add `adapter_ready`, `protocol_revision`, and last probe result. Do not infer execution from catalog presence. |
| STT | OpenAI, Deepgram, AssemblyAI, and ElevenLabs marked stable; NVIDIA/local candidates | `RuntimeSttBridge` constructs only AssemblyAI and hard-codes `u3-rt-pro`. | Implement the stable adapters or downgrade their displayed readiness. Introduce versioned partial/final/endpoint events. |
| TTS | ElevenLabs stable; NVIDIA and other hosted/local candidates | Generic adapter types exist, but credential resolution only covers ElevenLabs and NIM. Deepgram, Cartesia, and Inworld deliberately return missing credentials. Selected adapter creation is used by tests/developer simulation, and the normal control plane exposes discovery without a complete selected TTS execution route. | Complete a production TTS session lifecycle: discovery, selection, streaming, cancellation, audio timestamps, retention disclosure, and errors. |
| Embedding | NVIDIA Nemotron-3, Cohere, and local candidates | The retrieval bridge only calls NVIDIA `nvidia/nemotron-3-embed-1b` and accepts exactly 2048 dimensions. | Keep this strict pin. Add availability probing and an offline fallback before claiming provider independence. |
| Local workers | llama.cpp, whisper.cpp/Moonshine, ONNX/TTS candidates | The frozen llama worker is the clearest supervised executable route. Several other pack entries remain qualification candidates. | Treat every pack as unavailable until artifact hash, license, runtime, health, latency, memory, cancellation, and cleanup gates pass on the current manifest. |

Readiness should be computed from all of the following:

1. A registered executable adapter for the exact capability and protocol.
2. A resolvable credential reference when the route is hosted.
3. A current endpoint/model/voice discovery or explicit provider probe.
4. A compatible terms and retention declaration.
5. For local packs, an installed, hash-verified manifest and current device-specific admission envelope.
6. A passing lifecycle fixture for streaming, cancellation, timeout, malformed frames, and cleanup.

A simple state vocabulary would be `cataloged`, `adapter-ready`, `credential-ready`, `discovered`, `qualified`, and `temporarily-unavailable`. “Stable” should be reserved for a route that is adapter-ready and has current qualification evidence.

## Hosted LLM decisions

### OpenAI

Keep the typed Responses adapter and stream event normalization. Model IDs and service tiers must remain catalog/discovery data. Capture request IDs and provider error envelopes without logging prompts, keys, or retained content. Qualification must cover incremental text, tool/cancel events if supported, usage events, and a connection that ends without a terminal event.

### Anthropic

Keep the Messages adapter. The official streaming contract uses SSE and can emit ping, error, and new event types. The parser must ignore or preserve unknown event kinds rather than aborting a turn. Cancellation must close the upstream stream and prevent a late terminal event from committing an interrupted response.

### Google Gemini

Replace the default legacy `v1beta generateContent` assumption with a versioned adapter that supports the GA Interactions API, which Google now recommends for new agentic applications. Retain the old route only as a compatibility adapter. Interactions are stored by default unless `store=false`; this must be an explicit retention option rather than an invisible provider default.

### Groq

Keep Groq as a low-latency hosted choice, but add its current Responses-compatible protocol and a deprecation feed/probe. Groq's official deprecation page contains multiple recent model retirements, which is direct evidence against baking model IDs into executable code.

### Cohere

Keep Cohere Chat as an optional hosted adapter and update it to the v2 API shape. Keep Embed v4 and Rerank v4 as catalog-only until the embedding/rerank bridge is implemented. Rate-limit and trial-key behavior must be displayed as provider policy, not converted into an application reliability promise.

## Hosted STT decisions

The STT interface needs a provider-neutral event contract before more providers are added:

```text
SpeechStarted(timestamp)
Partial(text, revision, audio_range)
SegmentFinal(text, segment_id, audio_range)
UtteranceFinal(ordered_segment_ids, endpoint_reason)
SpeechEnded(timestamp)
ProviderWarning(code, retryability)
```

This prevents adapters from flattening incompatible notions of partial, final, and endpointing into one string.

### AssemblyAI

Keep the current `u3-rt-pro` path. It remains an explicitly documented public Streaming v3 model ID; it is not a private alias and this review found no official removal notice. AssemblyAI's documentation is in a rolling transition: its Universal-3 Pro migration guide, language matrix, and several current examples still prescribe `u3-rt-pro`, while a newer official migration guide calls `universal-3-5-pro` the latest/highest-accuracy streaming model. Add `universal-3-5-pro` to the provider editor/catalog only as a separately qualified route. Do not silently alias or substitute it because its supported parameters, context behavior, modes, and turn semantics may differ. Compare both on held-out game speech containing names, fantasy terms, accents, noise, interrupted phrases, and long pauses. Measure time to first partial, time to committed text, utterance-final latency, word error rate, entity/name accuracy, revision churn, and reconnect behavior. AssemblyAI's published latency numbers are useful hypotheses, not acceptance evidence because they are vendor-run and workload-dependent.

### Deepgram

Implement Deepgram as a first-class adapter only with correct endpoint semantics. Its official documentation distinguishes `is_final` from `speech_final`; clients must concatenate finalized segments until the speech endpoint. A consumer that treats the first final segment as the whole utterance will truncate pause-separated NPC speech. Tune endpointing against game audio; the documentation's 300–500 ms suggestion is a starting range, not a universal value.

### ElevenLabs

Implement Scribe v2 Realtime behind an explicit partial/committed event mapper. Display retention behavior from the actual account/tier configuration; the official documentation says zero-retention requests are eligibility-dependent. No UI should imply that requesting zero retention proves that it was applied.

### OpenAI

Keep catalog support but mark the route unavailable until a real adapter is registered and its realtime/transcription event protocol is fixture-tested. A stable catalog label alone is insufficient.

### NVIDIA NIM ASR

Use a hosted experimental adapter if it passes live qualification. Do not make local NIM ASR the default for the target Windows laptop. NVIDIA's current speech matrix sets a 16 GB GPU platform baseline, and the WSL tutorial supports selected ASR models only. Nemotron streaming ASR itself is listed without WSL support. A model-level batch-memory figure does not override the platform/support matrix.

## Hosted TTS decisions

A usable TTS route needs more than a synthesize call. It needs discovered voice identity, deterministic selection, streamed audio framing, cancellation/barge-in, sample-rate metadata, word or phoneme timing when available, retry rules, and a retention/voice-policy declaration.

### ElevenLabs

Finish this route first because credential resolution and adapter structure already exist. For low-latency NPC turns, qualify the official `eleven_flash_v2_5` recommendation and compare it with the chosen quality tier. The realtime WebSocket documentation notes that the ordinary streaming route does not support every model, including `eleven_v3`, so the catalog must bind protocol and model compatibility rather than offering arbitrary combinations.

### Deepgram

Flux TTS is designed for streaming conversational turns and interruption. It is a strong second hosted candidate, but the repository needs a credential resolver and full WebSocket lifecycle before it can be advertised as usable.

### Cartesia and Inworld

Keep as catalog candidates. Both have current official streaming TTS APIs, but the repository deliberately lacks their credential resolution and executable normal path. They should appear as “adapter required,” not as selectable providers.

## NVIDIA NIM: voices, availability, and terms

NVIDIA must be presented as a hosted evaluation/enterprise option rather than a universal free tier.

The official API Trial Terms control over the developer page's “unlimited prototyping” wording. The terms limit trial access by time, credits, or other restrictions; without a subscription they permit internal testing and evaluation rather than production use; and NVIDIA may change, deprecate, suspend, or terminate access. One API key can authenticate to currently enabled endpoints, but it does not guarantee that every catalog model or voice will remain enabled.

For Magpie multilingual TTS:

- `Magpie-Multilingual.EN-US.Jason` is an official stock voice and can remain the deterministic English fallback.
- The current voice matrix also exposes styles such as neutral, calm, angry, and happy for supported voices. Store a character selection as `{model, voice_id, language, style}` and validate the tuple against current discovery.
- Call `/v1/audio/list_voices` at configuration refresh and cache the result with a timestamp. A hard-coded list may seed offline UI text but cannot establish current availability.
- If the chosen voice disappears, preserve the character setting, report temporary unavailability, and offer a deterministic stock fallback. Never silently remap a character to a different identity.
- This review does not recommend voice cloning. Stock voices avoid consent and provenance ambiguity and fit the repository's declared product scope.

Local Magpie is unsuitable for the ordinary target. NVIDIA's current TTS support matrix says TTS is not supported through WSL and lists the multilingual model's default batch-eight footprint at about 5.182 GiB host RAM plus 12.58 GiB GPU memory. That exceeds the 12,282 MiB device recorded in the repository before reserving the game, compositor, and desktop. Hosted Magpie can be offered only after credential, terms, retention, latency, and cancellation qualification.

The current NVIDIA model catalog still lists `nvidia/nemotron-3-embed-1b` as a free hosted endpoint, updated 2026-07-16. Its official support matrix fixes output at 2048 dimensions and does not support reduced dimensions, so the retrieval bridge's exact model and dimensionality check is correct. Do not replace it with `llama-nemotron-embed-1b-v2`; NVIDIA currently marks that older entry as deprecated/downloadable. Add a startup/configuration probe because endpoint availability remains mutable.

## Local models and Windows runtime

### LLM

Keep llama.cpp as the primary local execution engine for broad Windows support. It has official Windows build paths and CUDA/Vulkan backends, a tractable subprocess boundary, and fits the existing hidden loopback supervision and Job Object containment.

Do not advertise the current frozen Qwen3 pack as recommended or admitted. Its repository evidence is one non-admissible Vulkan sample: roughly 17.47 seconds to load, 7.80 seconds to first token, 18.47 tokens/second, 2.70 GB process working set, and a 3,623 MiB GPU-memory delta on the recorded RTX 4080 Laptop GPU. The migrated pack manifest also changes the artifact identity, so that sample cannot admit the current pack.

Qwen3.5-4B is a reasonable current candidate because the official model card exposes a newer compact instruction model. It still requires a reproducible GGUF conversion or an audited upstream artifact, llama.cpp revision pin, prompt-template conformance, cancellation tests, long-context memory measurements, and at least the repository's required 20 device-specific samples. Compare answer quality on NPC constraints and tool-free dialogue, not generic benchmark score alone.

TensorRT-LLM should not become the default desktop runtime. NVIDIA's official installation path centers on Linux containers and Ubuntu-tested Python wheels, creating packaging and support burden on an ordinary Windows game companion. Keep it outside the core unless a future native supported path produces a material measured gain.

Foundry Local and Windows ML deserve an experimental Win11 lane. Foundry Local provides an in-process native library, a Rust SDK, model load/unload management, ONNX Runtime, and automatic execution-provider selection. However, the newer dynamic execution providers require Windows 11 24H2 or later; DirectML is now the legacy compatibility path. This conflicts with the repository's Windows 10 support promise. The practical plan is:

1. Keep llama.cpp as the Win10/Win11 baseline.
2. Add a feature-gated Foundry Local probe for eligible Win11 hosts.
3. Record the selected execution provider and exact runtime/model artifact in evidence.
4. Fall back cleanly when the OS, adapter, or model is unsupported.

For ONNX Runtime workers, pin runtime and CUDA/cuDNN compatibility. `gpu_mem_limit` only limits the provider's arena; the official documentation warns that total device use can be higher. External allocators and streams are advanced optimization options that should be introduced only with synchronization and lifetime tests. DirectML sessions must disable memory-pattern optimization and cannot run parallel calls on the same session.

### STT

Moonshine remains a strong local streaming candidate, but the catalog points at the old organization. The official repository now lives under `moonshine-ai/moonshine`, supports Windows, and distinguishes current code licensing from legacy non-English model licensing. Update the source/revision and record the exact model license rather than applying a repository-wide assumption.

Keep whisper.cpp as the robust local fallback. Its official project supports Windows and multiple GPU backends, but its common chunked transcription path is not equivalent to a native incremental streaming recognizer. Measure endpoint latency and repeated/revised text honestly.

Evaluate sherpa-onnx as a consolidated VAD plus streaming/non-streaming ASR runtime. It has official Windows and Rust support and a large model family. That breadth is also its main risk: each downloaded model has separate provenance, language coverage, and licensing. It is a candidate runtime, not blanket approval for its model zoo.

### TTS

Use Kokoro-82M as the first small stock-voice candidate. The official model card identifies an 82M Apache-2.0 model. If using the community `kokoro-onnx` runtime, pin the converter/runtime commit and hash the converted ONNX plus voice data. Do not consume an unverified pickle checkpoint during normal app startup.

Replace the catalog's vague Chatterbox entry with exact candidates. The current official repository distinguishes Nano (110M, CPU-oriented), Turbo (350M), and Multilingual V3 (500M). Nano is the best desktop experiment, but vendor “3x realtime on eight cores” is a claim to test on the target while the game runs. Examples for Nano/Turbo accept reference audio prompts, so a product default must use redistributable stock prompts or a separately reviewed consent/provenance flow. Do not imply that an MIT code repository automatically grants rights to arbitrary cloned voices.

Qwen3-TTS is an optional quality tier. Its official repository is Apache-2.0 and offers streaming-capable models, but even the smaller tiers create a larger runtime and GPU collision domain than Kokoro. Qualify it only after foreground game headroom, interruption, first-audio latency, and cleanup pass.

### Embeddings and reranking

Keep hosted Nemotron-3 as the only currently executable embedding route. Add an offline route so retrieval does not depend on a mutable endpoint.

EmbeddingGemma is the strongest small offline candidate found in this pass: the official documentation describes a 308M multilingual model, 2K input, Matryoshka dimensions from 128 to 768, and quantized memory below 200 MB. Those are vendor characteristics, not repository measurements. Its Gemma terms/access flow and the provenance of any ONNX conversion must be recorded in the pack ledger.

Qwen3-Embedding-0.6B is a larger instruction-aware alternative with Matryoshka output, but its roughly 1.21 GB model artifact and extra compute make it a second-tier comparison rather than the default. Hosted Cohere Embed v4 and Rerank v4 should remain optional until the bridge exists and retention/cost/latency are measured.

## Retrieval architecture

Keep the current SQLite FTS5 external-content design and deterministic reciprocal-rank fusion. The code correctly maintains the FTS shadow table through triggers and combines lexical and semantic ranks with a stable constant. Preserve public/character scopes, provenance, confidence, and rebuildable embeddings.

Two changes are required:

1. Bind every semantic query to the active embedding generation, exact model ID, dimension, normalization rule, and preprocessing version. Current rows carry model/generation metadata, but a query without an exact generation can scan mixed generations; incompatible vector lengths are silently skipped by cosine scoring. Use staged dual-generation rebuilds followed by an atomic active-generation flip.
2. Establish a scale threshold. The current semantic candidate path loads eligible vectors and computes exact cosine in Rust, which is `O(ND)`. That is a good correctness baseline for small NPC stores. At measured scale, add a pinned sqlite-vec adapter and compare recall, latency, database size, crash recovery, migration, and filtered-scope behavior.

sqlite-vec is attractive because it is an embedded SQLite extension with Windows builds and int8/binary options, but its official repository still calls it pre-v1. It must remain optional, version-pinned, and removable because embeddings can be rebuilt from source records. LanceDB is capable but solves a larger-scale problem than the current desktop needs. Qdrant's Windows quickstart requires Docker and is not an acceptable ordinary desktop dependency.

## Resource scheduling and process containment

The repository already contains the right two layers but only one is visibly used.

`ResourceGovernorV1` is a conservative loadout gate. It requires a signed measurement envelope for the exact device, model, runtime, backend, and mode; requires at least 20 samples; expires old evidence; sums selected models' resident and p99 workspace GPU memory plus p99 total RAM; applies 90% GPU and 85% RAM limits; reserves at least 1.5 GB or 15% GPU memory and 2 GB RAM; and fails closed on stale or incomplete game telemetry. Keep this behavior.

`ResourceBroker` is a per-job runtime scheduler. It models resident and transient GPU memory, transient duration, GPU time, RAM, shared/exclusive leases, priority, deadlines, staleness, supersession, utilization pressure, and residency. It is the stronger solution to STT/TTS/LLM overlap. Repository search found no production construction of `ResourceBroker::new` outside its defining module, so its existence is not evidence of runtime enforcement.

### Bounded core integration completed on 2026-09-05

The audit finding was converted into an opt-in runtime-core seam without fabricating hardware values:

- `TurnSupervisor::new_with_resource_admission` now accepts one process-owned `ResourceBroker` and a host-owned `TurnResourcePlanner`.
- Once that constructor is chosen, every turn requires an explicit `TurnResourcePlan`. A hosted route must return an explicit zero-local-resource target; it cannot bypass the gate by returning no plan.
- Runtime core constructs the job ID, `InteractiveTurn` kind, utterance binding, and privacy context from the validated turn. The host supplies only the measured target envelope, fallbacks, deadline, priority, and drop policy.
- Turn start requires an immediate lease. A queued submission is removed and returned as a concrete `ResourceNotAdmitted` error because `TurnSupervisor` cannot safely claim an arbitrary lease from a later global `poll_ready` call.
- Turn-handle cancellation, barge-in, shutdown, and broker-initiated cancellation converge on the same generation cancellation. The lease remains alive during cooperative audio drain and is then released.
- Lifecycle completion without a trustworthy usage sampler is recorded with `actual_usage_measured: false`; zero values from that path cannot be mistaken for benchmark evidence.
- `BudgetSnapshot` now carries `additional_game_ram_reserve_bytes` and `additional_game_vram_reserve_bytes`. These fields mean reserve headroom not already present in the reported used values, preventing double counting while preserving the configured game floor.

Focused tests demonstrate that additional game reserve denies a turn before provider work, turn-handle cancellation releases its reservation so the next turn can run, and broker-initiated cancellation reaches the turn generation and releases the lease. All 51 `npc-runtime-core` tests pass. The desktop/runtime host still needs to construct this opt-in path from fresh native telemetry and qualified envelopes; keeping the old constructor until those inputs exist is preferable to inventing numbers.

This bounded integration deliberately uses a conservative whole-turn envelope. Existing types can express resident and transient memory, and the transient amount can include measured KV-cache use. They do not carry trustworthy per-phase overlap or KV-cache provenance. Phase-aware LLM/TTS/embedding leases should be added only after signed measurements identify each phase and the process-wide dispatcher can hand a later queued lease to its correct waiter.

Use the layers as follows:

- **Loadout preflight:** `ResourceGovernorV1` decides whether the chosen installed set is allowed on the current device and selected game reserve.
- **Live admission:** one process-wide `ResourceBroker` decides whether to run, queue, degrade, reroute, or drop each concrete job.
- **Residency lifecycle:** model load/unload events update one authoritative residency ledger used by the broker and exposed to telemetry.
- **Foreground priority:** VAD/STT finalization and first TTS audio get short deadlines. LLM continuation is cancellable. Retrieval embedding rebuild, pack validation, and prefetch are background jobs that may yield.
- **Degradation:** prefer hosted fallback or a smaller already-resident local tier before loading a second large model. Never evict the active TTS/STT model during an utterance.
- **Game pressure:** use DXGI process budget/usage, safely loaded NVML process memory and utilization, fresh selected-game telemetry, and an optional frame-time signal. A static GPU total is insufficient.

The governor currently sums model peaks conservatively. Preserve that default until temporal phases are proven. Later, a signed workload contract may declare mutually exclusive phases, but only if the live broker actually enforces them. Otherwise phased arithmetic would admit combinations that can overlap in reality.

All workers should remain in Windows Job Objects with kill-on-close. Add per-process accounting and graceful cancellation before termination. Windows Job Objects also support memory and CPU-rate controls, but hard caps can make foreground speech miss deadlines. Use those limits for background indexing/validation after measurement. EcoQoS is appropriate for background prefetch, embedding rebuild, and validation; do not apply it to foreground capture, STT, TTS, audio playback, or the runtime broker. DXGI explicitly warns that exceeding the reported video-memory budget causes paging and performance penalties, so admission should react before the budget is crossed.

## Workload-specific qualification matrix

| Workload | Required evidence | Important caveat |
|---|---|---|
| Hosted LLM | 100+ scripted turns across normal, long, cancel, timeout, malformed stream, quota, and model-deprecated cases; p50/p90/p99 first-token and completion latency | Network geography, account tier, prompt size, provider load, and output length dominate. A single successful response proves little. |
| Hosted STT | Held-out clean/noisy/game audio; partial and final latency; WER; name/entity accuracy; endpoint truncation; revisions; reconnect | Vendor benchmarks are self-run and use different audio, geography, and concurrency. |
| Hosted TTS | First-audio and completion latency, real-time factor, underruns, cancellation tail, timestamp coverage, voice discovery failure | Voice/model availability, account tier, text length, region, sample rate, and style change results. |
| Local LLM | At least 20 current-manifest runs for cold load, warm first token, tokens/s, RAM/VRAM p99, cancellation, unload, crash cleanup, simultaneous game pressure | The existing Qwen sample is one run on an older artifact identity and is non-admissible. |
| Local STT | Continuous microphone simulation plus recorded corpus; CPU/GPU usage, real-time factor, endpoint delay, WER, revisions, hour-long leak test | Chunked whisper output must not be mislabeled as native streaming. |
| Local TTS | Stock-voice intelligibility/MOS proxy review, real-time factor, first audio, CPU/GPU/RAM, interruption, repeated load/unload, concurrent game | Vendor speed claims generally exclude this app's capture, compositor, and game load. |
| Embedding/retrieval | Recall@k on annotated NPC facts, lexical-only misses, semantic-only misses, filtered scopes, rebuild/migration, p99 latency at realistic N | Changing dimension/model/preprocessing creates a new generation; mixed generations invalidate comparison. |
| Runtime broker | Deterministic simulated pressure tests plus target-device soak with overlapping capture, STT, retrieval, LLM, TTS, and game reserve | Passing unit scheduling tests does not prove the broker is wired into production dispatch. |

## Required implementation sequence

1. Make capability readiness executable: register adapters, expose protocol revision, and compute UI availability from adapter + credential + discovery + qualification state.
2. Complete the TTS session path and lifecycle using ElevenLabs first; include streaming cancellation and timing metadata needed by lip-sync.
3. Version the STT event contract, keep the official `u3-rt-pro` native route, add `universal-3-5-pro` as a separately qualified provider-editor choice, then implement Deepgram correctly around segment-final versus utterance-final behavior.
4. Add endpoint/model/voice discovery with cached timestamps and graceful unavailability. Apply this to NVIDIA first.
5. Construct the new opt-in `TurnSupervisor` resource-admission path in the runtime host from fresh native telemetry and qualified envelopes. Then extend the process-wide `ResourceBroker` to model load, STT, retrieval embedding, LLM generation, and TTS dispatch. Keep `ResourceGovernorV1` as signed loadout preflight.
6. Requalify the llama.cpp pack against its current manifest. Benchmark a pinned Qwen3.5-4B candidate without changing the default until it passes.
7. Build small local fallback lanes: Moonshine or sherpa-onnx for streaming STT, Kokoro for stock TTS, and an audited EmbeddingGemma ONNX artifact for embeddings.
8. Enforce exact embedding generation at query time. Keep exact cosine as the baseline; introduce sqlite-vec only after a measured scale trigger.
9. Run headless fixtures and target-device workload tests. Write evidence to the ledger with device, OS, driver, runtime, backend, artifact hash, workload, sample count, and date.

## Acceptance gates

The provider/runtime overhaul is ready for a local review application only when all of these statements are true:

- Every selectable capability has an executable registered adapter, and catalog-only entries are visibly non-selectable.
- No plaintext provider key is stored in repository data, logs, screenshots, or evidence artifacts.
- Hosted streams handle cancellation, late frames, unknown events, quota/rate errors, credential errors, and endpoint/model removal.
- Voice and model discovery results display their retrieval time and do not silently rewrite character choices.
- NVIDIA trial/evaluation status is described according to the governing terms, without “unlimited” or production-ready claims.
- The normal app route, rather than only developer simulation, performs selected STT, retrieval, LLM, and TTS lifecycle calls.
- The runtime host constructs `TurnSupervisor::new_with_resource_admission` from fresh telemetry and qualified evidence, and all local inference jobs request a lease before allocating/loading/running.
- Current-manifest local packs have license/provenance records, hashes, install/uninstall recovery, health checks, cancellation, cleanup, and device-specific admission evidence.
- Retrieval never mixes embedding generations and can rebuild all vectors from source records.
- Headless end-to-end tests cover cold start, offline mode, no credentials, invalid credentials, provider removal, local worker crash, interruption/barge-in, game-pressure rejection, and clean shutdown.

## Kokoro local TTS execution qualification on 2026-09-05

The existing Kokoro candidate was downloaded from its two pinned official sherpa-onnx GitHub release URLs into `E:\temp\InteractiveNPCs\model-packs`. The lifecycle code verified archive sizes and SHA-256 digests, extracted them safely, checked all pinned critical model files, wrote a complete inventory, and verified that inventory again. The payload is ONNX plus the fixed upstream `voices.bin`; no pickle or model conversion is involved. Model weights and DLLs remain outside the review application.

The new `npc-local-tts-native` crate is an executable local route. Its safe Rust provider implements runtime-core `TtsProvider` directly and launches a hidden, kill-on-drop worker. The worker contains the isolated unsafe sherpa-onnx 1.13.6 C ABI, restricts Windows dependency loading to the selected pack directory, verifies six critical model files again, checks sherpa/Git/ONNX Runtime identities at load, exposes only the 28 pinned English stock voice IDs, and emits bounded framed PCM. It has no credential or network path. The selected voice and locale must match exactly; it never substitutes another voice. Both pipe writes and worker-event inactivity have explicit deadlines. Cancellation also applies while waiting for the one-synthesis gate and suppresses PCM already queued in the OS pipe. A malformed, timed-out, or internally rejected synthesis retires the child; the next synthesis launches and handshakes a fresh process. The core-facing terminal audio chunk retains real PCM rather than emitting an empty end marker.

Execution disproved an earlier assumption: sherpa's Kokoro progress callback delivered one complete input fragment, not continuous neural audio. The original 20-sample Python qualification reached its load and synthesis work but failed cancellation because an eightfold long utterance produced no callback within 15 seconds. It produced no accepted report or resource envelope. The native worker now divides a runtime sentence at natural boundaries into fragments of at most 36 characters. Each fragment is an independent official C-ABI generation. This gives honest clause-incremental PCM and cancellation between clauses; it is not neural token streaming.

Fresh optimized native observations on this PC, under varying concurrent build/model load, were:

| Observation | Result |
|---|---:|
| worker load | 4,274-8,400 ms in the final clause-streaming runs |
| first PCM, 120-character test | 3,337-6,587 ms across observed runs |
| total synthesis | 18,297-34,392 ms |
| emitted audio | 6,681-6,684 ms, 160,340-160,412 frames, 24 kHz mono PCM s16le |
| real-time factor | 2.74-5.15 |
| exact worker working set observed during load/audio | 325,951,488-326,324,224 bytes |
| cancellation drain after immediate cancel | 6,467-7,170 ms, bounded by the in-flight fragment |
| signal | peak 11,448/32,767, RMS 0.0543, normalized DC 0.0000007 after filtering |
| warm session reuse | below 100 ms by assertion; the verified model stays resident across turn sessions |

The failed Python run left three structurally valid listening-review WAVs. FFprobe confirmed 24 kHz mono PCM s16le. None clipped: peaks were -10.1 dBFS (`af_heart`, 1.036 s), -6.2 dBFS (`af_jessica`, 6.115 s), and -5.6 dBFS (`af_bella`, 22.075 s). Their full-file DC offsets were 0.0261-0.0268, large enough to reject unfiltered delivery. The native worker applies a continuous one-pole DC blocker at about 19 Hz; its real test enforces absolute normalized DC below 0.002.

Seven process-boundary tests use a real child executable to prove cancellation while queued, stream-drop cancellation and gate release, post-cancel PCM suppression, non-empty terminal audio, malformed-frame retirement and restart, hung-read timeout/restart, and a cancelled hung child returning a bounded cancellation result. Two deterministic consumer-boundary tests additionally queue PCM before cancellation and prove that no audio escapes, then prove cancellation wakes a pending receiver without any child event. The public stream emits one cancellation result, discards its buffer, terminates, and triggers the existing worker drain through its abandonment token. These are lifecycle checks, not model qualification. A release-mode real-pack rerun after the final lifecycle fixes passed with 3,445 ms load, 5,521 ms first PCM, 27,556 ms total synthesis, 6,682 ms audio, 4.12 RTF, 325,951,488 bytes observed worker working set, and 6,467 ms immediate-cancel drain. The latency and cancellation results reinforce the decision not to admit this route.

The frozen source passes `cargo clippy --locked --offline -p npc-local-tts-native --all-targets --all-features -- -D warnings`. The matching locked, offline all-target test run passes the five library tests, native-worker segmentation test, and seven child-process boundary tests; the asset-gated real-pack test remains explicitly ignored in that ordinary run.

**Eligibility decision:** keep Kokoro as an explicit optional offline fallback and native integration candidate. Do not select it by default and do not admit it yet. It is slower than real time on this CPU, the 20-sample suite did not complete cancellation acceptance, zero VRAM was not retained in a successful report, the 326 MB value is one observed process working set rather than a p99 additional-RAM envelope, and no signed current-device envelope binds the configured game reserve. The corrected hosted NVIDIA path reached roughly 193-197 ms warm first PCM during the same review, so Kokoro is not a latency competitor on this host. A later signed catalog revision should replace the legacy Python entrypoint declaration with the native worker only after repeated native load/synthesis/cancel measurements and listening review pass.

## Source ledger

All sources were accessed on 2026-09-05. “Current” below means current as observed on that date, not a promise of future availability.

| # | Official source | Date/revision observed | Claim used | Workload or policy caveat | Decision |
|---:|---|---|---|---|---|
| 1 | [Google Gemini Interactions API overview](https://ai.google.dev/gemini-api/docs/interactions-overview) | GA/current page, accessed 2026-09-05 | Google recommends Interactions for new agentic applications; interactions are stored by default unless disabled. | API availability and storage controls may vary by service/account. | Add a versioned Interactions adapter and explicit `store=false` option. |
| 2 | [Anthropic streaming Messages](https://platform.claude.com/docs/en/build-with-claude/streaming) | Current, accessed 2026-09-05 | SSE can include ping, error, and future event types. | Fixtures are still required for disconnections and cancellation races. | Tolerate unknown events and normalize terminal/error semantics. |
| 3 | [Groq overview](https://console.groq.com/docs/overview) | Current, accessed 2026-09-05 | Groq exposes OpenAI-compatible APIs including Responses. | Compatibility does not prove identical event/error behavior. | Add an explicit Responses protocol revision. |
| 4 | [Groq deprecations](https://console.groq.com/docs/deprecations) | Multiple 2026 entries | Model IDs and capabilities are actively retired. | Removal timing can change. | Discovery/probe state must control availability. |
| 5 | [Cohere v2 API release](https://docs.cohere.com/v2/changelog/v2-api-release) | v2/current, accessed 2026-09-05 | Cohere has a v2 API shape for Chat/Embed/Rerank. | Existing v1 wrappers do not establish v2 correctness. | Version the adapter; do not revive the v1 LangChain path. |
| 6 | [Cohere rate limits](https://docs.cohere.com/v2/docs/rate-limits) | Current, accessed 2026-09-05 | Trial and production limits differ by endpoint/account. | Limits are account- and product-dependent. | Surface retryability and avoid availability promises. |
| 7 | [AssemblyAI streaming benchmarks](https://www.assemblyai.com/docs/streaming/benchmarks) | May 2026 page | Vendor reports latency distributions for current streaming models. | Self-reported benchmark on vendor workloads and regions. | Use only as a hypothesis; benchmark game audio locally. |
| 8 | [AssemblyAI Universal Streaming to U3 Pro migration guide](https://www.assemblyai.com/docs/streaming/migration-guides/universal-to-u3-pro-streaming) and [newer Gladia migration guide](https://www.assemblyai.com/docs/streaming/migration-guides/gladia-to-aai-streaming) | Both current, accessed 2026-09-05 | Official pages still require `u3-rt-pro` for U3 Pro while the newer guide identifies `universal-3-5-pro` as the latest/highest-accuracy route. | Documentation is in a rolling transition; neither page proves aliasing or event compatibility. | Keep the existing native route. Add 3.5 to the provider editor only after separate fixtures and live qualification. |
| 9 | [Deepgram endpointing and interim results](https://developers.deepgram.com/docs/understand-endpointing-interim-results) | Current, accessed 2026-09-05 | `is_final` segments must be accumulated until `speech_final`; endpoint tuning is workload-specific. | Suggested millisecond settings are starting points. | Define segment-final and utterance-final separately. |
| 10 | [ElevenLabs realtime STT server streaming](https://elevenlabs.io/docs/eleven-api/guides/how-to/speech-to-text/realtime/server-side-streaming) | Current, accessed 2026-09-05 | Scribe realtime emits evolving and committed transcript events. | Network/account/retention settings affect behavior. | Add an event-normalizing adapter. |
| 11 | [ElevenLabs realtime STT API reference](https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime) | Current, accessed 2026-09-05 | Realtime WebSocket parameters include retention-related options. | Requested zero retention is eligibility/tier-dependent. | Report actual applicable policy, not requested intent. |
| 12 | [ElevenLabs realtime TTS WebSocket guide](https://elevenlabs.io/docs/eleven-api/guides/how-to/websockets/realtime-tts) | Current, accessed 2026-09-05 | Low-latency model guidance and model/protocol restrictions are explicit. | First-audio latency depends on text chunking, region, voice, and tier. | Qualify `eleven_flash_v2_5`; bind model compatibility to protocol. |
| 13 | [Deepgram Flux TTS quickstart](https://developers.deepgram.com/docs/flux-tts/quickstart) | v2/current, accessed 2026-09-05 | Flux provides streaming conversational TTS and interruption-oriented control. | Quickstart success is not production lifecycle evidence. | Strong second hosted candidate after adapter completion. |
| 14 | [Cartesia TTS WebSocket reference](https://docs.cartesia.ai/api-reference/tts/websocket) | Current, accessed 2026-09-05 | Cartesia has a streaming WebSocket API. | At inspection the repository had no complete credential/execution route; the snapshot notice records the later transport. | Keep catalog-only until implemented and qualified; later evidence clears the component route only. |
| 15 | [Inworld TTS quickstart](https://docs.inworld.ai/quickstart-tts) | Current, accessed 2026-09-05 | Inworld exposes a current TTS API. | Quickstart does not establish app retention, timing, or cancellation semantics. | Keep catalog-only until implemented and qualified. |
| 16 | [NVIDIA Riva NIM TTS voices](https://docs.nvidia.com/nim/speech/latest/tts/voices.html) | Updated 2026-08-13 | Current stock voice IDs and styles include Magpie multilingual voices; runtime voice listing is supported. | Hosted availability may change; local support differs. | Discover at runtime; retain Jason as a deterministic stock fallback. |
| 17 | [NVIDIA Magpie multilingual API](https://build.nvidia.com/nvidia/magpie-tts-multilingual/api) | Current, accessed 2026-09-05 | Hosted endpoints expose voice listing and synthesis. | Catalog presence is governed by account access and trial/subscription terms. | Hosted optional adapter only. |
| 18 | [NVIDIA speech TTS support matrix](https://docs.nvidia.com/nim/speech/latest/reference/support-matrix/tts.html) | Current, accessed 2026-09-05 | TTS has no WSL support; Magpie multilingual default memory footprint is large. | Figures are configuration-specific and exclude the game/app reserve. | Reject local Magpie for the ordinary 12 GB Windows target. |
| 19 | [NVIDIA API Trial Terms PDF](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf) | Current document accessed 2026-09-05 | Trial use is limited and intended for internal evaluation absent a subscription; service may change or end. | Governing terms and the user's actual subscription control. | UI says trial/evaluation; no “unlimited” or production promise. |
| 20 | [NVIDIA NIM developer page](https://developer.nvidia.com/nim) | Current marketing page, accessed 2026-09-05 | Markets “unlimited prototyping” and downloadable/hosted options. | Marketing wording is subordinate to applicable terms and entitlement. | Do not use the phrase as a product entitlement claim. |
| 21 | [NVIDIA NIM offerings](https://docs.nvidia.com/nim/large-language-models/latest/about-nim-llm/nim-offerings.html) | Updated 2026-09-04 | Exploration, NIM Certified, and production branches have different support/subscription conditions. | Product branch and support entitlement matter. | Record offering/entitlement per selected route. |
| 22 | [NVIDIA model catalog search for embeddings](https://build.nvidia.com/models?q=embed) | Nemotron-3 updated 2026-07-16 | Nemotron-3 Embed 1B remains a hosted endpoint; older Llama Nemotron Embed v2 is deprecated/downloadable. | Catalog status can change. | Keep current Nemotron-3 pin and add availability probe. |
| 23 | [NVIDIA NeMo Retriever embedding support matrix](https://docs.nvidia.com/nim/nemo-retriever/text-embedding/2.2/support-matrix.html) | Version 2.2/current page | Nemotron-3 uses fixed 2048-dimensional output with no reduced dimensions. | Version-specific. | Keep exact 2048 validation and generation identity. |
| 24 | [NVIDIA NIM speech WSL tutorial](https://docs.nvidia.com/nim/speech/latest/get-started/tutorials/wsl.html) | Public beta/current, accessed 2026-09-05 | WSL support is limited to selected speech models and generally asks for substantial WSL memory; TTS/NMT are excluded. | WSL support differs by exact model/version. | Do not generalize “NIM supports WSL” across speech tasks. |
| 25 | [NVIDIA ASR support matrix](https://docs.nvidia.com/nim/speech/26.05.0/reference/support-matrix/asr.html) | 26.05.0 | ASR platform requirements and WSL support vary; Nemotron streaming ASR lacks WSL support. | Model batch memory alone does not override platform baseline. | Hosted experiment only on the target machine. |
| 26 | [llama.cpp official repository](https://github.com/ggml-org/llama.cpp) | Current, accessed 2026-09-05 | Official Windows builds support multiple CPU/GPU backends and GGUF inference. | Performance/compatibility depend on exact commit, quantization, driver, and model. | Keep as the cross-Windows local baseline. |
| 27 | [TensorRT-LLM installation guide](https://nvidia.github.io/TensorRT-LLM/latest/installation/installation-guide.html) | Current, accessed 2026-09-05 | Primary supported install paths center on Linux containers and Ubuntu-tested wheels. | Future native support may change. | Reject as the ordinary Windows default. |
| 28 | [Foundry Local architecture](https://learn.microsoft.com/en-us/azure/foundry-local/concepts/foundry-local-architecture) | Current, accessed 2026-09-05 | Provides native in-process model management, Rust SDK, ORT, and provider selection. | OS/model/provider support is narrower than llama.cpp. | Experimental Win11 lane. |
| 29 | [Windows ML execution providers](https://learn.microsoft.com/en-ie/windows/ai/new-windows-ml/supported-execution-providers) | Current, accessed 2026-09-05 | Dynamic EPs require Windows 11 24H2+; DirectML is the legacy compatibility option. | Conflicts with Win10 baseline. | Feature-gate and retain fallback. |
| 30 | [ONNX Runtime CUDA provider](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html) | Current, accessed 2026-09-05 | CUDA/cuDNN versions must match; arena memory limit can be below total GPU use. | Allocator/stream tuning changes safety assumptions. | Pin runtime stack and measure process/device totals. |
| 31 | [ONNX Runtime DirectML provider](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html) | Current, accessed 2026-09-05 | Broad DirectX 12 support comes with session concurrency/memory-pattern restrictions. | DirectML is a compatibility path, not automatic best performance. | Use as fallback with correct session options. |
| 32 | [Qwen3.5-4B official model card](https://huggingface.co/Qwen/Qwen3.5-4B) | Current, accessed 2026-09-05 | Current compact Qwen generation and runtime guidance. | Official framework examples do not qualify GGUF/llama.cpp packaging. | Benchmark candidate only. |
| 33 | [Moonshine official repository](https://github.com/moonshine-ai/moonshine) | Current, accessed 2026-09-05 | Current project supports local streaming STT on Windows and documents licensing distinctions. | Exact model/language artifacts have their own terms and performance. | Update stale catalog source and qualify exact artifacts. |
| 34 | [whisper.cpp official repository](https://github.com/ggml-org/whisper.cpp) | Current, accessed 2026-09-05 | Windows and multiple acceleration backends are supported. | Chunked demonstrations do not prove native incremental streaming quality. | Keep robust fallback with honest latency labels. |
| 35 | [sherpa-onnx official repository](https://github.com/k2-fsa/sherpa-onnx) | Current, accessed 2026-09-05 | Windows/Rust support spans VAD, streaming/non-streaming ASR, and TTS model families. | Each model needs separate license/provenance qualification. | Evaluate as a consolidated speech runtime. |
| 36 | [Chatterbox official repository](https://github.com/resemble-ai/chatterbox) | Current 2026 family, accessed 2026-09-05 | Nano/Turbo/Multilingual variants, sizes, streaming characteristics, and MIT code license are documented. | Vendor speed claim is CPU/workload-specific; voice prompts add rights/provenance concerns. | Replace vague catalog entry with exact Nano experiment. |
| 37 | [Kokoro-82M official model card](https://huggingface.co/hexgrad/Kokoro-82M) | Current, accessed 2026-09-05 | Small 82M Apache-2.0 TTS model. | Community ONNX conversions are separate artifacts. | First small stock-voice local candidate. |
| 38 | [kokoro-onnx community runtime](https://github.com/thewh1teagle/kokoro-onnx) | Current, accessed 2026-09-05 | Practical ONNX runtime and voice packaging exists. | It is not the model author's repository; conversion and hashes require audit. | Pin converter/runtime and ship verified ONNX/voice artifacts only. |
| 39 | [Qwen3-TTS official repository](https://github.com/QwenLM/Qwen3-TTS) | Current, accessed 2026-09-05 | Apache-2.0 family includes streaming-capable compact tiers. | Runtime size and GPU overlap must be measured with the game. | Optional quality tier after Kokoro. |
| 40 | [EmbeddingGemma official documentation](https://ai.google.dev/gemma/docs/embeddinggemma) | Current, accessed 2026-09-05 | 308M multilingual embeddings, 2K context, 128–768 dimensions, and quantized low-memory claims. | Access terms, tokenizer, conversion, and target-device performance remain to qualify. | Preferred small offline embedding experiment. |
| 41 | [Qwen3-Embedding-0.6B official model card](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B) | Current, accessed 2026-09-05 | Instruction-aware multilingual embeddings with Matryoshka dimensions. | Larger artifact/compute than EmbeddingGemma. | Second-tier offline comparison. |
| 42 | [SQLite FTS5](https://www.sqlite.org/fts5.html) | Current, accessed 2026-09-05 | External-content FTS indexes require content/index consistency; triggers are a supported pattern. | Application migrations/recovery still need tests. | Keep the current trigger-maintained FTS design. |
| 43 | [sqlite-vec official repository](https://github.com/asg017/sqlite-vec) | Pre-v1/current, accessed 2026-09-05 | Embedded SQLite vectors support Windows and compact vector types. | Project explicitly remains pre-v1. | Optional pinned accelerator with exact-scan baseline. |
| 44 | [LanceDB official repository](https://github.com/lancedb/lancedb) | Current, accessed 2026-09-05 | Embedded vector database supports very large scales. | Adds a new storage/runtime layer that current NPC scale does not justify. | Do not adopt absent measured need. |
| 45 | [Qdrant local quickstart](https://qdrant.tech/documentation/quickstart/) | Current, accessed 2026-09-05 | Windows-oriented local quickstart uses Docker. | Docker is an unacceptable ordinary desktop prerequisite here. | Do not adopt for the local review app. |
| 46 | [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) | Current, accessed 2026-09-05 | Job Objects group processes and support lifecycle/resource accounting. | Limits require careful nested-job and process-lifecycle handling. | Keep kill-on-close and add measured accounting. |
| 47 | [Windows Job Object CPU rate control](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information) | Current, accessed 2026-09-05 | Jobs can enforce CPU rate policies. | Hard caps can violate realtime speech deadlines. | Restrict to measured background work. |
| 48 | [Windows process power throttling / EcoQoS](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation) | Current, accessed 2026-09-05 | Process information APIs expose power-throttling controls. | Foreground realtime work should not be throttled. | Apply only to background validation/indexing/prefetch. |
| 49 | [DXGI video memory budgeting](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/query-video-memory-info) | Current, accessed 2026-09-05 | Applications should remain under current video-memory budget to avoid paging/stutter. | Budget is dynamic and per adapter/process context. | Use fresh budget telemetry in live admission. |
| 50 | [NVIDIA NVML API reference](https://docs.nvidia.com/deploy/nvml-api/) | Current, accessed 2026-09-05 | NVML exposes utilization, memory, thermals, power, clocks, and process accounting. | Driver/device support and process accounting can be incomplete. | Keep safe System32 loading and fail closed where evidence is missing. |

## Final keep/replace record

**Keep:** typed provider bridges; secrets-by-reference; signed production catalog; supervised hidden loopback workers; llama.cpp baseline; NVIDIA Nemotron-3 exact embedding pin; SQLite/FTS5; deterministic RRF; rebuildable embeddings; target-specific signed measurement envelopes; `ResourceGovernorV1`; the design of `ResourceBroker`; DXGI plus NVML telemetry; Job Object kill-on-close.

**Replace or complete:** v1 LangChain/Chroma/random-key/subprocess patterns; catalog-status-as-readiness; hard-coded provider models without probes; single-provider STT; simulated-only selected TTS; legacy-only Gemini/Groq protocols; single fixed AssemblyAI model choice without separately qualified revisions; vague local pack identities; mixed embedding generation scans; unwired runtime broker; any NVIDIA “unlimited” or production-ready wording; local Magpie on the target laptop.

**Research candidates, not defaults:** Foundry Local/Windows ML, Qwen3.5-4B, Moonshine, sherpa-onnx, Kokoro, Chatterbox Nano, Qwen3-TTS, EmbeddingGemma, sqlite-vec. Each still needs exact artifact/license identity and repository-defined headless qualification.
