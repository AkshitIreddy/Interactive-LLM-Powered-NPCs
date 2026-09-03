# Local LLM evidence ledger

Checked 2026-08-30. This ledger records the sources read before implementation,
the material claim from each, and the resulting decision. It deliberately favors
primary model cards, pinned source, specifications, and platform documentation.

| # | Source | Material evidence | Decision |
|---:|---|---|---|
| 1 | [Qwen3-4B-Instruct-2507 model card](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507/tree/cdbee75f17c01a7cc42f958dc650907174af0554) | Official non-thinking 4B checkpoint, 262K native context, Apache-2.0 tag. | Prefer over hybrid-thinking Qwen3-4B for lower dialogue latency; cap product context at 32K. |
| 2 | [Pinned Qwen license](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507/blob/cdbee75f17c01a7cc42f958dc650907174af0554/LICENSE) | Exact Apache-2.0 grant and notice. | Bundle the exact pinned license as a model artifact. |
| 3 | [Pinned Qwen config](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507/blob/cdbee75f17c01a7cc42f958dc650907174af0554/config.json) | Qwen3 architecture, 36 layers, 4B class. | Admit GPU-layer choice explicitly; never assume every device can offload all layers. |
| 4 | [Pinned generation config](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507/blob/cdbee75f17c01a7cc42f958dc650907174af0554/generation_config.json) | Official sampling/end-token metadata. | Preserve embedded model/chat metadata and bound caller sampling. |
| 5 | [Pinned tokenizer config](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507/blob/cdbee75f17c01a7cc42f958dc650907174af0554/tokenizer_config.json) | Official Jinja chat template/tokenizer settings. | Require llama.cpp Jinja support; do not hand-build ChatML strings. |
| 6 | [Qwen3 local llama.cpp guide](https://github.com/QwenLM/Qwen3/blob/main/docs/source/run_locally/llama.cpp.md) | Qwen recommends official GGUFs/llama.cpp, Jinja, GPU-layer control, flash attention, and workload-specific sampling. | Use local file `-m`, Jinja, governor-selected threads/layers, and pinned runtime. |
| 7 | [Qwen3 GGUF quantization guide](https://github.com/QwenLM/Qwen3/blob/main/docs/source/quantization/llama.cpp.md) | Q4_K_M is a common supported quant; calibration matters and very low quants can lose quality. | Choose Q4_K_M as latency/storage tier; do not descend to Q2/Q3 for the default local option. |
| 8 | [Qwen3 launch blog](https://qwenlm.github.io/blog/qwen3/) | Dense Qwen3-4B is Apache-2.0 and designed for multilingual instruction/dialogue. | Retain generic/multilingual capability rather than game-specific integration. |
| 9 | [Qwen3 technical report](https://arxiv.org/abs/2505.09388) | Architecture/training/evaluation provenance for the Qwen3 family. | Keep the base-model citation and provenance with the pack. |
| 10 | [Qwen3 repository](https://github.com/QwenLM/Qwen3) | Official docs point local use to maintained llama.cpp and note the retired qwen.cpp path. | Do not revive qwen.cpp; use llama.cpp. |
| 11 | [Qwen qwen.cpp retirement](https://github.com/QwenLM/qwen.cpp) | Qwen states active maintenance ended after integration into llama.cpp. | Reject qwen.cpp as the runtime. |
| 12 | [Pinned Unsloth GGUF conversion](https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/tree/a06e946bb6b655725eafa393f4a9745d460374c9) | Rights-clear Apache-tagged conversion tied to the official Qwen base; exact LFS SHA-256 is available. | Pin commit, size, LFS digest, and provenance README; never use `main`. |
| 13 | [Hugging Face download guide](https://huggingface.co/docs/huggingface_hub/en/guides/download) | Full commit hashes select immutable revisions; Hub/Xet reconstructs by LFS SHA-256. | Use full 40-character resolve revisions and verify the final byte stream independently. |
| 14 | [Hugging Face file-download reference](https://huggingface.co/docs/huggingface_hub/en/package_reference/file_download) | Version-aware download/cache behavior and metadata contracts. | Product downloader owns a private staging/receipt flow instead of mutating a shared HF cache. |
| 15 | [Hugging Face Hub security](https://huggingface.co/docs/hub/en/security) | Hub scanning/signing features supplement, but do not replace, consumer verification. | Treat host reputation as non-authoritative; require signed catalog plus SHA-256. |
| 16 | [GGUF specification](https://github.com/ggml-org/ggml/blob/master/docs/gguf.md) | GGUF is a structured single-file model container with typed metadata/tensors. | Never execute/install GGUF as an archive; treat it as a size/hash-verified regular file. |
| 17 | [llama.cpp pinned source](https://github.com/ggml-org/llama.cpp/tree/57291f2644af8c9df0dd8d44395881c5bdcf0ecd) | Maintained C/C++ inference runtime with Windows CPU and GPU backends. | Pin source commit and release build number in the runtime ABI. |
| 18 | [llama.cpp pinned MIT license](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/LICENSE) | Runtime is MIT, distinct from model Apache-2.0. | Keep runtime as a separate signed/licensed installation unit. |
| 19 | [b10689 release](https://github.com/ggml-org/llama.cpp/releases/tag/b10689) | Official Windows x64 CPU/Vulkan assets and GitHub SHA-256 asset digests. | Pin exact CPU/Vulkan ZIP byte lengths and digests; offer no mutable latest URL. |
| 20 | [Pinned server README](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/tools/server/README.md) | Loopback default, health/models/chat endpoints, SSE, API keys, slots, and JSON-schema `response_format`. | Supervise one authenticated loopback slot and stream OpenAI-compatible SSE. |
| 21 | [Pinned argument definitions](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/common/arg.cpp) | Exact b10689 flags include offline, no Web UI, API-key file, Jinja, context, threads, GPU layers, reasoning format, and logging control. | Construct a fixed argv; no shell, user flags, built-in tools, or remote model identifiers. |
| 22 | [Pinned server schema](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/tools/server/server-schema.cpp) | Server validates request/response structures. | Still enforce tighter worker-side size/depth/field ceilings before HTTP. |
| 23 | [Pinned server request conversion](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/tools/server/server-common.cpp) | `response_format` is converted into JSON-schema grammar; grammar and JSON schema conflict. | Expose one structured-output path only and reject user grammars. |
| 24 | [llama.cpp grammar guide](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/grammars/README.md) | Only a JSON Schema subset is supported and pathological repetitions can be slow. | Bound schema bytes, depth, nodes, and arrays; reject remote references. |
| 25 | [llama.cpp function calling guide](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/docs/function-calling.md) | Tool support depends on templates and can regress under extreme KV quantization. | Do not expose local tools in v1; NPC action proposals remain inert validated data. |
| 26 | [llama.cpp Windows build/release matrix](https://github.com/ggml-org/llama.cpp/releases) | Official CPU, Vulkan, CUDA, SYCL, ROCm and other assets have different dependencies. | Ship explicit CPU/Vulkan choices only for this qualified pack; do not auto-download CUDA runtimes. |
| 27 | [llama.cpp Vulkan build preset](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/CMakePresets.json) | Vulkan is an intentional maintained build backend. | Use Vulkan as the cross-vendor GPU qualification choice and retain CPU fallback as a separate install. |
| 28 | [Microsoft process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) | `CREATE_NO_WINDOW` and process-group behavior are defined Windows process controls. | Launch hidden without a console and without `shell=True`. |
| 29 | [Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) | Jobs supervise groups of processes and enforce lifecycle/resource limits. | Put llama-server in an app-owned kill-on-close Job Object. |
| 30 | [AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject) | Assignment rules and nested-job constraints are explicit. | Fail closed if assignment is rejected; do not leave an unsupervised child running. |
| 31 | [SetInformationJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject) | Extended limits configure job behavior. | Set active-process and optional admitted memory ceilings before serving requests. |
| 32 | [Extended job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information) | Kill-on-close, active-process, and memory-limit fields have defined semantics. | Limit the runtime unit to one process and terminate it on supervisor loss. |
| 33 | [Python subprocess security/creation](https://docs.python.org/3/library/subprocess.html) | Argument arrays avoid shell interpretation; Windows creation flags are supported. | Pass a fixed argv list, minimal environment, fixed cwd, DEVNULL stdin, and drained pipes. |
| 34 | [Python ZIP handling](https://docs.python.org/3/library/zipfile.html) | ZIP metadata exposes filenames, sizes, flags, modes, and CRC-checked streams. | Preflight every member and extract manually under exact ceilings. |
| 35 | [JSON Schema 2020-12](https://json-schema.org/draft/2020-12) | Versioned schema semantics include references and recursive structures. | Permit bounded local schemas, reject remote refs, and leave final `npc_response.v1` validation to runtime-core. |
| 36 | [b10689 GitHub release/API](https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b10689) | Release `b10689` resolves to commit `57291f2644af8c9df0dd8d44395881c5bdcf0ecd`; GitHub publishes immutable CUDA 12.4 x64 program and cudart asset identities. | Pin both assets by URL, GitHub asset ID, byte length and SHA-256; CUDA is a two-archive trust unit. |
| 37 | [Pinned Windows release workflow](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/.github/workflows/build.yml) | Official Windows release jobs build distinct CUDA versions/backends and publish runtime dependencies separately. | Use upstream binaries instead of silently installing a toolkit or mixing DLLs from another release. |
| 38 | [Pinned llama-bench documentation](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/tools/llama-bench/README.md) | Repetitions, warmup behavior and individual JSON/JSONL observations are explicit. | Preserve warmups and raw observations; compute percentiles in the evidence layer instead of timing one CLI headline. |
| 39 | [Pinned b10689 argument definitions](https://github.com/ggml-org/llama.cpp/blob/57291f2644af8c9df0dd8d44395881c5bdcf0ecd/common/arg.cpp) | Batch defaults 2048, ubatch defaults 512, KV defaults f16, Flash Attention accepts auto, and threads/batch/gpu layers are independent controls. | Pass every comparison control explicitly and unit-test identical Vulkan/CUDA argv after normalizing paths. |
| 40 | [CUDA 12.4 EULA](https://docs.nvidia.com/cuda/archive/12.4.1/eula/index.html) | NVIDIA lists permitted redistributable runtime components and the applicable license terms. | Record a separate NVIDIA license component; keep this qualification download local and do not bundle or redistribute it. |
| 41 | [NVIDIA CUDA minor-version compatibility](https://docs.nvidia.com/deploy/cuda-compatibility/minor-version-compatibility.html) | CUDA 12.x applications have documented driver minimums and later-driver minor compatibility within the major family. | Label the runtime truthfully as 12.4 and preflight the installed driver; do not install CUDA Toolkit 12.4 over the host's 12.1 toolkit. |
| 42 | [NVIDIA CUDA Windows installation guide](https://docs.nvidia.com/cuda/archive/12.4.1/cuda-installation-guide-microsoft-windows/index.html) | Toolkit, driver and redistributable/runtime responsibilities are distinct on Windows. | Use release-contained runtime DLLs and leave the system toolkit unchanged. |
| 43 | [Project benchmark strategy](../../../docs/research/benchmark-strategy.md) | Exact revisions/settings, constrained remaining resources, raw evidence, game pressure and no substitution of absent results are required. | Separate direct backend evidence from bounded synthetic host-load observations and any eventual real-game frame-impact test. |
| 44 | [LM Studio CLI load documentation](https://lmstudio.ai/docs/cli/load) | The CLI can select GPU offload, context, parallelism, TTL and identifier, but not all direct llama.cpp controls. | Treat LM Studio as observational only; it cannot be promoted to the apples-to-apples direct comparison. |
| 45 | [LM Studio CLI import documentation](https://lmstudio.ai/docs/cli/import) | Import supports symbolic links and dry-run in addition to move/copy/hard-link. | Permit only a temporary symbolic link to the already verified GGUF; skip if the link cannot be created and never copy the model. |
| 46 | [LM Studio model-load REST documentation](https://lmstudio.ai/docs/developer/rest/endpoints/models/load) | LM Studio exposes explicit local model load/unload controls and configurable context/flash/eval-batch settings. | Bound the control lifecycle and capture its different settings/revision rather than implying equivalence. |
| 47 | [LM Studio structured output documentation](https://lmstudio.ai/docs/developer/core/structured-output) | JSON Schema constrained output is supported, with model-dependent caveats. | Reuse the exact structured self-test but keep results non-admissible and validate output independently. |

## Synthesis

The evidence converges on llama.cpp as the maintained Qwen local runtime and on
Q4_K_M as a reasonable small-model latency/storage tier. It also shows why the
one-stage quickstart is insufficient for a game product: production needs a
multi-stage trust path—signed catalog, exact download verification, atomic
install, runtime ABI check, device-bound resource admission, structured
self-test, supervised load, bounded generation, cancellation, and unload.

The evidence does **not** establish universal RAM/VRAM or latency numbers. Those
depend on device, backend, context, GPU-layer placement, game pressure, and
runtime revision. Therefore this pack keeps manifest measured fields null and
provides measurement hooks instead of turning planning estimates into admission.

For the CUDA comparison, the evidence additionally supports a strict
same-model/same-commit direct A/B: two warmups per backend, 20 measured
observations per backend, alternating ABBA/BAAB order, exact explicit server
controls, lifecycle checks on every sample, and archive-anchored runtime
verification. LM Studio does not meet that equivalence standard because it is
`b10662` with bundled CUDA 12.8 and a smaller control surface, so its 10-sample
lane is observational and cannot emit p99 or admission claims.
