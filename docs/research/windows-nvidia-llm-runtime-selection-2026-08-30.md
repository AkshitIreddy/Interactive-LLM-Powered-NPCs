# Windows/NVIDIA local LLM runtime selection (2026-08-30)

## Decision

The current qualified `llama.cpp` **Vulkan** runtime is a sound, portable
fallback, but it is not yet demonstrated to be the best runtime for the actual
target machine: Windows x64 with an NVIDIA GeForce RTX 4080 Laptop GPU (12 GB,
Ada, compute capability 8.9). The first missing experiment is not a wholesale
runtime replacement. It is an exact-revision **official CUDA 12.4 versus
official Vulkan** test of the existing `llama.cpp` stack and the existing GGUF.

Recommended order:

1. **Production candidate: the official `llama.cpp` b10689 CUDA 12.4 Windows
   release.** Compare the official CUDA executable archive plus its separate
   official CUDA 12.4 runtime-DLL archive with the already-qualified official
   b10689 Vulkan archive. This preserves the exact upstream source revision and
   exact GGUF without installing a new toolchain. Preserve the existing worker,
   API, security, cancellation, and process-lifecycle code, and make CUDA the
   NVIDIA preference only if the protocol below passes. Keep the current
   b10689 Vulkan and CPU variants as fallbacks.
2. **Format-changing performance challenger: ExLlamaV3 v1.4.4.** Its active
   Windows CUDA path and EXL3 quantization are credible speed/VRAM challengers.
   Benchmark it only in phase two because it cannot consume the exact GGUF.
   TabbyAPI provides a convenient OpenAI-compatible harness, but its AGPL-3.0
   license and explicit hobby/rolling-release posture make it a benchmark tool,
   not an automatic product dependency. A product integration would need legal
   review or a small first-party adapter around the MIT ExLlamaV3 library.
3. **Native embedding R&D candidate: ONNX Runtime GenAI v0.15.2, CUDA INT4,
   optionally the TensorRT-RTX execution provider.** This has the most
   interesting Windows-native C/C++/C# and game-coexistence story, but GenAI is
   still labeled Preview, needs a separately converted ONNX artifact, and no
   credible public measurement was found proving a win for this exact
   Qwen3-4B/RTX-4080-Laptop workload. It is not the first shipping choice.

A b10689 source build against CUDA Toolkit 12.8 remains a later experiment, not
the executable plan. The host currently has only `nvcc` 12.1. This task does not
authorize silently installing CUDA 12.8 or changing the machine toolchain.

LM Studio's **CUDA 12** runtime should be used as an end-to-end comparison and
developer control. It should not be embedded or redistributed as the product
runtime. Ollama is a useful usability control but is mostly another managed
`llama.cpp` distribution, not evidence of a stronger inference core. Native
Windows TensorRT-LLM is a stale path; NVIDIA deprecated Windows support in
0.18.0. vLLM is not native-Windows software. MLC is viable but does not offer a
demonstrated advantage sufficient to justify another converted artifact and
compiler pipeline for this 4B/single-user workload.

This conclusion follows a state-of-the-art-first review of more than 50
official or primary sources. It does **not** rely on running or downloading any
model while the shared GPU lock belongs to STT.

## Qualified baseline that must be preserved

The exact model is:

- `Qwen3-4B-Instruct-2507-Q4_K_M.gguf`
- 2,497,281,120 bytes
- SHA-256 `3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597`
- upstream model revision
  `Qwen/Qwen3-4B-Instruct-2507@cdbee75f17c01a7cc42f958dc650907174af0554`
- GGUF conversion revision
  `unsloth/Qwen3-4B-Instruct-2507-GGUF@a06e946bb6b655725eafa393f4a9745d460374c9`

The exact runtime is `llama.cpp` b10689, commit
`57291f2644af8c9df0dd8d44395881c5bdcf0ecd`, with runtime ABI
`npc-llama-server-openai-v1+b10689+qwen3`.

One bounded Vulkan review run on the RTX 4080 Laptop reported:

| Measure | Existing single-run result |
|---|---:|
| Cold load | 17,471.928 ms |
| Reload | 8,572.354 ms |
| TTFT | 7,799.840 ms |
| Output throughput | 18.4712 token/s for 64 tokens |
| Inter-token p50 / p95 / p99 | 55.754 / 80.397 / 262.554 ms |
| GPU memory before / loaded / after unload | 438 / 4,061 / 437 MiB |
| Measured resident GPU delta | 3,623 MiB |
| Working set / peak working set / private bytes | 2.698 / 3.699 / 4.050 GB |
| Structured output, cancellation, runtime reuse, unload/reload | Passed |

That evidence is deliberately marked non-admissible: it is unsigned and only
one sample. It is a useful baseline, not a production p99 resource envelope.

## What the LM Studio screenshot actually shows

The screenshot is evidence about available choices, not an instruction. Its
runtime rows map as follows, based on the installed LM Studio package manifests
and binaries on this machine:

| Screenshot row | Actual role | Local package finding | Relevance |
|---|---|---|---|
| CPU `llama.cpp` Windows v2.31.2 | LM Studio engine package using CPU kernels | Engine metadata maps LM Studio engine `2.31.2` to upstream `llama.cpp` release **b10662**, commit `18443257a` | CPU fallback/control, not faster on this NVIDIA target |
| CUDA 12 `llama.cpp` Windows v2.31.2 | NVIDIA backend built with the CUDA 12 family; LM Studio describes it as CUDA 12.8 accelerated | CUDA-12 vendor package contains `cudart64_12.dll`, `cublas64_12.dll`, and `cublasLt64_12.dll`; manifest includes sm 8.9 | The correct LM Studio row to test on the RTX 4080 |
| CUDA `llama.cpp` Windows v2.31.2 | Legacy NVIDIA compatibility package | Vendor package contains CUDA 11-family `cudart64_110.dll`, `cublas64_11.dll`, and `cublasLt64_11.dll` | Not the preferred row for Ada unless CUDA 12 is incompatible |
| Vulkan `llama.cpp` Windows v2.31.2 | Cross-vendor Vulkan backend | Same LM Studio engine line, different accelerator bundle | Useful LM Studio-internal control and non-NVIDIA fallback |
| Harmony renderer v0.3.6 | Message/content renderer | Not a model inference backend | Irrelevant to token throughput or VRAM |

Important version boundary: **LM Studio engine `2.31.2` is not upstream
`llama.cpp` b10689.** Its installed display metadata names b10662/`18443257a`,
27 upstream build tags behind this project's b10689. An LM Studio CUDA-12 versus
the product's Vulkan run therefore changes both wrapper and engine revision. It
is useful as an end-to-end product comparison, but it is not the fair backend
A/B.

The relevant inspected metadata lives under:

- `%USERPROFILE%\.lmstudio\extensions\backends\llama.cpp-win-x86_64-nvidia-cuda12-avx2-2.31.2\display-data.json`
- the corresponding `backend-manifest.json`
- LM Studio's installed CUDA and CUDA-12 vendor package directories

The CUDA-12 manifest lists code targets including 7.5, 8.0, **8.9**, 9.0,
10.0, and 12.0. The target GPU is sm 8.9. The manifest's
`minimum_driver_version: "12040"` is LM Studio's own compatibility encoding;
the benchmark must still record the actual NVIDIA driver and validate startup
rather than trying to infer a Windows driver marketing number from that field.

## Runtime comparison for this product

`Yes*` means the behavior exists through a wrapper or object/session teardown
rather than as a documented first-class server operation. `Test` means the
claim is insufficiently specified upstream and must be included in qualification.

| Runtime | Native Windows + RTX 4080 | Exact current GGUF | Stream / structured | Cancel / hot unload | VRAM and game coexistence | License / redistribution | Packaging and verdict |
|---|---|---|---|---|---|---|---|
| **Official llama.cpp b10689 CUDA 12.4** | Yes; CUDA is NVIDIA-native, RTX 4080 is sm 8.9 | **Yes** | SSE/OpenAI-style streaming; JSON and JSON Schema response formats | Yes* through the already-qualified worker's request cancellation, child termination, and Windows Job Object; repeat the tests | Full or partial GPU offload; measure CUDA workspace and reserve modes. Existing Vulkan delta is 3,623 MiB | llama.cpp MIT; CUDA redistributable DLLs remain subject to NVIDIA's EULA and notice list | Smallest change and only immediate no-model-confound candidate. **First choice** |
| **Direct llama.cpp b10689 Vulkan** | Yes | **Yes** | Same server contract | Already passed once through product wrapper | Cross-vendor and easier fallback; existing full-offload review leaves about 8.2 GiB of the 12 GiB free before game load | MIT plus third-party notices | Already qualified once; keep as fallback and control |
| **LM Studio CUDA 12 engine 2.31.2** | Yes; correct LM Studio row for this GPU | Yes | Streaming APIs; structured output; explicit load/unload and idle TTL | Abort behavior needs an adversarial test; unload/TTL documented | Managed load/eviction and GPU controls; still test game contention | LM Studio application terms allow personal/internal business use but prohibit distributing/sublicensing the Software. `lms` being MIT applies to the CLI, not the proprietary app/runtime bundle | Excellent developer and end-to-end benchmark control; **do not ship as the product runtime** |
| **Ollama Windows** | Yes; native Windows CUDA, Vulkan optional/experimental | Can import GGUF, although wrapper/runtime configuration differs | Streaming by default; JSON/schema structured output | `keep_alive: 0` unloads immediately; no first-class per-request cancellation contract found in current API docs | Scheduler estimates VRAM; parallelism and context multiply memory. Explicitly verify CUDA on hybrid laptops | MIT; standalone Windows ZIP is documented for embedding/service use | Easy distribution, but another managed llama-family stack rather than a proven faster core. End-to-end control only |
| **ExLlamaV3 v1.4.4 + TabbyAPI** | Yes; Windows cu128 wheels, CUDA 12.4+; RTX 4080 supported | **No**, needs EXL3 quant | Tabby provides OpenAI API, streaming, JSON Schema/regex/EBNF, batching | Model unload endpoint terminates pending generations; a 2026 disconnect/hang regression means cancel must be stress-tested | EXL3 and low-bit KV cache can reduce residency; designed for GPU residency, so game reserve must be tested | ExLlamaV3 MIT; TabbyAPI **AGPL-3.0** and calls itself a hobby project with rolling releases | Plausible speed/VRAM challenger but Python/PyTorch/CUDA wheels plus a second model artifact. Phase two only |
| **ONNX Runtime GenAI v0.15.2 CUDA** | Yes; Windows x64 CUDA packages | **No**, needs ONNX INT4 conversion | Token-by-token API and constrained/structured decoding | Generator/session destruction and `terminate_session`; product must provide server/lifecycle wrapper | CUDA EP exposes a GPU memory limit. It may be more controllable than a server, but actual peak and allocator behavior need measurement | MIT | Attractive native-library R&D option; Preview API and conversion/qualification cost make it third |
| **ONNX Runtime GenAI + TensorRT-RTX EP** | Yes; RTX LLM path requires compute capability 8.6+, so Ada 8.9 qualifies | No | GenAI layer supplies generation/constraints | GenAI/session wrapper owns lifecycle | NVIDIA explicitly documents simultaneous compute/graphics; kernels using no more than 48 KiB shared memory are relevant on Ampere/Ada. This is the most game-aware research route | ORT GenAI MIT; TensorRT-RTX has a proprietary SDK agreement and restricted redistributable portions/conditions | Potentially strong for an embedded game runtime, but new CUDA/driver prerequisites, engine build/cache, license review, and no exact performance proof |
| **DirectML / Windows ML path** | Native Windows, cross-vendor; usable on NVIDIA | No | Through ORT GenAI/WinML | Session lifecycle | Cross-vendor, but CUDA is the more direct NVIDIA path | ORT MIT; Windows component terms apply | DirectML EP is in sustained engineering; prefer CUDA or current Windows ML/TRT-RTX for a new NVIDIA-first path |
| **MLC LLM** | Native Windows CUDA and Vulkan wheels | **No**, needs MLC weight conversion/compile | OpenAI-compatible REST streaming and JSON-schema response format | No sufficiently clear unload/cancel contract found for this product; test/build it | Compile-time targets and quantization can be efficient; no exact 4080-Laptop/Qwen3 evidence found | Apache-2.0 repository; dependency notices still required | Technically viable, but another compiler/runtime/artifact pipeline without a demonstrated win. Not shortlisted |
| **vLLM under WSL2** | WSL/Linux only; native Windows unsupported | No practical exact-product path | Excellent OpenAI streaming, structured outputs, cancellation/sleep-wake features | Yes in server APIs | Tunable GPU memory utilization, but designed for throughput/concurrency and commonly reserves a large GPU fraction | Apache-2.0 | Strong data-center server, high operational/package overhead for a single Windows game and 4B model. Reject now |
| **TensorRT-LLM** | Current native Windows path is deprecated; WSL/Linux only direction | No | Rich server capabilities | Rich executor/server lifecycle | High-throughput server focus | Apache-2.0 code plus NVIDIA dependencies | Do not revive stale Windows 0.6.x/0.17 tutorials. Windows was deprecated in 0.18 and current releases have major backend changes |

## Support, driver, and architecture conclusions

- NVIDIA lists GeForce RTX 4080 as compute capability **8.9**. `llama.cpp`'s
  CUDA build supports selecting this architecture; LM Studio's local CUDA-12
  manifest includes it explicitly.
- CUDA minor-version compatibility documentation gives the CUDA 12.x driver
  family floor, but packaged applications can impose higher requirements. The
  actual driver version, laptop power mode, TGP, thermals, and clocks must be
  part of each benchmark record.
- The official upstream b10689 Windows CUDA-12 release separates
  `llama-b10689-bin-win-cuda-12.4-x64.zip` from
  `cudart-llama-bin-win-cuda-12.4-x64.zip`. Both release assets must be pinned
  independently by byte length and SHA-256. This is the executable production
  A/B. It is not a CUDA-12.8 build and must not be labeled as one.
- The host's installed compiler is `nvcc` 12.1. A b10689 CUDA-12.8 source build
  therefore requires a new Toolkit installation and remains future work subject
  to explicit authorization. LM Studio's self-contained CUDA-12.8 package does
  not change the host compiler and does not make its b10662 engine a fair b10689
  backend comparison.
- Ollama currently documents NVIDIA compute capability 5.0+ and driver 531+ in
  its GPU guide. Its older Windows page lists a lower legacy driver floor; use
  the stricter current GPU requirement and current NVIDIA driver. A recent
  upstream issue on a 4080 Laptop reports a hybrid-GPU machine selecting Intel
  Vulkan and running much slower, so every harness must log the selected device
  and backend rather than trust the word “GPU.”
- ExLlamaV3 currently calls for CUDA 12.4+ and publishes Windows CUDA-12.8 wheels.
- ONNX Runtime release families bind CUDA and cuDNN versions. Current ORT CUDA
  packages must be pinned together with their documented DLL dependencies.
- TensorRT-RTX's current prerequisites are substantially newer and more
  restrictive than plain llama.cpp CUDA (current docs specify CUDA 12.9 Update
  1 or CUDA 13.4 combinations). Treat it as a separate deployment target, not a
  drop-in execution provider toggle.

## Redistribution boundary

### Safe starting point, subject to the existing notice audit

- Qwen3-4B-Instruct-2507 model: Apache-2.0.
- Pinned GGUF conversion: derived from that Apache-2.0 model, with conversion
  provenance retained.
- llama.cpp: MIT, with the release's third-party notices retained.
- ExLlamaV3 library: MIT.
- ONNX Runtime GenAI: MIT.
- Ollama: MIT.
- MLC LLM and vLLM: Apache-2.0.

### Requires a separate boundary or legal review

- **LM Studio:** its app terms prohibit distributing, sublicensing, or
  transferring the Software. Installing it locally to benchmark is not
  authorization to put its runtime directory in this game's installer. The MIT
  `lms` CLI license does not relicense LM Studio or its backend packages.
- **TabbyAPI:** AGPL-3.0. Network/server integration and distribution require an
  explicit compliance decision. Do not casually copy it into a closed-source
  desktop product.
- **CUDA DLLs:** NVIDIA lists eligible redistributable components under the CUDA
  EULA; ship only the necessary allowed DLLs, include required notices, and pin
  their exact bytes. Do not treat the whole CUDA Toolkit as redistributable.
- **TensorRT-RTX:** NVIDIA's SDK agreement permits only defined redistributable
  portions and imposes incorporation/access/notice conditions. Legal and
  release engineering must approve the packaging design.

## Public performance evidence and its limits

No current primary source was found that tests all of these simultaneously:
Qwen3-4B-Instruct-2507 Q4_K_M, the same prompt and context, Windows, an RTX 4080
Laptop, and matched current CUDA/Vulkan engine commits. Therefore this report
does not assert a CUDA speedup number.

There is directional evidence only:

- An upstream llama.cpp issue measured CUDA roughly 20–30% ahead of Vulkan on
  an NVIDIA A100 in that reporter's matched setup. That is evidence that backend
  choice can matter, not a transferable RTX 4080 Laptop result.
- The llama.cpp CUDA and Vulkan scoreboard discussions contain strong desktop
  RTX 4080-class results, but they mix models, quantizations, builds, prompts,
  and GPUs. They are useful smoke-test ranges, not a selection result.
- A Qwen3 4B Q4_K Vulkan report showed `llama-bench` and server results can
  diverge dramatically. The product's streaming server path—not only
  `llama-bench`—must decide promotion.
- ExLlamaV3, ORT GenAI/TensorRT-RTX, and MLC all have architectural reasons to
  be competitive, but none supplied sufficiently comparable evidence for this
  exact workload. Any cross-format result also confounds the runtime with a new
  quantization.

## Exact fair official CUDA 12.4 versus Vulkan protocol

### Test arms and pins

Use two primary arms from the same signed upstream b10689 release so source
revision, release process, model, and server surface remain fixed:

| Arm | Source and build | Purpose |
|---|---|---|
| **A: qualified Vulkan baseline** | Existing pinned official b10689 Vulkan archive, SHA-256 `3da600ff52a746d82e32a2ba3f0382e3bf782bd3a8fece661b646e1dee7ac1c6` | Reproduce the already-recorded behavior |
| **B: official CUDA 12.4 candidate** | Official b10689 `llama-b10689-bin-win-cuda-12.4-x64.zip` plus separate `cudart-llama-bin-win-cuda-12.4-x64.zip`; pin each archive's exact byte length and SHA-256 in the runtime trust record | Immediate NVIDIA production candidate without a new toolkit install |

Pin and record the b10689 release/commit, release attestations, both archive
URLs, byte lengths and SHA-256 values, every extracted EXE/DLL SHA-256 and size,
Vulkan loader version, NVIDIA driver, Windows build, and that the official CUDA
DLLs are app-local. Keep the backends in their separate official archives for
the isolation run. Clearly label B **CUDA 12.4**, never 12.8.

LM Studio CUDA-12 engine 2.31.2/b10662 is arm **C**, an end-to-end convenience
comparison. Its bundled CUDA-12.8 family does not require modifying the host
toolkit, but it never substitutes for A versus B because its engine revision,
wrapper, defaults, and redistribution terms differ.

### Invariants

All arms must use:

- the exact GGUF path and SHA-256 above, with no conversion or repack;
- the same Jinja/chat template, system/user messages, stop tokens, and reasoning
  configuration;
- the same deterministic seed and greedy decoding (`temperature=0`) for the
  primary performance cells; separately repeat one production sampling preset;
- the same input token count after tokenization and the same requested output
  token count; reject a cell if tokenization differs;
- identical context size, batch, micro-batch, KV-cache data types, Flash
  Attention setting, thread counts, `parallel=1`, GPU layer count, cache/reuse
  settings, mmap/mlock policy, and speculative decoding disabled;
- loopback-only server, identical product worker, SSE parser, API-key handling,
  structured schema, timeout, cancellation, and unload path;
- AC power, Windows performance mode, discrete NVIDIA adapter, fixed laptop fan
  profile, stable ambient conditions, and no other GPU compute. Record GPU TGP,
  clocks, temperature, p-state, power, utilization, and throttling reason. Lock
  clocks/power only if supported and use the identical setting in every arm;
- an explicit 10-minute cool-down/idle condition or temperature band before
  each block. Alternate order with ABBA/BAAB blocks, not all Vulkan then all
  CUDA.

### Workload cells

1. **Cold lifecycle:** start process, load model, readiness probe, first
   structured request, unload/kill, verify port and process tree are gone.
2. **Warm prefill/decode:** input lengths 128, 512, 2,048, and 8,192 tokens;
   request exactly 256 output tokens. Include the product's actual 32K context
   ceiling only if it is a supported product scenario; do not benchmark the
   model card's 262K maximum merely because it exists.
3. **NPC dialogue:** fixed ten-turn transcript, alternating short and long turns,
   with 500–1,500 input tokens at request time and 64 output tokens. Exercise
   the actual SSE-to-sentence-release path.
4. **Structured dialogue:** the exact checked-in schema and fixture. Record
   byte-level/digest result, JSON parse, schema validity, stop reason, and token
   count.
5. **Cancellation:** cancel after the 16th streamed token and separately at 250
   ms, 100 times. Require no late tokens, slot/cache recovery, no worker leak,
   and a succeeding request within the SLO.
6. **Lifecycle stress:** 20 load/unload/reload cycles. Measure load/reload p50,
   p95, and p99; verify the process tree exits and dedicated VRAM returns to
   within 64 MiB of that block's pre-load baseline within 10 seconds.
7. **Concurrency boundary:** `parallel=1` is the decision cell. A small
   `parallel=2` product stress cell may be reported separately, never averaged
   into single-dialogue latency.

Use at least two warmups, then **20 measured repetitions per cell per arm** so
the existing resource-envelope minimum is met. Preserve raw per-token timestamps
and raw telemetry, not only aggregates.

### Measurements

Report per arm and cell:

- cold start, model load, reload, readiness, TTFT p50/p95/p99;
- prompt-processing token/s and output token/s p50/p95/p99;
- inter-token gap p50/p95/p99/max and total request latency;
- structured-output validity and exact self-test digest;
- cancellation acknowledgement, last-token-to-stop latency, next-request
  recovery, outstanding slots, process/thread/handle deltas;
- process working set/private/commit and system RAM;
- NVIDIA dedicated VRAM before, loaded-idle, peak-prefill, peak-decode, and
  after unload; log Windows shared GPU memory separately;
- GPU utilization, power, temperature, clocks, p-state, and throttle reason;
- runtime crashes, driver resets, errors, and invalid outputs.

### Game-coexistence matrix

For A and B, repeat the product dialogue workload under:

| Game state | LLM state | Offload mode |
|---|---|---|
| Fixed representative scene, LLM absent | None | Baseline |
| Same scene | Model loaded and idle | Full offload |
| Same scene | Active streaming generation | Full offload |
| Same scene | Loaded and idle | Reserve mode |
| Same scene | Active streaming generation | Reserve mode |

Reserve mode must select GPU layers/context/KV settings so available dedicated
VRAM is at least:

`measured game peak + configured game reserve + 1.5–2.0 GiB safety margin`

Measure game median FPS, 1% low, frame-time p95/p99/max, render-thread stalls,
game VRAM, LLM TTFT/decode rate, and shared-memory spill. Reject a configuration
that starts material WDDM shared-memory paging or causes a visible hitch even if
LLM token/s is high. The actual product scene and deterministic camera path must
be identical. Run at least five 5-minute blocks per state with alternating
backend order.

### Promotion gates

Promote the official b10689 CUDA 12.4 runtime only if all are true:

1. Same model hash, tokenization, schema validity, self-test digest, and stop
   behavior as qualified Vulkan.
2. At least **10% median output-token/s improvement or 15% median TTFT
   improvement** in the product NPC workload, with no worse TTFT p95 or
   inter-token p99 beyond a 5% tolerance.
3. Cancellation, next-request recovery, structured output, process cleanup, and
   all 20 unload/reload cycles pass.
4. Loaded, peak, and post-unload VRAM form a valid signed resource envelope;
   post-unload returns within 64 MiB of baseline.
5. Game-active frame-time p95 and 1% low stay within the product SLO in reserve
   mode, with no material shared-memory spill or driver reset.
6. CUDA runtime/DLL redistribution, notices, byte pins, repair flow, and driver
   compatibility are approved.

If CUDA is within 10% of Vulkan and does not materially improve TTFT, keep
Vulkan as the default to avoid an extra NVIDIA-specific trust unit. If CUDA
wins, prefer it only after NVIDIA detection and retain Vulkan/CPU fallback. Do
not auto-fall back silently after a runtime failure; surface the degraded mode
through the existing product policy.

## Phase-two cross-format protocol

Only after the no-confound CUDA decision, compare:

- ExLlamaV3 v1.4.4 with a pinned EXL3 4.0-bpw conversion from the exact base
  model revision;
- ONNX Runtime GenAI v0.15.2 CUDA with a pinned INT4 conversion from that same
  revision, then optionally the exact TensorRT-RTX EP version.

Use the same semantic prompts, requested tokens, product API scenarios,
telemetry, cancellation, unload, and game matrix. These rows **cannot** be
called a pure runtime A/B because quantization and container changed. Add a
separate output-quality gate over the checked-in dialogue suite and record the
conversion recipe, calibration data, model bytes, hashes, and licenses. LM
Studio and Ollama may join as end-to-end controls with the exact GGUF, but their
engine revisions and hidden/default arguments must be logged and treated as
confounders.

## Primary and official source ledger

All dated conclusions above were checked on 2026-08-30. Mutable `main`, `latest`,
and product-doc pages should be re-pinned or snapshotted before a release audit.

### LM Studio

1. [LM Studio system requirements](https://lmstudio.ai/docs/app/system-requirements) — Windows x64, AVX2, RAM/VRAM guidance.
2. [LM Studio application overview](https://lmstudio.ai/docs/app) — GGUF/llama.cpp runtime management.
3. [LM Studio changelog](https://lmstudio.ai/changelog/lmstudio) — current app/runtime behavior; mutable.
4. [LM Studio headless deployment](https://lmstudio.ai/docs/developer/core/headless) — `llmster`, JIT loading, and auto-unload.
5. [LM Studio 0.3.15 CUDA 12.8 announcement](https://lmstudio.ai/blog/lmstudio-v0.3.15) — origin and scope of the CUDA 12.8 engine line.
6. [LM Studio runtime CLI](https://github.com/lmstudio-ai/docs/blob/main/3_cli/4_runtime/runtime.md) — runtime list/select/update operations; mutable `main`.
7. [LM Studio 0.4.0 architecture](https://lmstudio.ai/blog/0.4.0) — engine 2.0 and continuous batching.
8. [LM Studio model loading/unloading](https://lmstudio.ai/docs/typescript/manage-models/loading) — explicit load and unload lifecycle.
9. [LM Studio REST API](https://lmstudio.ai/docs/developer/rest) — streaming and model operations.
10. [LM Studio TTL and auto-evict](https://lmstudio.ai/docs/developer/core/ttl-and-auto-evict) — idle unloading.
11. [LM Studio application terms](https://lmstudio.ai/app-terms) — internal-use and redistribution/sublicensing boundary.
12. [`lms` CLI license](https://github.com/lmstudio-ai/lms/blob/main/LICENSE) — MIT applies to CLI code, not the whole app/runtime.

### llama.cpp, CUDA, and the GPU

13. [llama.cpp b10689 release](https://github.com/ggml-org/llama.cpp/releases/tag/b10689) — exact signed commit and Windows CPU, CUDA-12/CUDA-13, Vulkan assets; CUDA-12.4 DLL label.
14. [Pinned b10689 build guide](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/docs/build.md) — `GGML_CUDA`, architecture selection, and Vulkan builds.
15. [llama.cpp server API](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) — streaming and JSON/JSON-Schema response formats; mutable `master`.
16. [Pinned llama.cpp license](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/LICENSE) — MIT and third-party notice boundary.
17. [Upstream CUDA versus Vulkan A100 report](https://github.com/ggml-org/llama.cpp/issues/17273) — directional matched-backend evidence, not transferable to this laptop.
18. [llama.cpp CUDA performance scoreboard](https://github.com/ggml-org/llama.cpp/discussions/15013) — community primary measurements and methodology limits.
19. [llama.cpp Vulkan performance scoreboard](https://github.com/ggml-org/llama.cpp/discussions/10879) — community primary measurements and methodology limits.
20. [Qwen3 4B Q4_K Vulkan report](https://github.com/ggml-org/llama.cpp/discussions/25768) — illustrates `llama-bench`/server divergence.
21. [NVIDIA CUDA GPU compute-capability table](https://developer.nvidia.com/cuda/gpus) — RTX 4080 Ada capability 8.9.
22. [NVIDIA CUDA minor-version compatibility](https://docs.nvidia.com/deploy/cuda-compatibility/minor-version-compatibility.html) — CUDA 12.x driver-family compatibility rules.
23. [NVIDIA CUDA Installation Guide for Windows](https://docs.nvidia.com/cuda/cuda-installation-guide-microsoft-windows/) — supported toolchain/Windows build requirements.
24. [NVIDIA CUDA 12.4 EULA](https://docs.nvidia.com/cuda/archive/12.4.1/pdf/EULA.pdf) — redistributable component conditions for the immediate official b10689 CUDA dependency.

### Ollama

25. [Ollama for Windows](https://docs.ollama.com/windows) — native Windows, standalone ZIP, service/embedding notes.
26. [Ollama GPU support](https://docs.ollama.com/gpu) — NVIDIA capability/driver floor, device selection, experimental Vulkan.
27. [Ollama generate API](https://docs.ollama.com/api/generate) — streaming, schema/JSON format, and `keep_alive` unload.
28. [Ollama FAQ](https://docs.ollama.com/faq) — context, parallelism, VRAM scheduling, Flash Attention, KV quantization.
29. [Ollama license](https://github.com/ollama/ollama/blob/main/LICENSE) — MIT; mutable `main`.
30. [Ollama release workflow](https://github.com/ollama/ollama/blob/main/.github/workflows/release.yaml) — current Windows CUDA-12.8/CUDA-13/Vulkan packaging; mutable `main`.
31. [Ollama RTX 4080 Laptop hybrid-GPU report](https://github.com/ollama/ollama/issues/16667) — recent primary failure evidence; do not assume it remains unfixed.

### ExLlama and TabbyAPI

32. [Archived ExLlamaV2 repository](https://github.com/turboderp-org/exllamav2) — explicitly superseded; stale as a new choice.
33. [ExLlamaV3 repository](https://github.com/turboderp-org/exllamav3) — Windows/CUDA requirements, Qwen3 support, EXL3 and KV-cache capabilities.
34. [ExLlamaV3 v1.4.4 release](https://github.com/turboderp-org/exllamav3/releases/tag/v1.4.4) — exact current candidate and Windows cu128 wheel.
35. [ExLlamaV3 license](https://github.com/turboderp-org/exllamav3/blob/master/LICENSE) — MIT.
36. [TabbyAPI repository](https://github.com/theroyallab/tabbyAPI) — V3-only main line, features, hobby/rolling-release warning.
37. [TabbyAPI usage and model lifecycle](https://github.com/theroyallab/tabbyAPI/wiki/03.-Usage) — streaming and unload behavior.
38. [TabbyAPI Windows installation](https://github.com/theroyallab/tabbyAPI/wiki/01.-Getting-Started) — Python/PyTorch/CUDA package footprint.
39. [TabbyAPI cancellation/disconnect regression](https://github.com/theroyallab/tabbyAPI/issues/428) — historical primary failure report; closed, so use it to design a test, not claim a current defect.
40. [TabbyAPI license](https://github.com/theroyallab/tabbyAPI/blob/main/LICENSE) — AGPL-3.0.

### ONNX Runtime GenAI and TensorRT-RTX

41. [ONNX Runtime GenAI repository](https://github.com/microsoft/onnxruntime-genai) — Preview status, Windows/CUDA/DirectML/TensorRT-RTX, structured decoding, language bindings.
42. [ONNX Runtime GenAI installation](https://onnxruntime.ai/docs/genai/howto/install.html) — Windows packages and execution providers.
43. [ONNX Runtime CUDA execution provider](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html) — CUDA/cuDNN compatibility and GPU memory limit.
44. [ONNX Runtime GenAI Qwen3 builder](https://github.com/microsoft/onnxruntime-genai/blob/main/src/python/py/models/builder.py) — Qwen3 architecture and INT4 provider paths; mutable `main`.
45. [ONNX Runtime GenAI runtime options](https://github.com/microsoft/onnxruntime-genai/blob/main/docs/RuntimeOptions.md) — session termination; mutable `main`.
46. [ONNX Runtime GenAI C API](https://onnxruntime.ai/docs/genai/api/c.html) — token-by-token generator and destruction lifecycle.
47. [ONNX Runtime GenAI v0.15.2](https://github.com/microsoft/onnxruntime-genai/releases/tag/v0.15.2) — exact experimental candidate release.
48. [ONNX Runtime GenAI license](https://github.com/microsoft/onnxruntime-genai/blob/main/LICENSE) — MIT.
49. [DirectML execution provider](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html) — current DirectML status and constraints.
50. [TensorRT-RTX architecture overview](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/architecture/architecture-overview.html) — LLM path and compute-capability floor.
51. [TensorRT-RTX prerequisites](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/installing-tensorrt-rtx/prerequisites.html) — current CUDA/driver/tool requirements.
52. [TensorRT-RTX simultaneous compute and graphics](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/inference-library/compute-graphics.html) — documented game-coexistence design constraints.
53. [TensorRT-RTX SDK agreement](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/reference/sla.html) — redistributable portions and incorporation/access conditions.
54. [TensorRT-RTX portable engines](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/inference-library/cpu-engines.html) — AOT/JIT cache and Ampere+ portability.

### Ruled-out or deferred runtimes

55. [TensorRT-LLM release notes](https://github.com/NVIDIA/TensorRT-LLM/blob/main/docs/source/release-notes.md) — Windows deprecated at v0.18.0; current 1.2 backend changes make old Windows instructions stale.
56. [TensorRT-LLM Qwen example](https://github.com/NVIDIA/TensorRT-LLM/blob/main/examples/models/core/qwen/README.md) — current Qwen3 support is not evidence of native Windows support.
57. [TensorRT-LLM quantization guide](https://github.com/NVIDIA/TensorRT-LLM/blob/main/docs/source/features/quantization.md) — Qwen quantization/hardware matrix; mutable `main`.
58. [TensorRT-LLM license](https://github.com/NVIDIA/TensorRT-LLM/blob/main/LICENSE) — Apache-2.0 repository plus dependency boundary.
59. [vLLM GPU installation](https://docs.vllm.ai/en/latest/getting_started/installation/gpu/) — Linux requirements and native-Windows limitation.
60. [vLLM OpenAI-compatible server](https://docs.vllm.ai/en/latest/serving/openai_compatible_server.html) — streaming and server surface.
61. [vLLM structured outputs](https://docs.vllm.ai/en/latest/features/structured_outputs/) — JSON/regex/grammar constraints.
62. [vLLM license](https://github.com/vllm-project/vllm/blob/main/LICENSE) — Apache-2.0.
63. [MLC LLM Windows installation](https://llm.mlc.ai/docs/install/mlc_llm.html) — Windows CUDA/Vulkan wheels.
64. [MLC GPU backend guide](https://llm.mlc.ai/docs/install/gpu.html) — CUDA recommended for NVIDIA and Vulkan caveats.
65. [MLC model compilation](https://llm.mlc.ai/docs/compilation/compile_models.html) — converted weights and Windows DLL build pipeline.
66. [MLC OpenAI-compatible REST API](https://llm.mlc.ai/docs/deploy/rest.html) — streaming and JSON-schema response format.
67. [MLC LLM license](https://github.com/mlc-ai/mlc-llm/blob/main/LICENSE) — Apache-2.0.

## Stale or unsupported claims to avoid

- “LM Studio v2.31.2 is newer than llama.cpp b10689.” They are different
  version namespaces; installed metadata maps it to b10662.
- “LM Studio generic CUDA is the newest NVIDIA path.” On this install it is the
  CUDA-11-family package; choose CUDA 12 for Ada.
- “The official llama.cpp b10689 CUDA 12 asset is CUDA 12.8.” Its release page
  labels the companion DLLs CUDA 12.4.
- “Harmony renderer accelerates generation.” It renders message content.
- “TensorRT-LLM is a supported native-Windows production choice.” Windows was
  deprecated in 0.18.0; old Windows examples are stale.
- “ExLlamaV2 is the current ExLlama runtime.” Its repository is archived;
  development moved to V3.
- “vLLM has native Windows wheels.” Official GPU installation remains Linux;
  WSL/community ports are not native support.
- “DirectML is automatically faster because it is Windows-native.” It is a
  cross-vendor path and is in sustained engineering; NVIDIA CUDA must be the
  performance control.
- “LM Studio/Ollama/ExLlama public numbers prove a win here.” No sufficiently
  matched public benchmark exists. Only the protocol above can decide.
- “A faster token/s number alone makes a better game runtime.” Cancellation,
  frame-time impact, peak/residual VRAM, structured validity, unload, packaging,
  and redistribution all remain promotion gates.
