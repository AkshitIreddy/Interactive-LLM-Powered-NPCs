# Identity-observed moving-frame lip-sync proof

Date: 2026-09-04
Status: **headless component qualified; desktop/live-game presentation remains unverified**

## Why v32 was rejected

The v32 procedural result was visually unacceptable even though its older
numeric gates passed. It held the upper lip nearly static, expanded a dark
source seam into a rigid cavity, and synthesized a flat enamel reflection. The
result looked like a rectangular hole below the real mouth.

A new deterministic artifact audit now rejects that failure family. On v32 it
measured median top/bottom edge-mode shares of `0.730072` / `0.787879` and only
`0.212121` maximum residual height-to-width. Those values fail the new `0.45`
maximum edge-mode and `0.24` minimum opening thresholds.

## Replacement architecture

The replacement does not paint teeth, tongue, lips, or an oral cavity. It:

1. enrolls a small set of real mouth appearances belonging to the confirmed
   character;
2. binds the atlas to the exact actor, identity revision, and cancellation
   generation;
3. chooses states from provider visemes when available, or the causal PCM
   fallback otherwise;
4. warps and blends only a curved mouth residual into the newest tracked game
   frame;
5. adapts only the exterior support ring to local lighting and preserves oral
   pixels; and
6. destroys atlas ownership on generation cancellation.

The expensive neural model is an optional enrollment teacher, not a resident
gameplay model. MuseTalk remains unsuitable for the hot path on the measured
laptop, but it can generate a tiny per-character atlas once. The gameplay path
is native CPU code and reserves zero GPU VRAM.

This is consistent with recent source-preserving texture-deformation work such
as [EfficientSync](https://arxiv.org/abs/2608.18832) and with the practical
separation between enrollment and streaming inference. NVIDIA Maxine remains an
optional proprietary fallback, but its required input buffering and private SDK
access make it a poor universal default; see the official
[AR SDK properties](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/properties.html)
and [performance reference](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/PerformanceReference.html).

## Moving-person proof

The proof uses the freely reusable Pexels clip
[Close-up View of a Man Talking](https://www.pexels.com/video/close-up-view-of-a-man-talking-4994154/).
The source face moves continuously and retains its own idle motion. Five real
mouth observations from the same person provide neutral, rounded,
labiodental/teeth, open, and wide states. The audio is the explicitly selected
stock male NVIDIA Magpie voice `Jason`.

OpenSeeFace accepted only 7 of 95 enrollment frames and 2 of 48 moving source
frames for this close-up subject, including implausible mouth spans on rejected
frames. The enrollment qualifier therefore used MediaPipe Face Mesh 0.10.21 on
CPU, which accepted 95/95 enrollment frames and 48/48 moving source frames.
This does not silently replace the product tracker: it proves that tracker
choice must be pluggable and that OpenSeeFace cannot be described as universal.

The v37 proof produced 40 frames at 540×960 and 30 FPS. Its artifact audit
passed with:

- 35 material-motion frames;
- `0.094340` median top-edge mode share;
- `0.086538` median bottom-edge mode share;
- `0.678947` maximum residual height-to-width; and
- `0.220000` maximum adjacent openness change.

Every visible oral pixel comes from the same enrolled identity. Inspection of
the all-frame board confirms curved lips, real teeth texture, clean return to
the untouched source at silence, and no upper-lip hole.

The portable Python proof path measured `29.030 ms` mean / `39.483 ms` p95
because it repeatedly decodes and resizes atlas images. That implementation is
an inspection harness, not the product hot path. Four sequential native
1920×1080 actor-bound atlas selector/compositor runs measured `4.549-4.724 ms`
mean / `5.101-6.692 ms` p95 across 250 iterations each, including queue, state
selection, and composition. The range is reported instead of selecting the
fastest run.

## Native integration evidence

- Six native CTest suites pass, including the hidden Windows GUI-subsystem
  check and authenticated cross-process D3D service lifecycle.
- The service protocol accepts a bounded atlas only after validating state
  count, dimensions, coefficients, pose values, byte count, and premultiplied
  BGRA pixels.
- The worker refuses cross-generation atlases, never applies one actor's pixels
  to another actor, binds output to the exact frame/track, and clears the atlas
  synchronously on cancellation.
- The Rust controller independently verifies the portable review manifest,
  file type, path containment, size, SHA-256, state layout, and premultiplied
  pixels before using the authenticated install command.

## Evidence paths

- Video: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\pexels-talking-man-jason-observed-atlas-lipsync.mp4`
- Source/output board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\source-output-mouth-board.png`
- All-frame mouth board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\all-output-mouth-frames.png`
- Artifact audit: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\mouth-quality-audit.json`
- Render manifest: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\observed-atlas-proof.json`

## Remaining boundary

This qualifies the component and its native transport; it does not claim that a
desktop overlay was visually reviewed. GUI launch/capture was intentionally not
performed because the user requested headless work after flashing-window and
screen-dimming interference. A longer cross-utterance sync evaluation,
occlusion/pose matrix, per-character enrollment UI, installed-app presentation,
and real-game compatibility matrix remain release gates. Until those pass, the
general profile selector stays disabled and fails open to the untouched game
frame plus ordinary audio/subtitles.
