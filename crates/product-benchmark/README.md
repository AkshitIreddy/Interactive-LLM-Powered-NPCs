# Product This-PC benchmark

`npc-product-benchmark` is the product-side coordinator for a bounded,
timestamped **This PC** benchmark. It is not a provider benchmark generator and
it does not manufacture substitute values when a live path is absent.

## Product boundary

The desktop shell constructs one `BenchmarkManager` with:

- a `BenchmarkReportStore` rooted under the app's private data directory;
- a `LiveBenchmarkProbe` adapter backed by the currently selected runtime,
  target, capture broker, playback broker, compositor, process counters, and
  game-frame sampler; and
- `NativeTelemetryCollector`, which samples the existing
  `npc-system-telemetry` boundary for application/game RAM and VRAM plus safe
  hardware identity.

The UI-facing lifecycle is deliberately small:

```text
BenchmarkManager::start(request)  -> running status
BenchmarkManager::cancel()        -> cancelling status
BenchmarkManager::status()        -> progress or terminal status
BenchmarkManager::report(id)      -> terminal redacted report
```

Only one run can be active. Requests are bounded to 3–120 iterations, 5–300
seconds, and a 0.5–30 second baseline window. Cancellation races every probe
operation rather than waiting for a cooperative provider. Every terminal state,
including `partial`, `unavailable`, `cancelled`, and `failed`, has an explicit
classification and component action codes.

## Receipt boundary

`LiveBenchmarkProbe` supplies typed evidence. Implementations must translate,
not reinterpret, existing product receipts:

- runtime turn timing -> `RuntimeTurnTimingReceiptV1`;
- broker capture evidence -> `CaptureReceiptV1`;
- broker playback receipt -> `AudioSubmissionReceiptV1`;
- native presentation receipt -> `CompositorReceiptV1`;
- native process/device counters and game-load/frame evidence ->
  `ProcessLoadSampleV1`.

Provider timing receipts are source-bound. A receipt must identify itself as a
`product_runtime_receipt`, match the selected LLM and TTS provider/model/route
revisions exactly, match the selected TTS voice and canonical egress snapshot,
prove that the structured LLM response was validated, and prove a terminal
cancellation probe for those selected routes. Qualification artifacts,
historical observations, and synthetic fixtures are rejected from provider
metric aggregation even when their raw calls were genuinely live.

If a source does not expose a required monotonic boundary, the adapter leaves
that receipt absent and marks its preflight component unavailable. It must not
use fixture timings, budgets, catalog latency labels, provider HTTP timing, or a
capture-frame rate as a replacement for game frame time.

## Report contents

`interactive-npcs-this-pc-benchmark-report/v1` includes R-7 p50/p95/p99
summaries for 20 metrics:

- 12 pipeline measures: capture, identity, LLM first-token/output/throughput,
  TTS first/final audio, endpoint submission, compositor latency/FPS/stale
  drops, and end-to-end latency;
- CPU and GPU utilization;
- application and selected-game RAM/VRAM; and
- selected-game frame time and FPS.

Frame impact compares a measured idle baseline with the active benchmark
window. The report also binds the game profile/executable digest, provider route
revisions, exact TTS voice, canonical egress disclosures, observation source,
hardware fingerprint, component revisions, time bounds, and evidence mode. A
report is acceptance-eligible only when it is complete, live, measured, sourced
from running-product receipts, and produced by the Windows build. Eligibility
still does not mean acceptance.

Reports contain no PID/HWND, paths, hostname, environment variables, prompts,
transcripts, provider payloads, audio, screenshots, credential values, or raw
per-turn samples. The store validates claim semantics, writes a maximum 1 MiB
JSON document atomically, and returns only the bounded file name.

## Verification

From the repository root on Windows:

```powershell
cargo test -p npc-product-benchmark
cargo +stable clippy -p npc-product-benchmark --all-targets --no-deps -- -D warnings
```

The deterministic suite covers percentile parity, a complete 20-metric run,
partial visual evidence, all-unavailable preflight, non-cooperative cancellation,
request/concurrency bounds, mocked-route relabelling prevention, report
validation, and atomic redacted persistence. It makes no provider, package,
installer, game, or GPU calls.
