# Independent moving-character lip-sync audit — 2026-09-05

Snapshot notice: this report preserves the September 5 candidate comparison.
The later Cyberpunk/Misty v14 replay uses recorded native YuNet/LM1 mouth
geometry, 12-frame detector refresh with tracked-ROI reuse, and the schema-3
photometric compositor. It passed containment and fail-open replay checks, but
the open-mouth shape and lighting/reference seam remain visibly unqualified.
It is not installed-provider, live-game, natural-animation, or latency proof.
See [the reconciled review record](../product-rework/local-review-2026-09-05.md).

## Decision

Arbitrary-game moving-character lip-sync is **not qualified**. The current native residual renderer has the correct safety shape—newest captured frame, actor/track binding, mouth-only output, and fail-open bypass—but its visible articulation remains too synthetic. The strongest locally runnable neural editor, MuseTalk 1.5, is visibly more natural on the same moving-frame source, yet its warm batch-one fresh-frame path measured **429.725 ms p95** before the final source-preserving ellipse pass and **7,594 MiB peak device memory** on the 12,282 MiB RTX 4080 Laptop GPU. It cannot share the current machine with a modern game while meeting the product's 50 ms current-frame budget.

The product should keep lip rendering disabled for ordinary game targets. A new private multi-observation prototype shows a more credible rendering direction: it retrieves from 12 generated-teacher mouth observations, transforms only a mouth ROI, and preserves all pixels outside its zero mask byte-for-byte. On a distinct full Jason utterance, its CPU selection/warp/composite path measured **7.425 ms p95** at 960 × 720. A second run used a controlled synthetic blink/breath frame stream and measured **28.205 ms p95** for the Python ROI path. Its mouth anatomy is materially better than v66 at display size and enlargement.

That method has now been joined to the native worker for the fixed debug Mara target. The native compositor measured **1.735–3.946 ms p95** across the controlled runs, rendered all 55 held-out frames, and produced plausible lips without v66's dark triangular hole. Warm moving-frame OpenSeeFace measured **30.208 ms p95** in the least-contended run and **63.623–80.222 ms p95** in other runs; representative game coexistence is unproven. The broker also has no post-presentation pixel recapture. The private synthetic route is therefore suitable for local review with an explicit performance-open label; it is not a qualified arbitrary-game feature. EfficientSync remains the closest published architecture, but its code and weights are not released, so its reported speed is research evidence rather than a deployable component.

For games or test scenes that expose an actual facial rig, NVIDIA Audio2Face-3D is a separate and much stronger path: use its audio-driven blendshapes additively over the game's existing head/body animation. That is a rig integration, not a solution for arbitrary captured games.

## Evidence boundaries

- All execution was headless. No game, terminal, browser, audio player, or review UI was shown.
- AI inference used the required `gpu use.txt` lock and restored it to `no` after every run.
- Private local test weights were used only from their existing test runtime. No model was copied into the repository or review package.
- The original Mara source is an FFmpeg zoom/pan over one synthetic photorealistic portrait. Its decoded frames are pixel-distinct because of that transform and re-encoding; the actor does not move naturally. A later headless control adds deterministic blink and breathing before lip articulation while preserving the current source mouth. This tests controlled non-mouth motion preservation, not natural actor or gameplay motion. Neither source covers camera cuts, actor pose, real occlusion, facial hair, crowds, identity switches, or low-resolution distant faces.
- Enlarged mouth boards expose texture and seam failures that are subtle at 960 × 720 display size. Both views matter: display size catches gross identity drift; enlargement catches tooth bars, blur, contour discontinuities, and residual leakage.

## Audit of `main` / `v1.0.0`

The old implementation is an offline portrait-animation pipeline:

1. `functions/main.py` saves one screenshot, crops one face, and selects a character from that still image.
2. `functions/create_facial_animation.py` launches the vendored SadTalker Python environment with `--source_image`, `--driven_audio`, `--still`, `--preprocess full`, and `--enhancer gfpgan`.
3. The call blocks until a complete generated MP4 exists, moves that file into `temp/facial_animation.mp4`, and deletes SadTalker's result folder.
4. The README describes replacing game-face pixels with that generated video, but the implementation has no authoritative per-frame capture identity, no actor/track epoch, no current-frame lease, no audio playback sample clock, no occlusion gate, no newest-only queue, and no late-result rejection.

This design cannot preserve live source motion. SadTalker generates a new face video from a still crop and complete audio. It also couples dialogue latency to whole-clip generation and GFPGAN enhancement. Vendoring the full research repository and model bootstrap does not turn it into a bounded Windows sidecar or a causal game compositor. `main` and `v1.0.0` have the same relevant SadTalker tree and orchestration.

The old method should not be revived for 2.0. Its output can remain an historical/demo reference only.

## Audit of the 2.0 native path

The present architecture is materially safer than 1.0:

- `signal_adapter.cpp` maps typed OpenSeeFace evidence into a provider-neutral mouth contour, binds it to actor/track/frame identity, applies appearance/occlusion/pose/confidence gates, and rate-limits CPU tracking.
- `worker.cpp` keeps a newest-only work queue, rejects wrong/stale frames and expired leases, enforces hard pose and mouth-area ceilings, and emits a mouth-bounded premultiplied residual.
- `compositor.cpp` can deform source pixels rather than replacing the whole face.
- `windows_service.cpp` validates the D3D11 source lease, obtains admitted landmarks, retains a worker-owned residual texture, and returns a proposal for broker-controlled presentation.

The audit found four material gaps:

1. **Audio lineage was incomplete.** The worker accepted a timestamp near the frame but did not prove which sample interval or playback cursor drove the result. `first_sample_index` was not validated and the residual discarded the audio binding.
2. **The old atlas is not visually qualified.** Its preview contains inconsistent lip shapes, rectangular texture fields, and identity/color changes. The v66 render turns the mouth into a toothless dark wedge and damages the upper-lip contour. It has been replaced for the fixed synthetic review route rather than retained as a fallback.
3. **Signal quality is not speech quality.** The native fallback maps PCM energy to aperture. Energy can time opening/closing, but cannot distinguish bilabial closure, rounded vowels, spread vowels, dental/labiodental contact, tongue, or teeth exposure.
4. **The presenter proof remains bounded.** The debug path now leases an exact current WGC texture, renders through the worker, submits occlusion evidence and the residual to the broker, and receives a real presentation receipt. It does not recapture the final overlay pixels. Synthetic-target metadata helps exact selection, but it is not the runtime frame clock, and the fixed portrait identity cannot qualify ordinary game targets.

## Native correction implemented in this audit

`AudioClockBinding` now includes:

- `first_sample_index`
- `sample_count`
- `playback_sample_index`
- sample rate, channels, stream generation, segment id, and playback timestamp

The worker now fails open when the interval is empty, overflows `uint64`, the playback cursor is outside `[first, first + count)`, the playback timestamp is in the future, the stream generation differs from the track cancellation generation, or a PCM payload does not exactly match the declared frame count. The accepted binding is copied into `ResidualPatch` and transported in `ResidualProposalV1` schema 3. The mouth-worker wire protocol is version 2 so old clients reject the extended layout instead of misreading it.

This closes the audio correctness hole. Provider phoneme or viseme metadata is accepted only when it arrived before its bound playback interval; late Inworld alignment-only chunks and late Cartesia phones must fall back to the causal PCM window rather than delay video or retroactively animate an expired sound.

The identity-bound atlas compositor was also changed from v66's procedural outer-lip deformation plus oral-only texture insert to a tight full lip-and-oral texture transfer over the immutable current frame. Atlas alpha is the modification boundary; silence/full bilabial closure returns a zero-alpha current-source mouth. Per-track, per-segment selection holds a texture for two admitted frames and requires a meaningful distance improvement before changing state. The state resets on generation, atlas replacement, track discontinuity, segment discontinuity, or timing discontinuity.

Fresh build and test result:

```text
Build: E:\temp\InteractiveNPCs\native-builds\mouth-worker-v79-native-proof
CTest targets: 6/6 passed
Failures: 0
Latest Release test time: 0.65 s
```

The tests include future audio time, sample-interval overflow, cursor outside interval, PCM count mismatch, protocol schema rejection, proposal round-trip, a Windows D3D11 service smoke assertion that the exact admitted audio interval survives into the residual proposal, full-lip atlas alpha bounds, zero-alpha source preservation, and the two-frame state dwell/sustained-switch behavior.

## Visual comparison on the same moving source

### Existing v53: single generated oral reference

Artifact:

`E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\source-output-mouth-board.png`

v53 is the best previous source-preserving classical attempt. It retains most current-frame skin and uses a plausible teeth/cavity image. It remains unqualified because:

- one generated open-mouth reference supplies nearly every non-closed frame;
- the driver is explicitly `rms-aperture-only`;
- the oral interior looks transplanted under enlargement;
- the selected state jumps between only two topologies;
- its cached local remap/composite measured 73.153 ms p95, already outside the 50 ms budget before capture, tracking, and presentation.

### Existing v66: generated atlas through native compositor

Artifacts:

- `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology\mouth-board.png`
- `E:\temp\InteractiveNPCs\review-mouth-atlas-v6-oral-normalized\atlas-preview.png`

v66 is rejected. The board shows a dark toothless wedge/oval, flattened articulation, and upper-lip damage. Its own proof status is `failed`; maximum upper-lip darkening is 0.532. OpenSeeFace inference and the compositor are fast enough (26.505 ms and 1.756 ms p95 respectively), but the visual failure is decisive. A fast broken renderer is not a fallback.

### Independent classical modes

Six modes were rerun headlessly under:

`E:\temp\InteractiveNPCs\voice-lipsync-20260905\independent-moving-comparison-v1`

| Mode | Mechanical audit | Enlarged visual result | Decision |
|---|---:|---|---|
| `flow-delta` | pass | black painted lips, implausible geometry | reject |
| `observed-delta` | pass | black painted lips, little phonetic shape | reject |
| `rigid-observed-lip` | fail | large pasted mouth and identity drift | reject |
| `full-observed-lip` | pass | jagged seams, double contours, black gaps | reject |
| `dense-rms` | pass | blurred tooth/skin smear | reject |
| `dense-spectral` | fail | topology popping and residual leakage | reject |

This comparison proves the existing numeric audit is necessary but insufficient. Several grotesque results pass aperture-motion and outside-mask thresholds. Human visual review at display size and enlargement remains a release gate.

### Fresh moving-video MuseTalk 1.5 comparator

Artifacts:

- Video with muxed Jason audio: `E:\temp\InteractiveNPCs\local-model-tests\musetalk-moving-mara-20260905-v5\output\musetalk-moving-source.mp4`
- Enlarged board: `E:\temp\InteractiveNPCs\local-model-tests\musetalk-moving-mara-20260905-v5\review\source-output-enlarged-mouth-board.png`
- Container/model evidence: `E:\temp\InteractiveNPCs\local-model-tests\musetalk-moving-mara-20260905-v5\qualification.json`
- Frame audit: `E:\temp\InteractiveNPCs\local-model-tests\musetalk-moving-mara-20260905-v5\review\moving-video-audit.json`

This is a real inference run at pinned code revision `0a89dec45a0192b824e3cf4daf96c239440c5ed8`, not a reuse of the earlier static portrait result. The 25 fps source has 35 unique decoded frames. MuseTalk produced 32 video frames because muxing stopped at the shorter 1.254 s audio stream. Same-index source/output presentation timestamps match for those 32 frames. The output contains both H.264 video and mono AAC audio.

At normal display size this is the most natural tested result. It preserves the source zoom/pan while speech has recognizable rounded/open/closed shapes and teeth appear in plausible phases. Because the source actor is static, this does not prove preservation of natural head/body/facial motion. Enlargement still shows:

- softened lip and surrounding-skin detail;
- inconsistent lip thickness and asymmetric contours;
- a bright, flat teeth bar in some states;
- a weak transition between generated lip texture and original skin.

Median decoded pixel delta outside the 200 × 150 mouth review crop is 1.711/255 and outside-crop p95 is at most 5/255; most of this is whole-frame H.264 re-encoding. This is useful source-preservation evidence but not exact source-pixel identity.

## Warm fresh-frame performance

Artifact:

Core path:

`E:\temp\InteractiveNPCs\local-model-tests\musetalk-fresh-frame-mara-20260905-v2\qualification.json`

Core path plus the final source-preserving ellipse pass:

`E:\temp\InteractiveNPCs\local-model-tests\musetalk-fresh-frame-mara-20260905-v3\qualification.json`

The persistent FP16 model was warmed, then run batch-one over 32 fresh moving source frames. File output and video encoding were excluded. A trusted fixed face ROI isolates renderer cost from detection; whole-clip Whisper extraction is reported outside the frame loop.

| Stage | p50 | p95 |
|---|---:|---:|
| per-frame semantic mask preparation | 116.966 ms | 131.272 ms |
| current-frame VAE encode | 58.129 ms | 64.388 ms |
| positional audio conditioning | 0.202 ms | 0.337 ms |
| UNet batch-one | 142.231 ms | 163.683 ms |
| VAE decode | 38.010 ms | 46.140 ms |
| resize/upstream composite | 38.141 ms | 42.187 ms |
| **total before final source-preserving ellipse** | **390.425 ms** | **429.725 ms** |

Even removing mask preparation and final composite does not approach 50 ms: current-frame encode + UNet + decode sum to roughly 274 ms at their individual p95 values. Whole-clip Whisper feature extraction took 8.536 s and is not proven causal. Model load took 14.813 s. Peak device-total memory was 7,594 MiB of 12,282 MiB, leaving too little predictable headroom for a modern game and the rest of the product. A second run that applied the final source-preserving ellipse at display resolution measured 565.162 ms total p95; that CPU implementation is intentionally unoptimized, but removing it cannot rescue the 274 ms neural core.

MuseTalk's prepared-avatar benchmark is faster because it reuses one source latent. A moving game frame cannot reuse that latent without freezing or overwriting current source motion. Batch throughput also does not satisfy newest-frame latency: batching future frames adds delay and cannot preserve a current-frame contract.

MuseTalk is therefore a valuable offline teacher and visual comparator, not a qualified optional runtime pack.

## Multi-observation generated-teacher residual prototype

Implementation:

- `scripts/benchmarks/render-multi-observation-mouth-residual.py`
- `scripts/benchmarks/synthesize-nvidia-jason-heldout.py`

Primary held-out artifact:

- proof and per-frame audio bindings: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\proof.json`
- private 12-state atlas metadata: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\atlas-manifest.json`
- muxed output: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\mara-multi-observation-residual.mp4`
- same-closeup source/v66/prototype board: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\source-output-enlarged-mouth-board.png`
- display-size source/v66/prototype board: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\display-size-comparison-board.png`
- six consecutive frames around the densest state-transition window: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\temporal-state-transition-board.png`
- all enrolled mouth states: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v77-full-heldout-tight-roi\atlas-state-board.png`

The prototype uses the successful MuseTalk render only as a private offline teacher. Sharpness-weighted farthest-point sampling bounds the atlas to 12 states and records each source/teacher frame hash, geometry, acoustic descriptor, state class, and enrollment sample interval. Runtime selection reads the current and two preceding 40 ms audio intervals, the current source-frame pose/scale, and past selected state. A two-frame causal dwell/hysteresis rule reduces one-frame texture popping without future audio. It then similarity-transforms the chosen lip/oral texture into a tight ROI on the fresh source frame. The rest of the source frame stays byte-identical. Generated output never feeds back into the atlas or tracker.

The principal proof uses a newly synthesized stock `Magpie-Multilingual.EN-US.Jason` utterance distinct from the enrollment audio:

- held-out text length: 6 words; only its SHA-256 is stored in the report;
- held-out WAV SHA-256: `1a7cd40e6e13a227cf9aa3fa1abc6914a04509a3a530b755efb6de9fd59d7752`;
- provider result: HTTP 200, 22,050 Hz mono PCM, 1.811 s, non-silent, no clipped samples, no voice cloning;
- rendered sequence: 46 frames / 1.84 s at 25 fps, covering the complete 1.811 s utterance;
- source schedule: explicitly synthetic smooth ping-pong through the 35 finite zoom/pan frames; this extends duration but adds no actor motion;
- distinct held-out selection: 10 of the 12 enrolled states were used;
- source pixels outside the zero mask: byte-exact on every delivered frame;
- simulated occlusion and unsupported-pose probes: both returned the untouched source frame;
- mouth-ROI selection/affine warp/mask/composite: 5.453 ms median, 7.425 ms p95, 36.268 ms maximum on CPU at 960 × 720. This excludes capture, tracking, audio capture, PNG/video encoding, and broker presentation.

At display size, lip shapes and teeth are much more plausible than v66's black wedge. Enlargement shows coherent upper/lower lip edges across the reviewed frames. The remaining visible defects are softer-than-source lip detail, slightly oversized lip states, occasional bright/flat teeth, and a weak color/texture transition at some poses. This is the strongest local residual result, but it does not qualify deployment.

The same-audio visual control is separately stored under `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v73-same-audio-visual-control`. It permits time-matched comparison with v66 on the original Jason fixture and measured 9.696 ms p95. It is explicitly an enrollment replay control, not generalization evidence. The held-out v72 board compares anatomy and seam quality to v66; because the utterances differ, it is not a phonetic phase comparison.

The held-out run proves causal acoustic-state reuse across a different audio file. It does not prove correct phonemes. The five-class descriptor is hand-built from RMS, zero crossings, and spectral bands. The atlas board also reveals that teacher frames labeled from enrollment audio do not always have the expected visual topology: all three `silence` labels retain a visible opening or teeth. The file harness makes each selection from a bounded causal interval history but is not a live WASAPI stream. The source actor is still one portrait under synthetic motion, so pose transfer, natural head motion, real occlusion, tracking continuity, and game/GPU coexistence remain open.

### Controlled non-mouth motion proof

The independent test-game lane exported a contiguous pre-lip sequence at:

`E:\temp\InteractiveNPCs\moving-prelip-control-v1\sequence.json`

It contains 36 P6 frames at 960 × 600 and 15 fps over 2.4 seconds. The renderer adds controlled blink and breathing, then preserves each current source mouth exactly. The sequence has 28 distinct source indices, 36 distinct frame hashes, and per-frame transformed mouth geometry. It created no window and no audio.

The multi-observation prototype then rendered the complete held-out Jason utterance over this stream:

- proof: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v78-controlled-blink-breath-heldout\proof.json`
- muxed video: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v78-controlled-blink-breath-heldout\mara-multi-observation-residual.mp4`
- display board: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v78-controlled-blink-breath-heldout\display-size-comparison-board.png`
- state-transition board: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v78-controlled-blink-breath-heldout\temporal-state-transition-board.png`

The 28-frame output covers 1.867 seconds at 15 fps, uses six enrolled states across seven transitions, and retains all zero-mask pixels exactly. The CPU mouth ROI measured 5.748 ms median, 28.205 ms p95, and 46.629 ms maximum. Independent visual inspection confirmed that the blink, breathing, background, hair, and HUD survive; the mouth is plausible at display size and avoids the v66 hole. Enlarged frames still expose soft philtrum/lip texture and an occasional flat tooth bar.

### Native fixed-target bridge

Implementation and pack:

- `scripts/benchmarks/prepare-multi-observation-review-atlas.py`
- private pack: `E:\temp\InteractiveNPCs\review-mouth-atlas-v80-native-compatible`
- `atlas.json`: 3,991 bytes, SHA-256 `c4270c252f382502aa5218f5bb0c6b30b01f2db24988b0fde6b2f9d36ef65757`
- `atlas-bgra8-premultiplied.bin`: 1,413,984 bytes, SHA-256 `420e518d3a1552cdf6407a59e78f5d14225a2c461c624cac3049509a8bef5ad1`
- layout: 12 states, 206 × 143, stride 824, 117,832 bytes per state, identity revision `14018431763358334153`

The exporter checks the proof schema, source audio hash, every selected decoded teacher-frame hash, sample intervals, dimensions, and state count. It normalizes the teacher texture into the native canonical mouth basis, writes premultiplied BGRA with a tight feathered alpha, and addresses each state with coefficients derived from its exact enrollment PCM interval. The pack contains no model weights. It is allowed only beside a debug review executable as `review-mouth-atlas`, for the exact fixed Mara fixture identity; ordinary targets cannot select it.

The native worker rendered the held-out audio headlessly over the controlled source:

- native report: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v80-native-controlled-heldout\headless-proof.json`
- display sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v80-native-controlled-heldout\native-display-sequence-board.png`
- enlarged native/v66 texture comparison: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v81-native-controlled-heldout-cadence-correct\native-v66-texture-comparison-board.png`. Its v66 row is labeled as an archived anatomy comparison because it uses different audio and source motion; matching column times do not claim phonetic or same-input alignment.
- audio-muxed cadence-correct video: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v81-native-controlled-heldout-cadence-correct\native-controlled-heldout-audio-mux.mp4`, SHA-256 `3877ddb6b3e08adfd955fbe648f663fdadad33ec2fee2a99c15567f34375a531`

All 55 native output frames carried residuals and were visibly changed. The first controlled native run produced 55 distinct output hashes and measured 3.112 ms compositor p95; the cadence-correct duplicated-source run produced 41 distinct output hashes from 28 distinct controlled source positions. The renderer used zero GPU memory and retained the current moving frame as background. The visual board is a decisive improvement over v66: lips retain recognizable anatomy and real teeth/cavity texture instead of a dark triangular hole. Soft lip texture, a visible patch transition under enlargement, and bright/flat teeth in some states remain.

Tracking performance is load-sensitive and remains open. The first native report measured 31.983 ms warm static OpenSeeFace p95 and 63.623 ms on successive controlled frames, missing the proof's 40 ms duty-cycle gate at 15 Hz. A cadence-correct 30 fps derivative duplicates each 15 fps source frame once rather than interpolating it. One later warm run on that derivative measured 28.526 ms static p95, **30.208 ms moving p95**, and 1.735 ms compositor p95, satisfying the timing part of the revised full-lip mechanical rubric. A subsequent run during concurrent project work measured 91.672 ms static and 80.222 ms moving p95 and failed. The earlier harness also required a source-identical silence frame and prohibited any upper-lip replacement; those checks apply to the procedural oral-only renderer, not an identity-bound full-lip observation. The current harness records this mode explicitly and instead bounds total mouth delta and articulation extent while requiring separate visual review. None of these runs establishes representative game coexistence. The product debug presenter permits 220 ms for its review inference stage, so the path can be exercised locally, but it must carry a performance-open label.

The broker join is real: the debug route selects the exact executable/PID/HWND fixture, leases the latest exact WGC texture, runs the admitted landmark/worker path, submits occlusion evidence, submits the residual, and acknowledges the broker's presented result. The proof stops at that receipt. It does not recapture the final overlay for a second pixel comparison, and its metadata does not replace the runtime frame clock.

The exact native denial boundary remains conservative: face confidence below 0.70, landmark confidence below 0.82, semantic landmark confidence below 0.55, visibility below 0.72, mouth occlusion, yaw beyond ±35°, pitch beyond ±25°, roll beyond ±45°, oversized mouth bounds, stale/replaced frames, expired leases, actor/track/generation mismatch, and invalid/future/replayed audio intervals all produce no residual. The controlled proof exercises valid geometry; its separate Python probes exercise occlusion and unsupported pose. It does not contain a real hand/weapon occlusion sequence. The debug presentation code supplies fixed expected/observed appearance digests for the known Mara actor after exact fixture selection; it is not a measured generic-game face recognizer.

## Recommended generic-game renderer

The next implementation should be a causal source-texture residual with the following contract.

### 1. Exact audio drive

Prefer typed provider phoneme/viseme alignment bound to the WASAPI playback sample clock. Record whether each symbol is a provider viseme, IPA-like phone, ARPAbet-like phone, or local estimate; never relabel a raw phone as a viseme. Apply an alignment only when it arrived before its bound playback interval. Late alignment falls back to the causal PCM window without delaying playback or revisiting an old frame. When exact timely symbols are unavailable, use a causal phoneme/viseme model whose receptive field and algorithmic lookahead are declared and measured. Use energy only for speech activity and amplitude, never for teeth/tongue/topology selection.

Every request and proposal must carry `(stream_generation, segment_id, first_sample_index, sample_count, playback_sample_index, sample_rate, channels, playback_qpc)` plus actor/track/frame/device/geometry generations. Presentation rejects any mismatch and returns the untouched newest frame.

### 2. Multi-observation identity atlas

Enroll only untouched source frames that pass actor identity, sharpness, pose, visibility, occlusion, lighting, and temporal-consistency gates. Store the actual lip/oral texture with:

- yaw, pitch, roll and local affine basis;
- outer and inner mouth contours;
- width, aperture, roundedness and corner displacement;
- tooth exposure, cavity area and tongue visibility;
- local color/illumination and sharpness;
- capture frame identity and enrollment confidence.

Select 2–5 observations by pose and lighting first, then desired topology. Use sharpness-weighted farthest-point sampling to retain diverse states. Never fabricate an unobserved tooth/tongue state: reduce articulation toward the nearest qualified state or bypass.

### 3. Dense bounded deformation

Warp each retrieved observation into the desired current lip mesh. Maintain separate confidence for lip edge, teeth, cavity, and surrounding skin. Blend aligned high-frequency texture by confidence; do not average incompatible tooth/cavity states. Estimate flow only inside a bounded mouth support region and validate forward/backward consistency, strain, foldovers, and boundary displacement.

### 4. Independent current-frame background

The newest untouched WGC frame is always the background and the only tracking/enrollment input. Generated pixels never feed back into tracking or the atlas. Use a larger internal suppression region and a tighter adaptive output mask so the generator has context without leaking lower-face replacement. Composite only when the exact frame/track/audio/lease binding is still current.

### 5. Deterministic fail-open behavior

Bypass on actor/track mismatch, stale or replaced frame, expired lease, future/replayed audio interval, occlusion, pose limit, insufficient atlas coverage, inconsistent flow, color mismatch, GPU/resource pressure, or missed deadline. Bypass produces no residual and never freezes the previous mouth.

## Tracking and identity choices

- Keep actor identity independent from short-lived geometry tracks. Associate detections using motion, overlap, appearance, and track age; ByteTrack and BoT-SORT are suitable design references.
- Benchmark MediaPipe's dense face mesh against OpenSeeFace and 3DDFA-V2 on the actual game corpus. Dense geometry can improve lip boundaries; it does not replace actor identity.
- Use NVIDIA Optical Flow SDK on the existing D3D11 path where available, with OpenCV DIS as a CPU fallback. Forward/backward flow disagreement should suppress the residual.
- Do not ship public InsightFace model-zoo weights as a commercial identity dependency. Their model licensing is not the same as the code license. The existing private SFace/identity tests remain qualification evidence only.

## Rigged-character path

NVIDIA Audio2Face-3D is the strongest released route for an actual rigged character. The SDK is open source, supports Windows, streams audio to facial blendshapes, and NVIDIA recommends roughly 4 GB GPU memory. It can preserve locomotion and head motion because the game combines the facial coefficients with its own animation graph.

A valid synthetic test game for this route must:

1. render a live rigged character with independent head/body/camera motion;
2. apply A2F3D jaw/lip/tongue/eye coefficients additively inside the renderer;
3. expose deterministic actor/turn/audio/frame telemetry;
4. include occlusion, pose, lighting, distance, onset/offset, plosives, rounded vowels, teeth, and speaker switching;
5. prove WGC capture and broker selection against the rendered moving result.

The current `interactive-npcs-synthetic-target.exe` replay fixture has a fixed portrait identity and declares `source_mouth_motion=false`. It can qualify launch, process/window selection, WGC capture, and presentation plumbing. It cannot qualify A2F3D, moving lips, or arbitrary-game support.

## Current alternatives and primary-source ledger

The following primary papers, official repositories, model cards, and vendor documentation were opened and compared. Speed claims are the authors' measurements on their hardware unless a local measurement is explicitly stated above.

| Family | Primary sources opened | Relevance and decision |
|---|---|---|
| EfficientSync | [paper](https://arxiv.org/abs/2608.18832) | Closest source-preserving architecture: five sharp/topologically diverse observations, deformation, dynamic texture mixing, adaptive mask; reports 166 fps/718 MB on A100. No code, weights, license, Windows, causal-audio, or occlusion proof. Architecture reference only. |
| FlashLips | [paper](https://arxiv.org/abs/2512.20033) | One-step editor and compact lip-pose control; reports 109 fps on H100. Audio transformer is not documented as causal; no code/weights. Future pack reference. |
| Lip Forcing | [paper](https://arxiv.org/abs/2606.11180), [repository](https://github.com/cvlab-kaist/LipForcing) | 1.3B model reports 31 fps, but its checkpoint is not released; released 14B path targets H200-class hardware. Not runnable here. |
| MuseTalk 1.5 | [repository](https://github.com/TMElyralab/MuseTalk), [paper](https://arxiv.org/abs/2410.10122) | Best runnable Windows comparator. Official docs acknowledge jitter and lip/facial-hair/color limitations. Locally failed latency/VRAM gates on moving current frames. |
| LatentSync | [repository](https://github.com/bytedance/LatentSync), [paper](https://arxiv.org/abs/2412.09262) | Strong offline teacher; multi-step diffusion and 8–18 GB documented memory tiers. Not causal/current-frame. |
| KeySync | [repository](https://github.com/antonibigata/keysync), [paper](https://arxiv.org/abs/2505.00497), [model card](https://huggingface.co/toninio19/keysync) | Whole-clip keyframe generation/interpolation; offline teacher, not game runtime. |
| Wav2Lip | [repository](https://github.com/Rudrabha/Wav2Lip), [paper](https://arxiv.org/abs/2008.10010) | Historical baseline; public weights are noncommercial and earlier local visual proof failed source preservation. Excluded. |
| VideoReTalking | [repository](https://github.com/OpenTalker/video-retalking), [paper](https://arxiv.org/abs/2211.14758) | Whole-video editing pipeline with identity/expression dependencies; offline and unsuitable for newest-frame composition. |
| IPTalker | [paper](https://arxiv.org/abs/2501.04586), [project](https://alunaticat.github.io/IPTalker/) | Identity-preserving research reference; no qualified causal Windows game path. |
| RASA | [paper](https://arxiv.org/abs/2503.11571) | Recent robust audio-driven lip-sync research; no released qualified low-latency path for this product. |
| Ditto | [repository](https://github.com/antgroup/ditto-talkinghead), [paper](https://arxiv.org/abs/2411.19509) | Streaming portrait generator with 0.4 s audio units and reported 385 ms first-frame latency on A100. Replaces the source portrait. |
| AVTR-1 | [repository](https://github.com/avaturn-live/avtr-1), [model card](https://huggingface.co/avaturn-live/avtr-1) | Five-frame/200 ms chunks, Linux-first, mixed component licensing; full portrait generation. |
| LivePortrait | [repository](https://github.com/KlingAIResearch/LivePortrait), [paper](https://arxiv.org/abs/2407.03168), [model card](https://huggingface.co/KlingTeam/LivePortrait) | Useful deformation reference, but no audio driver; bundled InsightFace model terms block the intended product path. |
| NVIDIA Audio2Face-3D | [collection](https://github.com/NVIDIA/Audio2Face-3D), [SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK), [paper](https://arxiv.org/abs/2508.16401), [model card](https://huggingface.co/nvidia/Audio2Face-3D-v3.0), [release article](https://developer.nvidia.com/blog/nvidia-open-sources-audio2face-animation-model/) | Best released rig controller. Windows/on-device streaming; SDK MIT, weights under NVIDIA Open Model License. Requires a facial rig and does not edit arbitrary captured pixels. |
| A2F3D service/NIM | [architecture](https://archive.docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/architecture/audio2face-ms.html), [performance](https://archive.docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/architecture/performance.html), [support matrix](https://archive.docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/architecture/support-matrix.html), [getting started](https://archive.docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/getting-started.html) | Linux container/service option with higher deployment and memory cost. The open Windows SDK is a better fit if the rig path is pursued. |
| NVIDIA Unreal integration | [plugin documentation](https://docs.nvidia.com/ace/ace-unreal-plugin/latest/ace-unreal-plugin-audio2face.html) | Supports local facial animation for rigged Unreal characters. Useful for a separate representative test game. |
| NVIDIA Audio2Face-2D | [overview](https://docs.nvidia.com/nim/maxine/audio2face-2d/latest/overview.html), [basic inference](https://docs.nvidia.com/nim/maxine/audio2face-2d/1.0.0/basic-inference.html), [release notes](https://archive.docs.nvidia.com/ace/audio2face-2d-microservice/latest/release-notes.html) | Portrait + audio to generated video. No documented moving-video/current-source residual contract. Rejected for arbitrary captured games. |
| NVIDIA Maxine AR SDK LipSync | [processing contract](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/using-ar-features.html), [properties](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/properties.html), [performance](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/PerformanceReference.html), [Windows installation](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/InstalltheARSDK.html) | Closest released direct-video API, but default `NumInitialFrames=14` implies about 467 ms lookahead at 30 fps. Requires visible/unoccluded face, good lighting, and about ±30° pose. Comparator spike only. |
| ARTalk | [repository](https://github.com/xg-chu/ARTalk), [paper](https://arxiv.org/abs/2502.20323) | Audio-to-3D facial motion research; relevant to rigged path, not arbitrary pixels. |
| ByteTrack | [paper](https://arxiv.org/abs/2110.06864), [repository](https://github.com/ifzhang/ByteTrack) | Strong association baseline; design reference for retaining low-score detections through brief motion/occlusion. |
| BoT-SORT | [paper](https://arxiv.org/abs/2206.14651), [repository](https://github.com/NirAharon/BoT-SORT) | Combines motion, camera compensation, and appearance. Design reference for actor/geometry separation. |
| MediaPipe Attention Mesh | [paper](https://arxiv.org/abs/2006.10962), [Face Landmarker](https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker), [repository](https://github.com/google-ai-edge/mediapipe) | Dense lip/iris geometry candidate; benchmark before replacing OpenSeeFace. |
| 3DDFA-V2 | [paper](https://arxiv.org/abs/2009.09960), [repository](https://github.com/cleardusk/3DDFA_V2) | Pose-robust 3D alignment candidate for difficult game angles; benchmark/licensing review required. |
| OpenSeeFace | [repository](https://github.com/emilianavt/OpenSeeFace) | Current tracker family; fast CPU path already measured, but 66-point geometry limits inner-mouth detail. |
| NVIDIA Optical Flow SDK | [documentation](https://docs.nvidia.com/video-technologies/optical-flow-sdk/), [download](https://developer.nvidia.com/opticalflow-sdk) | D3D11-compatible dense flow option for mouth-local deformation and consistency gating. |
| DIS optical flow | [paper](https://arxiv.org/abs/1603.03590) | Practical CPU fallback for bounded local flow. |
| SEA-RAFT | [paper](https://arxiv.org/abs/2405.14793), [repository](https://github.com/princeton-vl/SEA-RAFT) | High-quality flow research comparator; likely too heavy for shared real-time game GPU without a dedicated profile. |
| UnFlow | [paper](https://arxiv.org/abs/1711.07837) | Forward/backward consistency reference for rejecting unreliable unsupervised flow. |
| SAM 2 | [paper](https://arxiv.org/abs/2408.00714), [repository](https://github.com/facebookresearch/sam2) | Temporal segmentation research reference; too broad/heavy for the default mouth mask. |
| Rhubarb Lip Sync | [repository](https://github.com/DanielSWolf/rhubarb-lip-sync) | Deterministic phonetic cue baseline for offline/known text audio; useful comparator, not sufficient visual renderer. |
| Inworld TTS timestamp alignment | [official documentation](https://docs.inworld.ai/tts/capabilities/timestamps) | Realtime TTS-2 can return phoneme timing plus 10 documented viseme symbols. Stronger topology address than the heuristic five-class PCM descriptor, but the local async adapter can receive alignment-only data after the last PCM chunk. Arrival relative to the playback cursor must be measured; late symbols cannot drive an expired frame. |
| InsightFace | [repository/license notice](https://github.com/deepinsight/insightface), [SFace licensing issue](https://github.com/deepinsight/insightface/issues/2022) | Code/model licensing differs. Do not bundle public model-zoo weights for commercial identity. |
| Windows capture/audio clocks | [Windows Graphics Capture](https://learn.microsoft.com/windows/uwp/audio-video-camera/screen-capture), [`IAudioClock2::GetDevicePosition`](https://learn.microsoft.com/windows/win32/api/audioclient/nf-audioclient-iaudioclock2-getdeviceposition) | Authoritative basis for exact captured-frame/QPC and playback-device sample association. |
| Full portrait generation | [Hallo2 repository](https://github.com/fudan-generative-vision/hallo2), [Hallo2 paper](https://arxiv.org/abs/2410.07718), [Hallo3 repository](https://github.com/fudan-generative-vision/hallo3), [READ project](https://readportrait.github.io/READ/), [READ paper](https://arxiv.org/abs/2508.03457), [REST](https://arxiv.org/abs/2512.11229), [Livatar-1](https://arxiv.org/abs/2507.18649), [Microsoft streamable portrait work](https://www.microsoft.com/en-us/research/publication/real-time-generation-of-streamable-talking-portrait-video-with-reference-guided-deep-compression-vaes/) | Valuable avatar research, but these methods generate/re-enact a portrait and do not preserve arbitrary moving game pixels. |

## Remaining qualification gates

No renderer should be enabled until one candidate passes all of these on a representative headless test game and captured real game footage:

- multiple actors, actor exit/re-entry, speaker switching, and camera cuts;
- yaw/pitch/roll, distance, motion blur, lighting shifts, facial hair, teeth/tongue, and hand/weapon/UI occlusion;
- source motion and outside-mask source-pixel preservation;
- exact WGC device/geometry/frame identity through presentation;
- exact WASAPI/QPC sample interval through presentation;
- p95 capture-to-composite at or below 50 ms under simultaneous representative game GPU load;
- no stale residual, previous-mouth freeze, feedback into tracking, or unbounded queue;
- blind visual preference at display size plus enlarged seam/texture review;
- clean packaged-app proof with every shipped model's code/weight/data license recorded.

The current evidence supports the safety architecture, rejects v66, and supports the new 12-state pack only for the exact private synthetic Mara review route with its performance-open label. It does not support an “arbitrary game solved” claim.
