# Real-video native tracking correction

Date: 2026-09-05

Status: real moving-person tracking replay passes; renderer quality remains a separate gate

Snapshot notice: this report covers the earlier per-sampled-frame detector path.
The current native policy performs a full YuNet detection every 12 frames and
uses LM1 tracked-ROI updates between refreshes, with explicit loss/high-motion
reacquisition. A later 90-frame Cyberpunk/Misty replay accepted 83 native
packets and preserved all recorded rejections, but it is still an offline
component replay; natural tracking, game contention, capture, and presentation
remain unqualified.

## Reproduction

The held-out Nicole Mann sequence under
`E:\temp\InteractiveNPCs\action-demo-20260905\source\target-frames`
reproduced the native failure deterministically in about 2.2 seconds. The
pre-fix headless proof stopped before output with:

```text
bypass_unsafe_roi detector=0.864673 landmarks=0.894691 visibility=1
```

Targeted instrumentation showed that the face box, raw lip contour, and mouth
corners were valid and face-bound. The exact failing predicate was a closed-mouth
inner-lip inversion: upper mean `0.434731`, lower mean `0.433678`. At the
540-pixel source height, the difference is about 0.57 pixel.

After that rejection was corrected, the longer replay exposed a second failure:
detector confidence fell to `0.693926`, just below the unchanged `0.70` product
floor. The detector's radial box inherited the aspect ratio of each resized
search crop. Feeding that result back on portrait video progressively widened
and flattened the next crop.

## Correction

`windows_ort_landmark_provider.cpp` now letterboxes the complete caller-supplied
actor seed into the detector's square input and maps the radial result back with
one uniform pixel scale. The padding uses the model's normalized mean value.
This preserves every pixel in the identity-authorized search region, works for
portrait and landscape inputs, and prevents recursive aspect distortion. It
does not crop an off-center face, search outside the seed, or reduce the detector
floor.

`signal_adapter.cpp` now treats only an inner-lip inversion no larger than 10%
of the observed lip-contour height as closed-mouth heatmap jitter. It emits an
ordered near-zero semantic aperture while preserving the raw contour. Larger
crossings still return `bypass_unsafe_roi`.

The pinned [OpenSeeFace tracker](https://github.com/emilianavt/OpenSeeFace/blob/85aa70fc67582d046e771ea73625182a0d8f7475/tracker.py)
confirms that this detector consumes a square 224x224 input and predicts a
radial box. Upstream stretches a one-shot full-frame scan and maps its axes back
independently. That is not stable when each mapped box recursively shapes the
next crop, so this repeatedly seeded product path preserves aspect with the
letterbox transform.

## Headless result

The post-fix 10 Hz replay processed the complete 9.38-second input:

- 301 moving source frames available;
- 282 output frames requested;
- 282 residual frames produced;
- no provider-inference or signal-adapter bypass;
- detector confidence `0.766` on the final packet;
- landmark confidence `0.885` on the final packet.

Additional model-backed probes detected the same real face in six cases: native
420x540 portrait and padded 960x540 landscape, each centered, left-shifted, and
right-shifted. Detector confidences were `0.91-0.93`, and all six packets passed
the unchanged runtime adapter. This specifically checks that the letterbox path
does not discard valid off-center content.

The native proof still exits with its separate failed quality/performance status:
moving inference p95 was `61.851 ms` against its `60 ms` budget, and the current
mouth renderer remains visually unqualified. This correction establishes tracker
continuity only.

Run `scripts/diagnostics/tracking-quality-nicole-replay.ps1` with the locally
admitted pack, held-out frames, WAV, same-identity atlas, native proof executable,
and a fresh `E:\temp` output directory to regenerate machine-readable evidence.
