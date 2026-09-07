# Lip-sync quality revision: local visual comparison

Snapshot notice: this v83 Mara comparison is retained as historical evidence.
The newer Cyberpunk/Misty v14 replay uses schema-3 full-lip photometric
references and recorded native YuNet/LM1 mouth geometry. Its containment and
fail-open checks pass, but the exaggerated open mouth and lighting/reference
seam remain; it does not qualify natural animation, an installed provider, or a
live game. See
[the reconciled review record](../product-rework/local-review-2026-09-05.md).

The revised native renderer preserves source lip detail, samples a separate oral texture, and deforms a small surrounding skin region so the old mouth corners do not remain underneath a contracted mouth. This is a visible improvement over the rejected v80 comparison, but it does not establish natural speech animation or live-game qualification. Category changes are still abrupt, and rounded articulation needs further work.

## Review artifact and scope

- Video: `E:\temp\InteractiveNPCs\lipsync-quality-20260905\mara-before-after-v83.mp4`.
- Before: `E:\temp\InteractiveNPCs\action-demo-20260905\mara-native-v80-sarah\frames`.
- Updated: `E:\temp\InteractiveNPCs\lipsync-quality-20260905\native-oral-v3-phonetic-sarah\frames`.
- Native measurements: updated directory's `headless-proof.json`.
- Atlas: `E:\temp\InteractiveNPCs\lipsync-quality-20260905\texture\atlas-landmarked-oral-v4-rounded`.

Both sides use the same 9.380875-second stock Sarah WAV and the same moving synthetic Mara source. The source is a short frontal camera sequence with synthetic blink/breath motion, looped for this comparison. Each side contains 282 native frames at 30 fps. This is a headless file render, not a running game's captured output. The source image's embedded fixture labels remain visible and describe the source, not the revised mouth overlay.

The six-state atlas uses higher-resolution observations of the same synthetic character. Rounded speech currently transfers that character's open-mouth oral anatomy into rounded geometry because the available rounded reference lacked enough usable interior detail. This is not a multi-reference neural model or a proven solution for arbitrary characters, poses, lighting, or occlusion. No Mara appearance data was applied to the real-person tracking probe.

## Implemented corrections

1. Versioned atlas representation: schema 1 retains legacy full-lip observations; schema 2 explicitly carries normalized oral interiors. Worker admission and service serialization reject mixed or unknown representations. Existing packs retain their original interpretation.
2. Source-derived lip deformation and fixed surrounding skin anchors remove stale source corners while maintaining a bounded residual. The mouth interior uses the enrolled character's texture instead of a flat teeth strip.
3. Clock-bound timed visemes no longer wait for the estimated-state atlas dwell filter. Estimated PCM-driven states retain their previous hysteresis. The fix has a first-frame regression test.
4. Real-video tracking accepts only tiny subpixel closed-mouth landmark inversions and letterboxes the entire authorized seed region uniformly. It does not crop to a hardcoded aspect ratio or search outside the seed.

## Timing and limits

| Measurement | Result | Meaning |
| --- | ---: | --- |
| Updated moving tracking, median / p95 | 26.015 / 30.990 ms | CPU inference, 94 samples at a configured 10 Hz |
| Updated compositor, p95 | 2.293 ms | Residual composition only |
| Model load | 329.966 ms | One observed cold load |
| Native process private memory | 220,880,896 bytes | About 211 MiB |
| Reported model GPU VRAM | 0 bytes | This experiment used CPU inference |
| Updated residual frames | 282 / 282 | Complete offline sequence |
| Real Nicole tracking residual frames | 282 / 282 | Separate CPU tracking replay; no appearance claim |

The user clarified that the practical interaction target is **5–8 seconds from input to response beginning**, and that hosted STT, LLM, and TTS are acceptable while lip-sync stays local. Local model options remain available for stronger PCs. The internal 60 ms tracking target is diagnostic; a small miss does not block a local review on its own. End-to-end response latency still needs a combined route measurement.

The starter setup already selects hosted AssemblyAI speech recognition, Groq replies, and Cartesia voice. Existing saved selections are preserved. The accompanying starter correction uses local SQLite full-text retrieval for memory, matching the implemented retrieval path and avoiding an unnecessary embedding-service credential dependency. This does not remove hosted or local model choices. It is a source change and is not packaged into the unchanged review-v15 application.

The focused native starter test and the NVIDIA provider-wide credential-authority regression passed. The latter now constructs its NVIDIA fixture explicitly instead of relying on the starter's former embedding default. Optional vision and generic lip-sync remain disabled in an unconfigured starter; the private review target uses its explicitly configured local visual path.

The final phonetic-cue replay reports passed native numeric gates. An earlier replay using the same renderer and the slower cue method recorded 62.518 ms tracking p95 and 3.286 ms compositor p95; its failed report is retained under `texture\native-oral-v3-anchor-rounded-sarah-cues`. The separate Nicole replay recorded 61.851 ms tracking p95. These variations are not evidence that a cue recognizer accelerates tracking, nor are the final numeric gates proof of live-game performance. Do not add independently measured stages together as an end-to-end latency measurement.

The final comparison uses 73 offline Rhubarb mouth cues bound to exact WAV sample intervals and SHA-256. The phonetic recognizer with four CPU threads took **2,243.385 ms** for the full recording. The earlier English recognizer with two threads took 9,066.665 ms and produced 69 cues; that comparison remains in v82. Both recognizer and thread count changed, so this is a measured configuration comparison, not an isolated algorithm benchmark. The faster configuration retains the improved renderer, but cue timing differs and no perceptual accuracy equivalence is claimed. This is an offline comparison driver, not streaming recognition or response latency. Mouth categories are not precise phoneme alignment. The parser rejects gaps, overlaps, unknown states, and a different WAV even when duration matches.

The reused ElevenLabs recording previously measured 533.344 ms to first decoded PCM, including 273.734 ms connection setup, and 1,115.192 ms to complete synthesis. These are earlier provider measurements, not new calls or live latency of this renderer. Three earlier fresh-connection short requests had a 675 ms median to first PCM. Provider latency and local tracking/rendering measurements remain separate.

## Review boundaries

Normal-size frames, enlarged mouth crops, and consecutive closure/rounding transitions were inspected. Sharper texture is established; natural coarticulation, arbitrary pose coverage, and perceptually verified audio/video synchronization are not. Sample-clock binding prevents a known scheduling error but is not by itself perceptual synchronization proof.

The final root warnings-as-errors native build passed, followed by all eight native test suites. Coverage includes source-corner replacement, closure of an open source, oral-texture aperture containment, unknown-representation rejection, legacy sampling, exact timed-cue admission, and service serialization. All 282 final frames are byte-identical to their source outside the mouth ROI `[390,240,240,112]`. Python compilation and `git diff --check` passed. These checks support implementation correctness, not aesthetic acceptance.

The existing `review-v15` application and test game remain unchanged. The new oral representation is supported by the native worker/service and this headless proof; the current Tauri atlas loader still accepts schema 1 only. The comparison is therefore not evidence that the review application ships this new renderer path.

No GUI or audio device was opened, no desktop capture was used, and no GPU model was loaded. The user's screen-dimming overlay does not affect these file renders and was left untouched. No provider keys are included in the artifacts, and nothing was pushed or released.

See [methods research](lipsync-quality-methods-2026-09-05.md) and [real-video tracking audit](tracking-quality-real-video-2026-09-05.md) for alternatives and separate tracking evidence.
