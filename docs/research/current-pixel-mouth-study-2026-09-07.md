# Current-pixel mouth comparison — September 7, 2026

Status: **private offline comparison; visual quality and native integration remain unqualified**.
The owner clarified that the rejected cheap appearance concerned the lip-sync
video, not the control UI. This work changes benchmark tooling only. It does
not change or rebuild the immutable v17 application, enable a visual runtime,
publish a pack, push a branch, or release anything.

## What changed

The rejected full-lip atlas copied a foreign lip/skin annulus. This experiment
deforms the current frame's own visible lip surfaces and inserts an optional
reference strictly inside an eroded inner aperture. Contact occludes the oral
surface instead of squeezing teeth into a one-pixel line. Geometry is sampled
from the untouched current frame; prior rendered RGB never enters tracking or
the next output.

Misty exposed a second, independent error: MediaPipe assigned an approximately
9-pixel cavity to closed lipstick in standing views. On source frames 54 and
68 the visible contact ridge is at most about one pixel. Closing the false
cavity removed real lower vermilion. The optional source-edge experiment uses
signed current-pixel gradients, a bounded dynamic-programming trace, and lip
surface ordering to reject that false aperture and retain the visible lips.
It is a contrast-dependent experiment selected for Misty, not a general lip
segmentation model or a per-frame hardcoded contour.

Continuous shape-preserving cue interpolation replaces abrupt category jumps.
The full utterance's cue schedule is known offline; this version **does not
claim the earlier 40 ms streaming lookahead**. Current-frame translation, roll,
and scale remain immediate while local contour shape is damped separately.
An optional one-frame forward/backward-flow geometry bridge rejects cuts,
untextured sources, excessive deformation, and insufficient feature support.
It is not semantic occlusion detection and did not recover Misty's one missed
observation in the final sequence.

Automatic enrollment contours also included external generated lipstick.
Misty and Claire therefore use source-hash-bound, manually inspected interior
contours. Johnny's generated reference is rejected for insufficient resolved
oral detail; his final mode uses current pixels only. Generated references
remain private hypotheses about anatomy, not observed game teeth.

## Final comparison and exact sources

Artifact root: `E:\temp\InteractiveNPCs\multicharacter-quality-20260907`.

- Video: `comparison-v10\three-character-current-pixel-comparison.mp4`
- Verification: `comparison-v10\verification.json`
- Recipe: `review-v10-manifest.json`
- Exact renderer snapshot: `renderer-v10.py`
- Corpus provenance: `FINAL-CORPUS-RECEIPT.md` and `provenance.json`
- Private reference prompts/disposition: `GENERATED-INTERIOR-PROVENANCE.md`
- Per-character output, geometry, runtime versions, and frame timing:
  `<character>\interior-v10\report.json` and `geometry.json`

The video contains 270 verified frames at 1920×1080/30 fps: three seconds each
of Misty, Claire, and Johnny. Each pair shows the same original source crop on
the left and the experiment on the right. The same three-second stock Sarah
speech repeats for comparison; it is not character voice casting. Original
game audio is removed. The PNG sequence remains the authority for pixel
equality, since the viewable video is lossy H.264/AAC.

Video SHA-256:
`73b380505c38145840816ec11fa079a7d3d41f215b457fcd447238862ea5038f`.
Audio SHA-256:
`2bcca3d0e776a0cc0f7d3991411c0154807f1f58a9082f2bdb9fa6942b1311fa`.
Renderer SHA-256:
`61a4eab8c90066785c6bfb5c7e96af668c8e64153094f37152be2b5df6ac13b4`.

All sources come from the owner's designated Cyberpunk video,
`Uc3OXiFjsSg`. The final comparison uses the first 90 frames of each selected
corpus sequence:

| Character | Source sequence | Mode and boundary |
| --- | --- | --- |
| Misty | `misty\source-frames`, beginning at 23.250 s | Current lips, source-edge correction, manually bounded generated interior; one missed geometry observation preserves source. |
| Claire | `claire\source-frames`, beginning at 6.000 s | Current lips and manually bounded generated interior; very dark, small mouth is a stress case, not an anatomy-quality pass. |
| Johnny | `johnny\source-frames-v2-no-hand`, beginning at 47.750 s | Current-pixel warp only. Frames 14–27 inclusive are manually annotated smoke exclusions and remain untouched. |

The earlier Johnny `source-frames` directory contains hand-obstructed footage
and is rejected as primary evidence. The final smoke annotation and exact
boundary-frame boards are retained under `johnny\manual-visibility-review`.
Manual exclusion must not be described as successful automatic occlusion.

## Measured result

The three final renders ran sequentially on the CPU. No local GPU model,
visible window, desktop capture, or audio playback was used.

| Character | Changed / 90 frames | Source-identical | CPU geometry + render p95 | Render alone p95 |
| --- | ---: | ---: | ---: | ---: |
| Misty | 80 | 10 | 25.641 ms | 7.994 ms |
| Claire | 81 | 9 | 24.678 ms | 9.234 ms |
| Johnny | 66 | 24 | 24.493 ms | 8.289 ms |

All 270 source/output pairs were decoded and checked. There are **zero changed
pixels outside each declared support region**, and every bypass is byte-exact
to its source. Johnny's 24 unchanged frames include 14 manual smoke exclusions;
an admitted extremely small deformation can also produce no changed pixel.
The renderer's admitted-frame count is therefore not the changed-frame count.

Timing includes CPU landmark work, source-edge refinement where selected,
cue evaluation, and compositing. It excludes image decoding/encoding, cue
extraction, capture, speaker output, display, and concurrent game load. These
numbers are not response-to-first-audio latency, live frame-rate certification,
or a claim that the application now runs this renderer.

Runtime: Python 3.12.2, OpenCV 4.11.0, NumPy 1.26.4, MediaPipe 0.10.21,
SciPy 1.17.1. The render reports separately expose the global inverse-map
Jacobian and the lip-surface minimum; expansion of the separately replaced
cavity must not be confused with a folded current-pixel lip surface.

## Verification and reproduction

The 12 synthetic tests cover reference-pixel confinement, source immutability,
contact with full and reduced visual strength, crossed geometry, small mouths,
known optical-flow translation, blank and scene-cut rejection, continuous cue
contact, and rejection of a false mesh cavity through visibly closed lips.
They prove mechanical contracts, not realistic anatomy.

Run `scripts/benchmarks/test_current_pixel_mouth_proof.py` using the isolated
CPU environment at
`E:\temp\InteractiveNPCs\runtimes\mediapipe-landmarks-py\Scripts\python.exe`.
`scripts/benchmarks/render-current-pixel-mouth-proof.py --help` documents fresh
E-only output, explicit face crop, cue, oral-reference, manual-contour,
source-edge, and manual-exclusion inputs. The retained final reports bind the
exact inputs. The assembly command is:

```powershell
& 'E:\temp\InteractiveNPCs\runtimes\mediapipe-landmarks-py\Scripts\python.exe' `
  scripts/benchmarks/assemble-current-pixel-mouth-review.py `
  --manifest 'E:\temp\InteractiveNPCs\multicharacter-quality-20260907\review-v10-manifest.json' `
  --output 'E:\temp\InteractiveNPCs\multicharacter-quality-20260907\new-review-directory' `
  --ffmpeg 'C:\ffmpeg\bin\ffmpeg.exe' --ffprobe 'C:\ffmpeg\bin\ffprobe.exe'
```

The assembler rejects missing/mismatched frames, altered bypasses, support
escapes, changed-pixel receipt mismatches, and wrong encoded frame counts.
It records the final media hash and probe and never opens a player.

## Remaining quality work

The broad pasted annulus and the diagnosed standing-view lower-lip collapse
are improved. This remains a small two-dimensional deformation model with
limited anatomy and pose coverage. Misty's source corner highlight remains;
the missed observation still briefly restores the source. Claire's dim oral
detail is not enough to qualify natural teeth/tongue rendering. Johnny has no
admitted oral reference, and his manually excluded smoke cannot qualify an
automatic live route. None of these results justify default activation or
conversion into a downloadable public pack.

The [method review](multicharacter-mouth-method-review-2026-09-07.md) records
current alternatives and future qualification. A further audit separated the
historical MuseTalk 1.5 fresh-frame total of 429.725 ms from its synchronized
FP16 batch-one UNet slice of 163.683 ms; encode/UNet/decode p95 sums to
274.211 ms before capture/compositing. That measured full-frame implementation
misses the hot-path budget. It does not establish that compact neural oral
rendering is impossible. Rehydration and GPU-resident profiling are distinct
future experiments, not work performed by this CPU comparison.
