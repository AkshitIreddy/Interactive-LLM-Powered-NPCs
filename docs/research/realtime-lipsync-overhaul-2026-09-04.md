# Real-time lip-sync overhaul: signal, renderer, and optional neural paths

Research date: 2026-09-04
Target: Windows 10/11, NVIDIA gaming laptops/desktops, API-first conversation,
arbitrary externally captured moving faces, and a CPU-safe default lip renderer.

## Decision

The default path is now the identity-bound observed mouth atlas. The earlier
procedural cavity renderer failed visual review and remains only a conservative
compatibility fallback. The active architecture combines a provider-neutral,
playback-clocked visual speech timeline with real per-character mouth pixels:

1. Use exact TTS phoneme/viseme events when the selected provider supplies them.
2. Use a small local causal spectral classifier when only PCM is available.
3. Keep RMS/peak amplitude as the final fail-safe, never as the preferred mouth
   shape selector.
4. Select and interpolate real identity-observed or enrollment-teacher states,
   then warp the curved residual with the current frame's mouth geometry.
5. Keep neural video editing as an explicit optional pack. It must never consume
   game VRAM silently or block audio/subtitles.

This split solves two different problems. The visual timeline decides *which*
mouth pose is needed and when. The compositor decides how much of that pose can
be rendered safely from the current face. A closed source image cannot reveal
real unseen teeth or tongue. The atlas solves that information gap during
enrollment and the runtime never invents those surfaces.

## Product architecture

```text
TTS provider visemes ─┐
                     ├─> canonical 11-viseme timeline ─> playback sample clock
Local PCM spectrum ──┤                                  │
Amplitude fail-safe ─┘                                  ▼
                                             coarticulated mouth coefficients
                                                          │
identity atlas + current frame + actor lock ──────────────┤
                                                          ▼
                                      bounded premultiplied mouth residual
```

The canonical classes are silence, bilabial, labiodental, dental, alveolar,
postalveolar, palatal, velar, rounded, open vowel, and spread vowel. Provider
symbols are mapped once at the runtime boundary. Visual workers never parse
provider-specific strings.

Every timed cue is bound to stream generation, segment, sample rate, start
sample, and duration. The WASAPI playback position is the master clock. Missing,
late, malformed, stale, wrong-generation, overlapping, or oversized cue data is
ignored and falls back to PCM energy; it cannot authorize a residual by itself.

## Why this is the low-latency default

- Runtime adds no model residency and consumes no GPU VRAM. A user may choose
  an optional enrollment teacher to create the per-character atlas.
- The local classifier is seven fixed Goertzel bands over a bounded PCM window,
  not ASR. It distinguishes broad rounded/open/spread/fricative shapes without a
  vocabulary, beam search, transcript, or network call.
- Exact provider events cost almost nothing and avoid guessing when available.
- The renderer is deterministic between frames, so it does not introduce the
  stochastic shimmer common to per-frame generative methods.
- The current face, pose, lighting, beard, and idle motion remain the source of
  truth outside a tightly bounded mouth region, while teeth and lip texture are
  real enrolled pixels from the same identity.

## Neural candidates and hard trade-offs

### NVIDIA Maxine AR LipSync

Maxine is the closest optional pack for arbitrary synchronized video frames on
consumer RTX hardware. NVIDIA reports about 59.7 ms on RTX 2060, 28.5 ms on RTX
3080, and 17.0 ms on RTX 4090 for the Windows AR SDK configuration. Its default
`NumInitialFrames` is 14, however: at 30 FPS that is about 467 ms of fixed visual
lookahead before processing. Audio must be delayed by the same amount or the
result will not be synchronized. It is therefore a quality mode, not the default
conversational path. Sample code is MIT, while SDK/model use and redistribution
remain governed by NVIDIA's separate terms.

### MuseTalk 1.5

MuseTalk is an open candidate for prepared-avatar or cutscene use. Its advertised
real-time route preprocesses and caches avatar detections, masks, coordinates,
and latents. The repository acknowledges single-frame jitter and imperfect lip
and identity preservation; reported transfer/post-processing behavior can cut a
4090 pipeline from over 60 to roughly 30 FPS, and default batching has produced
24 GB out-of-memory reports. It is not the resident arbitrary-game-frame default.

### LatentSync, KeySync, Lip Forcing, and research-only systems

LatentSync and KeySync target higher offline quality and temporal consistency but
remain multi-stage GPU pipelines. Lip Forcing is causal, yet its published
consumer-available path is far beyond ordinary laptop VRAM; the smaller real-time
checkpoint reported on H100 was not available in the reviewed repository.
EfficientSync and FlashLips are architecturally promising because they use
reference texture or deterministic reconstruction, but no deployable official
implementation and product-usable artifact set was available at review time.

Portrait animators such as Ditto and LivePortrait solve a different problem:
they animate a static portrait. They are useful references but do not preserve an
already moving, breathing, blinking game character frame by frame.

## Qualification rules

The default CPU path can be called a *real-time mouth compositor* only after:

- exact provider cues survive the selected TTS route and authenticated playback
  broker without changing audio frame accounting;
- same-level rounded/open/spread test signals produce distinct coefficients;
- moving-face proof keeps source motion and protects the upper lip;
- compositor p95 stays within a 30 FPS frame budget with zero GPU VRAM;
- malformed and stale cue tests fail to the amplitude path or untouched frame.

It cannot be called a neural talking-head generator, universal natural-mouth
completion, desktop presentation proof, live-game certification, installer
qualification, or evidence that unseen anatomy is reconstructed.

An optional neural pack additionally needs measured end-to-end latency, VRAM
under representative game load, artifact/license provenance, clean unload and
device-loss behavior, and explicit user selection. Model Manager must account for
the game budget before activation; no pack may claim VRAM merely because it fits
on an idle GPU.

## Implemented quality path

The strongest non-generative extension is now implemented in the native worker:
a track-local multi-reference mouth atlas. Retain sharp, pose-compatible mouth
crops observed from the same authorized character; index them by canonical
viseme, aperture, width, yaw, and lighting; then warp the nearest compatible
source textures into the current mouth geometry. This reveals real previously
observed teeth without inventing a new identity. The worker enforces
actor/generation binding, bounded state count and dimensions, premultiplied
pixels, pose-aware selection, current-frame validation, and synchronous
cancellation. The moving-frame evidence and remaining product boundary are
recorded in the [v5 proof](headless-realistic-lipsync-proof-2026-09-04-v5.md).

## Sources

Primary documentation and official project repositories were preferred. Issue
links are included only where they expose practical behavior absent from headline
benchmarks.

1. [NVIDIA AR SDK: using LipSync](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/using-ar-features.html)
2. [NVIDIA AR SDK LipSync properties](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/properties.html)
3. [NVIDIA Windows AR SDK performance reference](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/PerformanceReference.html)
4. [NVIDIA Windows AR SDK installation](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/InstalltheARSDK.html)
5. [NVIDIA Maxine AR SDK samples](https://github.com/NVIDIA-Maxine/AR-SDK-Samples)
6. [NVIDIA Maxine SDK agreement](https://developer.download.nvidia.com/licenses/Maxine_SDK_License_1Apr2021_updated.pdf)
7. [MuseTalk repository](https://github.com/TMElyralab/MuseTalk)
8. [MuseTalk paper](https://arxiv.org/abs/2410.10122)
9. [MuseTalk real-time inference implementation](https://github.com/TMElyralab/MuseTalk/blob/main/scripts/realtime_inference.py)
10. [MuseTalk issue 33: transfer and end-to-end speed](https://github.com/TMElyralab/MuseTalk/issues/33)
11. [MuseTalk issue 310: high-VRAM failure](https://github.com/TMElyralab/MuseTalk/issues/310)
12. [LatentSync repository](https://github.com/bytedance/LatentSync)
13. [LatentSync paper](https://arxiv.org/abs/2412.09262)
14. [KeySync repository](https://github.com/antonibigata/keysync)
15. [KeySync paper](https://arxiv.org/abs/2505.00497)
16. [KeySync issue 29: frame/audio-rate corruption](https://github.com/antonibigata/keysync/issues/29)
17. [Ditto repository](https://github.com/antgroup/ditto-talkinghead)
18. [Ditto model card](https://huggingface.co/digital-avatar/ditto-talkinghead)
19. [Ditto paper](https://arxiv.org/abs/2411.19509)
20. [LivePortrait repository](https://github.com/KlingAIResearch/LivePortrait)
21. [LivePortrait paper](https://arxiv.org/abs/2407.03168)
22. [FasterLivePortrait repository](https://github.com/warmshao/FasterLivePortrait)
23. [DINet paper](https://arxiv.org/abs/2303.03988)
24. [DINet repository](https://github.com/MRzzm/DINet)
25. [Wav2Lip repository and model-use restriction](https://github.com/Rudrabha/Wav2Lip)
26. [VideoReTalking repository](https://github.com/OpenTalker/video-retalking)
27. [Diff2Lip paper](https://arxiv.org/abs/2308.09716)
28. [SyncTalk paper](https://arxiv.org/abs/2311.17590)
29. [EfficientSync paper](https://arxiv.org/abs/2608.18832)
30. [FlashLips paper](https://arxiv.org/abs/2512.20033)
31. [Lip Forcing paper](https://arxiv.org/abs/2606.11180)
32. [Lip Forcing repository](https://github.com/cvlab-kaist/LipForcing)
33. [OmniSync paper](https://arxiv.org/abs/2505.21448)
34. [ReSyncer paper](https://arxiv.org/abs/2408.03284)
35. [Teller paper](https://arxiv.org/abs/2503.18429)
36. [MediaPipe Face Landmarker documentation](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker)
37. [MediaPipe face blendshape model card](https://storage.googleapis.com/mediapipe-assets/Model%20Card%20Blendshape%20V2.pdf)
38. [Meta XR Audio SDK documentation](https://developers.meta.com/horizon/documentation/unity/audio-ovraudio-lipsync-unity/)
39. [uLipSync repository](https://github.com/hecomi/uLipSync)
40. [Rhubarb Lip Sync repository](https://github.com/DanielSWolf/rhubarb-lip-sync)
