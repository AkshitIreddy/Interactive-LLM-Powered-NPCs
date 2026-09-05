# Benchmark evidence harness

This directory contains a dependency-free Python 3.10+ aggregator and a
PowerShell 5.1-compatible Windows entry point. It does **not** call providers,
launch the app, read credentials, capture prompts, retain audio, or invent live
measurements. Instrumented components write strict aggregate observations to
JSONL; the harness validates, bounds, aggregates, and emits the versioned report
described by `docs/product-rework/benchmark-evidence-schema.md`.

## Classification rules

Every observation has two independent labels:

- `execution_mode`: `simulated`, `mocked`, or `live` describes what ran.
- `measurement_kind`: `measured` or `planning_estimate` describes whether the
  value was observed or is only a budget/forecast.

The report repeats both labels on every metric and at report level. Only a
report collected by this harness on Windows with `live + measured` is marked
`acceptance_eligible`; that flag means “eligible for review,” not “accepted.”
Simulated and mocked measurements are useful test evidence. Planning estimates
are never benchmark evidence.

## Required metrics

Each report requires observations for all of these metrics and emits `p50`,
`p95`, and `p99` for each:

| Metric | Unit | Boundary |
| --- | --- | --- |
| `capture.latency_ms` | `ms` | Capture request to owned frame availability |
| `identity.latency_ms` | `ms` | Owned frame to identity-lock decision |
| `llm.ttft_ms` | `ms` | LLM submit to first token |
| `llm.output_tokens` | `tokens` | Delivered output-token count |
| `llm.tokens_per_second` | `tokens_per_second` | Post-first-token output throughput |
| `tts.ttfa_ms` | `ms` | TTS submit to first decoded playable audio |
| `tts.audio_completion_ms` | `ms` | TTS submit to final decoded audio chunk |
| `audio.submission_ms` | `ms` | Decoded audio ready to endpoint submission |
| `compositor.latency_ms` | `ms` | Compatible source frame to composed frame |
| `compositor.fps` | `frames_per_second` | Compositor output FPS over the sample window |
| `compositor.stale_drops` | `count` | Stale outputs discarded in the sample window |
| `end_to_end.turn_ms` | `ms` | Final user-input boundary to first submitted response audio |

Percentiles use deterministic R-7 linear interpolation:
`position = (sample_count - 1) * q`.

## Observation JSONL

One line is one numeric observation. All fields are required and no additional
fields are accepted:

```json
{"schema_version":"interactive-npcs-benchmark-sample/v1","sample_id":"turn-0001:capture","turn_id":"turn-0001","metric":"capture.latency_ms","value":8.25,"unit":"ms","observed_at_utc":"2026-08-30T12:00:00.000Z","execution_mode":"live","measurement_kind":"measured"}
```

Use the same `execution_mode` and `measurement_kind` throughout one input file.
Identifiers are bounded opaque IDs; do not place names, prompts, paths, or
provider content in them. The strict schema rejects unknown fields, including
credential-like fields, and errors never echo field values. Reports contain a
source SHA-256 and aggregate statistics, but no input path, sample IDs, raw
values, hostname, environment variables, or provider payloads.

## Windows usage

Run a deterministic harness check from repository root:

```powershell
.\scripts\benchmarks\run-bounded.ps1 `
  -Simulate `
  -OutputPath .\artifacts\benchmarks\simulation.json `
  -ReportId deterministic-simulation `
  -Iterations 100 `
  -TimeoutSeconds 30
```

If Python is pinned in a local virtual environment rather than available as
`py`/`python` on `PATH`, pass its exact executable with
`-PythonPath C:\path\to\venv\Scripts\python.exe`. The path is used only to
start the fixed harness command and is not written to the report.

Aggregate a live instrumented run:

```powershell
.\scripts\benchmarks\run-bounded.ps1 `
  -InputPath .\artifacts\benchmarks\live-samples.jsonl `
  -OutputPath .\artifacts\benchmarks\live-report.json `
  -ReportId this-pc-live-001 `
  -TimeoutSeconds 60 `
  -MaxRecords 100000 `
  -MaxInputBytes 16777216
```

The wrapper exposes no arbitrary producer command or credential argument. It
applies a Windows process timeout and terminates the fixed harness child if the
deadline expires. The Python process independently enforces the record, byte,
iteration, and elapsed-time bounds. The hard ceilings are 1,000,000 records,
256 MiB input, and 900 seconds.
If any required metric is absent, labels are mixed, or a bound is exceeded, no
valid report is produced.

Direct Python use is equivalent:

```powershell
py -3 .\scripts\benchmarks\benchmark_harness.py summarize `
  --input .\artifacts\benchmarks\live-samples.jsonl `
  --output .\artifacts\benchmarks\live-report.json `
  --report-id this-pc-live-001

py -3 .\scripts\benchmarks\benchmark_harness.py validate `
  --input .\artifacts\benchmarks\live-report.json
```

## Prepared MuseTalk hot-path qualification

`qualify-musetalk-realtime-path.py` is a separate, model-specific probe. It
does not feed the general benchmark report and it does not qualify an app route
or distributable model pack. It exists to distinguish MuseTalk's cached,
persistent neural path from the previously measured cold file/video batch path.

Run it only with the pinned Windows MuseTalk Python environment and explicit
runtime/source roots on `E:\temp`. The probe refuses an output directory outside
`E:\temp`, takes the shared GPU marker only when it contains exactly `no`, and
restores the original marker in a `finally` path. It downloads nothing.

```powershell
E:\temp\Alystria Studio\models\musetalk-runtime\venv\Scripts\python.exe `
  .\scripts\benchmarks\qualify-musetalk-realtime-path.py `
  --portrait .\scripts\synthetic-game-replay\assets\mara-venn-portrait-v1.png `
  --audio E:\temp\InteractiveNPCs\local-model-tests\musetalk-mara-v2\inputs\narration.wav `
  --runtime-root "E:\temp\Alystria Studio\models\musetalk-runtime" `
  --source-root "E:\temp\Alystria Studio\models\musetalk-source" `
  --output-root E:\temp\InteractiveNPCs\local-model-tests\musetalk-realtime-NEW `
  --face-box 698,158,1010,564 `
  --batch-sizes 1,4,8,20
```

The output report separates cold model load, one-time avatar preparation,
whole-clip audio feature extraction, first generated batch, neural/decode FPS,
CPU composite FPS, combined hot-path FPS, GPU telemetry, and exact hashes. A
fast result is still only a standalone probe; installed current-frame evidence,
game coexistence, cancellation, licensing and pack admission remain separate
gates.

The Mara face box above was recovered from the automatic DWPose enrollment
artifact. A guessed or stale face box can leave timing representative while
making the saved sample visually meaningless; always bind a cached box to the
exact portrait hash and identity revision.

## Character mouth-atlas architecture proof

`prototype-viseme-atlas.py` turns an enrollment teacher clip into 8–12 tiny
mouth states, a feather mask and CPU audio centroids. It writes a contact sheet,
visual-label upper-bound preview, audio-label prototype preview, motion board,
close-up comparison, compressed proof pack, portable premultiplied-BGRA binary,
versioned `atlas-manifest.json`, and measured JSON report. The manifest conforms
to `schemas/character-mouth-atlas-v1.schema.json`. The proof does not use the
GPU, download files, or claim current-game-frame/product admission.

```powershell
E:\temp\Alystria Studio\models\musetalk-runtime\venv\Scripts\python.exe `
  .\scripts\benchmarks\prototype-viseme-atlas.py `
  --portrait .\scripts\synthetic-game-replay\assets\mara-venn-portrait-v1.png `
  --teacher-video E:\temp\InteractiveNPCs\local-model-tests\musetalk-mara-v2\output\musetalk-mara-venn-v1.mp4 `
  --audio E:\temp\InteractiveNPCs\local-model-tests\musetalk-mara-v2\inputs\narration.wav `
  --face-box 698,158,1010,564 `
  --states 8 `
  --output-root E:\temp\InteractiveNPCs\local-model-tests\mara-atlas-NEW
```

The visual-label preview measures atlas quantization of the same teacher clip.
The audio-label preview trains and evaluates on that same short clip and is
therefore a mechanics proof, never cross-utterance accuracy evidence.

## Source-preserving moving-frame visual prototype

`render-dense-observed-lip-proof.py` is a headless inspection renderer for the
v53 source-preserving experiment. It keeps the moving source frame's face and
outer lips, continuously deforms the current mouth geometry, and admits a
one-time enrollment reference only inside the oral opening. It refuses inputs or
outputs outside `E:\temp`, downloads nothing, and does not launch the app.

The retained v53 artifact was produced with an explicit `oral-interior` mode:

```powershell
E:\temp\InteractiveNPCs\runtimes\mediapipe-landmarks-py\Scripts\python.exe `
  .\scripts\benchmarks\render-dense-observed-lip-proof.py `
  --source-frames E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1 `
  --source-landmarks E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1-mediapipe.json `
  --atlas-frames E:\temp\InteractiveNPCs\sources\mara-musetalk-teacher-frames-v1 `
  --atlas-landmarks E:\temp\InteractiveNPCs\sources\mara-musetalk-teacher-frames-v1-mediapipe.json `
  --single-open-image E:\temp\InteractiveNPCs\generated-enrollment\mara-imagegen-mouth-v1\mara-ah-open.png `
  --single-open-landmarks E:\temp\InteractiveNPCs\generated-enrollment\mara-imagegen-mouth-v1\mara-ah-open-mediapipe.json `
  --audio E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-magpie-fixture.wav `
  --output E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture-NEW `
  --source-first 1 `
  --source-count 40 `
  --fps 30 `
  --transfer-mode oral-interior `
  --video-name mara-jason-imagegen-natural-aperture-v53-NEW.mp4
```

The script writes PPM frames, an H.264/AAC review video, mouth contact sheets,
and `dense-observed-lip-proof.json`. The retained v53 manifest says
`rendered-not-qualified` and `rms-aperture-only`: its male Jason fixture has no
provider viseme events, so the result is an aperture, containment, and source-
preservation experiment rather than phoneme-accurate lip-sync evidence.

The v2 mouth-quality audit can additionally consume source/output landmarks and
the proof manifest. It checks residual shape, changed pixels outside the
expanded lip contour, requested/rendered aperture correlation, corner-width
drift, roll drift, output-landmark coverage, and—when applicable—discrete-state
shape popping. The retained report is:

`E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mouth-quality-audit-v2.json`

That audit passed, but it is not a perceptual metric and does not qualify an app
route. The Python renderer measured `54.907 ms` mean / `73.153 ms` p95 and is not
the native hot path. Separately, the latest native 1920×1080, 250-iteration CPU
run measured `6.402 ms` geometric mean, `6.323 ms` direct-atlas mean, and
`4.612 ms` mean / `4.682 ms` p50 / `7.004 ms` p95 / `7.264 ms` p99 for atlas
worker select-and-compose, with zero GPU VRAM; six native CTest suites passed.
Native generated-reference parity, desktop/live-game presentation, and installer
qualification remain open. See the [v6 proof report](../../docs/research/headless-realistic-lipsync-proof-2026-09-04-v6.md).

## Deterministic self-tests

```powershell
py -3 .\scripts\benchmarks\test_benchmark_harness.py
py -3 .\scripts\benchmarks\test_qualify_musetalk_realtime_path.py
E:\temp\Alystria Studio\models\musetalk-runtime\venv\Scripts\python.exe `
  .\scripts\benchmarks\test_prototype_viseme_atlas.py
```

The tests cover percentile math, all required outputs, seeded repeatability,
missing metrics, mixed classifications, secret-like fields, planning-estimate
eligibility, and byte/record bounds. They require only the Python standard
library and do not use the network.
