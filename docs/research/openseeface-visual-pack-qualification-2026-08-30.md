# OpenSeeFace visual signal pack qualification

Date: 2026-08-30  
Decision: **current-device candidate passed; release trust and product activation still blocked**  
Manifest: [`openseeface-visual-pack-manifest-2026-08-30.json`](openseeface-visual-pack-manifest-2026-08-30.json)

## Decision

The small OpenSeeFace MNV3 detector plus `lm_model1_opt.onnx` is suitable for an explicitly selected, CPU-only experimental pack that supplies current-frame face and mouth landmarks to the existing residual-warp pipeline. It is not a complete lip-sync model, does not generate pixels, and cannot identify an NPC. It must never be presented as an identity recognizer or a talking-head generator.

The selected payload is 5,411,995 bytes (5.161 MiB), including the upstream BSD-2-Clause license and excluding the shared ONNX Runtime. On the Eclipse Harbor replay it processed 23.316 frames per second using approximately one logical CPU core, no GPU, and an 88.945 MiB peak RSS increase. The steady landmark p95 was 52.159 ms, which is compatible with a capped 15 Hz signal worker. Detector reacquisition pushed detector-plus-landmark p95 to 84.446 ms, so queue depth must be one and any late result must be discarded in favor of the untouched current frame.

This is deliberately not a default pack. Promotion beyond experimental requires live-game evidence across camera motion, head pose, scale, lighting, skin tones, facial hair, helmets, partial faces, HUD occlusion, scene transitions, and multiple simultaneous characters.

### Current-device candidate rerun

The strict v2 current-device rerun used the exact pinned MNV3 and LM1 artifacts,
ONNX Runtime 1.22.1 CPU EP, one inference thread, and direct byte decoding of the
same task-owned Eclipse Harbor video. It produced 245 exact 66-point packets
across 453 frames and recorded the following independent sample counts and p99s:

| Lifecycle/operation | Samples | p99 |
| --- | ---: | ---: |
| Cold model load | 20 | 102.327 ms |
| In-process reload | 20 | 147.527 ms |
| Exact packet operation | 245 | 54.050 ms |
| Packet frame age | 245 | 54.051 ms |
| Cancel after worker/model readiness | 20 | 3.412 ms |
| Session unload | 40 | 30.719 ms |

Resident RAM increased by 35,086,336 bytes and the measured total-process delta
peaked at 125,935,616 bytes. Resident and transient VRAM deltas were both zero.
The separate 15 Hz inference-process versus fixture-decode contention proxy lost
11.58 percent throughput; it is explicitly not a live-game FPS certification.

All packet coordinates were finite, actor/frame/device/geometry/QPC binding and
stale fail-open tests passed, and 28 low-confidence mouth packets were marked
occluded rather than promoted. Rendered evidence was visually inspected. The
large-face four-panel proof shows source, exact mask, bounded reference composite,
and 8x difference; 333 pixels changed inside the mask and zero outside it.

The report is
`out/qualification/openseeface-mnv3-lm1/evidence/openseeface-current-device-qualification.json`
(SHA-256 `e8ee03e11f8ca55c5e588e393c4668c188c6896722ac4e7bfb382dd9aa2435c8`).
The candidate envelope SHA-256 is
`b74876101b2ffddcedc201a24f5b62f6f0bfd62798aa138db0a64a42714f4b9c`.
Its Ed25519 key is intentionally ephemeral and not in the release trust root, so
this evidence cannot authorize activation. Whole-loadout admission, live-game
coverage, a native pack inference producer, and broker-authoritative live audio
timing remain required.

The later legal correction changed only optional-runtime/ThirdPartyNotices and
blocked-admission governance metadata. It did not change model bytes, the exact
ORT 1.22.1 archive, ABI, CPU backend, one-thread execution, self-test, resource
placement, fixture, report, or rendered evidence. The non-inference derivation
record is
`out/qualification/openseeface-mnv3-lm1/evidence/openseeface-final-manifest-no-inference-derivation.candidate.json`
(SHA-256 `34a2c799110642d4493bc64e5efa355051caeb10866ff76f5dfd3c729fe431ae`).
It binds final raw/canonical/normalized manifest digests to the authoritative
privacy-preserving `npc-system-telemetry` device fingerprint and copies all
measurement values without recomputation. Its signature is also ephemeral and
non-authoritative. Release activation remains false until the resource governor
independently verifies this derivation and issues a threshold-signed envelope.

The user-reported Transparency App dimmers on DISPLAY1/DISPLAY5 were not stopped,
moved, focused, or reconfigured. Because qualification decoded the fixture file
directly, desktop/perceived brightness and screenshot color values were not used
as model-input truth. Exact game-HWND WGC evidence remains a separate native
product proof.

The authenticated capture-evidence v2 product seam separately names the pixel
source and scope. Only `windowsGraphicsCaptureTexture` plus
`exactSelectedWindow`, with external display-overlay pixels and desktop
luminance explicitly excluded, may satisfy exact-HWND proof. Desktop Duplication
or a full-display screenshot cannot be promoted to exact game-window evidence
from matching luminance or appearance.

## Pinned, redistributable payload

The upstream is pinned to OpenSeeFace commit `85aa70fc67582d046e771ea73625182a0d8f7475` (`v1.20.4-63-g85aa70f`, committed 2025-12-28). The [repository](https://github.com/emilianavt/OpenSeeFace) and the [license at the pinned revision](https://github.com/emilianavt/OpenSeeFace/blob/85aa70fc67582d046e771ea73625182a0d8f7475/LICENSE) explicitly cover the code and models under BSD-2-Clause.

| File | Purpose | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `models/mnv3_detection_opt.onnx` | 224x224 face detector | 568,302 | `0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809` |
| `models/lm_model1_opt.onnx` | 66-point face landmark model | 4,842,329 | `5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f` |
| `LICENSE` | BSD-2-Clause redistribution notice | 1,364 | `28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146` |

The tested separately acquired optional runtime was ONNX Runtime 1.22.1, CPU
execution provider, one inference thread. Its project license hash was
`2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c`;
the official archive also carries the pinned `ThirdPartyNotices.txt` and its
bundled third-party terms. Runtime bytes are not included in the 5.161 MiB model
payload figure or the base installer, but the strict install manifest accounts
for the exact optional archive and extracted runtime inventory.

No upstream Python entrypoint or pickle checkpoint was executed. The pinned ONNX
graphs were loaded only by the project-owned headless qualifier after immutable
size/SHA verification. A separate parser first ran full ONNX checking and strict
shape inference, enforced node/initializer/size bounds, rejected external tensor
data, sparse initializers, control flow, unknown domains, and unknown operators,
then admitted only the audited `com.microsoft::FusedConv` contribution. The
detector graph is `[batch, 3, 224, 224] -> [batch, 2, 56, 56]`; the landmark
graph is `[batch, 3, 224, 224] -> [batch, 198, 28, 28]`.

## Eclipse Harbor replay results

The replay was `docs/assets/demo/demo.mp4`, SHA-256 `a27480c3cfeca36b72f3a2b8baff204fbe04e23d7ccd3a762f719d0da350a3a3`: 960x600, 453 frames, 15 fps, 30.2 seconds, and silent. The evaluation annotation used the known synthetic avatar region only to score the run; it is not proposed product logic.

Test host: Windows 11 build 26200, Intel Core i9-13980HX, 24 physical / 32 logical cores, 32,386.6 MiB system RAM. The process used ONNX Runtime's CPU provider with one inference thread.

| Measurement | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Detector invocation | 11.879 ms | 37.406 ms | 38.789 ms |
| Landmark on detected frame | 47.048 ms | 52.159 ms | 56.567 ms |
| Detector + landmark on detected frame | 47.726 ms | 84.446 ms | 87.738 ms |
| Full frame processing | 48.498 ms | 86.210 ms | 92.425 ms |
| Mouth ROI center delta / face diagonal | 0.0057 | 0.0145 | 0.0173 |
| Mouth ROI fractional area delta | 0.0272 | 0.4045 | 0.5396 |

Other resource and tracking results:

- Load time: 71.22 ms.
- Sustained replay throughput including load and CPU decode: 23.316 fps.
- CPU: 104.466% of one logical core; peak RSS delta: 88.945 MiB.
- GPU/VRAM: not used; dedicated and transient VRAM attributable to this CPU path are 0 MiB.
- Detector invocations: 239 across 453 frames because the worker reused a tracked face ROI between guarded reacquisitions.
- Track IDs created: 3; switches: 2 across initial acquisition, modal occlusion, and scene exit. The selected visible actor retained one track during the unoccluded run. This is tracking continuity, not identity recognition.
- The known target was unoccluded for 214 frames; 214/214 produced eligible guarded landmarks. Across all 243 target-visible frames, including the blocking modal, eligible recall was 88.066%.
- Five raw detector hits after the target left were rejected by the appearance gate, producing zero eligible false-positive mouth outputs.

The large p95 and p99 ROI area deltas are not cosmetic noise that can be ignored. They combine stylized mouth motion with landmark-shape variation and require temporal smoothing plus a conservative dynamic mask bounded by current landmarks.

### Visual inspection and the required guard

Rendered overlays were inspected at pre-character, character, modal, and post-character frames. At frames 225, 300, and 375, the mouth landmarks and residual ROI aligned with Mara's stylized mouth. Pre-character frame 150 and post-character frame 435 remained untouched.

An unguarded sticky tracker failed dangerously: at modal frames 400 and 408 it confidently placed mouth landmarks on the `CLOSE SIMULATION` button. The guarded replay kept detector/landmark diagnostic labels but emitted no red mouth ROI and no eligible mouth output during that interval. Therefore the temporal appearance/occlusion latch is a mandatory activation gate. A confidence threshold alone is insufficient.

## Comparator result

The official OpenSeeFace RetinaFace graph was statically validated but rejected for the game-first CPU path. It measured 4.732 sustained fps; detector p50/p95/p99 was 182.677/212.406/253.425 ms and combined detector-plus-landmark p50/p95/p99 was 227.106/240.599/296.991 ms.

The smaller `lm_model0_opt.onnx` combination reached 17.353 fps but had lower landmark confidence and visibly less stable landmark placement. Running MNV3 plus model 1 naively on every frame reached 15.280 fps with a 96.576 ms combined p95. Sticky ROI scheduling made model 1 the better qualified compromise without adding GPU contention.

MediaPipe remains a technically current Apache-2.0 alternative, but its separately shipped model-asset redistribution provenance is less explicit than OpenSeeFace's license statement. Open Model Zoo's 98-point landmark option is much larger and still needs a detector; the repository also describes itself as maintenance-mode. For this narrowly scoped, redistributable, lightweight CPU pack, neither displaced the measured OpenSeeFace pair. Primary references: [MediaPipe](https://github.com/google-ai-edge/mediapipe), [MediaPipe Face Mesh](https://github.com/google-ai-edge/mediapipe/blob/master/docs/solutions/face_mesh.md), [Open Model Zoo landmark model](https://github.com/openvinotoolkit/open_model_zoo/blob/master/models/intel/facial-landmarks-98-detection-0001/model.yml), and [Open Model Zoo status](https://github.com/openvinotoolkit/open_model_zoo).

## Causal audio-to-viseme signal

The candidate does not bundle a learned viseme model. The measured fallback is a project-authored, causal 5-cue analysis stage (`closed`, `round`, `open`, `fricative`, `wide`) inspired by the MFCC-profile approach documented by [uLipSync](https://github.com/hecomi/uLipSync) at pinned research revision `060587907655235dba86dc7052d0b395c8ecd840` (MIT). No uLipSync code was bundled or executed.

On the existing 24 kHz mono, 1.579-second ElevenLabs fixture, the stage used a 20 ms lookback window and 10 ms hop with zero future-sample lookahead. Compute p50/p95/p99 was 0.274/0.334/0.494 ms per hop. The raw benchmark field named `algorithmic_lookahead_ms` is semantically the 20 ms of accumulated past/current audio, not 20 ms of future audio.

This probe establishes compute cost only. The clip has no phoneme labels, and the visual replay has no audio, so it establishes neither viseme accuracy nor audiovisual sync. Provider-timed visemes should win when available: [Azure Speech visemes](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/how-to-speech-synthesis-viseme) and [Amazon Polly speech marks](https://docs.aws.amazon.com/polly/latest/dg/viseme.html) expose explicit timing. ElevenLabs streaming character alignment can help schedule text but is not phoneme truth; the audio heuristic remains the fallback and needs per-voice calibration. Relevant references are [ElevenLabs real-time TTS](https://elevenlabs.io/docs/eleven-api/guides/how-to/websockets/realtime-tts), [forced alignment](https://elevenlabs.io/docs/overview/capabilities/forced-alignment), and [latency guidance](https://elevenlabs.io/docs/eleven-api/guides/how-to/best-practices/latency-optimization).

## Required activation contract

The pack may be wired into the existing `tracked-mouth-warp` option only when all of the following are enforced:

1. It is an explicit user download and selection, never an automatic/default dependency.
2. Every file matches the pinned hash and the BSD-2-Clause notice is installed with it.
3. Static ONNX validation completes before any inference session is created.
4. The default execution contract is CPU provider, one thread, maximum 15 Hz.
5. The product runtime supplies and verifies actor lock, track ID, captured frame ID, and scene epoch; the model supplies no identity.
6. Queue depth is one. Late, stale, superseded, cancelled, or scene-mismatched results are dropped and the untouched current frame is shown.
7. The temporal appearance/occlusion latch suppresses all mouth output on rejection.
8. The residual mask is dynamic, conservative, smoothed, and contained by current landmarks.
9. Provider-timed visemes are preferred. The five-cue causal audio signal is an explicitly lower-quality fallback.
10. Status remains experimental until broader live-game and audible audiovisual tests pass.

## What this evidence does not prove

- It does not prove acceptable appearance in arbitrary games or photoreal faces.
- It does not prove real-game frame-rate impact. The synthetic replay used CPU software decoding rather than a running game; the measured CPU cost is a proxy, not a game-FPS guarantee.
- It does not prove 30 or 60 Hz suitability. The admitted contract is capped at 15 Hz with late-result dropping.
- It does not prove identity stability across multiple similar faces. Identity recognition is absent.
- It does not prove lip-reading, phoneme accuracy, audio/video onset, or audiovisual skew because the replay is silent.
- It does not qualify full-frame talking-head generation. Only current-frame landmarks and a residual-mouth signal are in scope.

## Evidence integrity

The earlier guarded benchmark remains under the gitignored
`artifacts/visual-pack-qualification/` directory. The strict v2 current-device
runner is `scripts/benchmarks/qualify-openseeface-visual-pack.py`; its task-local
runtime, exact pack, report, raw packets, candidate envelope, and rendered proofs
remain under `out/qualification/openseeface-mnv3-lm1/`. The earlier three retained
report digests are:

- Static ONNX validation: `859cecb2ca18fc252b8e9601949d0be73a3700415287abcbf87e94c601f44c91`.
- Comparator plus causal-audio benchmark: `41efeb8f773dc277e0790c995252d3fe41a1a031d6b6dd3cd8bdb3bdfdb0b3ec`.
- Guarded sticky benchmark: `7bbd78e9c2aae7b760fb6258878437009cf6f31c7bf13df34ae5867f6be8cf97`.

No API credentials were read or needed. No AI-model GPU run occurred, the coordination file was not modified, and no untrusted Python or checkpoint was executed.
