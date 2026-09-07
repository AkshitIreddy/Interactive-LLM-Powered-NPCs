# Moving-character lip-sync quality decision

**Research date:** 2026-09-05
**Scope:** source-preserving lip editing for a moving captured character, with rigged-character and generated-portrait paths kept separate
**Evidence rule:** primary papers, author repositories/model cards, and provider or platform documentation only. Published throughput is treated as a hypothesis; no paper or vendor FPS number is used as product evidence.

Snapshot notice: this decision predates the schema-3 Cyberpunk/Misty v14 replay.
That replay implements part of the proposed photometric multi-reference path and
improves teeth/rounded articulation, but visible open-mouth exaggeration and a
lighting seam keep it experimental. YuNet now performs a full detector refresh
every 12 frames and tracks the ROI between refreshes. Neither change establishes
natural animation or live-game qualification.

## Decision

Do not spend another iteration tuning the v80 feather, nearest-state weights, or state-hold interval. The rejected blur, upper-lip darkening, flat tooth strip, and limited mouth-shape variety follow from the representation: the runtime chooses one complete atlas patch, bilinearly warps it, and holds discrete selections to suppress flashes. Timing and tracking fixes can stabilize that patch, but cannot invent crisp teeth, tongue, cavity, or intermediate articulation that the selected patch does not contain.

The recommended full method is a **continuous, compositional multi-reference residual**. This is a proposal to test after the current six-state pilot; it is not a description of that implementation:

1. Drive it from provider phoneme/viseme timestamps bound to the playback clock. Keep the PCM classifier only as an explicitly weaker fallback.
2. Enroll several sharp observations for each useful articulation and pose, retaining dense mouth geometry plus separate exterior-lip, teeth, cavity, and tongue layers.
3. Retrieve three to five pose-compatible references, but compose anatomy by confidence rather than averaging whole mouth patches. A closed-mouth reference must not contribute teeth; an `FV` reference must preserve the lower-lip/upper-incisor contact; rounded and spread shapes must not be blended into a generic oval.
4. Warp with a triangulated dense mouth mesh. Match low-frequency colour and illumination separately from high-frequency detail, and keep the transition band narrow and edge aware. Preserve the current source frame outside the residual support exactly.
5. Interpolate continuous geometry and appearance weights. Apply hysteresis only to genuine topology changes such as teeth becoming visible.
6. Fail open to the untouched source when pose, visibility, or reference confidence is inadequate.

This combines the practical parts of EfficientSync's diverse multi-reference texture mixing, FlashLips' separation of lip pose from reconstruction, SyncTalkFace's audio-to-visual memory, and identity-prior renderers without making an unreleased neural model the shipping dependency.

## Proposed full method and the testable pilot

Call the full proposal **CMAR-1**: compositional multi-reference anatomical residual, revision 1. The six-state implementation now being tested selects one oral observation at a time. It can validate source-derived outer-lip preservation and cue timing, but it cannot establish multi-reference mixing, anatomy-layer gating, or continuous appearance synthesis.

### Enrollment contract

- Capture 24–36 sharp same-identity states, not twelve synthetic variations of one softened teacher crop.
- Cover at least silence/closure, `M/B/P`, `F/V`, `TH`, rounded `W/OO`, open `AA`, mid-open `EH`, spread `EE`, and alveolar/palatal tongue or cavity states.
- Store three coarse pose bins where source material permits: left, frontal, right. Store the original sharp frame, mouth mesh, visibility flags, and a quality score for every observation.
- Store teeth, cavity, tongue, lip body, and transition-ring masks independently. A missing anatomy layer is unknown, not transparent evidence that it should be borrowed from another state.
- Reject observations with motion blur, occlusion, material lighting mismatch, or fabricated flat teeth before they enter the atlas.

### Runtime contract

- Convert timed provider cues to a continuous target vector containing closure, aperture, width, rounding/protrusion, lower-lip tuck, tongue/cavity evidence, and voiced energy.
- Use source pose and visibility as hard compatibility gates. Rank compatible observations by articulation distance, sharpness, pose distance, and temporal continuity.
- Deform selected observations to the current dense mouth mesh. Blend low-frequency chroma/illumination, then choose or Laplacian-blend high-frequency anatomy only among topology-compatible references.
- Maintain a confidence value per anatomy layer. Use the current source for any layer below confidence and for all pixels outside the bounded residual.
- Smooth coefficients over the playback clock; do not hold a complete visual state for two frames.

### One bounded comparison

First compare v80 with the implemented six-state oral-residual pilot using the same moving source and audio. If the pilot removes the rejected blur/dark-band failure without adding oral seams, use that result as the baseline for a later CMAR-1 implementation. The utterance must contain repeated bilabial closures, `F/V`, `TH`, a rounded vowel, a spread vowel, and rapid open-close transitions. Produce a real-time output clip plus contact sheets containing at least twelve consecutive frames through the hardest transition.

The existing generated-enrollment set is enough for a **six-state pilot**, not final coverage. It contains neutral/closure, `AA`, `EE`, `F/V`, and two rounded `OO` images with MediaPipe mouth geometry. Use it first to test whether preserving source-derived outer lips while transferring only compatible oral anatomy removes the blur and dark band. It lacks a dedicated `TH`/tongue state, a clearly separate `M/B/P` pressed-lip state, multiple observations per articulation, and pose diversity. Do not synthesize those missing distinctions by renaming the nearest image.

Source-derived outer lips plus an oral-only atlas is a sound first correction because it removes the largest identity and lighting discontinuity. It still fails if “oral-only” means pasting one complete teeth/cavity strip: the flat-tooth artifact will survive inside a smaller mask. The pilot must gate teeth, cavity, and tongue independently, leave unknown regions sourced from the current frame, and keep the source inner-lip boundary when the selected observation is not topology compatible. Exact timed cue dwell should fix truncation of a valid state; it does not supply coarticulation or make two visually similar states distinct. Interpolate geometry around cue boundaries and reserve hard dwell for closure/contact constraints.

The method passes only when all of these hold:

- A reviewer can identify the intended closure, labiodental contact, rounded, spread, and open shapes without seeing the cue labels.
- No frame contains a flat horizontal tooth bar, doubled lip edge, pasted rectangular mouth, or dark upper-lip band.
- Teeth and tongue appear only in compatible articulations and remain anchored through motion.
- Pixels outside the declared residual support are byte-identical to the source; the transition ring has no visible seam at normal playback or in the contact sheet.
- Frame-to-frame state variety is visible. A successful timing trace attached to repeated near-identical mouth images does not pass.
- Native compositor timing and total capture-to-present latency are reported separately. End-to-end wall time that includes model load, pacing, QA, and serialization is not display latency.

Sync scores, identity similarity, landmark error, and sharpness ratios can reject regressions. They cannot override a failed visual review.

## Released comparators and product boundaries

| Candidate | Correct use here | Current qualification | License/resource boundary |
| --- | --- | --- | --- |
| **NVIDIA Maxine AR SDK LipSync** | Gated black-box comparator for direct current-frame editing | The API is the closest documented released match for the actual moving-frame problem. It accepts synchronized images and raw audio, but the official operating envelope requires a visible, well-lit, mostly frontal, unoccluded face and defaults to 14 initial frames. At 30 fps that setting implies about 467 ms of warm-up/look-ahead; this is an inference, not a measured local latency. It is not installed locally and is not publicly downloadable now. | The current NGC Windows package is v0.8.8.1_GA, 1.49 GB, and explicitly requires an NVIDIA AI Enterprise subscription. The old official GitHub URL currently returns 404. A 90-day evaluation is internal testing only; production/distribution depends on the applicable subscription and product terms. An NGC API key authenticates an entitled account but does not create entitlement. Do not block the oral-residual pilot on Maxine access. |
| **NVIDIA Audio2Face-3D** | Rigged 3D test game | Best released path for an authored face rig. Use this to make the rigged test game look good and label it as rig animation. It does not edit arbitrary captured game pixels. | MIT SDK; model uses NVIDIA Open Model License. Official Unreal guidance lists roughly 4.4+ GiB for local v3 diffusion and about 2.9–3.0+ GiB for local v2.3 regressive execution, before the rest of the game. |
| **KeySync** | Offline enrollment teacher experiment | Best next bounded teacher candidate because it explicitly separates keyframes and interpolation and addresses leakage/occlusion. It consumes the clip and audio features rather than providing a causal current-frame API. VRAM is not stated by the authors, so stop the experiment before 10.5 GiB device use on the 12 GiB host. | Apache-2.0 repository/model card. CUDA/PyTorch, WavLM/HuBERT, face preprocessing, and 25 fps assumptions add deployment weight. |
| **JoyGen** | Secondary offline teacher experiment | Public depth-aware source-video editor that predicts 3D expression, renders a depth condition, and synthesizes 256-pixel face crops. It may preserve source motion better than a prepared-avatar path, but the authors publish no VRAM requirement and report testing only V100/A800 GPUs. Its MuseTalk-derived synthesis stack gives no reason to prefer it over KeySync as the first bounded test. | Apache-2.0 repository. Complex dependencies include HuBERT, BFM assets, nvdiffrast, DWPose, face parsing, a VAE, and several separately hosted checkpoints; each dependency and checkpoint needs review. |
| **MuseTalk 1.5** | Existing direct neural baseline | Already disqualified locally for the product path: fresh-frame p95 was about 430 ms before the final game-present path, device-total memory peaked around 7.6 GiB, and the resulting mouth remained soft. Its documented real-time mode prepares an avatar and reuses source latents; that does not prove current moving-frame editing. | MIT code; model permits commercial use; all bundled dependencies and weights still require their own review. |
| **LatentSync 1.6** | Offline high-quality teacher/comparator | The release explicitly targets blurry lips and teeth, but the official minimum inference memory is 18 GiB. It cannot be the next 12 GiB test. Version 1.5 fits a smaller card but is the blurrier generation. | Apache-2.0 code, OpenRAIL++ weights. |
| **Lip Forcing 14B** | Research reference | Excluded on this host. The author repository states about 37 GiB for inference with precomputed text embeddings and about 50 GiB when the text encoder is loaded. | Apache-2.0 repository; base and vendored model terms remain separate. |
| **EfficientSync / FlashLips** | Architecture references for CMAR-1 | Both directly attack redundant full-frame generation. EfficientSync's diverse reference texture mixing and FlashLips' compact reconstruction plus low-dimensional lip-pose control are the closest conceptual matches. No public runnable code and weights were located from the paper/author links on the research date. | Runtime, causal behavior, Windows support, weights, and model license are unqualified. Do not turn author FPS into a local claim. |
| **DINet / VideoReTalking** | Offline baselines only | DINet's author repository warns that its limited training data generalizes mainly to frontal, normally lit inputs. VideoReTalking is a multi-stage offline pipeline with an explicit extreme-pose limitation. Neither is a robust fresh-frame game runtime. | DINet repository does not provide a clear shipping license. VideoReTalking code is Apache-2.0, but its several model dependencies need individual review. |
| **LivePortrait / FasterLivePortrait / Ditto** | Portrait replacement or warping references | Useful deformation and streaming references. They render a portrait representation rather than preserving arbitrary source frames, so success cannot be reported as the requested game capture capability. | LivePortrait code is MIT but its default InsightFace models are noncommercial. FasterLivePortrait code is MIT with separate model/plugin constraints. Ditto is Apache-2.0. |
| **Wav2Lip** | Historical sync-control baseline | Not a shipping option. The public weights are restricted to personal/research/noncommercial use because of their training data, and its lower-face replacement is not the current quality bar. | Noncommercial public weights. |

## Existing local evidence and runtime inventory

- The current v80 proof reports 2.650 ms compositor p95 and zero GPU use, while its OpenSeeFace moving-tracking p95 is 60.402 ms. That means the poor mouth cannot be explained by an expensive compositor; tracking is a separate bottleneck.
- v80 selected one discrete atlas state at a time and deliberately held it to prevent tooth flashes. The atlas path is fast enough to evolve, but the whole-patch state model is the wrong quality primitive.
- The current branch now letterboxes the authorized tracking crop without stretching it and maps model coordinates back through the recorded scale and padding. It also bypasses atlas dwell for exact timed viseme cues, with a focused unit test for the short-cue case. These fixes address tracking geometry and cue truncation respectively; neither proves that the selected mouth texture looks natural.
- The local MuseTalk fresh-frame qualification measured approximately 429.725 ms p95 and 7,594 MiB peak device-total memory. This host already contains the resulting teacher frames and native atlas artifacts.
- The existing `mara-imagegen-mouth-v1` enrollment images are high-resolution and include genuinely useful spread, open, rounded, and labiodental examples. They provide one observation each, all near frontal, with no explicit anatomy masks. Their resolution is promising, but the set is not yet evidence of multi-reference or pose-robust performance.
- The inspected `E:\temp\InteractiveNPCs\runtimes` contains MediaPipe, OpenCV identity, OpenSeeFace, and mouth-proof Python environments. The inspected installed NVIDIA directories contain ordinary NVIDIA tooling and CUDA, but no Maxine AR SDK runtime was located. A Maxine experiment therefore has setup cost and must remain a separate bounded comparator.

These are the relevant local records: [independent moving-lip-sync audit](./independent-moving-lipsync-2026-09-05.md), v80 proof JSON at `E:\temp\InteractiveNPCs\action-demo-20260905\mara-native-v80-sarah\headless-proof.json`, and MuseTalk qualification JSON at `E:\temp\InteractiveNPCs\local-model-tests\musetalk-fresh-frame-mara-20260905-v2\qualification.json`. All three paths were rechecked on 2026-09-05.

## Timing cues: choose native alignment before inferred phonemes

Use cue quality in this order:

1. Cartesia WebSocket IPA phoneme timestamps or Inworld synchronous phoneme/viseme timestamps.
2. Azure viseme offsets and blendshape frames, if Azure becomes a supported provider.
3. Polly viseme speech marks for Polly output.
4. ElevenLabs character alignment plus caller-supplied phoneme markup when available. Character fragments are not phonemes and must not be relabeled as such.
5. Rhubarb for deterministic prerecorded/offline alignment, especially when text is known.
6. uLipSync-style MFCC or the existing spectral bands only as a live fallback with an honest reduced-quality label.

Provider timestamps must be consumed against the samples actually released by the playback buffer. Network receipt time and synthesis wall time are the wrong clocks for visible articulation.

## Evidence ledger

Each numbered item is a distinct primary source assessed for this decision.

1. **EfficientSync paper (2026)** — local masked editing, reference selection, and multi-reference texture mixing directly motivate CMAR-1; executable release status remains unqualified. [arXiv](https://arxiv.org/abs/2608.18832)
2. **FlashLips paper (2025)** — separates a reconstruction editor from a low-dimensional lip-pose predictor and uses mask-free inference; useful architecture, not a located runnable dependency. [arXiv](https://arxiv.org/abs/2512.20033)
3. **Lip Forcing paper (2026)** — few-step autoregressive diffusion improves temporal context but remains a large generator. [arXiv](https://arxiv.org/abs/2606.11180)
4. **Lip Forcing author repository** — documents the released 14B memory requirements and Apache-2.0 code. [GitHub](https://github.com/cvlab-kaist/LipForcing)
5. **OmniSync paper (2025)** — mask-free diffusion transformer targeting diverse identities, styles, and occlusions; valuable research comparator without a qualified 12 GiB Windows path. [arXiv](https://arxiv.org/abs/2505.21448)
6. **KeySync paper (2025)** — keyframe generation plus interpolation and explicit leakage/occlusion handling make it a promising enrollment teacher. [arXiv](https://arxiv.org/abs/2505.00497)
7. **KeySync author repository** — exposes the CUDA/PyTorch, audio-feature, face-preparation, and 25 fps pipeline and an Apache-2.0 license. [GitHub](https://github.com/antonibigata/keysync)
8. **KeySync author model card** — confirms released checkpoints and intended inference path; no safe VRAM ceiling is specified. [Hugging Face](https://huggingface.co/toninio19/keysync)
9. **MuseTalk paper (2024)** — latent inpainting of a 256-pixel face region; a relevant baseline whose local result is now stronger evidence. [arXiv](https://arxiv.org/abs/2410.10122)
10. **MuseTalk author repository** — its real-time mode prepares an avatar and reuses material, which is different from arbitrary fresh moving frames. [GitHub](https://github.com/TMElyralab/MuseTalk)
11. **LatentSync paper (2024)** — audio-conditioned latent diffusion and temporal layers; quality reference rather than current runtime. [arXiv](https://arxiv.org/abs/2412.09262)
12. **LatentSync author repository** — v1.6 addresses blurry lip/teeth output while documenting an 18 GiB minimum inference requirement. [GitHub](https://github.com/bytedance/LatentSync)
13. **LatentSync model card** — OpenRAIL++ model terms must be carried separately from Apache-2.0 code. [Hugging Face](https://huggingface.co/ByteDance/LatentSync)
14. **Wav2Lip paper (2020)** — landmark audio-visual synchronization discriminator; still useful for a regression score, not for current rendering quality. [arXiv](https://arxiv.org/abs/2008.10010)
15. **Wav2Lip author repository** — public weights are restricted to research, academic, or personal use because of LRS2. [GitHub](https://github.com/Rudrabha/Wav2Lip)
16. **DINet paper (2023)** — deformation plus inpainting with multiple references supports dense deformation and reference diversity. [arXiv](https://arxiv.org/abs/2303.03988)
17. **DINet author repository** — warns that the small training set limits generalization to mainly frontal, normal-light inputs; no clear repository license was found. [GitHub](https://github.com/MRzzm/DINet)
18. **VideoReTalking paper (2022)** — canonical expression, lip synchronization, and enhancement form a useful offline comparison, but the stages do not fit a tight live frame budget. [arXiv](https://arxiv.org/abs/2211.14758)
19. **VideoReTalking author repository** — Apache-2.0 code with multiple model dependencies and a stated extreme-pose limit. [GitHub](https://github.com/OpenTalker/video-retalking)
20. **Diff2Lip paper (2023)** — diffusion inpainting is a quality reference but not evidence of bounded live execution. [arXiv](https://arxiv.org/abs/2308.09716)
21. **StyleSync paper (2023)** — mask-guided detail and per-person style adaptation support identity-specific enrollment rather than a universal pasted mouth. [arXiv](https://arxiv.org/abs/2305.05445)
22. **ReSyncer paper (2024)** — personalized lip-style adaptation reinforces identity-specific motion, but its renderer does not preserve current arbitrary source pixels. [arXiv](https://arxiv.org/abs/2408.03284)
23. **SyncTalkFace paper (2022)** — its audio-lip memory is direct prior art for retrieving identity-specific visual articulation from audio cues. [arXiv](https://arxiv.org/abs/2211.00924)
24. **Identity-preserving talking-face priors paper (2023)** — separates audio-to-landmark motion from appearance-prior rendering, supporting the CMAR-1 motion/appearance split. [arXiv](https://arxiv.org/abs/2305.08293)
25. **LivePortrait paper (2024)** — implicit keypoint deformation and stitching are useful warp references, but the system animates a portrait representation. [arXiv](https://arxiv.org/abs/2407.03168)
26. **LivePortrait author repository** — MIT code, with default InsightFace models called out as noncommercial. [GitHub](https://github.com/KlingAIResearch/LivePortrait)
27. **FasterLivePortrait repository** — documents Windows/TensorRT portrait inference and separate model/plugin constraints; this remains a portrait path. [GitHub](https://github.com/warmshao/FasterLivePortrait)
28. **Ditto paper (2024)** — streaming portrait generation with chunked audio is relevant to scheduling, but it replaces the source portrait. [arXiv](https://arxiv.org/abs/2411.19509)
29. **Ditto author repository** — released Apache-2.0 implementation; use only under the portrait-product label. [GitHub](https://github.com/antgroup/ditto-talkinghead)
30. **NVIDIA Maxine AR LipSync properties** — defines synchronized image/audio inputs, 16 kHz mono audio, 14 initial frames, visible-face and pose constraints. [NVIDIA documentation](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/properties.html)
31. **NVIDIA Maxine AR system guide** — establishes supported Windows and NVIDIA GPU families and describes development/redistributable packages; its other feature timings are not LipSync timing evidence. [NVIDIA documentation](https://docs.nvidia.com/deeplearning/maxine/ar-sdk-system-guide/index.html)
32. **Audio2Face-3D paper (2025)** — validates a separate audio-to-3D-face-animation product class rather than captured-pixel editing. [arXiv](https://arxiv.org/abs/2508.16401)
33. **Audio2Face-3D SDK repository** — Windows C++/CUDA SDK released under MIT. [GitHub](https://github.com/NVIDIA/Audio2Face-3D-SDK)
34. **Audio2Face-3D model card** — released ONNX model under the NVIDIA Open Model License. [Hugging Face](https://huggingface.co/nvidia/Audio2Face-3D-v3.0)
35. **Audio2Face-3D Unreal documentation** — gives local model memory guidance and lifecycle controls for a rigged test game. [NVIDIA documentation](https://docs.nvidia.com/ace/ace-unreal-plugin/latest/ace-unreal-plugin-audio2face.html)
36. **Cartesia WebSocket TTS documentation** — can return IPA phoneme arrays with start/end timestamps while streaming. [Cartesia documentation](https://docs.cartesia.ai/api-reference/tts/websocket)
37. **Cartesia endpoint comparison** — persistent WebSockets amortize connection setup and support continuations and timestamps. [Cartesia documentation](https://docs.cartesia.ai/use-the-api/compare-tts-endpoints)
38. **Inworld timestamp documentation** — exposes phoneme timing and ten viseme categories, with synchronous alignment intended for real-time use. [Inworld documentation](https://dev.docs.inworld.ai/tts/capabilities/timestamps)
39. **ElevenLabs WebSocket API** — returns character alignment; ARPAbet alignment depends on supplied phoneme markup, so generic character fragments must not be treated as phonemes. [ElevenLabs documentation](https://elevenlabs.io/docs/api-reference/text-to-speech/v-1-text-to-speech-voice-id-stream-input)
40. **ElevenLabs real-time WebSocket guide** — documents buffering and chunk scheduling, which must be aligned to playback rather than response receipt. [ElevenLabs documentation](https://elevenlabs.io/docs/eleven-api/guides/how-to/websockets/realtime-tts)
41. **Azure viseme documentation** — exposes 22 viseme IDs with audio offsets and optional 60 fps blendshape frames. [Microsoft documentation](https://learn.microsoft.com/en-us/azure/cognitive-services/speech-service/how-to-speech-synthesis-viseme)
42. **Amazon Polly viseme documentation** — exposes viseme speech marks as a provider-native cue path. [AWS documentation](https://docs.aws.amazon.com/polly/latest/dg/viseme.html)
43. **Rhubarb Lip Sync repository** — MIT, Windows-capable offline cue generation with dialogue-guided recognition and machine-readable output. [GitHub](https://github.com/DanielSWolf/rhubarb-lip-sync)
44. **uLipSync repository** — Unity MFCC analysis provides a reasonable audio-only fallback and pre-bake path, but it cannot provide provider-grounded phoneme timing. [GitHub](https://github.com/hecomi/uLipSync)
45. **MediaPipe Face Landmarker documentation** — 478 3D landmarks, 52 blendshapes, transforms, and tracked video/live-stream modes make it a stronger dense-geometry candidate than a sparse mouth box. [Google documentation](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker)
46. **OpenSeeFace repository** — BSD-2-Clause CPU tracking remains a useful fallback and current baseline, with a custom 66-point topology. [GitHub](https://github.com/emilianavt/OpenSeeFace)
47. **NVIDIA Optical Flow SDK** — dedicated Windows optical-flow hardware can propagate a bounded mouth region between detector frames, but local latency and failure under occlusion must be measured. [NVIDIA developer documentation](https://developer.nvidia.com/optical-flow-sdk)
48. **SEA-RAFT repository** — BSD-3-Clause reference flow estimator suitable for offline quality comparison, not assumed live deployment. [GitHub](https://github.com/princeton-vl/SEA-RAFT)
49. **Maxine Windows AR SDK catalog entry** — the current 1.49 GB v0.8.8.1_GA artifact is subscription-gated; public documentation does not make it a public runnable comparator. [NVIDIA NGC](https://catalog.ngc.nvidia.com/orgs/nvidia/maxine/resources/maxine_windows_ar_sdk_ga/-?_lr=1)
50. **NVIDIA AI Product Specific Terms** — developer-program and trial access is limited to internal evaluation/testing, while production and distribution require the applicable licensed grants and conditions. [NVIDIA agreement](https://www.nvidia.com/en-us/agreements/enterprise-software/product-specific-terms-for-ai-products/)
51. **JoyGen paper (2025)** — separates audio-to-3D expression motion from depth-aware visual synthesis for source-video editing. [arXiv](https://arxiv.org/abs/2501.01798)
52. **JoyGen author repository** — public Apache-2.0 code uses a multi-stage 256-pixel pipeline, reports V100/A800 test GPUs, and does not publish an inference-memory ceiling. [GitHub](https://github.com/JOY-MM/JoyGen)

## Immediate order of work

1. Visually compare the implemented six-state oral-residual pilot against v80 using the existing native path: source-derived outer lips, one selected oral observation, and exact timed cues that bypass estimated-drive dwell. This directly tests the owner's rejected blur and cue-truncation defects without pretending the pilot implements multi-reference synthesis. Expand to CMAR-1 only if this representation first wins visually.
2. If this account already has NVIDIA AI Enterprise entitlement, run one separately labeled Maxine AR SDK qualification: same source/audio, local GPU memory, first editable frame, p50/p95/p99 processing time, warm-up, sustained frame pacing, failure on pose/occlusion, and inspected contact sheets. Do not request a subscription or wait for Maxine before continuing the native method.
3. Run KeySync only as a bounded offline-teacher experiment with a 10.5 GiB stop limit. Its output must outperform the current teacher in tooth/tongue structure and sharpness before any atlas regeneration.
4. Use Audio2Face-3D for a rigged-character test game if a suitable face rig is available. Present that result as rig animation, not proof of arbitrary moving-frame editing.

The shippable claim remains narrow until visual proof passes: the product can preserve and edit the tested source under its qualified pose and visibility envelope. It must not claim universal arbitrary-game lip sync from a portrait-generator demo, a rigged-character demo, or a paper's throughput figure.
