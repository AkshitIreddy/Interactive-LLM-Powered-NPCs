# Misty: moving-mouth test on the user's Cyberpunk footage

This is a private, headless native replay using the user's
[AI NPCs in Cyberpunk 2077 video](https://www.youtube.com/watch?v=Uc3OXiFjsSg).
The user states that they made this Cyberpunk-only video and that its audio
exchange has no lip-sync. Its original soundtrack is removed from the comparison.
The rejected Pepe dialogue clip is not reused.

## Controlled input

The primary interval is source seconds 23.0–26.0: 90 frames at 30 fps, preserved
at the original 1920×1080 resolution. Misty faces the player, initially leaning
forward, then stands up around frame 48. Her source blinking, body motion,
lighting, hair and idle facial movement remain part of the test.

The replacement is the existing ElevenLabs Sarah stock-voice sentence,
“This is a synthetic voice demonstration.” No new provider request or voice
clone is used. The excerpt contains 2.65 seconds of the existing PCM with a
5 ms terminal fade and 0.35 seconds of zero padding: 72,000 mono PCM16 samples
at 24 kHz. Its peak is 0.871796, RMS 0.181632, with no clipped samples. The
21 sample-bound cues include five native cue categories and the padded silence.

| Input | SHA-256 |
| --- | --- |
| Replacement WAV | `2bcca3d0e776a0cc0f7d3991411c0154807f1f58a9082f2bdb9fa6942b1311fa` |
| Cue TSV | `d162b86d7aad4a21541186c620f71b384a7fbb9c261d26fd5070a4ec16db25d3` |
| 1080p model packets | `217c2cc5c6dffe45ea632599efeb6e75ed25fefc46c2c05edd9460f15cfbc8d8` |
| Misty oral texture | `80abbab133acd1dcb85887e5acaa99cb9f5b65e9bc1bb414b8d3966fd1f959f4` |
| Atlas manifest | `fdb4ed649ece22659209b70e2266130cbc0695b4b66c200a9069b934378fe286` |

The atlas uses only this same appearance of Misty from the supplied footage.
Observed states cover near-closed, narrow slit/contact, mid-open and open poses.
There are no clean rounded O/U or transcript-aligned phoneme observations;
those slots reuse same-character observations or geometry. It is a sparse
experimental reference set, not a complete phonetic character pack. Her mouth
contains roughly 25–43 original source pixels across; enlarging a crop cannot
recover missing tooth detail.

## Defect found by the real movement

The existing stateful adapter compared every proposed mouth position with its
last accepted position. Once Misty stood up, all later observations differed
too far from that old position. Even good tracking could never restore output.
The 960×540 baseline rendered only 46 of 90 frames; all later frames bypassed.
Static landmark validation alone had accepted every frame and missed this bug.

The repair collects at least two consecutive compatible observations of the
same authoritative actor. The first large jump bypasses. Votes must respect
the admitted inference cadence and remain within two inference intervals.
Invalid, occluded, identity-uncertain, wrong-frame, stale or cancelled evidence
clears the candidate. Duplicate and too-fast timestamps cannot advance it.
Recovery to the original position follows the same consensus rule.

Once a relocated position has fresh consensus, the adapter adopts its geometry
directly, avoiding a smoothed mask sweeping from the old face position to the
new one. Normal small-motion smoothing remains. Detector, landmark, geometry,
identity, occlusion and freshness thresholds are not lowered.

## CPU tracking evidence

YuNet at 640-pixel detector input plus OpenSeeFace LM1 selected Misty and passed
static packet checks in all 90 original-resolution frames. Model hashes are
validated by the diagnostic script; ONNX Runtime reports CPUExecutionProvider.

| Stage | Median | p95 |
| --- | ---: | ---: |
| YuNet face detection | 29.623 ms | 35.014 ms |
| LM1 landmark inference | 13.358 ms | 23.632 ms |

Detector confidence mean/minimum is 0.915453/0.902799. Global landmark confidence
is 0.876132/0.847989; mouth confidence is 0.879027/0.848365. Visibility is 1.0.
The separate medians total 42.981 ms; their sum is not an end-to-end percentile.
Native replay admits model observations at 15 Hz and carries their geometry
for one intervening presentation frame without inventing a new measurement time.

The repaired native replay rendered 84/90 frames at both 960×540 and 1920×1080.
Frames 46–51 (0.200 seconds) bypass during the rapid stand-up; rendering resumes
at frame 52. The original 960×540 replay had stopped permanently at frame 46.
At 1080p, worker selection/rendering measured 1.9177 ms p95. The broader native
frame stage, including adapter, worker, source copy and composition, measured
11.2352 ms p95; model inference, frame I/O, capture and presentation are excluded.

The 1080p pixel audit found zero changed pixels outside the per-frame residual
bounds and zero changes on all six bypassed frames. All 90 frames also preserve
every pixel outside the broad moving-mouth review region. Release `/WX` and
all eight native CTest suites passed after the recovery fix.

This recovery result still exposed a separate visible closure defect: some
silence frames pinch dark lipstick into an asymmetric mark. Uniformly shrinking
the mean inner-mouth gap can cross the narrower side pairs while leaving the
center open. For example, source frame 88 has lip-local paired gaps of 8.216,
10.399 and 8.203 pixels. Containment and successful tracking do not qualify
that appearance; closure quality requires a separate correction and review.

These are external model packets with manually selected identity. They do not
qualify automatic recognition, an installed provider, capture/overlay transport,
real game contention, microphone-to-response latency, or physical audio delivery.
The 5–8 second response-start target still requires an end-to-end measurement.

## Final v5 comparison and remaining quality limit

The final comparison is `misty-lipsync-comparison.mp4` under the artifact root
below. It contains 90 video frames at 30 fps and exactly 3.000 seconds of the
replacement audio; both streams start at zero. Full source/output views and
an identically positioned, moving mouth crop make the change inspectable.
The crop follows measured source geometry only in the review layout; it does
not change the native rendering coordinates. The original video's watermark,
game UI and original subtitle pixels remain part of the source image.

Pairwise lip-local closure prevents narrower side pairs from crossing while
the center is still open. Actual image inspection found that this geometric
fix alone did not restore natural lipstick: the model's inner contour encloses
much of the lipstick fill rather than precisely locating the visible cavity.
Holding the outer lip contour fixed made the appearance worse and was reverted.
A raw-geometry comparison did not materially remove the defect, so absolute
position smoothing remains at its default 0.42. The two v4 comparison processes
briefly overlapped; their timings are not used as final performance evidence.

Pure silence now produces a valid transparent residual, preserving the newest
game frame exactly. This matches the neutral atlas state's passthrough intent
and preserves the game's own resting expression. Active M/B/P cues still close
the mouth using the corrected pairwise geometry. This does not remove the
secondary requirement to replace incidental game speech during app speech;
only game-audio overlap handling was deferred by the user.

| Final native result | Measured value |
| --- | ---: |
| Valid residual proposals, including transparent silence | 84 / 90 frames |
| Frames with actual changed pixels | 74 |
| Exact original frames | 16: six movement bypasses plus ten silent tail frames |
| Changed pixels outside dynamic residual bounds | 0 |
| Changed pixels on bypassed frames | 0 |
| Worker selection/rendering p95 | 1.8572 ms |
| Adapter, worker, source copy and composition p95 | 10.7008 ms |

The final native timings come from a separate run after the v4 comparisons
finished. They exclude model inference, frame I/O, capture, playback and
presentation. All eight Release CTest suites passed; focused regressions cover
exact silence passthrough, active bilabial closure and uneven-gap folding.

Root and an independent reviewer inspected the final mouth sequence: silence
frames 80 and 89 preserve the original resting lips, and rendering recovers
after the stand-up. Active shapes still show thin horizontal bands/slits,
especially over dark lipstick. This is an experimental tracking/idle-preservation
improvement, not natural-articulation acceptance. Better cavity alignment and
more representative same-character observations remain necessary.

The existing `review-v15` app was independently reverified during this turn.
Its Tauri atlas loader and portable builder still accept the schema-1 Mara
v80 pack, while this replay uses a schema-2 Misty oral atlas. The Misty video
must not be presented as the installed app's current game flow. A source-fresh
portable rebuild can include the native fixes but does not by itself close
that atlas-loading/integration gap.

All footage, models, atlas pixels and rendered media remain outside Git under
`E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260905`. No GPU model was loaded,
no app or player window was opened, and no push or release was performed.
