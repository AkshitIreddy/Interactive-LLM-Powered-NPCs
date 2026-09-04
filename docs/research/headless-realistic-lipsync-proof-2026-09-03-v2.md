# Source-preserving headless lip-sync proof — 2026-09-03

> **Rejected after user visual review on 2026-09-04.** The geometric midpoint
> and symmetric cavity cut a dark hole through the subject's upper lip. Its
> numeric motion gate rewarded those corrupt pixels. This file is retained as
> failure evidence; see the
> [upper-lip-protected replacement](headless-realistic-lipsync-proof-2026-09-04-v3.md).

## Verdict

**Superseded failure evidence.** This revision addressed
the two observed defects in the rejected v8 artifact: it uses the explicitly
selected NVIDIA Magpie stock male voice
`Magpie-Multilingual.EN-US.Jason`, and its PCM fallback warps source lip pixels
inside a lip-local feathered mask instead of painting procedural teeth, tongue,
or a near-black cavity. Full-frame, mouth close-up, and source/rejected/final
comparison sheets were inspected after the numeric gate passed.

This is still an energy-driven talking-mouth fallback rather than phoneme
recognition or neural full-head generation. It does not qualify the desktop
WGC/broker/DirectComposition presentation path, a selectable visual pack, a
commercial game, or broad face/camera generality.

## Measured result

| Measurement | Result |
| --- | ---: |
| Output | 40 frames, 960x720, 30 FPS, H.264/AAC |
| Duration | 1.300 seconds |
| OpenSeeFace moving inference | 27.066 ms p50; 29.251 ms p95; 14 samples at 10 Hz |
| Procedural compositor | 1.337 ms p95 |
| Provider/model load | 241.625 ms |
| Process private bytes | 168,902,656 bytes, including decoded source frames held by the harness |
| GPU VRAM | 0 bytes |
| Source motion | 32 changed adjacent source frames |
| Output motion | 39 changed adjacent frames; 40 distinct frame digests |
| Visible mouth motion gate | 27 frames; mean absolute delta 0.012 closed / 2.654 maximum |
| Final tracking confidence | detector 0.783; landmarks 0.861; visibility 1.000 |
| Magpie voice | `Magpie-Multilingual.EN-US.Jason`; cloning disabled |
| Magpie WAV | 22,050 Hz mono, 1.300 seconds, 57,388 bytes |
| Audio samples | RMS 0.045705; peak 0.373505; zero clipped samples |

The provider smoke discovered 86 voices, found 30 stock English voices, and
confirmed the exact Jason identifier before synthesis. The WAV is non-silent,
has a 0.000044 tail RMS over its final 20 ms, and is retained outside the
repository. Voice identity is based on the provider's discovered stock name;
no subjective acoustic-gender claim is inferred from waveform metrics.

## Visual correction

The rejected compositor applied a broad elliptical overlay, darkened its
synthetic cavity to 12–27% of sampled source colour, and painted teeth/tongue.
That produced a detached black slit with a flat white strip. The replacement:

- limits alpha to a narrow measured lip band with horizontal and vertical
  feathering;
- preserves the upper face/lip anchor and moves the lower lip more strongly;
- chooses the darkest real source lip/seam texel in a narrow vertical
  neighbourhood for cavity texture;
- bounds output colour so the PCM fallback cannot invent near-black or
  brighter-than-source anatomy; and
- retains untouched source pixels outside the bounded residual.

A regression test first failed against the rejected implementation for both
near-black cavity pixels and invented bright teeth. It passes against the
replacement. The atlas/viseme path remains separate and can represent richer
phoneme-specific mouth states when qualified identity-specific assets exist.

## Evidence

- Report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving\headless-proof.json`
- Video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving\pexels-man-jason-source-preserving-lipsync.mp4`
- Full-frame sheet: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving\full-contact-sheet.png`
- Mouth sheet: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving\mouth-contact-sheet.png`
- Source/rejected/final comparison: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving\source-rejected-final-comparison.png`
- Male voice report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-nim.json`
- Video SHA-256: `b36a62a5d99f020c52131490b61f1e3e514ecb3ce5dbca9be4ecb467a670eacb`
- Report SHA-256: `444297d2a8f50e885422ea4419fa22cc6bc8dde450061d07dec1f20c7f319c63`
- WAV SHA-256: `86645661e3d6753d8a8038c83db4152eb627ede12fb244aef5aeb951fbaa22aa`

The primary source remains the license-safe
[Pexels video 6682450 by Kampus Production](https://www.pexels.com/video/person-looking-at-camera-6682450/).
Source footage and all generated evidence remain under `E:\temp`; no
identifiable-person media is committed to the repository and no endorsement is
implied.

## Remaining gates

The accepted scope is intentionally narrow: one moving male source, one short
stock-voice utterance, CPU landmarks, and headless composition. Longer speech,
phoneme articulation, diverse facial hair/skin/camera scales, occlusion,
profiles, HDR, WGC transport, broker presentation, and live-game behavior all
remain unqualified. Any failure continues to fall back to the untouched source
frame and audio/subtitles.
