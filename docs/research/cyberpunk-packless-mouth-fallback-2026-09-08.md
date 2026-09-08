# Cyberpunk packless mouth fallback

Date: 8 September 2026
Status: connected native fallback, bounded offline review; passive enrollment is a follow-up contract

## Decision

Cyberpunk characters without a prepared mouth pack now use the current game
frame itself as their only appearance source. When the selected actor has a
valid full OpenSeeFace mouth contour and no matching atlas, the ordinary
`ReferenceMouthWorker` routes that frame through
`compose_current_pixel_residual(..., nullptr, ...)`. Timed visemes use the same
schema-four sample-clock trajectory, exact contact handling, mouth-shape filter,
queue-depth-one rule and cancellation gates as reviewed packs.

The fallback cannot invent oral anatomy. With no observed oral strip, the
compositor caps the target gap to the already visible source aperture and
warps only current pixels. It may produce restrained lip compression, rounding
and small opening changes. It cannot produce a convincing wide-open vowel when
the captured source mouth is closed. That is intentional: a quiet or subtle
mouth is preferable to the black oval, duplicate teeth, waxy lower face and
identity drift seen in prior generic generators.

An exact bilabial cue still reaches the motion trajectory immediately, but the
no-pack appearance applies only 45% of a full geometric seal. A full unobserved
seal compressed Misty's asymmetric dark lipstick into a thin downturned line.
The reduced deformation visibly narrows the current mouth while preserving more
of its vermilion curve. Reviewed packs retain full contact strength because they
provide same-character oral evidence.

The existing schema-one contour fallback remains only for old callers that do
not provide the ordered 18-point semantic contour. Matching schema-one through
schema-three character atlases keep their existing behavior. A matching
schema-four reviewed pack remains the richer current-pixel route and may supply
same-character oral pixels inside the eroded aperture.

This is a connected runtime change, not a standalone planner. The product
runtime records the installed pack's actor. An actor without a matching pack
uses current-frame geometry in the signal adapter; installing, clearing or
cancelling an atlas updates that authority. An atlas for one actor can never
lend its pixels to another actor.

## Scalable coverage design

There are three honest levels for Cyberpunk:

1. **Immediate source-only motion.** Every identity-locked actor with usable
   landmarks can receive restrained current-pixel motion without a pack, model
   download or model VRAM. Invalid geometry, occlusion, actor mismatch, stale
   frames and budget pressure still restore the untouched frame.
2. **Passive same-actor enrollment.** A future bounded collector can retain
   diverse, high-confidence oral observations that naturally occur while the
   same actor track is locked. It should cluster closed, contact, rounded,
   mid-open and open observations by pose and exposure, then construct an
   ephemeral schema-four atlas. Until a real open observation exists, open
   vowels stay on the source-only cap. Session observations must be discarded
   on identity ambiguity and remain private/unreviewed until a user accepts a
   preview.
3. **Optional one-click authored pack.** Named characters can use local or
   user-supplied headshots and a heavyweight enrollment teacher outside active
   gameplay. Generated oral states are proposed assets, never identity truth;
   they require enlarged review before enablement. The teacher unloads before
   the game starts. This preserves the lightweight runtime while allowing a
   better ceiling for important characters.

The passive collector is deliberately not represented as implemented here.
Its proposed cap is twelve 128x64 BGRA candidates per actor (384 KiB), four
simultaneous actor encounters (1.5 MiB pixel budget), with metadata and hashes
kept under a 2 MiB session budget. It needs tests for track epoch, pose bins,
scene cuts, exposure drift, eviction and accidental cross-actor promotion
before connection.

## Headless Cyberpunk evidence

The red-capable regression originally compared a no-atlas worker result against
the schema-four source-only compositor. Before the fix, the digests differed:
the worker selected `compose_current_frame_residual`, the older procedural
cavity route. After the fix, the exact worker residual equals the direct
source-only compositor; it differs from the legacy compositor, uses no oral
reference, and its target gap stays at or below
`max(1.8 px, 2.25 * visible source gap)`.

The same path was replayed on the user's moving Cyberpunk Misty sequence with
`npc_mouth_worker_current_pixel_replay --source-only`. The replay used the
exact 90 source frames, full 66-point packets and timed cue stream sampled at
15 Hz. It passed with 45/45 residuals, 39 changed frames, six source-exact
silence frames, six exact bilabial cue-coefficient frames, zero pixels changed
outside residual support and 9.084 ms worker-plus-composition p95. Timing excludes
landmark inference, capture, presentation, audio decode, game load and file IO.

- Replay:
  `E:\temp\InteractiveNPCs\integration-20260908\misty-source-only-v4-soft-contact`
- Replay report SHA-256:
  `3d7f1b14fabc4e0e2ee90260b2776c38d070d5457745ff3b04f6885e018cf46d`
- Enlarged source/no-pack/reviewed-pack board:
  `E:\temp\InteractiveNPCs\integration-20260908\misty-source-only-review-v2-soft-contact\source-only-mouth-board.png`
- Board SHA-256:
  `755bbee9309ab6b18056c73ecfa32399531a3c02c860475fb033cd5a7c4e7180`
- Temporal source/basic/full review:
  `E:\temp\InteractiveNPCs\integration-20260908\misty-source-only-temporal-v4-follow-crop\misty-source-basic-full.mp4`
- Temporal review SHA-256:
  `a6a5ec3b17fe0c3a72f5fb83b1725de7c6ae03251da91366074a6b282f9c5666`

All six no-pack close-ups and the combined board were inspected at original
resolution. The fallback preserves Misty's face and current lipstick, produces
subtle open/rounded motion, and narrows contact cues. The softened contact
reduced changed pixels from 1,756 to 1,431 on the first two contact frames and
from 1,299 to 1,068 on frame 26 compared with the original full-seal fallback.
Wide vowels remain much less articulated than the reviewed same-character pack.

The same frozen worker path was replayed on Claire's separate 90-frame moving
Cyberpunk sequence. It again passed with 45/45 residuals, 39 changed frames,
zero changes outside residual support and 9.181 ms worker-plus-composition p95.
Claire begins almost closed and neither her source-only route nor her reviewed
pack contains rich visible open-mouth anatomy, so both remain visibly subtle.
This second actor confirms the source-anatomy ceiling rather than universal
quality.

- Claire replay:
  `E:\temp\InteractiveNPCs\integration-20260908\claire-source-only-v2-soft-contact`
- Claire temporal source/basic/full review:
  `E:\temp\InteractiveNPCs\integration-20260908\claire-source-only-temporal-v3-follow-crop\claire-source-basic-full.mp4`
- Claire temporal review SHA-256:
  `78e286188d4718664fe7d58d327071c68e7073239c0df96a194f1c7b277ab3f5`

Both temporal comparisons are 1,440x414 H.264 at 15 Hz for exactly 45 frames
and 3.000 seconds. They contain the same prerecorded AAC audio used to drive
the cues. They were encoded and inspected through lossless frame sequences and
contact sheets; the audio was not played. This passes a conservative basic-
motion review for two actors, not natural-quality or all-NPC qualification.

## Current alternatives reviewed

The review covered 29 primary or maintained sources on 8 September 2026. The
common split is consistent: current talking-head models generate or reconstruct
an avatar/portrait, while game integration needs a small residual on the exact
newest moving frame. Reported model FPS does not establish capture age,
cancellation, game contention, identity stability or bounded pixel ownership.

| Source | Current claim and conditions | Relevance here |
| --- | --- | --- |
| [MuseTalk repository](https://github.com/TMElyralab/MuseTalk) | Version 1.5 reports 30+ FPS on V100 after avatar preparation; official Windows example still uses a large Python/CUDA stack and documents identity, moustache, lip-colour and jitter limits. | Closest open neural residual comparator; prior measured Windows path was far too slow for live game use. |
| [MuseTalk paper](https://arxiv.org/abs/2410.10122) | One-step latent inpainting with spatio-temporal training. | Reconstructs a 256px face region rather than guaranteeing exact fresh-frame exterior pixels. |
| [LatentSync repository](https://github.com/bytedance/LatentSync) | Diffusion lip-sync with substantial model/runtime requirements. | Offline comparator; unsuitable beside a 12 GB game loadout. |
| [Wav2Lip repository](https://github.com/Rudrabha/Wav2Lip) | Established video dubbing code; pretrained results retain a non-commercial research boundary. | Useful reference, unsuitable product default and not a fresh-frame worker. |
| [Wav2Lip paper](https://arxiv.org/abs/2008.10010) | Sync-expert-trained lower-face generation for unconstrained video. | Foundational sync evidence; does not solve bounded current-frame presentation. |
| [VideoReTalking](https://github.com/OpenTalker/video-retalking) | Multi-stage face enhancement, audio lip-sync and face restoration. | Offline repair pipeline with more replacement surface and latency. |
| [LivePortrait repository](https://github.com/KlingAIResearch/LivePortrait) | Efficient portrait animation with retargeting and stitching controls. | Strong enrollment-teacher component, but output is a synthesized portrait frame. |
| [LivePortrait paper](https://arxiv.org/abs/2407.03168) | Implicit-keypoint animation with explicit stitching/retargeting modules. | Supports one-shot pack generation research; not direct commercial-game pixels. |
| [SadTalker](https://github.com/OpenTalker/SadTalker) | Single-image 3D motion-coefficient talking-head generation. | Full portrait generator; legacy project integration was not suitable. |
| [SyncTalk](https://github.com/ZiqiaoPeng/SyncTalk) | Person-specific NeRF talking-head synthesis with synchronization refinements. | Requires avatar training/data; not arbitrary NPC capture. |
| [EchoMimicV2](https://github.com/antgroup/echomimic_v2) | Half-body diffusion; accelerated example still reports about 50 seconds for 120 frames on A100. | Too heavy and replaces body/portrait motion owned by the game. |
| [GeneFace++](https://github.com/yerfor/GeneFacePlusPlus) | Real-time 3D talking-face rendering for trained avatars. | Useful architecture evidence, but character-specific training is the pack problem in another form. |
| [GeneFace](https://github.com/yerfor/GeneFace) | Generalized 3D talking-face synthesis with a NeRF renderer. | Not a bounded screen-space residual. |
| [Meta audio2photoreal](https://github.com/facebookresearch/audio2photoreal) | Audio-driven photoreal codec avatars and body motion. | High-quality avatar research; requires captured avatar data and replaces game motion. |
| [OpenSeeFace](https://github.com/emilianavt/OpenSeeFace) | CPU ONNX 66-point tracking; upstream notes its mouth topology differs from iBUG-68 and targets stable avatar controls. | Existing signal dependency; needs exact topology and conservative pixel gates. |
| [MediaPipe Face Landmarker](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker) | Live-stream face landmarks and blendshape output. | Viable signal comparator, not a pixel generator. |
| [OpenCV sparse optical flow](https://docs.opencv.org/4.13.0/d4/dee/tutorial_optical_flow.html) | Pyramidal Lucas-Kanade propagation between detector samples. | Appropriate for carrying current geometry; still requires confidence/reset gates. |
| [uLipSync](https://github.com/hecomi/uLipSync) | Real-time MFCC-driven Unity lip profiles. | Lightweight fallback cue reference; appearance remains game-specific. |
| [Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync) | Offline phonetic mouth-shape extraction. | Useful prepared-audio comparator, not streaming turn output. |
| [Azure Speech visemes](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/how-to-speech-synthesis-viseme) | Timestamped viseme events and, for some voices, blendshape animation. | Preferred provider cue when selected; does not supply NPC pixels. |
| [Amazon Polly speech marks](https://docs.aws.amazon.com/polly/latest/dg/output.html) | Time-aligned viseme speech marks. | Preferred cue path when available. |
| [ElevenLabs timing](https://elevenlabs.io/docs/api-reference/text-to-speech/convert-with-timestamps) | Character alignment accompanies synthesized audio. | Useful scheduling input, but character timing is not phoneme/viseme truth. |
| [NVIDIA Audio2Face-3D](https://github.com/NVIDIA/Audio2Face-3D) | Audio-to-facial geometry/blendshape models. | Optional richer motion signal; it does not modify captured game pixels. |
| [Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK) | Native C++/CUDA SDK with Windows support and post-processing. | A high-headroom signal experiment only; must compete with the game for GPU. |
| [Claire model card](https://huggingface.co/nvidia/Audio2Face-3D-v2.3.1-Claire) | Open model release with its own model/runtime terms and payload. | Potential coefficient source; not a no-pack renderer. |
| [NVIDIA LipSync NIM overview](https://docs.nvidia.com/nim/maxine/lipsync/latest/overview.html) | Video-plus-audio service with chunked and transactional flows. | Direct video comparator, but output ownership and deployment do not match the Windows worker. |
| [NVIDIA LipSync support matrix](https://docs.nvidia.com/nim/maxine/lipsync/latest/support-matrix.html) | Linux container/GPU stack with a bounded supported-hardware list. | Not a clean ordinary Windows integration. |
| [Ditto](https://github.com/antgroup/ditto-talkinghead) | Apache-2.0 inference code, source image/video registration and TensorRT/PyTorch paths. | Strong future one-shot teacher; avatar registration and reconstruction remain outside exact-frame residual semantics. |
| [Teller](https://arxiv.org/abs/2503.18429) | Reports autoregressive streaming portrait animation up to 25 FPS, evaluated as generated portrait video. | Promising motion model, but no maintained Windows residual integration was established. |
| [MirrorMe](https://arxiv.org/abs/2506.22065) | Causal-audio half-body diffusion built on LTX video. | Research watchlist; wider synthesis and model cost are wrong for the default runtime. |

## Acceptance boundary

This change establishes a useful no-pack floor for two moving Cyberpunk
characters under replay. It does not establish automatic actor identification,
passive pack persistence, all Cyberpunk NPC coverage, live WGC presentation,
HDR, occlusion robustness, game-load performance, physical audio or natural
speech quality. Those claims require their own evidence.
