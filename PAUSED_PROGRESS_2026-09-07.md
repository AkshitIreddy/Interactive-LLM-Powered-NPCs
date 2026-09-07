# Paused progress — 7 September 2026

## Resume here

The owner explicitly requested: **stop for now, save the work, and record enough
progress in Markdown to resume in a few days**. Work is paused. No background
work, scheduled continuation, push, release, or deployment was started.
Resume implementation only when the owner asks to continue.

Repository: `C:\Users\akshi\Desktop\Code Palace\interactive llm\Interactive-LLM-Powered-NPCs`
Branch: `feat/2.0-overhaul`.

The owner accepted the visual improvement of the offline three-character v10
video, then asked to finish loose ends and connect it to the application and
test game. This session implemented that native integration. It is **not yet
a finished v18 review package or a qualified live commercial-game walkthrough**.
The application UI was not the rejected item; avoid restarting its redesign.

### First actions after a future continue

1. Read `see me.md` completely as requested by the owner, then this checkpoint.
   Check Git branch/status and preserve any changes made since this pause.
2. **Rerender the native cases with the final compiled code into fresh v3
   directories.** The final 15:29 native rebuild recompiled the compositor after
   a reversed OpenSeeFace corner correction. Existing v2 visuals may predate
   that correction. Compare exact output pixels and inspect enlarged temporal
   boards; do not assume v2 proves the final binary.
3. Rerun the native review assembler into a fresh directory. Its final source
   has a clearer Johnny bypass label added after the frozen video was encoded;
   the old receipt's assembler hash therefore does not match current source.
4. Review Misty/Claire/Mara anatomy and motion from the new outputs. The root
   reviewed Mara's complete 21-frame board and the wide Misty/Claire boards,
   but had **not inspected the new enlarged Misty/Claire mouth-contact boards**
   when the owner paused. Rebind private review evidence to the final outputs
   before packaging. Keep old artifacts immutable.
5. Create fresh reviewed Cyberpunk packs with canonical IDs below. Current
   scratch `misty-v1`/`claire-v1`/`johnny-v1` use short IDs and remain unreviewed.
   Only Mara has a separately promoted reviewed copy. Do not rename Cyberpunk
   data to impersonate Mara or treat Johnny's bypass as lip-sync success.
6. Run the final exact reviewed-pack/native join against the final binary;
   update the evidence docs. Commit coherent source changes locally.
7. Build **fresh `E:\temp\InteractiveNPCs\review-v18`** headlessly, using the
   schema-4 Mara receipt and reviewed optional character receipts. Validate
   the staged and final package. Preserve `review-v17` unchanged.
8. Deliver the fresh local app/test game and final native video with explicit
   latency boundaries. Live actor authority, WGC capture, WASAPI output, overlay
   presentation and game-load qualification remain separate unfinished gates.

## Safety and ownership constraints

- Hidden/headless work only. `see me.md` contains serious epilepsy/display
  safety directions. No visible app/game/player/terminal, desktop capture,
  audio playback, beep or sound test during automated work.
- Large artifacts, models, builds and caches belong under `E:\temp\InteractiveNPCs`.
- Read `C:\Users\akshi\Desktop\Code Palace\gpu use.txt` before heavy GPU work.
  This integration session used CPU models, plus owned D3D smoke tests; no
  heavy GPU model was loaded.
- Keys remain private in `C:\Users\akshi\Desktop\Code Palace\Commonly used Keys.txt`.
  No key values were read, logged, committed or used in this integration pass.
- Before any removal, read `C:\Users\akshi\.codex\WINDOWS_DELETE.md` completely.
  Preserve other projects/agents' data. Only task-owned scratch cleanup occurred.
- Local atomic commits are authorized. No push, tag, release, upload or publish.
- Game owns blinking/body motion; this system is responsible for mouth motion.
- The owner's response target is 5–8 seconds. Rendering time and first decoded
  audio time are separate measurements, not interchangeable latency claims.

## Saved implementation

Local commits made during this integration/checkpoint:

| Commit | Change |
| --- | --- |
| `0cbf69e` | Bounded sample-clock mouth trajectories |
| `6c7cc77` | Claire canonical Cyberpunk authored context |
| `42f445a` | Native current-frame oral-strip rendering, schema 4, current-pose tracking, local-shape filter, replay/preparation tools |
| `5463ecf` | Diagnostic-only source-frame appearance guard and tests |
| `3cedc57` | Rust schema-4 parser/wire and real reviewed-pack/native join |
| `e91f893` | Reviewed artifact receipts and portable review staging/verification |
| `3e7c40d` | Native landmark diagnostics and failed-model evidence |
| `aaebac6` | Native multi-character review assembler |

The final documentation checkpoint commit contains this file and reconciled
review reports; use `git log -1` for its exact hash rather than a self-reference.

### Native renderer

Atlas schema 4 is `normalized-oral-strip-v1`. Each state adds little-endian f64
reference-context luminance and a 0/1 source-edge-refinement byte after pose and
before pixel length. Schemas 1–3 preserve their old layouts. All schema-4 states
must agree on refinement policy.

New native files are `current_pixel_compositor.*`, `current_mouth_shape_filter.*`,
and `streaming_trajectory.*` under `native/mouth-worker`.
The compositor inverse-maps ordered lip strips from the **current** frame,
retains current outer-lip/skin pixels, confines optional reference anatomy to
the eroded oral aperture, and keeps exact bilabial contact. It does not use RGB
frame averaging. Source-edge repair is opt-in and contrast-qualified.

The adapter follows current face translation/scale and retains hard confidence
and mouth-only jump gates. Shape smoothing operates in mouth-local coordinates
(25 ms time constant; max 1.5% mouth-width correction), preserving current pose.
OSF indices 58/62 may be reversed relative to screen-left/right; both renderer
and filter now handle this and have regressions.

The final integration bug fixed before pause: routine 30 Hz capture requests
rejected by the 15 Hz admission cap must **not** reset smoothing/trajectory.
Invalid request identity still resets even if the signal is rate limited.
The regression compares full residual bytes and coefficients against an
admitted-only control, and proves the invalid-identity reset is distinguishable.

The source-frame appearance guard is compiled only in its standalone diagnostic
test target, not wired into the product's render veto. Its small corpus catches
Johnny's annotated smoke interval but also suppresses six clean recovery frames.
It is appearance continuity, not semantic smoke/hand detection.

### Rust/application/packaging join

`apps/control/src-tauri/src/visual_runtime.rs` validates and encodes schema 4.
`visual_runtime_schema4_join_tests.rs` now accepts an already reviewed canonical
Mara pack, copies its manifest and texture byte-for-byte, loads the dedicated
hidden Eclipse Harbor profile, imports/enables/resolves the exact pack in an
isolated test registry, and sends the resolved bytes to the real hidden worker.
The test-only actor lock and isolated registry do not grant product authority.

`scripts/windows/test-schema4-native-join.ps1` is the opt-in orchestrator.
The harness also self-tests the hidden game and runs an owned-texture D3D smoke.
Production account settings, registry and ordinary targets are unchanged.

`prepare-portable-review.ps1` accepts `-ReviewedMouthAtlasReceiptPath` and
`-ReviewedCharacterMouthPackReceiptPaths`. The new receipt helper verifies exact
file hashes, schema, semantic binding, refinement policy and closed-world paths.
Optional character packs stage disabled. They are not auto-enabled or general
quality-qualified by packaging.

## Verification at the pause

- **Final native build and all 12 CTest groups passed** after the final
  rate-limit fix. Main build directory:
  `E:\temp\InteractiveNPCs\native-builds\photometric-schema3-20260905`.
  Final worker timestamp was 7 September 15:29 local time. No native process
  remains running. Build command was `cmake --build <dir> --config Release -j 4`;
  tests were `ctest --test-dir <dir> -C Release --output-on-failure`.
- Rust schema-4 focused tests: 30/30; final enhanced harness compiled and real
  reviewed Mara join passed 1/1. Clippy passed before the final one-line hidden
  profile-loader correction; that correction was formatted, compiled and
  exercised by the successful join. Repeat final clippy if needed at freeze.
- Packaging receipt tests pass schema-4 acceptance plus semantic, texture hash,
  source-edge policy and reference-context tampering rejection. Legacy receipt
  compatibility tests also pass. Final v18 preflight/build has not run.
- Claire profile/content-pack/generator tests passed; Markdown links checked
  before the final pause document; secret scan passed index/worktree plus
  untracked files, with values intentionally omitted.
- All delegated workers reported stopped. No background build/model/ffmpeg
  process, tool session, or scheduled continuation is intentionally left active.

### Latest reviewed Mara join receipt

`E:\temp\InteractiveNPCs\schema4-native-join-proof-20260907-r7-reviewed-mara\schema4-native-join-receipt.json`

SHA-256: `dcc8b4003a17770c1f95d6793c56212131559cc2d4962899532f100ca6f6fee1`.
Passed 1/1 in 6.20 s; worker, D3D smoke and test-game children exited cleanly.
Exact Rust wire SHA:
`8f3cffc868b39fb7cd5320f7ef6ef077e58212042ef4ed96833d55858dad851c`.
Log: `E:\temp\InteractiveNPCs\completion-20260907\schema4-native-join-reviewed-mara-r7.log`.
Earlier r6 failed because the public profile loader excludes hidden Eclipse
Harbor; r7 uses its dedicated canonical loader. Do not reopen that solved issue.

Provider command-10 evidence reused from:
`E:\temp\InteractiveNPCs\activation-proof-20260907-r7-schema4-join\real-provider-activation-evidence.json`.
This separately validates hidden provider load/unload (313 ms), exact inventory,
attestation and retry rejection. It does not measure the live capture pipeline.

## Native replay artifacts and their limits

Artifact root: `E:\temp\InteractiveNPCs\integration-20260907`.
The following are **pre-final-binary visual evidence**; rerun as described above.

| Case | Directory | Native 15 Hz output | Changed | Worker + composition p95 |
| --- | --- | ---: | ---: | ---: |
| Misty | `misty-native-v2` | 45/45 residuals | 39 | 10.303 ms |
| Claire | `claire-native-v2` | 45/45 residuals | 39 | 6.024 ms |
| Johnny | `johnny-native-v2` | 0/45 residuals; 45 exact source bypasses | 0 | N/A |
| Mara | `mara-native-v1` | 21/21 residuals | 20 | 16.369 ms |

The replays consume exact full-66 native provider packets, not invented points
from old 18-point dumps. Manual fixture identity is explicit. Geometry is current
frame, while cues are delivered as one currently available sample-bound snapshot.
The timings exclude inference, capture, presentation, game load, audio decode,
PPM writes and integrity scans. All 16 bilabial residuals had exact contact;
no changes occurred outside residual support. Transparent residuals are not
counted as visible movement.

Inputs under the artifact root:

- `{misty,claire,johnny}-native-source`: 90 PPM frames each, 1920x1080.
- `{misty,claire,johnny}-native-landmarks-v2.json`: full66 dumps.
- `{misty,claire,johnny}-native-replay-v1.tsv`: strict converted packet replay.
- `mara-native-landmarks-v1.json` and `mara-native-replay-v1.tsv`: 42 frames,
  960x720. Mara source is `E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1`.
- `sarah-mara-42frames.wav` / `sarah-mara-42frames-cues.tsv`: exact 1.4 s,
  33,600 samples. Cyberpunk uses the 3 s files in
  `E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260905`:
  `sarah-first-sentence-3s.wav` and `sarah-first-sentence-cues.tsv`.

Replay executable in the build's `Release` directory:
`npc_mouth_worker_current_pixel_replay.exe <ppm-dir> <audio.wav> <atlas-root> <cues.tsv> <landmarks.tsv> <fresh-output>`.
It samples source indices 0,2,4,... at 15 Hz. Never present alternating source
and residual 30 Hz frames as smoothing. Output PPMs are zero-based; sources
are one-based. Cue rows are start,end,viseme, not start,count,viseme.

### Frozen comparison video

`native-review-v2\native-current-pixel-four-character-review.mp4`: 1920x1080,
15 fps, 156 frames, 10.4 s; Sarah audio is deliberately reused, not live synthesis.
Video SHA: `bd20aa2c56c9fd442bb5ba4083e6eb9a6e97f87b7e923b39254ff31f6be0c609`.
`native-review-v2\verification.json` SHA:
`675a7893878a2a121599d393bd3892c870186e8a6c32ae7eb092ae215076c466`.
The final assembler label change was not rerendered: its recorded source hash
is stale. Keep this video as an intermediate artifact, not final reproducibility
proof. Root separately decoded audio headlessly: zero-lag correlation 0.9998087
with the exact expected 249,600 PCM samples; no playback occurred.

Boards in the same folder include `*-comparison.png`, `*-temporal-board.png`
and the newer `*-mouth-contact-board.png`. Claire's enlarged oral inspection
board applies +2 stops equally to source and result and labels that adjustment;
the comparison video retains original exposure.

## Mouth packs

Canonical identities:

- `eclipse-harbor / mara-venn`
- `cyberpunk-2077 / misty-olszewski`
- `cyberpunk-2077 / claire-russell`
- `cyberpunk-2077 / johnny-silverhand`

Scratch packs are `integration-20260907\packs\{misty,claire,johnny,mara}-v1`.
The Cyberpunk short-ID scratch packs cannot be joined to canonical product
profiles. Regenerate fresh reviewed versions after final visual inspection.
Misty uses source-edge refinement, Claire does not; Johnny is source-only and
currently fails native LM1 confidence. The preparation script is
`scripts/benchmarks/prepare-current-pixel-mouth-pack.py`; its CLI records
reference/source provenance and optional explicit review-evidence hash.

Misty reference:
`E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260907\moderate-oh-reference-v1\generated-oh.png`.
Misty annotation:
`E:\temp\InteractiveNPCs\multicharacter-quality-20260907\misty\manual-oral-contours.json`.
Claire reference/annotation are `generated-oral-ah.png` and
`manual-oral-contours.json` in that corpus's `claire` directory.
Cyberpunk source provenance is the corpus's `provenance.json`.

Mara reviewed copy:
`E:\temp\InteractiveNPCs\integration-20260907\packs\mara-reviewed-v2\reviewed-artifact-receipt.v1.json`.

- Receipt SHA `51e7e5e67b57cf63a871a35130fff48b46f92cc293d0cca531d4f3b0a0932a91`
- Atlas SHA `05205f33b22529c1e7621f09085806ba4df8439b37807e7705ac30abd7ff029e`
- Texture SHA `c83e476885e43a4de67fe58aeafc226f09fb65886c305b0d96177c8278f11557`
- Identity revision `11563918363598984498`
- Enrollment binding `73cda095a7cbe585c6536a4017d05eb692e7b01a0d94c70c4a016390e8aebc23`

It binds `mara-native-v1-visual-audit-v2.json`, SHA
`1f7686c0f60034a87b67d0d87b28d7cc6fefa76d5a7e2a4bc1d97b086affa205`.
It is accepted only for private fixture review, with natural quality and ordinary
target activation both false. Generated teeth are soft/repetitive and a single
frontal reference is not broad pose proof. The original unreviewed pack remains
unchanged. Reassess/rebind against the final-binary render before final packaging.

## Why Johnny remains a blocker

The native LM1 provider rejects all 90 frames below its unchanged 0.82 confidence
floor. Exact CPU replay reproduces native scores within 1.37e-7, disproving the
suspected RGB/normalization bug. LM3 yields 24/90; LM4 31/90; LM4 plus CLAHE and
a tighter crop 75/90. But enlarged boards show unstable contours and high-score
false mouth geometry on other characters. Higher scores alone are not quality.
No new model/default or lowered gate was installed.

Read [the diagnosis](docs/research/native-landmark-provider-diagnosis-2026-09-07.md)
for pinned models, official preprocessing, exact-input reports and latency.
CLAHE adds 16–18 ms p95 on 1080p; candidate model inference alone is 24–25 ms.
The full Python replay under contention was much slower and is separately labeled.
Preserve original source pixels until a provider and geometry are qualified.

## Existing product and latency reference

`E:\temp\InteractiveNPCs\review-v17` is the previous immutable local app.
`E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game` is the
verified six-file test-game input for packaging. It adds synthetic blink/breathing
to a project-owned camera-transformed Mara portrait; it is not genuine gameplay
actor motion or preexisting product lip-sync. The new mouth effect is separate.
The private review model catalog is the previously verified r3 bootstrap catalog;
locate its exact path from the prior packaging preflight/receipts. Its September
14 validity may have expired when the owner returns, so recheck before building.

The previously measured Groq Qwen to validated speech to Cartesia first decoded
PCM was 492.964 ms. Warm provider component measurements and streaming/cancellation
analysis are recorded in the existing review docs. These exclude microphone/STT,
OS output, physical audibility and live game presentation. No new provider API
request was made in this integration session.

The accepted offline v10 reference remains:
`E:\temp\InteractiveNPCs\multicharacter-quality-20260907\comparison-v10\three-character-current-pixel-comparison.mp4`.
SHA `73b380505c38145840816ec11fa079a7d3d41f215b457fcd447238862ea5038f`.
It uses different offline geometry and manual Johnny smoke exclusions, so it
cannot substitute for native provider qualification.

Further context is in [native integration](docs/research/native-current-pixel-integration-2026-09-07.md),
[local review](docs/product-rework/local-review-2026-09-05.md), and
[packaging](docs/development/packaging.md). The pause and final-binary caveats in
this file supersede earlier prose that might sound complete.
