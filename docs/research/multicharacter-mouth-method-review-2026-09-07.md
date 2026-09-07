# Multi-character current-frame mouth method review — 2026-09-07

## Scope and evidence boundary

This review answers the owner's rejection of the Cyberpunk/Misty result as
cheap-looking and the request to test other characters. It concerns the
lip-sync video only. The control-app UI is outside this decision.

The research lane was performed without model downloads, API calls, GPU use,
visible windows, or audio playback. It combines direct inspection of the current
native pipeline and its retained enlarged boards with 32 opened primary papers,
official repositories, model cards, and platform documents. Subsequent private
prototype work outside this research lane made two built-in image-generation
calls to prepare Claire and Johnny oral-interior enrollment references. Those
experiment calls are not evidence that the research review itself ran a model.
Author speed numbers are references on their hardware, not measurements on this
PC.

## Decision

Retire full-lip atlas replacement as the generic renderer. Its seam is not a
parameter-tuning problem: the atlas replaces the identity-bearing vermilion
border, wrinkles, makeup, lighting, and surrounding skin with pixels captured
or generated under a different condition. Smoothing can hide state changes but
cannot make those pixels belong to the current frame.

Build the next prototype as a **current-frame lip warp with enrolled oral
interior assistance**:

1. Estimate a dense current-frame lip mesh and a pose-aware local coordinate
   frame. Compare MediaPipe Face Landmarker/Attention Mesh with the existing
   OpenSeeFace path; use 3DDFA-V2 only where its 3D pose stability proves useful.
2. Carry the current mouth support forward with mouth-local optical flow. Use
   forward/backward agreement, strain, foldover, boundary motion, occlusion,
   blur, and confidence checks to reject unreliable warps.
3. Produce the desired lip contours from timely phoneme/viseme cues, then use a
   bounded piecewise-affine or thin-plate deformation of the **current frame's
   own upper and lower lip pixels**. Preserve the live game's lip color,
   lighting, facial hair, makeup, grain, and edge texture.
4. When opening the lips exposes content that the current closed frame does not
   contain, retrieve only the best-matching identity-bound oral interior
   observation: cavity, teeth, and tongue. Match pose and lighting before mouth
   topology; photometrically normalize it in linear light; composite inside the
   inner-lip contour. Do not copy the observation's outer lips or skin.
5. Deform less when the pack lacks the required pose/topology. Bypass when no
   safe source exists. A small, plausible mouth is preferable to a large pasted
   one.

This is the practical subset of the August 2026 EfficientSync direction:
deformation, sharp and topologically diverse genuine references, independent
current-frame background, and an adaptive mask. EfficientSync itself has no
released code, weights, Windows implementation, or software/model license and
is still under review, so it is an architecture reference rather than a
dependency.

## Why this is the strongest fit

The product edits arbitrary already-moving game footage. Its current WGC frame
must continue to own pose, head and body motion, blinking, breathing, lighting,
camera motion, post-processing, and occlusion. Full portrait generators and
lower-face inpainting models optimize a different problem. They can look good
on prepared portraits while reconstructing or softening pixels that the game
already rendered correctly.

The local evidence reinforces that distinction. MuseTalk 1.5 was the best
neural visual comparator but its moving-current-frame path measured 429.725 ms
p95 before the final source-preserving pass and 7,594 MiB peak device memory on
this RTX 4080 Laptop GPU. The atlas compositor is fast but visibly rejected.
Therefore the next useful comparison is not another full-lip atlas style; it is
current-frame deformation versus current-frame deformation plus oral-only
assistance.

The geometry path can remain CPU-first. Mouth-local flow and warping operate on
a small ROI. NVIDIA Optical Flow can later be compared on the existing D3D11
path, but a CPU DIS implementation is a sensible fallback and keeps game VRAM
free. A learned renderer should be admitted only if it beats this design in a
same-frame blind comparison under representative game load.

## Proposed runtime contract

The existing actor, track, frame, geometry, audio interval, lease, and
cancellation-generation bindings remain mandatory. The renderer adds:

- current-frame dense lip mesh and confidence per landmark;
- previous untouched-frame mesh and mouth-local forward/backward flow;
- desired outer and inner contours derived from timely cues;
- current-frame lip deformation field and foldover/strain score;
- optional enrolled interior observation ID plus pose, topology, illumination,
  sharpness, and provenance scores;
- separate alpha for current-frame lip warp and oral-only insert;
- a signed residual support mask, deadline, and complete bypass reason.

Generated or previously composited pixels never enter tracking, flow, color
estimation, enrollment, or the next frame. State smoothing applies to geometry
and cue trajectories, not to RGB output from old frames. Every output starts
from the newest untouched game frame.

## Character-pack enrollment

The pack format should store observations rather than finished pasted mouths.
For each character and pose bin, keep neutral/closed lips plus qualified open,
rounded, spread, and contact examples when available. Each observation records
source rights, source and crop hashes, actor identity, yaw/pitch/roll, local
lip mesh, aperture, roundedness, tooth/tongue visibility, illumination,
sharpness, mask, and qualification result.

An optional heavy model may act as a one-time private enrollment teacher, then
fully unload. Teacher pixels still need human review and may supply only the
oral interior unless a direct captured observation exists. A headshot alone is
not sufficient evidence for every tooth/tongue state or side pose. User
overrides remain allowed, but the runtime should reveal missing coverage and
reduce articulation instead of inventing coverage.

Setup work and gameplay work stay separate:

| Phase | Allowed work | Runtime footprint |
| --- | --- | --- |
| Enrollment | detection, segmentation, heavy teacher, manual review, pack signing | may be slow/heavy; unload before play |
| Warm setup | verify pack, index pose/topology, precompute masks and local bases | bounded CPU/RAM; no frame synthesis |
| Gameplay hot path | dense landmarks, small-ROI flow, cue-to-contour, current-pixel warp, optional oral insert | target <= 50 ms p95 capture-to-composite under game load |

## Future production multi-character acceptance matrix

This is a future production gate, not a description of the current experiment.
The current private study covers Misty, Claire, and Johnny only. Johnny is
partially visible and is useful as a stress case, but cannot count as a clean
full-face qualification. That study does not pass this matrix.

Before product acceptance, use at least four visibly distinct characters in
addition to Misty, with source clips where the actor is idle and not speaking
before the synthetic response:

| Case | Failure it exposes | Required comparison |
| --- | --- | --- |
| lipstick or sharp lip makeup | color/edge discontinuity and mask spill | untouched, current-warp only, hybrid oral insert |
| beard or moustache | hair destruction and boundary shimmer | hair preservation and occlusion bypass |
| dark skin or low lip/skin contrast | unstable contour and bad color transfer | geometry confidence and linear-light match |
| older or deeply textured face | smoothing, waxiness, lost wrinkles | high-frequency current-pixel preservation |
| stylized/non-human-proportioned game face | human-prior failure | conservative warp range and bypass |
| side pose around 25–35 degrees | wrong interior, foldover, double contour | pose-matched retrieval and strain rejection |

For each character, render the same audio/cue schedule in three modes:

1. untouched source with audio;
2. current-frame lip warp only;
3. current-frame lip warp plus enrolled oral interior.

The current full-lip atlas may appear as a clearly labeled rejected baseline,
not as a candidate. Inspect normal-size playback, 4x close-ups, consecutive
transition strips, onset/offset, bilabial closure, rounded vowels, teeth states,
pose changes, and occlusion. Record exact outside-mask equality, boundary
gradient discontinuity, temporal flicker, landmark/flow confidence, p50/p95
latency, RAM/VRAM, and bypass rate. Blind preference at display size and enlarged
seam review are both required; numeric motion gates cannot pass visual quality.

## Alternative assessment

| Method | Visual potential | Hot-path/resource fit | License/deployability | Decision |
| --- | --- | --- | --- | --- |
| current-frame dense warp + oral-only observations | high source fidelity; limited safely by observed topology | small ROI, CPU-capable, no recurring API cost | own implementation and private/data-only packs | build first |
| EfficientSync-style deformation and texture mixer | strongest directly relevant published direction | authors report 166 FPS on one GPU; local cost unknown | no released implementation or runtime license | reproduce concepts, watch release |
| FlashLips compact latent editor | promising locality and >100 FPS author claim | GPU model; causal audio behavior and PC footprint unproven | paper only; no released code/weights | watch, do not block prototype |
| Lip Forcing 1.3B | strong causal V2V research result, 31 FPS author claim | 1.3B checkpoint not released; released 14B stack is unsuitable for a game-sharing 12 GB GPU | Apache repository plus large third-party dependency chain | exclude current machine path |
| MuseTalk 1.5 | best locally tested neural appearance | 429.725 ms p95 and 7,594 MiB locally on fresh moving frames | MIT code; project allows model commercial use, dependency review still required | offline teacher/comparator only |
| LatentSync 1.6 | strong offline diffusion quality | 20–50 steps; official minimum 18 GB VRAM for v1.6 | Apache code; dependency/model audit required | offline teacher only |
| KeySync / OmniSync | strong long-clip/offline editing concepts | keyframe/interpolation or diffusion pipeline, not newest-frame gameplay | KeySync Apache; OmniSync public implementation not established in reviewed source | offline research only |
| LivePortrait/FasterLivePortrait family | efficient deformation and stitching reference | can be real-time with TensorRT, but animates a prepared portrait/reference representation | LivePortrait dependency/model terms require full audit; common InsightFace weights are noncommercial | renderer comparator, not default |
| NVIDIA Maxine AR LipSync | released Windows direct-video comparator; official RTX 4090 table says 17.0 ms | GPU/RTX, fixed initial-frame latency, face visibility/pose limits | proprietary SDK/distribution review required | bounded comparator if available |
| NVIDIA Audio2Face-3D | best released rig controller | >60 FPS claim; 4 GB+ VRAM recommended; Windows/Linux | MIT SDK; NVIDIA Open Model License weights | use only for rigged games/test target |
| ARTalk and other audio-to-3D motion models | useful audio-to-expression priors | requires a rig/renderer and does not edit captured pixels | research stack and dependency audit required | rigged-path research only |
| SEA-RAFT or SAM 2 in gameplay | strong flow/segmentation research | broader neural workload than a tiny mouth ROI needs | BSD-3-Clause SEA-RAFT code; SAM 2 separately licensed | evaluation tooling, not default |

## Primary-source ledger

All links below were opened on 2026-09-07. Dates are submission/release/update
dates visible in the source where available.

1. [EfficientSync paper, 2026-08-19](https://arxiv.org/abs/2608.18832) — deformation, channel-wise multi-reference texture selection, shifted adaptive masking, sharp/topologically diverse sampling; 166 FPS author result; under review and unreleased.
2. [FlashLips paper v3, 2026-04-19](https://arxiv.org/abs/2512.20033) — compact one-step reconstruction editor plus low-dimensional lip pose; >100 FPS author claim; no deployable release found.
3. [Lip Forcing paper, 2026-06-09](https://arxiv.org/abs/2606.11180) — two-step causal V2V diffusion; 1.3B student reports 31 FPS.
4. [Lip Forcing repository](https://github.com/cvlab-kaist/LipForcing) — Apache-2.0 code; released inference checkpoint is 14B; 1.3B is marked coming soon.
5. [MuseTalk paper](https://arxiv.org/abs/2410.10122) — latent-space 256x256 lip editor with spatiotemporal training.
6. [MuseTalk repository, 1.5 update 2025-03-28](https://github.com/TMElyralab/MuseTalk) — 30 FPS+ claimed on Tesla V100; acknowledges center placement sensitivity; MIT code and commercial model permission, subject to dependencies.
7. [LatentSync paper](https://arxiv.org/abs/2412.09262) — diffusion lip editing with SyncNet supervision.
8. [LatentSync repository, 1.6 update 2025-06-11](https://github.com/bytedance/LatentSync) — 18 GB minimum inference VRAM for 1.6, 20–50 steps, guidance can trade sync for distortion/jitter.
9. [KeySync paper, 2025-05-01](https://arxiv.org/abs/2505.00497) — leakage-free high-resolution keyframe plus interpolation approach.
10. [KeySync repository](https://github.com/antonibigata/keysync) — Apache-2.0; clip-oriented pipeline; occlusion repair is optional and coordinate-driven.
11. [OmniSync paper, 2025-05-27](https://arxiv.org/abs/2505.21448) — mask-free diffusion-transformer V2V editing and progressive noise initialization.
12. [NeurIPS 2025 OmniSync proceedings](https://papers.nips.cc/paper_files/paper/2025/hash/10b99813d5992673da90a499b6bcd5f0-Abstract-Conference.html) — peer-reviewed publication record; no released real-time Windows path established.
13. [LivePortrait paper, 2024-07-03](https://arxiv.org/abs/2407.03168) — implicit-keypoint deformation, stitching, and retargeting controls.
14. [LivePortrait repository](https://github.com/KlingAIResearch/LivePortrait) — Windows support, video-to-video mode, and source/driving constraints; `torch.compile` acceleration is explicitly unavailable on Windows.
15. [ARTalk paper, 2025-02-27](https://arxiv.org/abs/2502.20323) — autoregressive audio-to-3D head motion; relevant to rigs, not screen pixels.
16. [ARTalk repository](https://github.com/xg-chu/ARTalk) — public research implementation and avatar-rendering dependencies.
17. [Audio2Face-3D paper, 2025-08-22](https://arxiv.org/abs/2508.16401) — audio to facial animation controls for digital avatars.
18. [Audio2Face-3D SDK repository](https://github.com/NVIDIA/Audio2Face-3D-SDK) — MIT SDK, Windows/Linux, >60 FPS claim, CUDA/TensorRT, 4 GB+ VRAM recommended.
19. [Audio2Face-3D v3 model card, 2025-09-24](https://huggingface.co/nvidia/Audio2Face-3D-v3.0) — 180M parameters, NVIDIA Open Model License, audio-to-facial-pose output, poor-audio limitation.
20. [NVIDIA Maxine AR feature contract](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/using-ar-features.html) — LipSync exposes a fixed initial-frame latency and requires queued input before output.
21. [NVIDIA Maxine AR performance reference](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/PerformanceReference.html) — official 17.0 ms LipSync latency on RTX 4090 under NVIDIA's test conditions.
22. [MediaPipe Face Landmarker guide](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker) — image/video/live-stream modes, dense landmarks, blendshapes, and transforms.
23. [Attention Mesh paper, 2020-06-18](https://arxiv.org/abs/2006.10962) — region attention improves lips/eyes/iris geometry while retaining real-time inference.
24. [3DDFA-V2 paper v2, 2021-02-07](https://arxiv.org/abs/2009.09960) — pose-stable 3D dense alignment; authors report >50 FPS on one CPU core.
25. [3DDFA-V2 repository](https://github.com/cleardusk/3DDFA_V2) — public implementation; weights and 3D assets need separate packaging/license review.
26. [OpenSeeFace repository](https://github.com/emilianavt/OpenSeeFace) — current CPU tracker; its 66-point LS3D-W-derived topology limits inner-lip detail.
27. [DIS optical-flow paper, 2016-03-11](https://arxiv.org/abs/1603.03590) — efficient CPU dense inverse-search flow; author benchmark is full-image research evidence, not local ROI timing.
28. [SEA-RAFT paper, 2024-05-23](https://arxiv.org/abs/2405.14793) — accurate learned flow and improved generalization; useful high-quality comparator.
29. [SEA-RAFT repository](https://github.com/princeton-vl/SEA-RAFT) — BSD-3-Clause implementation; still a neural GPU workload.
30. [UnFlow paper, 2017-11-21](https://arxiv.org/abs/1711.07837) — bidirectional consistency and occlusion reasoning support fail-open flow validation.
31. [SAM 2 paper, 2024-08-01](https://arxiv.org/abs/2408.00714) and [repository](https://github.com/facebookresearch/sam2) — temporal segmentation reference; too broad for default mouth-local masking.
32. [Windows Graphics Capture documentation](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture) and [`IAudioClock2::GetDevicePosition`](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock2-getdeviceposition) — authoritative frame and playback-device clock foundations.
33. [InsightFace repository license notice](https://github.com/deepinsight/insightface) — code is MIT, but distributed training data and models are noncommercial research only; do not treat code and weight licenses as equivalent.

## Acceptance gate

The method remains off by default. The present Misty/Claire/Johnny
current-pixel/oral-only prototype is offline and unqualified; Johnny's partial
visibility makes it a stress observation rather than an acceptance case. The
future production matrix must pass blind preference over the rejected atlas and
demonstrate no full-lip/skin replacement, no previous-frame freeze, no
generated-pixel feedback, exact outside-mask preservation, causal audio binding,
and <= 50 ms p95 capture-to-composite under representative game load. If it
cannot pass, ship audio plus subtitles and keep arbitrary-game visual mouth
motion disabled.
