# Generic captured-game lip-sync qualification

Status: research decision and qualification plan, not a release claim

Evidence refreshed: 2026-08-30 UTC

Target: an external Windows game companion running beside a game on a 12 GB GPU

## Decision

There is no honestly qualified neural lip-sync pack today that simultaneously
supports arbitrary moving game faces captured from the framebuffer, exact
current-frame anchoring, one-refresh cancellation, a clean Windows install,
redistributable artifacts, and reliable operation beside a game on a 12 GB GPU.

The only shippable direction is the project-owned, CPU-first current-frame mouth
warp. It remains an **engineering baseline** until live-game visual and
contention gates pass. The safe product default is therefore the untouched game
frame with audio and subtitles. After qualification, the recommended optional
pack can be the conservative current-frame warp; installation and activation
must remain explicit.

The native capture-to-worker-to-presentation path and authenticated product
caller now exist and have an exact WGC/D3D synthetic-target proof. This is not
proof of arbitrary-game compatibility. A real current-device OpenSeeFace CPU run
now passes the candidate measurement and rendered-evidence gates, but its
ephemeral signature is deliberately not release-trusted. The ordinary
selected-turn route also lacks a broker-authoritative live audio envelope and a
pack-admitted native inference producer. Until those independent gates land,
product activation remains unavailable and the exact behavior is the untouched
game frame with audio and subtitles.

## Honest default and experimental boundary

- `npc-causal-viseme-mouth-warp-v1` is the existing project-owned candidate. If
  the optical-flow or coefficient work changes its ABI, publish a v2 candidate
  rather than changing v1 silently.
- The pack is a conservative motion effect, not neural face generation, lip
  reading, character identification, or a promise to reconstruct unseen teeth
  and tongue.
- OpenSeeFace is an optional current-frame signal dependency, not complete
  lip-sync and not an identity recognizer.
- No pack is bundled or installed silently. A user explicitly downloads and
  selects a qualified revision.
- A late, stale, occluded, cancelled, mismatched, over-budget, or uncertain job
  yields the untouched current frame within one refresh.
- Until live-game qualification succeeds, the UI must label the pack
  Experimental and must not present it as the default working state.

## Exact OpenSeeFace signal artifacts

The selected signal pack is `openseeface-mnv3-lm1-mouth-signal`, pinned to
OpenSeeFace commit `85aa70fc67582d046e771ea73625182a0d8f7475`. Its explicit-download
payload is 5,411,995 bytes. The shared ONNX Runtime is excluded from that size.

| Artifact | Purpose | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `models/mnv3_detection_opt.onnx` | 224x224 face detector | 568,302 | `0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809` |
| `models/lm_model1_opt.onnx` | 66-point landmark model | 4,842,329 | `5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f` |
| `LICENSE` | BSD-2-Clause notice | 1,364 | `28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146` |

The tested signal runtime was ONNX Runtime 1.22.1, CPU execution provider, one
inference thread. OpenSeeFace code and models are BSD-2-Clause at the pinned
revision; ONNX Runtime carries its own MIT notice. The project-owned worker is
MIT. If OpenCV is used rather than a project-owned optical-flow implementation,
its exact runtime files and Apache-2.0 notices become additional pack artifacts.

Existing Eclipse Harbor replay evidence measured 23.316 processed frames per
second, about one logical CPU core, an 88.945 MiB peak RSS increase, no model
VRAM, landmark p95 of 52.159 ms, and detector-reacquisition p95 of 84.446 ms.
Those results support a capped 15 Hz, queue-depth-one experimental signal worker.
They do not establish 30/60 Hz suitability, real-game frame impact, reliable
identity across similar faces, or acceptable appearance across arbitrary games.

A second current-device run exercised the exact pinned MNV3/LM1 files under
ONNX Runtime 1.22.1 CPU EP with one thread. It recorded 20 cold loads, 20
reloads, 40 unload samples, 20 cancellation-after-readiness samples, and 245
66-point packet operations across all 453 replay frames. P99 load/reload,
operation/frame-age, cancel, and unload were 102.327/147.527, 54.050/54.051,
3.412, and 30.719 ms respectively. Resident and total process RAM deltas were
35,086,336 and 125,935,616 bytes; measured resident and transient VRAM deltas
were both zero. This is a signed candidate measurement, not release admission:
the candidate key is intentionally ephemeral, live-game impact remains
uncertified, and activation remains false.

Rendered review includes a six-frame 66-point contact sheet and a readable
four-panel large-face proof showing the untouched source, exact mouth mask,
bounded reference composite, and 8x absolute difference. The exact mask changed
333 pixels inside and zero pixels outside. The user-reported Transparency App
dimmer overlays were left untouched; model input was direct fixture-byte decode,
not a desktop screenshot or perceived display brightness sample.

## Project-owned current-frame mouth-warp pipeline

1. The media broker owns the exact Windows Graphics Capture/D3D source frame and
   the explicit locked actor. The visual worker cannot select a character from
   text, memory, or whichever face has the highest detector confidence.
2. OpenSeeFace runs detector and landmark inference at no more than 15 Hz on the
   bounded locked face region.
3. Between detector frames, a project-owned sparse pyramidal Lucas-Kanade
   tracker propagates the four semantic mouth anchors plus a small set of
   high-texture points in the current mouth and cheek neighborhood.
4. Forward/backward error, inlier ratio, a robust similarity transform,
   detector disagreement, scale/rotation limits, visibility, pose, appearance,
   lighting and occlusion gates decide whether propagation is admissible.
   Flow never creates an identity and cannot survive a scene or track epoch.
5. The worker constructs a dynamic mask from current landmarks and current
   tracked texture. A static avatar mask is not admissible.
6. Audio drive priority is:
   - exact provider-timed visemes or blendshapes;
   - provider character timings converted through a language-aware
     grapheme-to-phoneme/viseme map;
   - a causal local MFCC/RMS/zero-crossing cue as a visibly lower-quality
     fallback.
7. A small piecewise-affine or moving-least-squares deformation modifies only
   current-frame pixels. Jaw opening and displacement remain conservative
   because this method cannot faithfully invent unseen oral detail.
8. The worker emits a separate residual texture lease. The authenticated broker
   revalidates cancellation generation, actor, track epoch, frame sequence,
   capture time, device and geometry generations, adapter LUID, lease nonce,
   keyed mutex, ROI and deadline before one presentation.
9. Queue depth is exactly one. A newer job replaces unprocessed work; no visual
   frame waits in a queue to play late.
10. Any failure bypasses the residual. Audio and subtitles continue.

The current native mouth-worker types already carry the necessary actor, track,
frame, audio-clock, lease and deadline identities and expose stale, cancelled,
wrong-frame, occluded, pose, confidence and unsafe-bounds bypasses. The current
PCM fallback is energy plus zero crossings; it must not be described as phoneme
recognition. The existing residual is also a simple elliptical warp, so the
optical-flow propagation and more defensible current-pixel deformation remain
candidate work, not a completed product claim.

The current CMake target produces a Release Windows GUI-subsystem
`npc-mouth-worker.exe`, static library, tests, benchmark and synthetic proof.
Release staging renames the executable to Tauri's target-triple sidecar name;
normal source configuration does not require an absent prebuilt artifact. The
native control plane owns an authenticated supervisor with kill-on-parent-death,
generation cancellation and exact PID/creation/executable attestation. Broker
commands 14/15 lease the exact current WGC source to that process and accept a
separate residual after adapter, format, keyed mutex, nonce, actor, track,
generation, frame, geometry, ROI, occlusion and deadline revalidation. Native
proof covers presentation and the fail-open case where the worker dies during a
live PCM stream: the audio submission still drains and is not cancelled.

This completed transport is deliberately not described as currently playable
lip-sync. The GUI worker now has an optional native OpenSeeFace MNV3/LM1 producer
that dynamically loads only Model Manager-authorized, rehashed ONNX Runtime
1.22.1 and model files. The native VisualCoordinator is reached from the
broker-issued selected-turn playback pool, consumes command 29's post-WASAPI
eight-bin RMS/peak envelope, and derives frame/track/ROI/appearance authority
only from the immutable identity actor-lock bus. The production identity pack
is not yet signed/admitted, so the bus starts unqualified and ordinary turns
emit a visual-unavailable receipt without launching inference or accepting
fabricated WebView/manual-selection evidence. No raw PCM is exposed and audio
and subtitles continue independently.

## Candidate comparison

| Approach | Moving captured face | Streaming/cancellation | 12 GB beside a game | Distribution/install | Decision |
| --- | --- | --- | --- | --- | --- |
| Provider-timed viseme/blendshape signal | Signal only | Good when timestamps bind to playback | Excellent | Provider-specific | Preferred audio drive |
| Project-owned current-frame warp | Yes, conservatively | Queue one and exact-frame cancellation | Excellent; CPU-first | MIT native binary, no weights | Only viable default candidate after qualification |
| OpenSeeFace plus optical flow | Landmark/anchor layer | Good with guarded reanchors | Excellent; measured CPU path | BSD-2 plus MIT runtime | Recommended optional signal dependency |
| MediaPipe Face Landmarker | Landmark/blendshape layer | Live callback | CPU-feasible but unmeasured here | Apache code; exact model asset review required | Comparator |
| NVIDIA Audio2Face-3D | No pixel output; rig/mesh signal | Stream capable | Optional high-headroom profile only | Model, SDK and runtime terms are separate | Signal-only experiment |
| NVIDIA LipSync NIM | Video-to-video | Chunked file streaming | No qualifying 4080 Laptop/game evidence | Private access, Linux container stack | Private comparator only |
| MuseTalk 1.5 | Target-frame-based editor | Stock path is batch/avatar-oriented | Existing measured path is far too slow | MIT top-level signal; heavy transitive stack | Refactor spike only |
| Wav2Lip | Target video | Offline/batch design | Poor fit | Pretrained results restricted to research/non-commercial use | Reject live path |
| LatentSync | Target video | Diffusion/offline design | Community evidence exceeds safe headroom | Exact transitive review still required | Reject live path |
| VideoReTalking | Target video | Multi-stage offline pipeline | Poor fit | Apache top-level code; old/heavy stack | Reject live path |
| QuickTalk | Preprocessed avatar/template | Faster after warmup | Promising reported 1.4-1.8 GB peak inference VRAM | InsightFace `buffalo_l` rights block a redistributable default | Avatar comparator only |
| EfficientSync | Reference-texture deformation | Paper claims real time | Unknown on the target Windows/game envelope | Paper only in this snapshot | Watchlist |
| FlashLips | Target-frame reconstruction plus lip pose | Paper claims over 100 FPS | Published claim does not prove 12 GB game coexistence | No immutable public pack found | Leading future watchlist |

### Generative candidate findings

MuseTalk 1.5 is the closest available neural residual experiment because it uses
a one-step 256x256 editor and has a permissive top-level license signal. Its
official limitations include identity-detail, moustache, lip-shape/color and
jitter failures. The supported flow is prepared-avatar and file oriented, while
maintained issues report substantial VRAM, difficulty achieving practical live
streaming, and Windows setup friction. This repository's isolated real-weight
Windows run generated a valid 1.579-second output but took about 102 seconds end
to end with 4.175 GiB of input assets. That measured path fails live admission;
it does not prove that a future bounded ROI refactor can never succeed.

Wav2Lip remains an offline reference and its pretrained-model use is not a
commercially redistributable product default. LatentSync is too expensive for
the target envelope: maintained reports include severe RTX 4090 latency, 8 GB
out-of-memory, large or growing memory use, and a TensorRT attempt still around
12.55 GB peak. VideoReTalking is a multi-stage offline repair pipeline with old
runtime assumptions, not a current-frame worker.

QuickTalk reports unusually promising steady-state throughput and peak inference
VRAM, but its first-video latency is roughly 1.1-2.7 seconds and it is designed
around preprocessed avatar/template assets. Its documented InsightFace
`buffalo_l` dependency also inherits a non-commercial-research pretrained-model
restriction.

EfficientSync is architecturally relevant because it transfers existing
reference textures instead of reconstructing the entire lower face and reports
166 FPS. FlashLips is the strongest future architecture because it separates a
low-dimensional lip-pose signal from a one-pass target-frame reconstruction
stage. Neither has a presently qualified, immutable, licensed Windows pack with
cancellation, current-frame and game-contention evidence.

## NVIDIA conclusions

Audio2Face-3D is useful only as an optional audio-to-motion signal. The open
Claire v2.3.1 model is approximately 39.8 million parameters with roughly 307 MB
of repository payload. NVIDIA's SDK supports Windows/native execution, but the
output is facial geometry or ARKit-style coefficients, not arbitrary captured
game pixels. A future high-headroom profile may reduce its canonical mesh or
blendshapes to normalized jaw/lip metrics and feed the same project-owned
current-frame warp. It must not be represented as complete generic game
lip-sync.

NVIDIA's newer LipSync NIM directly accepts video and audio and can stream file
chunks, but it does not qualify for this product envelope:

- access requires the AI for Media Private Access Program and an entitled NGC
  key; an ordinary NVIDIA Developer API key does not grant it;
- the documented runtime is a Linux Docker/Triton/DeepStream stack launched
  with all selected GPUs and an 8 GB shared-memory allocation;
- the optimized consumer list names RTX 4090, 5080 and 5090, not RTX 4080
  Laptop;
- vendor FPS is whole-file request-to-complete throughput, not first-output
  age, p95 frame age, cancellation latency or game frame impact;
- higher concurrency is documented to increase GPU memory and risk OOM;
- output is video data rather than a bounded, authenticated current-frame mouth
  residual;
- the Holoscan-for-Media variant documents a 30-frame lookahead, approximately
  one second at 30 fps.

The NIM is therefore a private enterprise/offline comparator, not a clean
Windows pack or a one-key default.

## Qualification and admission gates

Promotion from `EngineeringBaseline` requires a signed, device/runtime/hash-bound
Windows evidence envelope. Planning estimates and vendor benchmarks cannot be
admitted as measurements.

### Test matrix

- Clean Windows 10 and 11 install, launch, offline self-test, uninstall and
  rollback with no Python, Git, CUDA toolkit, FFmpeg or developer shell.
- RTX 4080 Laptop 12 GB while a game or controlled renderer occupies 6, 8 and
  10 GB of dedicated VRAM.
- At least 30 captured sequences across real, stylized and CG faces, 40-256 px
  face sizes, yaw/pitch/roll, rapid camera motion, scene cuts, HDR/lighting
  changes, facial hair, helmets, partial occlusion, multiple similar faces,
  actor handoff, HUD/modal overlap, non-human faces and 30/60/120/144 Hz
  presentation.
- Provider-timed visemes and the local fallback across multiple voices and
  languages.
- Cancellation before inference, during tracking, after residual production,
  during broker import and immediately before presentation.
- Device reset, resize, adapter change, lease expiry, source loss and worker
  crash.

### Measurements

- Capture-to-residual and capture-to-presentation p50/p95/p99/max.
- Source-frame age, tracking-evidence age and audiovisual skew.
- Detector, optical flow, warp, broker import and present time separately.
- Wrong-actor, wrong-track and wrong-frame presentation counts; all must be zero.
- Cancellation, scene-cut and stale-output hide-within-one-refresh rate; it must
  be 100%.
- Queue replacement and depth; depth must never exceed one.
- CPU, private working set, GPU engine use, dedicated/shared VRAM and load time.
- Game average FPS, 1% low and p95/p99 frame-time impact.
- Optical-flow forward/back error, inlier ratio, detector disagreement and
  reset rate.
- Exact unchanged pixels outside the residual, residual containment, temporal
  jitter, identity preservation and occlusion failure rate.
- Blind human A/B review for synchronization, identity and artifacts across
  every major content stratum. SyncNet/LSE may be secondary evidence only
  because their calibration does not cover arbitrary stylized game faces.
- Artifact hashes, SBOM, transitive license notices, security scan and offline
  post-install self-test.

Suggested admission targets are zero dedicated model VRAM, less than 256 MiB
added steady RAM for the complete signal/warp path, warp p95 below 2 ms, broker
GPU work below 0.25 ms, game p95 frame-time regression no more than 2%, 1%-low
drop no more than 3%, zero stale or mismatched presentations, and one-refresh
hide compliance in every injected failure. These are proposed gates, not current
measurements.

## Primary and maintained evidence ledger

The following 40 sources were assessed. Vendor and paper performance is used to
choose experiments, never as a product benchmark. Issue reports are community
evidence of risks to reproduce, not controlled measurements.

1. [OpenSeeFace repository](https://github.com/emilianavt/OpenSeeFace) — CPU
   tracking design, model inventory, upstream performance claims and license.
2. [OpenSeeFace pinned BSD-2 license](https://github.com/emilianavt/OpenSeeFace/blob/85aa70fc67582d046e771ea73625182a0d8f7475/LICENSE) — redistribution basis for
   the exact signal revision.
3. [MediaPipe Face Landmarker guide](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker) — live-stream landmarks and blendshape output.
4. [MediaPipe blendshape model card](https://storage.googleapis.com/mediapipe-assets/Model%20Card%20Blendshape%20V2.pdf) — documented motion, lighting, overlap and input-quality limitations.
5. [OpenCV sparse pyramidal LK documentation](https://docs.opencv.org/4.13.0/d4/dee/tutorial_optical_flow.html) — sparse current-frame feature propagation and periodic re-detection basis.
6. [uLipSync repository](https://github.com/hecomi/uLipSync) — MIT real-time
   MFCC-profile audio-cue reference.
7. [Rhubarb Lip Sync repository](https://github.com/DanielSWolf/rhubarb-lip-sync) — offline phonetic mouth-shape reference, not a streaming engine.
8. [Azure Speech visemes](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/how-to-speech-synthesis-viseme) — timestamped visemes and animation frames suitable as a preferred signal.
9. [Amazon Polly speech output](https://docs.aws.amazon.com/polly/latest/dg/output.html) — timestamped speech/viseme marks.
10. [ElevenLabs timestamped TTS](https://elevenlabs.io/docs/api-reference/text-to-speech/convert-with-timestamps) — character alignment that can aid scheduling but is not phoneme truth.
11. [Wav2Lip repository](https://github.com/Rudrabha/Wav2Lip) — offline stack and pretrained-model non-commercial boundary.
12. [Wav2Lip paper](https://arxiv.org/abs/2008.10010) — target-video generative architecture and SyncNet framing.
13. [MuseTalk repository](https://github.com/TMElyralab/MuseTalk) — v1.5 runtime, top-level license signal and stated quality limitations.
14. [MuseTalk paper](https://arxiv.org/abs/2410.10122) — one-step latent editor architecture.
15. [MuseTalk streaming issue 239](https://github.com/TMElyralab/MuseTalk/issues/239) — maintained evidence that streaming integration is not a supported turnkey path.
16. [MuseTalk real-time issue 384](https://github.com/TMElyralab/MuseTalk/issues/384) — community evidence of practical real-time difficulty.
17. [MuseTalk VRAM issue 310](https://github.com/TMElyralab/MuseTalk/issues/310) — community report of roughly 11 GB use in one configuration.
18. [MuseTalk Windows issue 40](https://github.com/TMElyralab/MuseTalk/issues/40) — Windows setup and dependency friction.
19. [LatentSync repository](https://github.com/bytedance/LatentSync) — diffusion runtime, released code and model workflow.
20. [LatentSync performance issue 94](https://github.com/bytedance/LatentSync/issues/94) — severe RTX 4090 latency report to reproduce.
21. [LatentSync memory issue 326](https://github.com/bytedance/LatentSync/issues/326) — large or growing VRAM report.
22. [LatentSync TensorRT issue 365](https://github.com/bytedance/LatentSync/issues/365) — reported TensorRT latency and approximately 12.55 GB peak memory.
23. [LatentSync 8 GB OOM issue 314](https://github.com/bytedance/LatentSync/issues/314) — low-VRAM failure evidence.
24. [VideoReTalking repository](https://github.com/OpenTalker/video-retalking) — multi-stage offline pipeline, dependencies and pose limitations.
25. [QuickTalk benchmark](https://github.com/datascale-ai/opentalking/blob/main/docs/en/avatar_models/quicktalk.md) — reported FPS, first-video latency and peak inference VRAM.
26. [QuickTalk model support](https://github.com/datascale-ai/opentalking/blob/main/docs/en/model-support/models/quicktalk.md) — avatar/template cache requirement.
27. [OpenTalking QuickTalk setup](https://github.com/datascale-ai/opentalking/blob/main/docs/en/quick-start/index.md) — CUDA, FFmpeg, HuBERT and InsightFace dependencies.
28. [InsightFace licensing](https://github.com/deepinsight/insightface/blob/master/server/LICENSING.md) — non-commercial-research boundary for pretrained models such as `buffalo_l`.
29. [Audio2Face-3D repository](https://github.com/NVIDIA/Audio2Face-3D) — current NVIDIA model/SDK collection and output scope.
30. [Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK) — Windows/native SDK and geometry/blendshape contract.
31. [Audio2Face-3D SDK requirements](https://github.com/NVIDIA/Audio2Face-3D-SDK/blob/main/docs/README.md) — CUDA, TensorRT, platform and resource requirements.
32. [Claire v2.3.1 model card](https://huggingface.co/nvidia/Audio2Face-3D-v2.3.1-Claire) — model size, Windows support signal and NVIDIA Open Model License.
33. [Claire v2.3.1 files](https://huggingface.co/nvidia/Audio2Face-3D-v2.3.1-Claire/tree/main) — exact repository payload scale.
34. [Audio2Face-3D NIM support matrix](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/support-matrix.html) — supported container stack and resource envelope.
35. [NVIDIA LipSync NIM overview](https://docs.nvidia.com/nim/maxine/lipsync/latest/overview.html) — video/audio inputs and chunked-versus-transactional service semantics.
36. [NVIDIA LipSync NIM support matrix](https://docs.nvidia.com/nim/maxine/lipsync/latest/support-matrix.html) — Linux stack, codec hardware and supported GPU list.
37. [NVIDIA LipSync NIM performance](https://docs.nvidia.com/nim/maxine/lipsync/latest/performance-results.html) — whole-file throughput definition and concurrency/OOM warning.
38. [NVIDIA LipSync NIM getting started](https://docs.nvidia.com/nim/maxine/lipsync/latest/getting-started.html) — private-access entitlement and container launch contract.
39. [NVIDIA H4M limitations](https://docs.nvidia.com/nim/maxine/lipsync-h4m/latest/limitations-and-known-behaviors.html) — 30-frame lookahead and pipeline-restart behavior.
40. [EfficientSync paper](https://arxiv.org/abs/2608.18832) and [FlashLips paper](https://arxiv.org/abs/2512.20033) — leading deformation/reconstruction watchlist architectures; neither is a qualified pack in this snapshot.

## Final disposition

- **Ship now:** no automatic visual modification; audio/subtitles remain the
  reliable product path.
- **Expose for explicit experimental selection:** the exact pinned OpenSeeFace
  signal pack only under its existing guards and truthful limitations.
- **Qualify next:** the project-owned CPU current-frame optical-flow mouth warp
  under the full live-game matrix above.
- **Optional advanced signal experiment:** Audio2Face-3D, only when measured
  whole-loadout headroom exists and only as coefficient input to the same
  current-frame mapper.
- **Private/offline comparators:** NVIDIA LipSync NIM and MuseTalk.
- **Reject as live defaults:** Wav2Lip, LatentSync, VideoReTalking and
  static-avatar/talking-head replacement paths.
- **Watch:** EfficientSync and FlashLips until immutable code, weights, exact
  licenses and reproduced Windows/game evidence exist.
