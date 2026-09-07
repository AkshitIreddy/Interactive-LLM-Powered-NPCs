# Native landmark provider diagnosis (2026-09-07)

## Decision

The current native YuNet 640 + OpenSeeFace LM1 provider remains unqualified for
`johnny-silverhand`. Keep the existing `0.82` whole-face landmark-confidence
gate and preserve the source frame whenever that gate or a later appearance gate
rejects a frame. Do not install LM4 or CLAHE as a product default from this
experiment.

LM4 with full-frame CLAHE is useful follow-up evidence, but it accepts only 75
of Johnny's 90 frames and its enlarged landmark sequence remains visually
unstable. A high model score is not proof that the mouth contour matches the
visible lips: the Misty sequence scores above the gate while visibly bunching
mouth points into a central mass.

## Reproduction boundary

This was a bounded, offline CPU replay of the 90-frame native PPM corpora. It
made no provider calls, used no GPU, and did not alter the checked-in native
provider or its thresholds. The replay:

1. Reconstructs each landmark inference box from native dump state. On tracker
   frames where the detector did not run, it uses the preceding locked box,
   which is the box the native provider actually inferred from.
2. Reuses the pinned model allowlist, 224 x 224 tensor construction, model
   decoding, and static packet policy from
   `scripts/diagnostics/prepare-cyberpunk-landmark-replay.py`.
3. Forces ONNX Runtime's `CPUExecutionProvider` with one inter-op and one
   intra-op thread.
4. Applies the unchanged `0.82` whole-face confidence gate. Acceptance here is
   static packet acceptance; it does not reproduce the stateful adapter,
   appearance checks, compositor, capture, or contention.

The source dumps and frames are under
`E:\temp\InteractiveNPCs\integration-20260907`. The exact-input v2 reports are
named `native-input-model-comparison-v2-<actor>-<variant>.json`. Instrumented v3
LM4 + CLAHE reports and enlarged boards use the same folder.

## Result

| Actor / model input | Gate passes | Whole-face mean / minimum | Mouth mean / minimum | ORT inference p95 |
| --- | ---: | ---: | ---: | ---: |
| Johnny, LM1 native input | 0 / 90 | 0.7317 / 0.6709 | 0.6881 / 0.5970 | 15.9 ms |
| Johnny, LM3 native input | 24 / 90 | 0.8007 / 0.7294 | 0.7689 / 0.6604 | 27.6 ms |
| Johnny, LM4 native input | 31 / 90 | 0.8060 / 0.7247 | 0.7562 / 0.6206 | 32.1 ms |
| Johnny, LM4 + CLAHE + 5% margins | 75 / 90 | 0.8519 / 0.7780 | 0.8337 / 0.7032 | 25.3 ms |
| Misty, LM1 native input | 90 / 90 | 0.8726 / 0.8370 | 0.8753 / 0.8374 | 20.5 ms |
| Misty, LM3 native input | 90 / 90 | 0.9163 / 0.8949 | 0.9066 / 0.8651 | 24.9 ms |
| Misty, LM4 native input | 90 / 90 | 0.9160 / 0.8959 | 0.9040 / 0.8529 | 26.7 ms |
| Misty, LM4 + CLAHE + 5% margins | 90 / 90 | 0.9151 / 0.9006 | 0.9128 / 0.8676 | 25.0 ms |
| Claire, LM1 native input | 90 / 90 | 0.8385 / 0.8259 | 0.8507 / 0.8039 | 30.0 ms |
| Claire, LM3 native input | 90 / 90 | 0.8885 / 0.8714 | 0.9070 / 0.8543 | 24.3 ms |
| Claire, LM4 native input | 90 / 90 | 0.8970 / 0.8775 | 0.9075 / 0.8747 | 25.9 ms |
| Claire, LM4 + CLAHE + 5% margins | 90 / 90 | 0.8932 / 0.8709 | 0.9001 / 0.8746 | 23.9 ms |

The ORT column is model inference only from the uncontended v2 runs. It excludes
PPM decoding, photometric processing, tensor construction, decoding, detection,
tracking, adapter, compositor, capture, and contention. It is not native runtime
latency.

The instrumented v3 runs put full-frame CLAHE at 16.1-18.0 ms p95 on these
1920 x 1080 sources. Those runs overlapped other machine work: their full model
path measured 90.2-96.8 ms p95 and inference alone measured 54.2-60.5 ms p95.
They are an overhead boundary, not a product throughput claim. Applying CLAHE
only to the landmark crop should cost less, but changes the transform and must
be treated as a different candidate with fresh visual and timing evidence.

## Diagnosis

The C++ RGB conversion, ImageNet normalization, bilinear pixel-center sampling,
and 10% horizontal / 12.5% vertical crop margins agree with the pinned
OpenSeeFace implementation. Replaying LM1 against the reconstructed native
input reproduced native whole-face confidence with maximum absolute error below
`1.37e-7` across all three corpora. An earlier Misty native-versus-Python parity
run also measured mean face-box IoU `0.99999937` and mean mouth-coordinate error
`0.00000639` pixels. This evidence rules out a material native preprocessing or
decoder mismatch on these inputs.

Johnny's face boxes are about 82-115 pixels wide, but larger boxes correlate
with *lower* LM1 whole-face confidence (`r = -0.677`) in this sequence. Detector
refresh frames and tracker-only frames are both weak (means about 0.738 and
0.731). Scale and tracker drift therefore do not explain the failure by
themselves. LM1 has a domain limitation on this dark, low-resolution character
with sunglasses, facial hair, and strong blue lighting.

The enlarged LM4 + CLAHE boards are:

- `johnny-lm4-clahe-m05-enlarged-landmark-board.jpg`
- `misty-lm4-clahe-m05-enlarged-landmark-board.jpg`
- `claire-lm4-clahe-m05-enlarged-landmark-board.jpg`

Johnny's contour is roughly in the right region but deforms and compresses as
the pose changes, including below-gate frames 41 and 58 in the sampled board.
Misty exposes the opposite failure: all sampled frames pass with high confidence
while the 18 mouth points collapse into an implausible central cluster. Claire is
more plausible, though the outer and lower lip contours remain broad enough to
require rendered temporal review. These visual failures prevent qualification
even where the numeric gate passes.

## Current alternatives

| Candidate | What the primary source supports | Fit for this product |
| --- | --- | --- |
| OpenSeeFace LM3 / LM4 | OpenSeeFace ships five ONNX landmark model entries in the pinned tracker. Its README documents model 3 as the slowest, highest-quality supported selection and describes model 1 as a lower-cost model. Code and models are BSD-2-Clause. | Lowest native integration cost and same 198 x 28 x 28 output ABI. LM4 is present in code but is not characterized in the README, and this corpus still rejects 15 Johnny frames after CLAHE. Keep it experimental until native, appearance, temporal, and rendered tests pass. |
| MediaPipe Face Landmarker | Google's current task outputs 478 3D landmarks and optional 52 blendshapes, supports image/video/live-stream operation, and exposes detection, face-presence, and tracking confidence. It does not expose OpenSeeFace-style per-point confidence. | Strong geometry candidate after a YuNet-provided crop, because the full 1920 x 1080 Johnny frames produced 0/90 detections in the isolated Python check. It needs a model-specific evidence contract instead of reusing the `0.82` mean-point gate. Native Windows integration carries build risk: Google's own build guide still labels Windows experimental. |
| 3DDFA_V2 | The official MIT repository includes an ONNX Runtime path, 68-point/dense 3D alignment, and reports about 1.35 ms CPU time for its regressor. Its documented test platforms are macOS and Linux; Windows requires separate build guidance. | Worth an isolated corpus experiment for pose robustness. The vendor timing excludes this product's detector and compositor, and its outputs do not satisfy the current confidence contract without new validation. |
| PIPNet | The official MIT repository describes robust, efficient landmarking and supplies PyTorch training/demo code. Its demo uses a modified FaceBoxes detector and compiled NMS; native ONNX implementations are community projects. | Promising research option, but packaging and evidence work are larger than an OpenSeeFace model swap. Do not import community ONNX weights without pinning provenance and license. |
| InsightFace model zoo | The code repository is MIT, but its model-zoo page says all provided models are for non-commercial research only. | Exclude the public pretrained packs from downloadable product packs unless a separately licensed model is obtained. |

Primary references:

- [OpenSeeFace README and model guidance](https://github.com/emilianavt/OpenSeeFace/blob/85aa70fc67582d046e771ea73625182a0d8f7475/README.md)
- [Pinned OpenSeeFace tracker model list and preprocessing](https://github.com/emilianavt/OpenSeeFace/blob/85aa70fc67582d046e771ea73625182a0d8f7475/tracker.py)
- [Google Face Landmarker task](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker)
- [Google's MediaPipe Windows build guide](https://developers.google.com/edge/mediapipe/framework/getting_started/install#installing_on_windows)
- [Official 3DDFA_V2 repository](https://github.com/cleardusk/3DDFA_V2)
- [Official PIPNet repository](https://github.com/jhb86253817/PIPNet)
- [InsightFace model-zoo license notice](https://github.com/deepinsight/insightface/tree/master/model_zoo)

## Next qualification step

If native landmark work continues, add a separately pinned experimental LM4
pack rather than replacing LM1 in place. Test plain LM4 and a crop-local
photometric variant independently at the unchanged gate. Require all of the
following before enabling it for a character pack:

- native output parity against the pinned offline model;
- identity, pose, occlusion, and appearance rejection across all three corpora;
- enlarged source/landmark sequences with no false lip contour;
- rendered temporal output with source-preserving bypasses on every rejection;
- end-to-end native CPU timing under representative capture and compositor load.

Until then, Johnny must take the source-preserving fallback path on every frame
that lacks separately qualified evidence.
