# Real-time talking-face selection for Interactive NPCs 2.0

Status: implementation decision plus qualification plan; not a pack-admission or
installed-app claim

Evidence refreshed: 2026-09-02 UTC

Target: Windows 10/11, NVIDIA RTX, game-agnostic capture/overlay, API-first
LLM/STT/TTS, optional local talking-face model

## Decision

There is no single downloadable model that has proved all of these at once:

- a previously unseen still or moving game face;
- SadTalker-like head motion and high-quality lip motion;
- true incremental audio input and bounded first-frame latency;
- native Windows/NVIDIA packaging;
- coexistence with a game inside a 12 GB VRAM budget;
- a redistributable dependency and weight set; and
- safe current-frame, actor, cancellation, and occlusion binding.

The measured answer is that a full generative model should not run for every
game frame. The 2.0 design therefore uses capability tiers:

1. **Default current-game-face path:** enroll each user-confirmed character into
   a tiny **mouth atlas**. A teacher generates a phonetically balanced clip once;
   the enrollment job clusters it into 8–12 feathered mouth residuals, binds the
   atlas to the portrait/identity revision and unloads the teacher. At runtime,
   provider viseme timestamps or a tiny CPU audio classifier select and blend
   atlas states onto fresh mouth landmarks. No neural visual model is resident.
2. **Enrollment teacher:** MuseTalk 1.5 is retained as an optional local teacher,
   not the gameplay renderer. It can produce useful teeth, tongue and lip
   appearances that geometry alone cannot invent. Hosted talking-photo services
   can implement the same enrollment interface without changing the hot path.
3. **Optional residual refiner:** only if the atlas fails blind visual review,
   train a 1–5M-parameter, 96×64 or 128×96 depthwise U-Net that consumes the
   current mouth crop, neighboring atlas states, blend weights, pose and local
   luminance, and emits a premultiplied RGBA residual. This is a design target,
   not a trained or qualified model.
4. **Full talking portrait:** qualify **Ditto TensorRT** as an experimental
   high-quality mode.  It is the closest open, arbitrary-photo, full-head
   replacement for SadTalker, but its official deployment is CentOS/A100/TRT
   8.6 and its native Windows plugin/runtime path is not product-ready.
5. **Prepared-avatar low-latency mode:** build/cache a short neutral idle/head
   loop for each known character, then run only mouth synthesis during a turn.
   LiveTalking and QuickTalk show why this architecture can be much faster and
   smaller than regenerating the whole portrait for every answer.
6. **High-end watchlist:** FlashLips validates a deterministic latent
   reconstruction plus compact lip-pose driver instead of diffusion, but its
   100-FPS evidence is on H100 and code was not publicly available when checked.
   SoulX-FlashHead Lite has strong streaming results but its PyTorch 2.7/CUDA
   12.8/FlashAttention/NCCL Linux stack and 4090-class evidence keep it out of
   the 12 GB Windows default.
7. **Hosted optional mode:** Runway Characters and LemonSlice can create a
   talking avatar from one image. They return a separate WebRTC video track,
   not a faithful same-frame mouth edit, so they require segmentation, tracking
   and overlay compositing and must be labeled generated-avatar modes.

NVIDIA Audio2Face-3D remains a rig/blendshape signal, not an arbitrary-photo
renderer.  NVIDIA LipSync is semantically much closer because it transforms
video frames, but it is private-access and has no qualified public hosted or
native Windows path for this product.  A general NVIDIA NIM key must not be
presented as granting LipSync access.

## Local measurements: why MuseTalk moved out of the hot path

The realistic Mara Venn run produced 39 frames for 1.56 seconds of audio and
peaked at 7,836 MiB total GPU memory used.  It remains valid evidence that the
stock file-oriented path fails live admission.  It is **not**, however, a
measurement of the official prepared-avatar hot path:

- the complete process included Python startup, loading PyTorch 2.0.1/CUDA
  11.8, loading the 3.4 GB UNet plus VAE/Whisper/face models, landmark work,
  frame-file output, H.264/AAC work and worker bookkeeping;
- the captured progress log shows the 39-frame neural loop itself took about
  7.0 seconds (roughly 5.5 FPS in that path);
- a second 39-frame full-size padding/paste-back loop took about 11.5 seconds
  (roughly 3.4 FPS); and
- the stock official `realtime_inference.py` caches face coordinates, frame
  latents and parsing masks and avoids repeating face preparation for later
  utterances.

The native-Windows prepared-avatar probe then measured the persistent path on
the RTX 4080 Laptop GPU with FP16 PyTorch 2.0.1/CUDA 11.8. The exact DWPose
enrollment crop was recovered from the prior job as `(698,158,1010,564)`; the
probe applies MuseTalk's additional 10-pixel jaw pad, producing an effective
`(698,158,1010,574)` crop.

- cold model load: 14.640 s;
- one-time avatar preparation: 3.666 s;
- whole 1.56-second clip audio feature extraction: 8.572 s;
- batch 1 first batch: 85.017 ms, 10.59 neural/decode FPS and 7.66 complete
  hot-path FPS;
- best tested complete throughput: 8.95 FPS at batch 4, with 166.082 ms first
  batch and 897.714 ms p95 batch latency;
- batch 8 regressed to 8.40 complete FPS with 1,822.617 ms p95 batch latency;
  and
- peak total GPU memory used: 7,617 MiB at 100% GPU utilization before any game
  allocation.

An earlier large-batch stress sequence reached 11,954 MiB total GPU use and
batch 20 collapsed to 0.735 FPS. Its manually supplied crop was wrong, so it is
not quality evidence, but it still demonstrates that offline-size batching is
not a viable latency or memory optimization for this runtime.

Those values reject stock MuseTalk as a resident renderer beside a game on this
12 GB device. The corrected-crop sample animates the intended face and is valid
standalone quality evidence, but the run still does not prove streaming audio,
current-game-frame integration, game coexistence or installed-app behavior.

The project-owned qualification probe at
`scripts/benchmarks/qualify-musetalk-realtime-path.py` measures that exact
question without PNG/video encoding.  It records cold model load, avatar
preparation, audio features, first generated batch, batch p50/p95/p99, neural
decode FPS, CPU composite FPS, combined hot-path FPS, total GPU memory and exact
artifact hashes.  It refuses to run while the shared GPU marker is occupied and
restores the marker in a `finally` path. The measured report is stored outside
the repository under
`E:\temp\InteractiveNPCs\local-model-tests\musetalk-realtime-hotpath-20260902-native-v3-correct-roi`.

## Measured mouth-atlas proof

`scripts/benchmarks/prototype-viseme-atlas.py` converts the already-generated
39-frame MuseTalk teacher clip into a character-specific atlas without using the
GPU or loading MuseTalk. The second native-Windows run produced:

- eight visually distinct states from a bounded 206×143 mouth region;
- a 287,298-byte compressed NumPy proof, 737,282 bytes of deduplicated runtime
  arrays, and a directly loadable 942,656-byte eight-state premultiplied-BGRA
  binary;
- zero changed pixels outside the mouth rectangle by construction;
- 2.464 ms mean / 2.984 ms p95 for the final portable Python/NumPy 26-value
  audio-centroid lookup plus ROI alpha composite, equivalent to about 406 patch
  updates/sec;
- a visual-label upper-bound preview, an audio-label prototype preview, a
  close-up comparison and a six-frame sequence board; and
- 87.18% same-clip nearest-centroid agreement, explicitly not a
  cross-utterance or production viseme-accuracy claim.

The proof is deliberately narrower than product acceptance. It uses one frontal
static portrait and the same short clip for clustering and audio-centroid
evaluation. It does not prove live landmark warping, lighting adaptation,
occlusion/pose handling, current-game-frame presentation, cross-utterance sync,
or installed-app behavior. It does prove that the heavy teacher can be reduced
to hundreds of kilobytes and that runtime mouth-only work is cheap enough to
move to CPU or a tiny D3D11 shader.

The native C++ mouth worker now also contains a disabled-by-default atlas
compositor primitive. It validates two premultiplied canonical states,
interpolates them, bilinearly warps them to the current mouth-corner axis and
emits the existing exact-frame/track-bound residual. A 2026-09-02
RelWithDebInfo microbenchmark measured a 206×143 canonical atlas to 173×71
current-mouth residual at 1.168 ms mean / 1.388 ms p95 over 250 iterations.
All six native mouth-worker tests pass, including premultiplication,
interpolation, malformed-artifact rejection and zero changes outside the mouth
rectangle. Artifact loading, provider state mapping, color adaptation and the
live product route remain intentionally unavailable.

The portable proof and schema are at
`E:\temp\InteractiveNPCs\local-model-tests\mara-viseme-atlas-proof-native-v3-portable`
and `schemas/character-mouth-atlas-v1.schema.json`. The generated manifest and
its hash/size bounds pass PowerShell 7.6's JSON Schema validation.

## Candidate matrix

| Candidate | Real input/output | Best relevant evidence | Windows/NVIDIA reality | License boundary | 2.0 role |
| --- | --- | --- | --- | --- | --- |
| Character mouth atlas | enrolled portrait + 8–12 teacher-derived mouth states + viseme/audio timing → tiny current-frame residual | local proof: 287 KB compressed, 1.792 ms mean and 2.093 ms p95 Python CPU lookup+blend, zero changed pixels outside bounded ROI | no resident GPU model; live D3D11/landmark warp and game coexistence still need product evidence | project-owned artifact format; teacher and source-image rights remain explicit provenance | default architecture candidate; not yet selectable |
| MuseTalk 1.5 | source image/video frames + audio → repainted 256×256 face region | corrected-crop local persistent probe: 7.66 FPS batch 1, best 8.95 FPS batch 4, 7,617 MiB peak before a game; large-batch stress reached 11,954 MiB and 0.735 FPS | official Windows path works, but the measured pinned runtime is categorically outside this game's 12 GB coexistence envelope | MIT code signal and permissive project model-use statement; transitive VAE, Whisper, DWPose, parsing and other artifacts need individual review | optional offline enrollment teacher; reject resident gameplay path |
| FlashLips | current face crop + audio → deterministic reconstructed latent residual | paper reports 109.4 FPS on H100 and introduces a 12-D lip-pose driver with one-step reconstruction rather than diffusion | consumer Windows performance is unknown and public integration code was not found when checked | unresolved until public code and weights exist | architecture reference for a future tiny residual refiner |
| Ultralight Digital Human | person-specific audio features + compact MobileOne-style U-Net → mouth ROI | project reports sub-10 ms ONNX inference on RTX 2080 and uses FeatherHuBERT plus mouth-focused temporal training | promising small-network design but requires person-specific training and is not a drop-in arbitrary current-frame editor | repository is MIT; every training input/checkpoint still needs provenance | implementation reference for optional distilled refiner |
| Ditto | one portrait + audio → full head or upper-body frames | paper: 385 ms first frame and online RTF 0.895 on A100; issue evidence shows roughly 0.5 RTF on RTX 4090 after an import fix | official CentOS/A100/TRT 8.6.1; Windows requires a qualified 3D GridSample plugin DLL or a different renderer path | Apache-2.0 top-level | best full-motion experimental replacement for SadTalker |
| LivePortrait + audio motion | portrait + driving motion → full portrait | official renderer is very fast; FasterLivePortrait reports 30+ FPS including pre/post on RTX 3090 | strongest Windows renderer ecosystem, but stock LivePortrait is not audio-driven; FasterLivePortrait is tied to TRT 8/custom GridSample and has TRT 10/contention risks | MIT for stock LivePortrait; every audio-motion and pretrained-face dependency separately reviewed | renderer beneath a future audio-to-motion model |
| JoyVASA | portrait + audio → LivePortrait motion | tested by upstream on Windows 11/RTX 4060 Laptop 8 GB | direct Windows evidence is useful, but upstream explicitly leaves real-time performance as future work | MIT signal | buffered full-head option, not live default |
| SoulX-FlashHead Lite | portrait + audio → infinite block-streamed portrait video | official 96 FPS/three 25+ streams on 4090; issues report closer to ~25 FPS and extra crop/upscale/paste cost | Linux, CUDA 12.8, FlashAttention/SageAttention/NCCL; no native Windows qualification | Apache-2.0 repo/model signal | 24 GB/high-end experimental watchlist |
| FasterLivePortrait | portrait + video/motion → full portrait | 30+ FPS TRT including pre/post on RTX 3090 | Windows bundle exists; TRT 8 and custom 5D GridSample are fragile; a TRT 10 hybrid fix is promising but independent | root/model/dependency terms require a distribution audit | optional renderer benchmark, not the base contract |
| QuickTalk-style prepared avatar | reusable template + audio → talking template | published project table: 46.9 FPS/1.84 GB peak on 4090, 20.7 FPS/1.40 GB on 3050 Laptop | promising low-VRAM architecture; not an arbitrary-photo motion generator and current dependency chain includes InsightFace weights | dependency/weight review blocks default packaging | architecture reference and future low-VRAM tier |
| Wav2Lip/Wav2Lip256 | existing face frames + audio → mouth-edited frames | maintained LiveTalking reports high throughput, but original reports and quality vary | easy to optimize/stream; older model and low-detail mouth output | official pretrained weights are personal/research/non-commercial | comparator only unless separately licensed weights exist |
| FLOAT / talking-portrait.cpp | portrait + audio → expressive full head | C++/GGML/CUDA port targets 25 FPS output | current C++ port encodes the complete clip before decode; microphone streaming is explicitly future work | FLOAT weights are CC BY-NC-ND | research-only offline comparator |
| LatentSync | existing video + audio → diffusion lip edit | maintained issues show far below real time and high/growing VRAM | 8–18+ GB model/runtime reports; unsuitable beside a 12 GB game | transitive review required | reject live path |
| Hallo2, EchoMimicV2, AniPortrait, EMO | still portrait + audio → full generated video | high visual quality but offline diffusion/animation pipelines | A100/Linux-heavy, no credible bounded interactive path | mixed code/model/dependency terms | offline quality references only |
| Teller, RAP, Livatar-1 | research architectures → streaming portrait | papers claim real-time streaming | no complete maintained public Windows implementation/weights suitable for integration | unresolved until artifacts exist | watchlist only |
| SoulX-FlashTalk 14B | portrait + audio → streaming video | 32 FPS on eight H800s and sub-second startup claim | wholly outside a laptop/game envelope | Apache signal does not make hardware practical | reject local path |
| NVIDIA Audio2Face-3D | audio → rig coefficients/geometry | vendor real-time SDK/microservice | Windows SDK exists; coefficients still need a character rig or project-owned mapper | NVIDIA SDK/model terms | optional motion signal, never arbitrary-photo output |
| NVIDIA LipSync | one-face video + audio → same-resolution video | vendor file/video pipeline, no qualified first-frame metric | private AI for Media access; Linux NIM stack; consumer/game coexistence unproved | private/NGC terms | access-dependent comparator |
| Runway Characters | one image → WebRTC character video | real-time service, no hard renderer latency published | hosted and easy to integrate through official SDK/LiveKit plugin | metered service; provider terms | hosted generated-avatar profile |
| LemonSlice | image bytes/URL → 20 FPS WebRTC portrait | vendor p99 inference TTFB 471 ms at 368×560 | hosted; direct image input is ideal for a crop bake-off | metered service; provider terms | hosted low-latency bake-off |

## Runtime architecture

### Enrollment/cache phase

For each user-confirmed character identity:

1. Canonicalize a stable portrait from the native actor lock, never from a text
   name or whichever detector has the highest confidence.
2. Store the portrait hash, identity revision, face-domain classification and
   consent/provenance alongside the character profile.
3. Synthesize a phonetically balanced 4–8 second calibration phrase in the
   selected character voice. Prefer TTS viseme/phoneme timestamps when the
   provider exposes them; otherwise retain bounded MFCC-like features only for
   clustering and immediately discard raw transient audio according to policy.
4. Run the selected teacher once, crop only the mouth domain, cluster 8–12
   representative states, store premultiplied residuals plus a neutral state,
   pose/lighting envelope, landmark anchors, source hashes and teacher
   provenance, then unload and, if policy requires, delete the teacher runtime.
5. Optionally generate a 4–8 second neutral blink/head-motion loop for prepared
   portrait mode.  This is a separate visual mode, not evidence that the current
   in-game body/pose was preserved.
6. Put all weights, engines, caches and benchmark artifacts under `E:\temp`.
   Package metadata may live in the repo, but no multi-gigabyte runtime belongs
   on C:.

### Turn hot path

1. Keep no heavyweight visual model resident for the default atlas path. Load
   only the selected character's bounded atlas (normally under 1 MiB) and its
   tiny audio centroids. A separately selected residual refiner receives its own
   VRAM admission envelope.
2. Drive states directly from provider viseme timings when available. Otherwise
   run a causal CPU MFCC/viseme classifier with a small look-ahead and explicit
   smoothing/coarticulation; energy/zero-crossing remains the lowest fallback.
3. Use audio playback as the master clock. Timestamp every atlas state,
   drop stale work, cap the queue at one, and never delay speech waiting for a
   late face frame.
4. Affine or piecewise-warp the two nearest atlas states from enrollment anchors
   onto current semantic mouth landmarks; blend between states, adapt local
   luminance/chroma, and reject pose or occlusion outside the enrolled envelope.
5. Transfer the premultiplied lower-face residual through a D3D11 shared
   texture. Keep the
   immutable captured source frame with the broker; the model worker never owns
   presentation authority.
6. Revalidate actor, track epoch, source frame, geometry/device generations,
   audio clock, ROI, occlusion, pose, deadline and cancellation at presentation.
7. On uncertainty, late output, GPU pressure, model failure or an unsupported
   face domain, reveal the untouched game frame within one refresh while audio
   and subtitles continue.

### Runtime order

Implement the cheapest validated path first:

1. provider-timed viseme → two-state atlas blend → D3D11 mouth residual;
2. CPU MFCC/viseme fallback → the same renderer;
3. pose-binned atlas states plus landmark warp and local color transfer;
4. optional 1–5M-parameter ONNX CPU/CUDA residual refiner only after the atlas
   has a measured visual failure it can fix; and
5. heavyweight local/hosted teachers remain enrollment-only tools.

If a refiner is admitted, prefer ONNX Runtime with fixed shapes and I/O binding,
then TensorRT FP16 where it produces a measured benefit. Do not assume
`torch.compile` is a Windows foundation, mix incompatible CUDA stacks on global
`PATH`, or use DirectML as the NVIDIA performance default. TensorRT engine
caches must be keyed by model hash, runtime version, driver, GPU architecture,
precision and shape profile.

## Resource and quality gates

Any neural refiner is selectable only when a current-device signed envelope
satisfies:

`game committed VRAM + resident visual VRAM + p99 workspace/transient VRAM + reserve <= current DXGI budget`

Suggested qualification targets (not current measurements):

- atlas load <=5 ms from warm file cache and <=2 MiB resident per active actor;
- audio/viseme lookup plus warp/composite p95 <=2 ms in the native worker;
- warm audio-window-to-first-visible-frame: target <=80 ms for provider visemes,
  maximum 200 ms for the local audio classifier;
- sustained complete hot path >=60 FPS at the admitted crop size;
- p99 generated-frame age <=40 ms after the playback lead is established;
- audio/visible-mouth skew within +/-40 ms;
- queue depth one, zero stale/wrong-actor/wrong-frame presentations;
- cancellation/occlusion/scene-cut reveal of the untouched frame within one
  display refresh in 100% of injected cases;
- no shared-memory spill, device OOM or silent CPU fallback;
- game p99 frame-time regression <=1 ms and 1%-low regression <=3%; and
- no material identity, moustache, lip-colour, tooth, edge or temporal-jitter
  failures in blind review of the supported face strata.

Run frontal and three-quarter realistic faces, varied skin tones, facial hair,
glasses, small heads, low light, motion blur, partial occlusion, two similar
faces, scene cuts, stylized 3D, anime/low-poly, helmets and non-human faces.
Unsupported domains must fail open; no result can justify “any possible face.”

## Hosted-provider boundary

Hosted avatar services are useful because this product is API-first, but their
output is a newly generated character video.  They cannot silently replace the
local same-frame mode.  A provider-neutral `AvatarRenderer` interface should
describe:

- instant-image versus trained/prebuilt avatar;
- arbitrary/stylized/nonhuman support;
- local versus cloud and exact egress;
- alpha/chroma/segmentation behavior;
- setup time, first frame, FPS, interruption and session cap;
- resolution and identity/pose preservation; and
- estimated price per minute.

Runway Characters and LemonSlice should receive the first identical
portrait/audio bake-off.  NVIDIA NIM remains the recommended consolidated
account for public LLM/STT/TTS/embedding experimentation, subject to rate limits
and no production SLA, but it is not the public talking-photo answer.  Apply for
NVIDIA LipSync private access separately and keep the option unavailable until
the exact entitlement and runtime pass qualification.

## Primary-source and maintained-GitHub ledger

The following sources were opened and assessed directly.  Vendor/paper FPS is
experiment-selection evidence, never a local product benchmark.  GitHub issue
measurements are field reports to reproduce, not controlled acceptance data.

1. [MuseTalk repository, Windows/realtime commands, limitations and license](https://github.com/TMElyralab/MuseTalk)
2. [MuseTalk 1.5 paper](https://arxiv.org/abs/2410.10122)
3. [MuseTalk realtime entrypoint](https://github.com/TMElyralab/MuseTalk/blob/main/scripts/realtime_inference.py)
4. [MuseTalk streaming-audio issue 239](https://github.com/TMElyralab/MuseTalk/issues/239)
5. [MuseTalk practical realtime issue 384](https://github.com/TMElyralab/MuseTalk/issues/384)
6. [MuseTalk 30 FPS issue 376](https://github.com/TMElyralab/MuseTalk/issues/376)
7. [LiveTalking repository, architecture and performance table](https://github.com/lipku/LiveTalking)
8. [LiveTalking MuseTalk avatar-cache generator](https://github.com/lipku/LiveTalking/blob/main/avatars/musetalk/genavatar.py)
9. [LiveTalking releases](https://github.com/lipku/LiveTalking/releases)
10. [Ditto repository and TensorRT runtime](https://github.com/antgroup/ditto-talkinghead)
11. [Ditto paper](https://arxiv.org/abs/2411.19509)
12. [Ditto staged offline streaming pipeline](https://github.com/antgroup/ditto-talkinghead/blob/main/stream_pipeline_offline.py)
13. [Ditto Windows field report](https://github.com/antgroup/ditto-talkinghead/issues/4)
14. [LivePortrait repository and Windows bundle](https://github.com/KlingAIResearch/LivePortrait)
15. [LivePortrait paper](https://arxiv.org/abs/2407.03168)
16. [FasterLivePortrait TensorRT/ONNX/Windows implementation](https://github.com/warmshao/FasterLivePortrait)
17. [TensorRT 10 hybrid LivePortrait warp and contention evidence](https://github.com/RemiEtien/liveportrait-trt10-hybrid-warp)
18. [JoyVASA repository and Windows 4060 Laptop environment](https://github.com/jdh-algo/JoyVASA)
19. [SoulX-FlashHead repository, streaming code and checkpoints](https://github.com/Soul-AILab/SoulX-FlashHead)
20. [FLOAT repository and non-commercial license](https://github.com/deepbrainai-research/float)
21. [FLOAT paper](https://arxiv.org/abs/2412.01064)
22. [talking-portrait.cpp C++/GGML/CUDA FLOAT port](https://github.com/localai-org/talking-portrait.cpp)
23. [Wav2Lip repository and pretrained-weight restriction](https://github.com/Rudrabha/Wav2Lip)
24. [Wav2Lip paper](https://arxiv.org/abs/2008.10010)
25. [Community Wav2Lip TensorRT implementation](https://github.com/wujinzhong/Wav2Lip_TensorRT)
26. [LatentSync repository](https://github.com/bytedance/LatentSync)
27. [LatentSync paper](https://arxiv.org/abs/2412.09262)
28. [VideoReTalking repository](https://github.com/OpenTalker/video-retalking)
29. [EchoMimicV2 repository](https://github.com/antgroup/echomimic_v2)
30. [EchoMimicV2 paper](https://arxiv.org/abs/2411.10061)
31. [Hallo2 repository, requirements and acceleration roadmap](https://github.com/fudan-generative-vision/hallo2)
32. [Hallo2 paper](https://arxiv.org/abs/2410.07718)
33. [AniPortrait repository](https://github.com/Zejun-Yang/AniPortrait)
34. [AniPortrait paper](https://arxiv.org/abs/2403.17694)
35. [EMO repository](https://github.com/HumanAIGC/EMO)
36. [EMO paper](https://arxiv.org/abs/2402.17485)
37. [Live Speech Portraits repository](https://github.com/YuanxunLu/LiveSpeechPortraits)
38. [Live Speech Portraits paper](https://arxiv.org/abs/2109.10595)
39. [Teller paper](https://arxiv.org/abs/2503.18429)
40. [RAP project page](https://markson14.github.io/RAP/)
41. [NVIDIA Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK)
42. [NVIDIA Audio2Face-3D microservice overview](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html)
43. [NVIDIA LipSync NIM overview](https://docs.nvidia.com/nim/maxine/lipsync/latest/overview.html)
44. [Runway Characters](https://docs.dev.runwayml.com/characters/)
45. [Runway official avatar React/core SDK](https://github.com/runwayml/avatars-sdk-react)
46. [LemonSlice Avatar API](https://lemonslice.com/avatar-api)
47. [LemonSlice examples](https://github.com/lemonsliceai/lemonslice-examples)
48. [LiveKit avatar plugins](https://docs.livekit.io/agents/models/avatar/plugins/)
49. [HeadAudio MFCC-to-viseme runtime](https://github.com/met4citizen/HeadAudio)
50. [uLipSync Unity/Burst MFCC implementation](https://github.com/hecomi/uLipSync)
51. [Amoner zero-dependency streaming viseme engine](https://github.com/Amoner/lipsync-engine)
52. [Rhubarb Lip Sync mouth-cue generator](https://github.com/sagpant/rhubarb-lip-sync)
53. [OpenFaceFX NumPy audio/transcript-to-viseme curves](https://github.com/OpenFaceFX/OpenFaceFX)
54. [VisemeNet audio-to-animation curves paper](https://arxiv.org/abs/1805.09488)
55. [Azure Speech viseme IDs and 2D/blendshape timing](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/how-to-speech-synthesis-viseme)
56. [Amazon Polly viseme speech marks](https://docs.aws.amazon.com/polly/latest/dg/viseme.html)
57. [ElevenLabs streaming audio with character timestamps](https://elevenlabs.io/docs/api-reference/text-to-speech/stream-with-timestamps)
58. [Ultralight Digital Human compact person-specific renderer](https://github.com/anliyuan/Ultralight-Digital-Human)
59. [metahuman-stream multi-renderer prepared-avatar host](https://github.com/lipku/metahuman-stream)
60. [IMTalker real-time portrait renderer](https://github.com/bigai-nlco/IMTalker)
61. [OpenCV landmark triangle-warp and seamless-clone tutorial](https://docs.opencv.org/4.x/d1/d52/tutorial_face_swapping_face_landmark_detection.html)
62. [FlashLips paper](https://arxiv.org/abs/2512.20033)
63. [FlashLips CVPR 2026 paper](https://openaccess.thecvf.com/content/CVPR2026/papers/Zinonos_FlashLips_100-FPS_Mask-Free_Latent_Lip-Sync_using_Reconstruction_Instead_of_Diffusion_CVPR_2026_paper.pdf)
64. [Dynamic viseme generation paper](https://arxiv.org/abs/2604.01756)

## Next qualification order

1. Define and validate a versioned character-atlas artifact: portrait and
   identity revision hashes, 8–12 premultiplied states, neutral state, landmark
   anchors, pose/lighting envelope, provider-viseme map, teacher provenance and
   deletion policy.
2. Add the atlas lookup, two-state interpolation, landmark warp and local color
   adaptation behind the existing native mouth-worker residual contract. Keep
   the current queue-one, current-frame, actor, epoch, occlusion, pose, audio
   clock, deadline and fail-open gates unchanged.
3. Generate multi-utterance enrollment/evaluation clips and measure unseen-text
   sync, temporal jitter and blind visual preference across realistic, stylized,
   facial-hair, small-face, low-light and three-quarter-pose strata.
4. Re-run the native product benchmark beside the test game and prove p95 <=2 ms
   atlas work, >=60 FPS presentation, <=2 MiB active atlas memory and no material
   game frame-time regression.
5. Only if atlas artifacts fail a named visual stratum, distill a tiny mouth-ROI
   residual refiner from the teacher and compare ONNX CPU, ONNX CUDA and
   TensorRT FP16 under the same current-frame gates.
6. Keep MuseTalk as an offline enrollment teacher. A newer runtime or TensorRT
   port is optional enrollment-time acceleration, not a prerequisite for the
   gameplay path.
7. Build a Ditto proof behind an experimental full-portrait flag, first in its
   known TRT 8.6 environment and then with a separately qualified Windows path.
8. Add Runway and LemonSlice only behind a clearly separate hosted-avatar
   capability contract; run identical first-frame, sync, identity, mask,
   interruption and cost tests.
9. Promote no visual path into the selectable catalog until its complete pack,
   runtime, license, current-device resource envelope, app route and rendered
   installed evidence pass.
