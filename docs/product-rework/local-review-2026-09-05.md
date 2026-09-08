# Independent local review rebuild — 2026-09-05, reconciled 2026-09-07

**8 September continuation:** final-binary native rerenders are complete;
all 156 frames match the preceding outputs and enlarged mouth sequences were
inspected. Final native tests pass 12/12 and the exact reviewed Mara join passed
at r7. See [native integration](../research/native-current-pixel-integration-2026-09-07.md)
for current timing and visual evidence. The generated v18 manifest is the
authority for package completion. The
[7 September checkpoint](../../PAUSED_PROGRESS_2026-09-07.md) is historical.

Status: independent implementation and headless review evidence recorded. The
verified v17 test game remains the current synthetic target, and the 2-of-2
signed private review catalog is complete. A fresh v18 application build is
pending the post-schema-4 source freeze; its generated
`REVIEW-MANIFEST.json`, rather than this prose, is authoritative for build
completion and exact identity. The isolated real-worker optional-pack activation
qualification is complete and recorded below. This file is not a release
approval or a claim that every product gate is closed.

## Reconciliation note

The audits linked below are immutable-date snapshots of what was observed on
September 5. Follow-up source and evidence supersede several of their uses of
“current”:

- Cartesia, Deepgram, and Inworld now have fixed-origin production Rust TTS
  transports. Cartesia is the measured starter route; a single production
  Groq Qwen → validated speech field → Cartesia bridge run reached first PCM in
  **492.964 ms**. This stops before `HostState`, native broker delivery, an OS
  audio endpoint, a microphone/STT turn, or physical audibility.
- YuNet detection no longer runs on every frame. The native policy performs a
  full detector refresh every 12 frames, within an admitted 10–15-frame range,
  and tracks the ROI between refreshes with explicit loss/high-motion
  reacquisition.
- The reachable product UI now includes the five job workspaces plus content,
  character override, and character mouth-pack flows. The old `Pages.tsx` and
  standalone `Onboarding.tsx` remain unreachable source specimens, not the
  mounted application.
- The owner rejected the September 7 Cyberpunk/Misty moderate-OH comparison as
  cheap-looking. It preserves the recorded native admission events and
  containment, but its full-lip photometric paste remains visibly separate from
  the game frame. Its immutable files and receipts remain regression evidence.
  The owner later accepted the private Misty/Claire/Johnny current-pixel v10
  comparison as a material offline visual improvement. That review does not
  qualify general anatomy, a live route, or Johnny's native landmark coverage.
- The real Rust schema-4 parser/native-worker boundary, cancellation ownership,
  hidden test-game self-test, and test-owned D3D service smoke now pass. This
  does not prove a production actor lock, commercial-game WGC capture, WASAPI
  delivery, broker presentation, or live-game visual quality.
- The v17 test game is verified. A v18 application package is complete only if
  its generated complete-source receipt records successful staged and final
  verification; this source report does not predict that later build outcome.

## Source and scope

Work started from clean `1f8fa78` on `feat/2.0-overhaul`, after reading the entire
`see me.md`. The old implementations were inspected textually at `main`
`503ef3b64a921b6a11efa9e3e0432a0c3de3b619` and `v1.0.0`
`9996575e69cf40d719e326adfea108476d80467b`. No legacy executable code, credentials,
pickle databases, or notebooks were run. No push, release, installer, desktop
capture, visible application test, or sound playback was authorized for this
pass. Review artifacts and model experiments belong under `E:\temp\InteractiveNPCs`.

The build must identify the complete current working tree, including untracked
files and dirty content hashes. Its Git commit alone cannot identify this work.

## What was replaced or corrected

- The actual mounted `ProductConsole` was redesigned around Session, Games &
  characters, Voice & models, Diagnostics, and Settings & help. Obsolete orphaned
  fixture pages were not mistaken for the product entrypoint.
- Model roles, provider accounts including AssemblyAI, stock-voice selection,
  game/process/character selection, and preferences now have direct workflows
  and progressively disclosed technical evidence. Empty panels and repeated
  architecture explanations no longer dominate the first screen.
- Guided setup has focus containment, focus restoration, actual native system
  telemetry, an integrated model editor, incomplete-state persistence, and a
  completion gate that requires a current live spoken-turn receipt. Synthetic
  fixture completions and file-only synthesis cannot satisfy that gate.
- The test-game launcher resolves a manifest-bound sibling executable and joins
  selection with exact target capture verification. `CREATE_NO_WINDOW` hides
  helper consoles; it does not make a WinForms game window invisible. The actual
  game launch/WGC presentation is therefore deferred under the display rule.
- Native subtitle/overlay intent is pinned at the actual character scope for a
  turn. Audio-clock bindings now retain the exact sample interval and playback
  cursor through the mouth-worker proposal, with invalid/stale input bypass.
- Architecture documents now distinguish protobuf transport from JSON business
  payloads, actual provider routes from catalog entries, conditional local packs
  from supported downloads, and runtime-core admission from a qualified host
  integration. Fixture effects are not described as game actuation.
- ElevenLabs quota errors are classified correctly. Provider timing now separates
  connection setup, first decoded audio, bridge delivery, and completion. A longer
  deadline alone is not treated as a latency fix. The runtime bridge forwards
  initial PCM immediately while retaining only one sample frame for its final
  non-empty end marker, replacing its former whole-chunk holdback.

## Fresh findings that changed the implementation

| Finding | Evidence and consequence |
| --- | --- |
| Existing "moving Mara" source is a camera transform of one still portrait | Its 42 frames do not prove genuine blinking, breathing, pose, or mouth articulation. New test-game motion must be labeled as synthetic renderer motion. |
| Rejected native v66 passes geometric tests but has an artificial dark mouth | It remains rejected. No acceptance checkbox or base-package model is promoted from that evidence. |
| MuseTalk improves anatomy but misses the current-frame budget | Warm fresh-frame batch-one path measured 429.725 ms p95 before the final source-preserving pass, with 7,594 MiB peak device memory on this machine. It remains a private comparator. |
| The historical NVIDIA delay is not a reproducible adapter defect | Identical-request A/B tests measured official Riva first PCM at 726 ms cold and 193–197 ms warm, and the unchanged repository adapter at 695 ms cold and 200–202 ms warm. Historical delays occurred before response headers (29.75 s median); their precise upstream cause is unknown. NVIDIA remains a viable candidate. |
| Persistent hosted streaming changes the latency comparison | Two warm requests on each established connection measured Cartesia Sonic 3.6 at 93–108 ms, Deepgram at 314–337 ms, and Inworld at 363–416 ms to first PCM. These small-sample TTS measurements exclude initial connection and LLM time. Cartesia returned phoneme timestamps; Inworld returned phonemes and provider visemes. Native adapter and cancellation qualification are recorded separately. The corrected Deepgram report excludes its server connection event from request timing. |
| The real Rust speech adapter reproduces the connection benefit | Cartesia produced first PCM in 607.3 ms including its 390.6 ms connection upgrade, then 115.2 ms on the reused socket. Two sentence sessions sharing the same public turn identity used distinct wire contexts. Cancellation discarded the socket and rejected subsequent audio; a new session succeeded. Inworld also returned real PCM and 44 visemes; Deepgram returned valid PCM but had a slower 2.09 s first-audio sample. These are synthesis results, not physical playback receipts. |
| The runtime bridge also delivers early audio | The exact Cartesia route reached the bridge's first Audio item in 551.721 ms for the first sentence, including 418.986 ms connection/session setup; the second sentence in the same turn took 85.066 ms and the next turn 81.684 ms. Provider PCM-to-bridge overhead was 12–28 microseconds, with exact PCM bytes and non-empty EOS verified. One socket was created and reused twice. No OS playback or broker drain was claimed. |
| Closing an Inworld context does not stop generated audio | The live cancellation probe received another 118,096 PCM bytes after `close_context`; the client must discard cancelled generations locally. Cartesia emitted no further PCM in the probe, but its documented guarantee also requires local late-frame rejection. Both supported a new context on the existing socket. |
| ElevenLabs is currently account-quota blocked | A non-generating subscription query confirmed the free account had used all 10,000 characters, with usage extension disabled. The reported reset is September 28, 2026. This does not establish any additional key-specific restriction. No paid extension was enabled. |
| CPU Kokoro does not qualify for interactive admission on this machine | The native worker produces valid stock-voice PCM and now passes adversarial cancellation, non-empty EOS, timeout, and child-restart tests. Its real runs still take several seconds to first audio and longer than real time overall. It remains an optional offline implementation; no measured admission envelope or base-bundle model is promoted. |
| Real provider synthesis and physical delivery are different | The selected LLM → runtime TTS adapter headless test produced a valid non-silent WAV, but explicitly records no broker receipt, physical audibility, or completed production audio delivery. |
| Passing DOM tests missed visual defects | Headless screenshot inspection caught clipped navigation at 720/960 logical pixels, inherited tiny settings labels, and an oversized loadout library. These were corrected and affected renders repeated. |

## Synthetic moving review game evidence

The review game's source is frozen for packaging. Its 42-frame input is a
deterministic camera transform of the project-owned Mara portrait, so the source
is labeled `source_actor_motion=false`, `source_camera_motion_only=true`, and
`source_mouth_articulation=false`. The fixture renderer adds a restrained blink
and breathing deformation while preserving the current source frame's mouth
pixels. It is a controlled moving capture target, not product lip-sync evidence.

The source sequence identity is
`22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d`.
The verified sibling pack is
`757f5cdd2dc5360564622ac40d1b73d5fb2748586affc0a614f75c14d81010dd`
(52,025,490 bytes). The final source-built executable is
`6029eb869f6480a0d6fbdfa32f00c0b0912f717b6aaa7bdb2fe5872fc4c0fc06`.
The staged six-file game is under
`E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game`.
Its distribution manifest is
`b35f43d144f4ab6227046dcb204d9bfe6067ade60803bb72e459a2c74ef6a51b`.

Headless preparation and verification passed. A modified sequence byte was
rejected by the package verifier, repeated 36-frame exports were byte-identical,
and the explicit static portrait control retained its invariant-mouth result.
No game window or audio output was created during these checks. Evidence is at:

- `E:\temp\InteractiveNPCs\review-game-validation\22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d\self-test.json`
- `E:\temp\InteractiveNPCs\review-game-validation\22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d\frames`
- `E:\temp\InteractiveNPCs\review-game-validation\22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d\contact-boards\moving-full-contact-board.png`
- `E:\temp\InteractiveNPCs\review-game-validation\22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d\contact-boards\moving-face-contact-board.png`
- `E:\temp\InteractiveNPCs\review-game-validation\22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d\contact-boards\moving-mouth-contact-board.png`
- `E:\temp\InteractiveNPCs\moving-prelip-control-v1\sequence.json` and its 36 contiguous RGB frames with transformed per-frame mouth geometry

## Native moving-mouth regression evidence

The retained September 7 moderate-OH, smoothed-state artifact is the latest
immutable full-lip paste regression baseline:

- labeled source/v14/new comparison for viewing:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\final-three-way-v3\misty-source-v14-moderate-oh-final.mp4`
  (`ea7c231c81af23fd95a4da6d689b4e3d42f55f29dd5ed27600a1fb605d0620bf`);
- video:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\misty-moderate-oh-smoothed-selection-v1.mp4`
  (`034a6d43f1a9f54af6d8852e851f0ae660607427346d7237c5e6f946f68954cc`);
- board:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\native-moderate-oh-dynamic-board.png`
  (`461a828a406b55de8b917ae5932c4141632f45e15e50e815c1678e8644355f68`);
- component receipt:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\native-comparison-receipt.json`
  (`0c88a06f0cf0eb1fa4495e4a7ee9e8fd9981854a8a428209125f5ea499ca74b5`).

The importable private pack is
`E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\misty-moderate-oh-private-pack-v1`.
Its manifest, texture, and provenance SHA-256 values are respectively
`6cb17fe26b9e1b6074d54cc9bbc894554bdcabf6a8315b9e8c6f74486a5afd2a`,
`1c9c58de2f1812cda7084a3c77f7d557c5aa231e414a7c7f0d939a370581ff63`,
and `cd691f889b4b332ff2103fe48f5a9e9757e1a2b1d060f50060b8970941308106`.
The final verification receipt is
`E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\final-verification-receipt.json`
(`0f561212082d099dcc13cf9428022179fb6f3fe29785aaf1603c97e31399aa13`).

Its recorded native admission-event digest is byte-identical to v14. All 20
source-identical frames remain source-identical and no pixels change outside
the dynamic residual bounds. The atlas changes only state 7; states 0–6 and
the alpha plane are byte-exact. Schema 3 now selects the atlas state from the
smoothed coefficients while retaining explicit contact/silence resets.
All 90 native outputs are byte-exact to verified sequence
`bada6e00cec63493bec823d055bf7b7bef71a1165e0bc7de27dfe7a1b6ad27a0`.
The replay measured 4.7075 ms compositor p95. That number excludes the detector,
capture, presentation, game load, and live frame-rate behavior.

The owner rejected the video as cheap-looking. Its more restrained OH shape does
not overcome the visible full-lip photometric/pasted seam, and the state-7
anatomy is a private generated hypothesis rather than teeth observed in the
game. The files and receipts remain immutable regression evidence. **Offline
recorded native admission replay only; not an accepted local-review renderer,
live app, live game, installed-provider, audible-quality, or latency proof.**
Containment and replay identity do not qualify arbitrary characters/poses,
natural quality, or production readiness.

The follow-up private prototype keeps current-frame lip pixels and inserts only
newly exposed oral interior. Its current study covers Misty, Claire, and Johnny;
Johnny is partially visible and provides a stress case rather than a clean
qualification. Two built-in image-generation calls prepared the Claire and
Johnny oral-interior enrollment references. The owner accepted the final v10
side-by-side video as a material **offline** improvement over the pasted-lip
approach. The generated/private anatomy remains experimental, Claire's dark
mouth does not qualify natural teeth or tongue detail, and the comparison has
not passed the future five-character production acceptance matrix.

The retained [current-pixel three-character comparison](../research/current-pixel-mouth-study-2026-09-07.md)
records the subsequent source-edge/contact correction, continuous offline cue
trajectory, rejected Johnny reference, manual smoke exclusions, exact final
video identity, and bounded CPU timing. This is benchmark evidence; it is not
itself native-runtime or live-game qualification.

The subsequent [native current-pixel integration](../research/native-current-pixel-integration-2026-09-07.md)
replayed exact full-66 LM1 packets through the native 15 Hz signal-admission
path, schema-4 `ReferenceMouthWorker`, current-pixel compositor, and CPU
composition:

| Character | Native residuals | Changed frames | Worker + composition p95 | Disposition |
| --- | ---: | ---: | ---: | --- |
| Misty | 45/45 | 39 | 10.303 ms | Offline native component pass |
| Claire | 45/45 | 39 | 6.024 ms | Offline native component pass |
| Johnny | 0/45 | 0 | Not applicable | Rejected: low LM1 confidence; all 45 frames stayed source-exact |
| Mara | 21/21 | 20 | 16.369 ms | Offline synthetic-character component pass |

Every admitted output stayed inside its declared residual support, and contact
frames returned to exact bilabial closure. These p95 values exclude landmark
inference, audio decoding, capture, presentation, game load, and file writing.
The 15 Hz replay cadence is the native admission cadence, not 30 or 60 FPS live
presentation evidence. Misty and Claire therefore establish bounded offline
native component behavior for these source sequences; Mara establishes the
same mechanical boundary for the fictional test character. Johnny remains a
native qualification failure rather than a hidden success.

The real cross-language schema-4 join is recorded at
`E:\temp\InteractiveNPCs\schema4-native-join-proof-20260907-r5\schema4-native-join-receipt.json`
(`428a02dcf6a0814e6fc5e7ae15171152d258ee0d88c1210ed7d5369e282c5dfa`).
Rust encoded a 131,556-byte schema-4 atlas wire payload
(`5688a7d3ceaa17365f635e49d0254babb6b02f6b9192a73ce5b1736583059306`),
and the fresh authenticated native worker accepted exact install and next-
generation replacement while the owning layers rejected malformed wire,
changed texture content, a foreign semantic identity, and cancelled-generation
replay. A fresh hidden command-10 provider load/unload completed in 313 ms in
that run. The hidden synthetic-game self-test and test-owned D3D native service
smoke also passed without desktop capture or audio playback.

That receipt uses a mechanical test-only atlas with the historical short
character ID `misty`; it was neither installed nor enabled as a selectable
canonical character pack. It does not establish production selected-character
actor authority, a fresh target-PID runtime admission, commercial-game WGC,
WASAPI playback, broker overlay presentation, or live-game lip-sync quality.

### v14 baseline

The three-second Cyberpunk/Misty v14 replay remains the exact baseline:

- video:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260905\misty-photometric-native-admission-comparison-v14-final.mp4`
  (`45702386cfcb678620ece8c00c70c9bc2f4a5c6dca87f30ba6a9dc0add981cde`);
- enlarged board:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260905\misty-native-photometric-native-admission-v14-final-enlarged-board.png`
  (`ca2ed7e0fbca52d6e066140678cb920b28d4e3259f22c5d8fa4957859d3679ab`);
- receipt:
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260905\misty-native-photometric-native-admission-v14-final-receipt.json`
  (`252b9def3ca71d183779004597138cb80466a463a79363d86f91246d2b5b7af1`).

It replays recorded native YuNet/LM1 mouth geometry and native admission through
the schema-3 compositor. All 90 frames stayed inside the dynamic residual
bounds; 83/90 native packets were accepted, 78 residual proposals were
produced, 70 frames changed, 12 bypass frames were source-exact, and 20 frames
were source-identical in total including silence. The first 48 landmarks in the
conversion TSV are historical fixture filler required by the 66-point parser;
only the final 18 native mouth landmarks drive composition. This is an offline
component replay, not installed-provider, live-capture, live-game, or latency
evidence.

Visual inspection found clearer teeth and rounded articulation than v66, but
its open-mouth volume was exaggerated and the generated reference did not fully
match source lighting. It remains useful baseline evidence rather than the
latest retained comparison.

The September 7 rejection board and receipt are under
`E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\misty-smoothing-study\rounded-reference-v5`.
The clean landmark morph was only a marginal offline improvement and was not
integrated. Those warp/blend experiments remain rejected. The later generated
moderate-OH reference and schema-3 smoothed selection are also owner-rejected
regression evidence as described above.

### Earlier synthetic Mara evidence

The September 5 C++ compositor snapshot transfers a tight, alpha-bounded full-lip texture
from a 12-state identity atlas over the newest source frame. This replaces the
rejected procedural lip deformation/oral-hole method. Two-frame state dwell
limits rapid atlas switching. Audio sample intervals, playback cursors, actor
tracks, and cancellation generations remain explicit validation boundaries.
Pixels outside the alpha mask are preserved exactly.

The private review uses only the two-file, native-compatible v80 Mara atlas at
`E:\temp\InteractiveNPCs\review-mouth-atlas-v80-native-compatible`. It contains
texture data, not model weights. Its scope is `private-synthetic-mara-only`.
The manifest hash is
`c4270c252f382502aa5218f5bb0c6b30b01f2db24988b0fde6b2f9d36ef65757`;
the 1,413,984-byte texture hash is
`420e518d3a1552cdf6407a59e78f5d14225a2c461c624cac3049509a8bef5ad1`.
The earlier v79 pack is superseded and must not be staged.

The actual native worker rendered heldout Jason audio over the controlled
blink/breath sequence. Root visual inspection confirms a substantial anatomy
improvement and removal of the v66 triangular hole. Lip softness and some flat
teeth remain visible. The archived v66 comparison has different audio/source
motion and is labeled as an anatomy comparison, not an identical-input test.

The least-contended run measured moving tracking at 30.208 ms p95 and composition
at 1.735 ms p95. A concurrent-load run reached 80.222 ms p95 tracking, so game
coexistence remains unqualified. These stage percentiles do not establish an
end-to-end capture-to-display percentile. Native Release build, all six CTest
targets, strict atlas schema, and premultiplied-alpha validation passed.
Physical overlay recapture, a real occlusion corpus, and generic-game
qualification remain open.

The audio-muxed native proof and labeled comparison board are under
`E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v81-native-controlled-heldout-cadence-correct`.

## Project-local storage cleanup

User-authorized cleanup removed 157 verified obsolete paths only within
`E:\temp\InteractiveNPCs`: 38,470,765,616 measured bytes (35.829 GiB), with no
deletion errors. Free space rose by 33.248 GiB during the deletion window while
other work continued writing to the drive. The post-cleanup snapshot reported
197.047 GiB free. All 30 critical retained paths were verified afterward.
Current review, UI/provider evidence, required teacher/proof inputs, build caches,
and optional Kokoro assets were retained. Other projects were outside the scope.
The exact plan, deletion results, and post-cleanup verification are under
`E:\temp\InteractiveNPCs\cleanup-20260905`.

## Free-friendly LLM options and real structured streaming

Fresh loadouts now select the measured Groq Qwen route and Cartesia's Greg stock
voice. Existing saved loadouts retain their selections. Mistral and OpenRouter
have fixed, named executable routes and separate vault targets; a browser cannot
supply an arbitrary credential destination URL. Groq, Mistral, and Gemini were
prioritized for repeated bounded tests. OpenRouter received only minimal free
compatibility requests; Cloudflare received one tiny test.

Direct provider tests found Groq GPT-OSS 20B at 488–500 ms to first text and
non-thinking Qwen 3.6 at 294–507 ms. Mistral Ministral 8B took 466–503 ms, while
Gemini 3.1 Flash Lite took 1,507–1,656 ms in its two samples. OpenRouter's current
Liquid free model passed one structured-response test at 1,627 ms. These are
small-sample integration results, not quality rankings or latency guarantees.

The actual adapters now use model-specific non-thinking settings for qualified
Groq Qwen and Cohere models. Without those settings, the tested requests could
consume their output budget without emitting dialogue. Gemini receives a
portable schema using singleton enums; full native validation still enforces
the stronger text and proposal constraints. OpenRouter disables provider
fallback and requires the requested parameters for its verified route.
Cloudflare remains unqualified because its tested streaming response format
differs and its documented strict JSON mode does not stream.

The normal runtime now attaches the response schema instead of relying solely
on a prompt. Its explicit `structured_speech_first_v1` path can release a
complete, decoded, validated spoken field once the schema marker is valid,
without waiting for the remaining envelope. It is complete-field streaming,
not per-token speech. Actions and memory proposals remain blocked until full
strict validation; late failure retains only actually delivered speech
receipts. Raw structured JSON is suppressed from ordinary text display events.
The original whole-envelope mode remains available for compatibility.

Deterministic tests hold the provider tail until speech is delivered and verify
that a late invalid tail commits no proposals. A real Mistral 8B request through
`HostState.simulate_turn`, `RuntimeBridge`, and `TurnSupervisor` completed in
1,087 ms with a validated envelope and one subtitle-delivered sentence. That
test disabled audio and claims no native subtitle presentation or physical
delivery. Its receipt is
`E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\normal-route-mistral-structured-20260905.json`.
The immutable direct-comparison report, model availability findings, and current
official plan references are in
`E:\temp\InteractiveNPCs\provider-live-2026-09-05\llm-alternatives\consolidated-20260905T050527Z`.

## Private YuNet review catalog

The September 7 CPU-only native qualifier exercised the exact official ONNX
Runtime 1.22.1 archive/DLL and pinned YuNet/LM1 model tuple across 20 fresh and
20 reload sessions. It measured 458.756 ms p99 fresh provider load (490.702 ms
maximum), 299.530 ms p99 reload, 102.274 ms worst inference p99, and 86,331,392
bytes absolute process-RAM p99. The OS disk cache was uncontrolled. The source
was fixture-idle, so these numbers do not cover game pressure, capture,
60-FPS presentation, natural tracking, or a universal cold start.

The private review catalog under
`E:\temp\InteractiveNPCs\private-review-catalog-yunet-20260907-r3` uses a 2-of-2
ephemeral signature threshold. Its metadata explicitly records
`productionTrust=false`, required rotation, and disabled promotion/publication.
Native inference qualification is recorded separately from provider load. The
catalog by itself is only eligibility evidence; the following isolated
activation run supplies the missing provider-load receipt.

The env-gated real activation test imported the private r3 catalog and cached
official YuNet/LM1/ORT tuple into an isolated `E:\temp` lifecycle root. It first
proved there was no active pointer, deliberately failed with a missing worker,
cleaned the exact challenge nonce, and retried. A fresh hidden authenticated
mouth worker then loaded and unloaded the exact provider. The earlier r4
receipt measured 785 ms; the final persisted r5 receipt measured 302 ms. The
lifecycle became Active, active inventory matched the attested tree, duplicate
activation was rejected, and stale runtime admission remained revoked.

The final test passed 1/1 in 18.90 seconds. Its evidence is
`E:\temp\InteractiveNPCs\activation-proof-20260907-r5\real-provider-activation-evidence.json`:

- manifest SHA-256:
  `192235cca52148087337678d4e6427a69c922d053f4792a9ba62ef35f914418b`;
- tree SHA-256:
  `6c5decaa7421a4350c84a68907491fe4647f176c8c5d1cbb16db3d09a9e28d1c`;
- attestation SHA-256:
  `ac319573f8853859127312a50edfc7b66b72fc27f664e164e9a830a3697745ea`.

The exact active inventory contains 17,849,614 bytes across the pinned models,
licenses, and official runtime files. The two setup observations are not a p99
or universal cold-load benchmark; OS cache state was uncontrolled.

This proves provider load, authenticated worker shutdown, retry, and exact
inventory activation in an isolated test state. It does not install or activate
the pack in the user's normal app state, authorize live rendering, prove
inference/tracking/visual quality, satisfy whole-loadout admission, or measure
game coexistence.

## Evidence index

- [Independent legacy/current architecture audit](independent-architecture-audit-2026-09-05.md)
- [Functional and rendered UI audit](independent-functional-audit-2026-09-05.md)
- [Native integration audit](native-slice-audit-2026-09-05.md)
- [Provider editor audit](provider-editor-audit-2026-09-05.md)
- [Game, character, and settings audit](workspaces-audit-2026-09-05.md)
- [Qualified LLM routes and structured streaming](llm-provider-integration-audit-2026-09-05.md)
- [Current provider/runtime research](../research/independent-provider-runtime-review-2026-09-05.md)
- [Hosted speech comparison, free plans, and exact protocols](../../scripts/provider-timing-alternatives/README.md)
- [Executable speech transport audit](hosted-tts-transport-audit-2026-09-05.md)
- [Moving-character methods and measured comparisons](../research/independent-moving-lipsync-2026-09-05.md)
- [Native current-pixel schema-4 integration](../research/native-current-pixel-integration-2026-09-07.md)
- [Acceptance map for this rebuild](review-rebuild-contract-2026-09-05.md)
- [Installer-free review packaging](../development/packaging.md)

Fresh machine-local evidence is under:

- `E:\temp\InteractiveNPCs\audit-20260905`: frozen baseline and revised UI renders
- `E:\temp\InteractiveNPCs\provider-live-2026-09-05`: sanitized real-provider metrics
- `E:\temp\InteractiveNPCs\provider-live-2026-09-05\independent-magpie-streaming-comparison.json`: official-client/repository NVIDIA A/B
- `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives`: immutable hosted HTTP/WebSocket comparisons
- `E:\temp\InteractiveNPCs\review-ui-tests.log`: full frontend test output

## Source checks and artifact receipt

The September 5 source snapshot passed 696 Rust tests across 69 test binaries, with 15
explicitly gated tests ignored. The nested Tauri application passed 218 tests
across four test binaries, including the real media-broker and authenticated
runtime-host integrations; one physical live-audio test remained ignored.
Both workspaces passed all-target, all-feature, locked/offline Clippy with
warnings denied. Full logs are `final-rust-workspace-tests-20260905.log`,
`final-rust-tauri-tests-20260905.log`, and the corresponding `clippy` logs under
`E:\temp\InteractiveNPCs`.

That frontend snapshot passed 143 tests, TypeScript checking, and formatting.
The broad visual audit produced 198 headless screenshots; the final provider and
setup pass passed 104 checks with no browser errors or horizontal overflow at
1440, 720, and logical 320 pixels at 200% scale. All 15 keyboard states passed
across 264 focus visits. Root inspected the corrected setup model and account
screens. Browser fixtures do not establish native account, capture, or playback
behavior; native-only controls remain explicitly identified in the audit.

The local speech worker passed five library, one worker, and seven child-process
tests, including cancellation before queued PCM is consumed and cancellation
wakeup while the receiver is idle. Strict offline Clippy and formatting passed.
The real model performance remains below interactive admission requirements.
Native mouth-worker Release passed all six CTest targets. The current media
broker Release passed five noninteractive CTest targets and its real-broker
Tauri integration test. Packaging security tests passed 28/28, with source
hygiene, secret scanning, strict dependency licenses, and acceptance consistency
checked separately.

The later pre-activation integrated tree passed 709 Rust tests across 70 groups
with 16 gated tests ignored, 238 cumulative nested Tauri tests, 158 frontend
tests across 21 files, strict Clippy, the 1,071-file secret scan, source hygiene,
40 packaging security tests, and eight non-GUI native CTest suites. These are
component/source checks. Activation changes and the final source freeze require
a new full rerun.

At the final source freeze, the locked/offline all-target, all-feature Rust
workspace run passed 712 tests across 71 groups, with 16 explicitly gated tests
ignored and no failures. Root and nested formatting passed. The source freeze
also passed the credential scan across 1,110 tracked and untracked files and all
40 packaging security tests. Locked/offline all-target, all-feature Clippy with
warnings denied passed, and the final native mouth-worker run passed all eight
non-GUI CTest suites. The final nested Tauri library run passed 244 tests with
one env-gated real-catalog activation test ignored by default; that exact opt-in
test passed separately against the private catalog. The final frontend suite
passed 162 tests, plus typecheck, production build, formatting, and wide/logical-
320 headless inspection with no overflow or browser errors.

Those counts identify the earlier v17 source freeze. The later schema-4 source
and native integration require a fresh v18 full-suite and package run. Focused
schema-4 protocol, worker, cancellation, hidden test-game, and owned-D3D checks
are recorded above; they do not substitute for the generated v18 package
receipt.

The intended fresh installer-free review destination is
`E:\temp\InteractiveNPCs\review-v18`. Its sibling test game is already verified
at
`E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game`.
The application executable and `REVIEW-MANIFEST.json` do not count as complete
until the final source snapshot, staged verification, destination verification,
and closed-world reconciliation all pass. This document deliberately does not
invent those pending hashes or claim an unrun desktop/playback check.

The remaining product gates are generic-game moving-mouth quality, real
occlusion coverage, tracking under game contention, physical overlay recapture,
and actual audio-endpoint delivery. Private character atlases are identity-bound
review inputs; ordinary targets keep unqualified visuals disabled. The YuNet/LM1
provider-load lifecycle is qualified in an isolated test state, while live
rendering authority, whole-loadout admission, normal user-state installation,
public catalog, and production trust remain disabled.
No push, release, or update-feed activation is part of this review.
