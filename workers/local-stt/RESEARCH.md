# Local STT decision record — 2026-08-30

## Decision

Use Moonshine Voice v0.1.5 Medium Streaming English through its native C API,
CPU-only, as the first optional Windows pack. Keep hosted STT as the product
default. Do not select the local pack until its exact installed artifact passes
real speech, accuracy-smoke, latency, RAM, CPU, load/unload, cancellation, and
game-contention qualification.

This replaces the repository's older Moonshine v0.0.45 assumption. The current
project is `moonshine-ai/moonshine`; v0.1.5 is pinned to commit
`234f60faa0eb388b01cdf7e60aca232af37aefda`. The GitHub release publishes a
self-contained Windows example archive at 402,991,031 bytes with SHA-256
`a56bcd27765fefa4ab9a9b219cbb26e6de7d48b6536c88b92b6d24ffa7eb4c25`.

Medium was chosen for the first verifiable pack because the self-contained,
strongly hashed release archive includes that exact model. Tiny remains a
future smaller option after its separately hosted components are downloaded
under the project's AI-model lock and independently SHA-256 pinned. The dated
upstream model paths and CRC32C checks are useful transport checks but are not
a substitute for our cryptographic activation gate.

## Synthesis

- Moonshine v2 is truly incremental: it uses bounded sliding-window encoder
  attention plus cached encoder/decoder state, unlike wrappers that repeatedly
  run an offline Whisper window.
- The upstream C API accepts natural PCM chunks, performs sample-rate
  conversion, owns VAD segmentation, marks completed lines immutable, exposes
  partial updates and word timestamps, and supports clean stop-and-drain.
- The upstream default 500 ms update interval is conservative. This worker
  starts at 250 ms but adapts when inference cost approaches the audio interval;
  the real qualification decides whether that is sustainable on the target.
- CPU is the upstream recommendation and avoids competing with the game and the
  mandatory local lip-sync path for VRAM. `0 MiB` VRAM is an enforced policy,
  while RAM and latency remain explicitly unmeasured.
- The model card warns about hallucinations/repetition on short or noisy audio.
  PTT key-up is therefore authoritative, blank/suspicious output is not
  invented, and the application must preserve typed/manual fallback.
- Word timestamps roughly double decoder payload size upstream, but the bundled
  archive already contains the attention decoder. They are enabled because the
  product needs exact turn timing and subtitle/animation alignment; diarization
  remains disabled because it adds substantial compute and is unnecessary for
  a single user microphone.
- `whisper.cpp`'s own streaming example calls itself naive and periodically
  reruns windows; faster-whisper is excellent for throughput but is not a native
  incremental microphone runtime. sherpa-onnx Zipformer is the strongest future
  alternative for broader language/runtime choice, but Moonshine has the best
  combination of current Windows bundle, low-latency design, permissive English
  streaming license, and direct event semantics for this lane.

## Primary source ledger

All sources were read on 2026-08-30. Claims are scoped to the pinned versions or
page state named here.

| # | Primary source | What it established |
|---:|---|---|
| 1 | [Moonshine v0.1.5 release](https://github.com/moonshine-ai/moonshine/releases/tag/v0.1.5) | Current stable tag, release asset identities, sizes, and GitHub SHA-256 digests. |
| 2 | [v0.1.5 commit](https://github.com/moonshine-ai/moonshine/commit/234f60faa0eb388b01cdf7e60aca232af37aefda) | Immutable source revision behind the tag. |
| 3 | [Repository README at v0.1.5](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/README.md) | On-device, streaming, cross-platform project scope. |
| 4 | [License at v0.1.5](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/LICENSE) | Code and every streaming STT model are MIT; exhaustive non-MIT legacy exceptions. |
| 5 | [Available models](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/models/available-models.md) | Model sizes, languages, license, WER/CER scope, and English streaming choices. |
| 6 | [Accuracy notes](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/models/accuracy.md) | Floating vs deployed quantized results and VAD evaluation caveats. |
| 7 | [Quantization notes](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/models/quantization.md) | Shipping representation and accuracy/size context. |
| 8 | [Moonshine v2 paper](https://arxiv.org/abs/2602.12241) | Sliding-window ergodic encoder, bounded TTFT motivation, model comparison methodology. |
| 9 | [Original Moonshine paper](https://arxiv.org/abs/2410.15608) | Variable-length input efficiency and live transcription design motivation. |
| 10 | [Flavors of Moonshine paper](https://arxiv.org/abs/2509.02523) | Language-specialized edge-model tradeoffs and permissive releases. |
| 11 | [Streaming Tiny model card](https://huggingface.co/moonshine-ai/moonshine-streaming-tiny) | Architecture, intended use, MIT tag, WER panel, hallucination/repetition limitations. |
| 12 | [C API header](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-c-api.h) | Stable ABI, stream lifecycle, PCM contract, line invariants, timestamps, options. |
| 13 | [Transcriber implementation](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/transcriber.cpp) | VAD-to-line processing, stop/drain behavior, streaming state ownership. |
| 14 | [Streaming model implementation](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-streaming-model.cpp) | Incremental state and cached streaming decode implementation. |
| 15 | [Model catalog](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-model-catalog.cpp) | Dated model directories, architecture IDs, required assets, default ordering. |
| 16 | [Generated model metadata](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-model-file-metadata.generated.cpp) | Per-component byte lengths and upstream CRC32C transport checks. |
| 17 | [Download smoke](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-download-smoke.cpp) | Upstream dependency-manifest/download verification behavior. |
| 18 | [Downloading models](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/using/downloading-models.md) | Opt-in download, `.part`, range resume, free-space checks, offline loading. |
| 19 | [Python downloader](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/language-bindings/python/src/moonshine_voice/download_file.py) | Concrete atomic/resumable downloader behavior and cancellation hooks. |
| 20 | [API classes](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/api/classes.md) | Microphone, PCM, partial/final events, multiple streams, mute, flush semantics. |
| 21 | [API options](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/api/options.md) | VAD defaults, update interval, timestamps, decoding and context options. |
| 22 | [Python Transcriber](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/language-bindings/python/src/moonshine_voice/transcriber.py) | ctypes ABI mapping, adaptive update cadence, event derivation, resource cleanup. |
| 23 | [Python MicTranscriber](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/language-bindings/python/src/moonshine_voice/mic_transcriber.py) | Capture thread ownership, mute/close/error flow, device fallback behavior. |
| 24 | [Concurrency test](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/transcriber-concurrency-test.cpp) | Upstream concurrency expectations and shared-model stream safety. |
| 25 | [Streaming memory test](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/transcriber-streaming-memory-test.cpp) | Streaming architecture asset and lifecycle coverage from memory. |
| 26 | [Benchmark guide](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/using/benchmarks.md) | Final-after-speech latency definition, RTF interpretation, thermal run variance. |
| 27 | [Execution providers](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/execution-providers.md) | CPU recommendation and platform provider constraints. |
| 28 | [Domain customization](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/models/domain-customization.md) | Key-term bias benefits and false-positive tradeoff. |
| 29 | [Windows example README](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/examples/windows/cli-transcriber/README.md) | Supported Windows build, bundled model, WASAPI mic and 16 kHz conversion. |
| 30 | [Windows example source](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/examples/windows/cli-transcriber/cli-transcriber.cpp) | Concrete WASAPI capture and upstream C++ streaming calls. |
| 31 | [Windows example packaging](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/scripts/publish-examples.bat) | Exact medium model payload and runtime files inside the signed release archive. |
| 32 | [Release process](https://github.com/moonshine-ai/moonshine/blob/v0.1.5/docs/release-process.md) | Tag/release production and verification expectations. |
| 33 | [ONNX Runtime threading](https://onnxruntime.ai/docs/performance/tune-performance/threading.html) | Thread count, spinning, affinity, and contention tuning requirements. |
| 34 | [ONNX Runtime performance guide](https://onnxruntime.ai/docs/performance/tune-performance/) | Latency, throughput, memory, profiling, and tuning dimensions. |
| 35 | [Microsoft WASAPI capture](https://learn.microsoft.com/en-us/windows/win32/coreaudio/capturing-a-stream) | Endpoint-buffer ownership and GetBuffer/ReleaseBuffer capture contract. |
| 36 | [Windows device capabilities](https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/enable-device-capabilities) | Packaged/unpackaged microphone consent and capability behavior. |
| 37 | [whisper.cpp stream example](https://github.com/ggml-org/whisper.cpp/blob/master/examples/stream/README.md) | Its real-time example is explicitly naive/windowed and uses basic VAD. |
| 38 | [faster-whisper README](https://github.com/SYSTRAN/faster-whisper/blob/master/README.md) | Strong batch/word-timestamp/VAD route, but not an incremental native mic contract. |
| 39 | [sherpa-onnx README](https://github.com/k2-fsa/sherpa-onnx/blob/master/README.md) | Broad Windows/native streaming alternatives including Zipformer. |
| 40 | [sherpa-onnx Rust examples](https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/README.md) | Rust/native streaming model and microphone integration alternatives. |

## Qualification boundary

The canonical manifest intentionally does not promote measurements from one
development PC into portable planning claims. A gated 21-cycle run records
installed artifact identity, load/reload/unload latency, working set and peak
working set, CPU utilization, first partial/final/VAD/PTT/cancel latency,
real-time factor, stable known-transcript output, and NVIDIA process telemetry
under `%LOCALAPPDATA%\InteractiveNPCs\out`. Zero VRAM is both a CPU-provider
policy assertion and a measured per-process result for that run. The manifest
keeps planning values null and admission blocked until the shared manager
validates current-device signed evidence and the complete selected loadout.
