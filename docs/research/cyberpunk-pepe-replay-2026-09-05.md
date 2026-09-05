# Cyberpunk 2077: Pepe mouth-replacement experiment

## Result and scope

The first private Cyberpunk comparison is complete. The real native mouth worker
replaced Pepe's mouth on 258 of 282 frames (91.5%) in a 9.4-second recorded-gameplay
sequence. The other 24 frames retain the source exactly. This is a headless,
frame-indexed component experiment using externally measured model geometry,
manually selected identity, and prerecorded replacement speech. It is not an
installed provider, live game, or end-to-end response-latency qualification.

The original lightweight detector could not initialize this scene reliably.
YuNet plus stronger OpenSeeFace landmarks provided a useful comparison without
lowering the native confidence requirements. The native mouth changes are now
causal and smoothed; the stock game supplies all eye, head, body, and lighting
motion. No blinking animation is added.

## Source and controlled inputs

- [Gamersyde: Cyberpunk 2077, The First 12 Minutes — Street Kid, Xbox One X](https://www.gamersyde.com/video_cyberpunk_2077_the_first_12_minutes_street_kid_xbox_one_x_-45506_en.html), published December 2020.
- Pepe at the bar, source time 55.0–64.4 seconds, 282 frames, 960×540 at 30 fps.
- Private reference acquisition covers source seconds 45–69 at 1080p.
- Replacement voice: the existing ElevenLabs Sarah stock-voice WAV, 24 kHz,
  mono PCM16, 225141 samples, 9.380875 seconds. This is neither Pepe's original
  dialogue nor a clone of the actor. No new TTS/API request was needed.
- Audio SHA-256: `bbcbcfbade9e0dbacf4ccbfeddab3c659038fa397a40bc955b51133678d1667e`.
- The existing 73 exact sample-bound phonetic cues drive both before/after runs;
  cue file SHA-256: `1dfadabceb2071d26ef9130fd65ab483c1034040242171e5f9234064b46feda1`.

All footage, reference pixels, models, and generated videos stay outside Git in
`E:\temp\InteractiveNPCs\cyberpunk-replay-20260905`. Public viewing availability
does not establish redistribution rights. These assets are not release packs.

## Tracking comparison

Official model sources:
[OpenCV YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet)
and [OpenSeeFace](https://github.com/emilianavt/OpenSeeFace).

YuNet is pinned to OpenCV Zoo revision
`47534e27c9851bb1128ccc0102f1145e27f23f98`; OpenSeeFace models are pinned to
`85aa70fc67582d046e771ea73625182a0d8f7475`. The diagnostic validates model hashes,
emits contiguous frame packets, and records actual confidences and CPU times.
It follows the manually selected Pepe region and does not silently select a
different face. It does not establish automatic character recognition.

| CPU candidate | Static policy accepted | Landmark p50 / p95 |
| --- | ---: | ---: |
| YuNet640 + LM1 | 0 / 282 | 12.555 / 25.328 ms |
| YuNet640 + LM3 | 259 / 282 | 38.351 / 55.448 ms |
| YuNet640 + LM4 | 257 / 282 | 36.317 / 53.966 ms |

LM1's maximum global confidence was 0.809958, below the unchanged 0.82 gate.
LM3 had fewer overall static drops; LM4 had higher confidence but more pose
rejections. LM3 was therefore selected for the native replay. Its YuNet640
detector cost was 31.346 / 38.140 ms p50/p95; YuNet960 cost 57.675 / 68.735 ms
and did not improve selection.

Model-only sequential median cost is approximately 69.7 ms per observation
for the selected combination (sum of separate medians, not an end-to-end
percentile). This does not meet a 33.3 ms frame budget. Production integration
still needs scheduled detection, landmark tracking between detections, or a
pipelined worker, followed by actual freshness and throughput qualification.

The C++ replay feeds the real OpenSeeFace adapter at 15 Hz and reuses accepted
geometry for one intervening frame while preserving its measurement timestamp.
That explains why 259 static accepted packets become 258 rendered video frames.
Twelve rejected sampled packets cause 24 untouched frames. No native threshold
was weakened. The prerecorded geometry has no inference cost inside the replay;
model inference timings above must not be replaced with packet lookup timings.

## Character references and rendering

The private eight-state atlas uses Pepe's own observed mouth interiors from this
footage. Current-frame lips, skin, beard, and pose remain the compositor's input.
No Mara texture or generated foreign anatomy is used. Reference selection covers
closed, open, spread, and approximate dental/contact shapes.

The footage does not contain a clean verified O/U observation. The rounded slot
therefore uses Pepe's observed open oral interior with rounded target geometry.
F/V, dental, and alveolar labels are visual approximations, not phonetic ground
truth from the original dialogue. Headshots help identity and appearance, but
closed-mouth portraits alone do not supply teeth, tongue, or open-vowel coverage.

Atlas texture SHA-256:
`8ab3e1296db94b743a507ddad63b09759fd8d25463227233d116f283b355fae8`.
Atlas manifest SHA-256:
`a90ad02350effdebff7d00cb4864e6ad361c3bbb0db4db0bc87434ee8bace0f1`.

The smoothing fix uses playback-clock elapsed time, with a 42 ms ordinary
timed-cue response constant and bounded faster contact/opening responses.
Compatible oral textures blend causally. Teeth-bearing textures do not blend
into a closed contact state. Exact M/B/P/silence closes an already-open source
mouth through the current-frame compositor; simply returning transparent pixels
would incorrectly leave the game's original talking mouth visible.

The before worker is commit `61ce863` with the same new replay harness copied
into its isolated build. The after worker includes commit `0fd64f2`. Same source,
audio, cues, atlas, and LM3 packets are used in both.

## Verification and observations

- Native Release build with warnings as errors: passed. All eight CTest suites
  passed, including exact closure over an open source mouth and transition/reset
  regressions. Changed Python tools compile.
- Native Pepe mouth-worker p95: 2.9762 ms after smoothing, 1.2078 ms before.
  These are local component timings, excluding tracking, capture, audio, and IPC.
- Per-frame residual containment: zero changed pixels outside declared native
  bounds (one-pixel serialization rounding allowance). All 24 bypassed frames
  match the source byte for byte.
- The first fixed review crop missed 22 pixels with one-channel delta 1 across
  six moving frames; dynamic containment still passed. A wider audit crop covers
  the entire moving mouth region. The original report remains preserved.
- Inspected landmark overlays, consecutive mouth crops, and encoded comparison
  stills. Mouth replacement follows the moving face, and smoothing is visible,
  but dark lighting, beard detail, missing rounded references, and tracking
  dropouts prevent calling this polished or production-qualified.
- Both comparison MP4s contain 282 video frames at 30 fps, 9.400-second video,
  9.381-second audio, and zero stream start offsets.
- The separate Mara control preserves every pixel outside its fixed mouth region.
  Residual transition p95 decreased from 1.4671 to 1.2222 and maximum from 1.8193
  to 1.3959. These are pixel-change measurements, not perceptual sync scores.

## Completed-audio limitation and packs

This comparison still uses completed audio for exact cue control. The separate
[incremental cue experiment](incremental-mouth-cues-2026-09-05.md) measured a
median 1.45-second first immutable cue and approximately 2.75 seconds of buffering
to avoid cue underruns. Repeated Rhubarb windows should not become the default
latency path. The preferred product direction is immediate audio-driven mouth
motion, optionally improving unplayed future audio with recognized cues when
the synthesizer has produced enough ahead. This has not yet been integrated or
shown to meet the user's 5–8-second complete-response-start target.

The [composable pack contract](optional-game-character-packs-2026-09-05.md)
separates base game, character, and appearance additions. It reuses existing
profile/provider types, supports lore and exact voice/model defaults, and gives
saved player overrides precedence over accepted pack defaults. It is a proposed
contract, not a completed installer or new UI. Reference assets require suitable
redistribution rights before a future optional GitHub release pack.

## Local artifacts and reproduction

Under the artifact root above:

- `cyberpunk-pepe-mouth-test.mp4`: original game versus replacement mouth,
  with enlarged detail and the replacement test voice.
- `mara-smoothing-comparison.mp4`: previous versus smoothed control.
- `pepe-smoothed-lm3/replay-report.json`, `frames.jsonl`, and `frames/`:
  native results and uncompressed evidence.
- `pepe-baseline-lm3/`: same experiment with the baseline worker.
- `yunet-openseeface-comparison.json`: all three tracker candidates.
- `yunet-lm3/qualification.json` and `pepe-yunet-lm3-landmarks.tsv`:
  actual model output, timing, hashes, provenance, and input manifest.
- `pepe-atlas-work/PEPE-ATLAS-COVERAGE.md`: exact reference coverage/disclosures.
- `pepe-smoothing-board-wide.json`: dynamic and fixed-region containment audit.

Build `npc_mouth_worker_tracked_replay` with the headless realistic proof option
enabled. Its positional inputs are source-frame directory, WAV, atlas directory,
exact cue TSV, landmark TSV, and a fresh output directory. Generate geometry
using `scripts/diagnostics/prepare-cyberpunk-landmark-replay.py`; export a private
atlas using `scripts/benchmarks/prepare-landmarked-oral-atlas.py --state-specs`.
Use `audit-native-mouth-sequence.py --events <frames.jsonl>` for dynamic
containment and `render-lipsync-quality-comparison.py` for a labeled review MP4.

The existing review-v15 application and test game were not replaced by this
experiment. Nothing was pushed or released.
