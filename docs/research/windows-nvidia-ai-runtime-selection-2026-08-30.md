# Windows NVIDIA AI runtime selection

Status: research decision and qualification plan, not a pack admission or release claim
Target: Windows 10/11 x64, NVIDIA GeForce RTX 4080 Laptop GPU (Ada, 12 GB)
Evidence refreshed: 2026-08-30 UTC
Scope: local STT, TTS, embeddings, vision, identity and lip-sync runtimes; model quality is evaluated separately

## Decision

The advanced Windows profile should resemble LM Studio's explicit CPU/CUDA
runtime choice, not install one global accelerator stack. CPU workers preserve
game frame time and provide a dependable fallback. NVIDIA-specific workers are
separate, versioned packs admitted only from measurements made on the current
device, driver, game workload, model revision and runtime build.

| Role | Default beside a game | Accelerated or quality mode | Fallback |
| --- | --- | --- | --- |
| STT | Moonshine v2 streaming in the native Moonshine Voice/ONNX Runtime CPU path | `faster-whisper` with CTranslate2 CUDA, FP16, batch 1; only after GPU admission or when the game is absent | sherpa-onnx Moonshine CPU INT8; whisper.cpp CPU |
| TTS | Kokoro 1.0 through sherpa-onnx C/C++, CPU FP32, 1–2 threads and short sentence chunks | ORT CUDA only if same-graph Windows testing materially improves first playable audio without hurting game frame time | Qualified Kokoro CPU INT8; Piper only as a different-model fallback |
| Embeddings | ORT CPU INT8/FastEmbed; compare OpenVINO INT8 on an Intel CPU | CUDA or PyTorch FP16 for bulk indexing while the game is absent | ORT CPU FP32 |
| Vision | ORT CUDA FP16 until the TensorRT-RTX game path qualifies | TensorRT for RTX FP16 with fixed/bounded profiles, runtime cache and simultaneous-compute-and-graphics preparation | OpenVINO on Intel CPU; ORT CPU; Windows ML/DirectML for cross-vendor compatibility |
| Identity | Sparse ORT/OpenVINO CPU inference using detections already produced by vision | Reuse an admitted TensorRT-RTX vision worker and GPU-resident crops; do not load a second continuous detector | ORT CPU with explicit/manual identity fallback |
| Lip-sync | Project-owned tracked current-frame mouth warp; fail open to the untouched frame | NVIDIA Audio2Face-3D regression Mark v2.3 through its direct TensorRT SDK after mapper and game-load qualification | Disable the visual residual; MuseTalk is experimental/offline and Wav2Lip remains offline-only |

These are runtime choices, not model-quality rankings. Changing Moonshine to
Whisper, FP32 Kokoro to another TTS model, or a coefficient driver to a pixel
generator changes model behavior and cannot be credited to the runtime.

## Evidence classes and claim boundary

| Label | Meaning |
| --- | --- |
| **Official capability** | A maintained first-party document, repository, release note, paper or model card describes support or a measurement. It may have different hardware and workload conditions. |
| **Vendor claim, unmeasured here** | An upstream author reports performance or quality, but this repository has not reproduced it on the target RTX 4080 Laptop beside a game. |
| **Repository measurement** | A preserved local artifact records an actual run. It applies only to its exact fixture, runtime, driver and configuration. |
| **Qualification candidate** | Public availability and plausible architecture justify a bounded test. It is not installed, selected, bundled or recommended to users yet. |

The following statements must remain distinct:

- NVIDIA's TensorRT for RTX documentation officially describes simultaneous
  compute and graphics for game inference. This repository has not yet measured
  that mode, its ONNX Runtime execution-provider wrapper, or its effect on a
  running game.
- NVIDIA's Audio2Face-3D SDK officially claims faster-than-60-FPS generation
  and publishes a Windows TensorRT path. No Audio2Face-3D SDK/model run has been
  performed here. The claim is not an RTX 4080 product benchmark.
- Existing local Wav2Lip work is an offline qualification, not a live runtime
  result. Its preserved CUDA run used approximately 9.1 GB peak allocated or
  reserved memory on the 12 GB RTX 4080 Laptop and produced a functional but
  visually unpolished result. It therefore supplies negative co-residency
  evidence, not support for a live default.
- Existing MuseTalk evidence exercises the upstream prepared-avatar batch path,
  not a current captured-game-frame residual. It must not be generalized into a
  live latency or cancellation claim.

## Per-role rationale and constraints

### Speech to text

Moonshine v2 is the latency-first default for supported languages because its
streaming encoder performs work before endpoint detection. The official
Moonshine repository ships Windows x64 C++ examples and memory-mappable `.ort`
models. The original paper reports roughly five times less compute than Whisper
Tiny on a 10-second segment without worse reported WER; the v2 paper targets
bounded streaming latency. Those are upstream results, not Windows 4080 results.

Current streaming Moonshine models are described as MIT by default. Legacy
non-streaming non-English models are enumerated under a different Moonshine
Community License and must not be silently substituted or redistributed.

CTranslate2 is the preferred high-accuracy Whisper-family runtime because it
has current Windows x64 wheels, CUDA execution, maintained faster-whisper
benchmarks, explicit `load_model()`/`unload_model()`, bounded queues and batch-1
operation. The official faster-whisper benchmark on an RTX 3070 Ti reports about
4.5 GB for unbatched large-v3 FP16 and about 6.1 GB at batch 8. Those figures
demonstrate why throughput batching is inappropriate beside a 12 GB game; they
do not predict this laptop's latency.

Whisper.cpp remains the smallest native deployment fallback: Windows/MSVC,
CPU quantization, CUDA, Vulkan, a streaming example and its own benchmark tool.
It should not become the CUDA default without a same-audio, same-decoding test.

### Text to speech

Kokoro-82M weights are published under Apache-2.0. sherpa-onnx provides a native
Windows-capable C/C++/C API with Kokoro phonemization assets and CPU/CUDA provider
selection. Short interactive synthesis often cannot amortize GPU setup, copies
and residency; CPU FP32 is therefore the default until measured otherwise.

INT8 is a candidate, not an assumed improvement. Upstream sherpa reports include
devices where Kokoro INT8 was slower than FP32 and a recent ARM-only corrupted
audio report. The latter did not reproduce in the reporter's Windows x64 control,
but it shows why qualification must test waveform validity and voice quality as
well as elapsed time. Cancellation occurs at phrase boundaries; one monolithic
paragraph is not an acceptable scheduling unit.

### Embeddings

Interactive retrieval is normally batch 1, where CPU INT8 avoids GPU contention.
FastEmbed is a lightweight ORT implementation, but its published GPU example is
a throughput case: approximately 43 ms for 500 documents versus 4.33 seconds on
CPU. It does not establish a batch-1 or beside-game win.

Current Sentence Transformers documentation reports PyTorch FP16 ahead of its
ONNX GPU configurations on the evaluated workloads and reports especially strong
OpenVINO INT8 CPU results with less than 0.5% quality loss in that benchmark.
Therefore test OpenVINO on an Intel host, but keep ORT CPU as the portable default.
An official FastEmbed issue also documents `onnxruntime` and `onnxruntime-gpu`
overwriting the same module and silently removing CUDA support. They belong in
different immutable environments.

### Vision and identity

TensorRT for RTX is the most relevant advanced vision candidate because NVIDIA
now documents a simultaneous-compute-and-graphics (SCG) path whose stated use
case includes inference beside video-game rendering. For Ada/Ampere the engine
must restrict tactic shared memory to 48 KiB. Runtime inference must use a
CUDA-in-Graphics context; CiG requires CUDA 12.6+ and RTX driver 555+. The current
release documents DirectX interoperability, with Vulkan support planned.

SCG generally reduces standalone inference performance because fewer kernels
qualify. Start with zero auxiliary streams to reduce activation memory and
contention, persist AOT/EP-context and runtime caches, and use fixed or tightly
bounded shape profiles. Validate capture, resize, transfer, inference and
post-processing together; kernel time alone is not end-to-end latency.

TensorRT-RTX 1.6 requires CUDA 12.9 Update 1 or CUDA 13.4. Its standalone ORT EP
is currently a plugin/source packaging path; the documentation says PyPI/NuGet
support is forthcoming. Windows ML distributes an NVIDIA TensorRT-RTX EP through
an MSIX package and can install/register it, but only its current version is
supported. Cache keys must include EP version, driver, GPU architecture, model
hash, precision and shape profile. Registering the EP does not by itself prove
that the required CiG context was created.

ORT CUDA FP16 is the simpler shipping fallback. Use I/O binding/device tensors
when inputs are already on the GPU. DirectML is cross-vendor fallback only: it is
in sustained-engineering mode, disables ORT memory patterns and parallel graph
execution, and permits only one concurrent `Run` per session. OpenVINO is an
Intel-oriented CPU/GPU/NPU runtime, not the NVIDIA accelerator default.

Identity models add a separate legal gate. InsightFace code is MIT and uses ORT,
but its distributed pretrained recognition packs are explicitly non-commercial
research models unless separately licensed. No public `buffalo_l`, `antelopev2`
or similar pack may become a redistributable product default. Prefer a licensed
or project-trained model and reuse vision detections/crops.

### Lip-sync

The public NVIDIA candidate is Audio2Face-3D, not the older private-access AR SDK
assumption. NVIDIA now publishes the Audio2Face-3D SDK source under MIT for
Windows 10/11 and Linux. Its stated requirements are CUDA `>=12.8,<13.0` (12.9
recommended), TensorRT `>=10.13,<11`, Python 3.8–3.10 for tooling, 8 GB RAM and
4 GB GPU memory recommended. Regression and diffusion weights are published
under the NVIDIA Open Model License; exact model and transitive asset terms must
still be captured before redistribution.

Regression Mark v2.3 is the first bounded candidate because it is more plausible
beside a game than the diffusion model. It outputs coefficients, blendshapes or
geometry for a prepared character. It does not produce a finished mouth patch
for an arbitrary captured game frame. A project-owned current-frame mapper and
compositor must prove actor/frame binding, occlusion, cancellation, stability and
visual quality. NVIDIA's faster-than-60-FPS SDK claim is **vendor-reported and
unmeasured here**.

MuseTalk 1.5 is not the default. Its code is MIT and its own weights permit
commercial use, but transitive weights retain their own terms. Its 30+ FPS claim
is for a prepared/reused avatar on a Tesla V100; the same README reports roughly
five minutes to generate eight seconds on an RTX 3050 Ti Laptop and lists identity
loss and single-frame jitter. Wav2Lip's official pretrained path is restricted to
personal/research/non-commercial use and remains an offline reference.

## Windows runtime-pack policy

Package independent processes rather than place multiple CUDA/cuDNN/TensorRT
generations on global `PATH`:

| Runtime pack | Contents and constraint |
| --- | --- |
| `speech-cpu-win-x64` | Moonshine Voice and/or sherpa-onnx CPU. STT and TTS may link the same library but remain independent supervised processes. |
| `whisper-ct2-cuda12-cudnn9-win-x64` | Pinned faster-whisper/CTranslate2 with a private DLL directory. Current upstream requires CUDA 12 and cuDNN 9. |
| `ort-cpu-win-x64` | Embeddings and sparse vision/identity. Keep resident when measured RAM fits. |
| `ort-cuda12-cudnn9-win-x64` | Pin ORT 1.26.x for the CUDA 12.8/cuDNN 9 lane. ORT 1.27+ default PyPI/NuGet GPU builds use CUDA 13. |
| `nvtensorrt-rtx-cuda12.9-win-x64` | Continuous-vision qualification pack. Use Windows ML EP delivery or an explicitly reviewed NVIDIA SDK distribution. |
| `audio2face-regression-trt10.13-cuda12.9-win-x64` | Separate direct SDK worker because its TensorRT ABI, model lifecycle, outputs and license ledger differ from ORT. |

ORT documents CUDA 12.8 builds as compatible across CUDA 12.x, but cuDNN major
versions are not interchangeable. Use an explicit DLL directory or ORT's
`preload_dlls()` rather than ambient search order. Each worker must report its
actual provider list and fail closed if requested acceleration falls back to CPU.

## Co-residency, cancellation and lifecycle policy

Admission uses measured totals, not names or vendor minimums:

`game reserve + resident model VRAM + p99 transient/workspace VRAM + safety margin <= current DXGI budget`

- Unknown resource values fail closed.
- The game, audio deadline and current-frame visual work outrank background
  indexing and optional quality passes.
- ORT's `gpu_mem_limit` covers only its arena; total GPU consumption can be
  higher. Admission uses process and DXGI/driver telemetry.
- Start TensorRT/TensorRT-RTX with one execution context, batch 1 and zero
  auxiliary streams. Add parallelism only when game measurements permit it.
- Keep CPU speech and embedding sessions warm. Test GPU idle eviction at 5, 30
  and 120 seconds and select the shortest threshold that avoids reload thrash.
- A CTranslate2 worker may use `unload_model()` or move weights to CPU. Destroying
  an ORT session or the worker is the actual ORT unload boundary. TensorRT caches
  may persist on disk, but contexts and allocations must die with the worker.
- ORT first receives `RunOptionsSetTerminate`. CTranslate2, sherpa and TensorRT
  have no trusted hard preemption for already-enqueued GPU work; reject queued
  work, generation-stamp every result, discard stale output, then terminate the
  worker after a bounded grace period.
- The supervisor owns each worker in a Windows Job Object so a crash, timeout or
  app exit reclaims descendants and runtime state. A restarted worker must pass
  provider and model-hash self-tests before becoming ready.

## Qualification order after the current STT lock

No step below authorizes a download. Acquire exact models only through the
existing model-pack review flow after the current GPU owner releases the lock.
Run one GPU lane at a time and restore the shared reservation in a `finally`
path.

1. **CPU-only baselines:** Moonshine v2 streaming, Kokoro FP32/INT8 and embedding
   ORT/OpenVINO. Record cold load, warm p50/p95/p99, RSS, cancellation and quality.
2. **Whisper CUDA:** faster-whisper/CTranslate2 FP16 then INT8-FP16, batch 1,
   followed by whisper.cpp CUDA only as a same-corpus comparator.
3. **ORT CUDA vision:** one fixed-shape model with full preprocessing and
   post-processing; establish the practical CUDA fallback before TensorRT work.
4. **TensorRT-RTX vision without a game:** compile/cache a 48 KiB-SCG-compatible
   engine, verify provider assignment, numerical agreement, cold/warm load and
   resource use.
5. **TensorRT-RTX vision beside the controlled DirectX game replay:** compare a
   normal CUDA context with a verified CiG context at 10, 15 and 30 inference FPS.
6. **Sparse identity:** add the licensed detector/embedding graph first on CPU,
   then reuse the admitted vision GPU path. Do not duplicate continuous detection.
7. **Kokoro CUDA:** test only if CPU first-audio latency misses its product budget.
8. **Audio2Face-3D regression:** last GPU candidate. First test coefficient output
   alone, then the current-frame mapper/compositor, then the full game replay.
9. **MuseTalk/Wav2Lip:** offline comparators only; they cannot promote themselves
   into the live chain from throughput or visual demos.

## Exact test matrix and pass criteria

Use the same laptop, driver, plugged-in power state, cooling state and game scene.
For every candidate run three cold launches, 30 warmups and at least 1,000 short
inferences or the role-specific corpus. Test with no game, capped game replay and
uncapped game replay. Capture PresentMon frame time, DXGI budget/usage, process
RSS, per-engine GPU utilization, VRAM, power/temperature, cold load, warm
p50/p95/p99 and cancellation latency.

Global admission criteria:

- no stale output after a cancellation-generation barrier;
- no crash, allocator leak or unreclaimed VRAM after 100
  load/run/cancel/unload-or-restart cycles;
- no device OOM, Windows shared-memory spill or silent CPU provider fallback;
- game p99 frame-time increase no greater than 1 ms and 1% low no worse than 3%
  from the matching baseline;
- output remains within the role's predeclared tolerance against its FP32 or
  trusted reference path.

| Role | Exact A/B workload | Additional pass evidence |
| --- | --- | --- |
| STT | 200 turns: 0.3–1.5 s commands, 2–8 s dialogue and 15–30 s monologues; clean, game-noise and interruption variants; cancel 25% at 20/100/300 ms | Partial stability, endpoint-to-final p50/p95, RTF, normalized WER, no post-cancel transcript and bounded restart |
| TTS | Same Kokoro graph/voice; 20/80/200/500-character inputs with numbers, abbreviations, punctuation and names | Phonemizer time separate from first playable chunk; full RTF, clipping/NaN/rail checks, spectral deviation and listening review |
| Embeddings | Same model/tokenizer; batch 1/8/32/128 and token length 32/128/512 across ORT CPU FP32/INT8, OpenVINO, ORT CUDA, TensorRT-RTX and PyTorch FP16 | End-to-end tokenization plus inference, top-10 overlap/cosine deviation, interactive latency and bulk throughput reported separately |
| Vision | Same authorized ONNX graph at fixed 640×640 and one bounded dynamic profile; 10/15/30 FPS schedules | Include capture/resize/H2D/inference/NMS/D2H; detection agreement, missed deadline rate and normal-CUDA versus verified-CiG frame impact |
| Identity | Same authorized detector/recognizer; sparse cadence plus scene cuts, crowds, occlusion and reacquisition | False association rate, track continuity, embedding deviation and proof no second continuous detector is resident |
| Lip-sync | 300–1,000 current captured frames with cuts, masks, profile faces, occlusion and identity changes | p95 inference-plus-map under one frame, exact source-frame binding, untouched-frame fail-open, no post-cancel frame and rendered visual review |

For TensorRT/TensorRT-RTX, record host latency including H2D and D2H, enqueue
time, GPU compute, engine build time, cache-hit load time and peak builder/runtime
memory separately. A fast cached kernel result cannot hide a minutes-long first
activation or an engine that hurts game frame pacing.

## Packaging and license gates

- Runtime code, model weights, tokenizer/voice/dictionary assets and optional
  samples have separate licenses. Review and hash each item.
- ORT and CTranslate2 code are MIT; FastEmbed and Kokoro are Apache-2.0 signals.
  sherpa-onnx source is Apache-2.0, but every selected model/assets package still
  needs its own ledger.
- Moonshine streaming models are MIT by default according to the current repo;
  legacy non-streaming non-English exceptions remain under the Community License.
- InsightFace public pretrained recognition packs are not a commercial default.
- MuseTalk's top-level code/model permissions do not override licenses of Whisper,
  VAE, DWPose, face parsing or other transitive assets.
- Wav2Lip official pretrained results are non-commercial.
- Audio2Face-3D SDK code and model weights have different terms; neither grants
  automatic redistribution rights for proprietary NIM, CUDA, cuDNN, TensorRT or
  Windows ML components. Preserve NVIDIA and third-party notices and obtain an
  explicit distribution determination before a public pack.
- TensorRT-RTX Windows SDK download requires NVIDIA Developer Program membership
  and acceptance of NVIDIA terms. Windows ML EP delivery changes installation
  mechanics, not the model or application license.

## Primary source ledger

Runtime and scheduling:

1. [ONNX Runtime CUDA EP and version matrix](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html)
2. [ONNX Runtime TensorRT EP](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html)
3. [ONNX Runtime TensorRT-RTX EP](https://onnxruntime.ai/docs/execution-providers/TensorRTRTX-ExecutionProvider.html)
4. [ONNX Runtime DirectML EP](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)
5. [ONNX Runtime I/O binding](https://onnxruntime.ai/docs/performance/tune-performance/iobinding.html)
6. [ONNX Runtime device tensors](https://onnxruntime.ai/docs/performance/device-tensor.html)
7. [ONNX Runtime threading](https://onnxruntime.ai/docs/performance/tune-performance/threading.html)
8. [ONNX Runtime `RunOptions` termination](https://onnxruntime.ai/docs/api/c/struct_ort_1_1_run_options.html)
9. [ONNX Runtime quantization](https://onnxruntime.ai/docs/performance/model-optimizations/quantization.html)
10. [ONNX Runtime profiling](https://onnxruntime.ai/docs/performance/tune-performance/profiling-tools.html)
11. [ONNX Runtime MIT license](https://github.com/microsoft/onnxruntime/blob/main/LICENSE)
12. [Windows ML supported execution providers](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/supported-execution-providers)
13. [Windows ML API and deployment behavior](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/api-reference)
14. [TensorRT-RTX prerequisites](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/installing-tensorrt-rtx/prerequisites.html)
15. [TensorRT-RTX Windows installation](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/installing-tensorrt-rtx/installing.html)
16. [TensorRT-RTX simultaneous compute and graphics](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/inference-library/compute-graphics.html)
17. [TensorRT-RTX runtime cache](https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/inference-library/work-with-runtime-cache.html)
18. [TensorRT benchmarking definitions](https://docs.nvidia.com/deeplearning/tensorrt/latest/performance/benchmarking.html)

Speech and embeddings:

19. [faster-whisper repository, requirements and benchmarks](https://github.com/SYSTRAN/faster-whisper)
20. [CTranslate2 Whisper lifecycle API](https://opennmt.net/CTranslate2/python/ctranslate2.models.Whisper.html)
21. [CTranslate2 hardware support](https://opennmt.net/CTranslate2/hardware_support.html)
22. [CTranslate2 installation](https://opennmt.net/CTranslate2/installation.html)
23. [CTranslate2 releases](https://github.com/OpenNMT/CTranslate2/releases)
24. [whisper.cpp Windows/CUDA/Vulkan support and benchmarks](https://github.com/ggml-org/whisper.cpp)
25. [Moonshine Voice Windows/runtime/model documentation](https://github.com/moonshine-ai/moonshine)
26. [Moonshine original paper](https://arxiv.org/abs/2410.15608)
27. [Moonshine v2 streaming paper](https://arxiv.org/abs/2602.12241)
28. [sherpa-onnx platform/runtime repository](https://github.com/k2-fsa/sherpa-onnx)
29. [sherpa-onnx Windows CUDA build/package](https://k2-fsa.github.io/sherpa/onnx/install/windows/build-cuda.html)
30. [sherpa-onnx Kokoro packages](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html)
31. [sherpa-onnx Kokoro C API](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/c-api/docs/tts.dox)
32. [Kokoro-82M model card and license](https://huggingface.co/hexgrad/Kokoro-82M)
33. [FastEmbed repository and GPU provider](https://github.com/qdrant/fastembed)
34. [FastEmbed GPU throughput example](https://qdrant.github.io/fastembed/examples/FastEmbed_GPU/)
35. [Sentence Transformers runtime benchmarks](https://sbert.net/docs/sentence_transformer/usage/efficiency.html)
36. [FastEmbed ORT CPU/GPU package collision](https://github.com/qdrant/fastembed/issues/608)

Vision, identity and animation:

37. [InsightFace runtime and pretrained-model license boundary](https://github.com/deepinsight/insightface/blob/master/python-package/README.md)
38. [NVIDIA Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK)
39. [NVIDIA Audio2Face-3D SDK architecture](https://github.com/NVIDIA/Audio2Face-3D-SDK/blob/main/docs/README.md)
40. [NVIDIA Audio2Face-3D component and license collection](https://github.com/NVIDIA/Audio2Face-3D)
41. [NVIDIA Audio2Face-3D Mark v2.3 model card](https://huggingface.co/nvidia/Audio2Face-3D-v2.3-Mark)
42. [NVIDIA Open Model License](https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/)
43. [MuseTalk official runtime conditions, limitations and license](https://github.com/TMElyralab/MuseTalk)
44. [Wav2Lip official restrictions](https://github.com/Rudrabha/Wav2Lip)

Related repository evidence and policy:

- [NVIDIA and latency qualification](./nvidia-and-latency-qualification-2026-08-30.md)
- [Generic captured-game lip-sync qualification](./generic-captured-game-lipsync-qualification-2026-08-30.md)
- [Model license ledger](./model-license-ledger.md)
- [Runtime process model](../architecture/process-model.md)
- [Local model runtime and packs ADR](../architecture/adr/0005-local-model-runtime-and-packs.md)
