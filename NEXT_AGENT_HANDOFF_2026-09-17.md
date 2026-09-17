# Successor handoff — lip-sync investigation, testing providers, speech fix (2026-09-17)

This is a context handoff for another AI agent, not a work plan. Challenge every
conclusion below against source and measurements; nothing here obliges you to
keep an approach. Read `see me.md` and `NEXT_AGENT_HANDOFF_2026-09-16.md` first
for product mission and earlier history.

## 0. Where things stand

- Branch: `feat/2.0-overhaul`. HEAD: `37f189d` (all work after remote
  `4874ca9` is local-only).
- Remote `origin` has `feat/2.0-overhaul` at **`4874ca9` only**, pushed per one
  explicit user request (plus a declined PR prompt — do not open one
  unasked). Every commit after `4874ca9` is **local-only**:
  `833e8be` connection fallback/errors, `2ae2464` docs, `7456296` Groq model
  rotation, `b22c246` + `e073c84` distribution hash re-pins, `93bc3ed` docs,
  `f073ad2` testing providers + lip-sync unblockers, `fee23ee` docs,
  `d06cecd` visual-pack resolution tests, `e4bed02` + `b93a0a0` docs,
  `994f22d` review-path Yunet preference, `14db70e` testing validation-gate
  fix. Do not push, tag, release, or
  publish anything without a fresh explicit instruction.
- Single review package (no more v24-style versioned folders):
  `E:\temp\InteractiveNPCs\review` (source `14db70e` clean, app SHA-256
  `e5995f95…738f20fa50`, all verifications passed, `launched=false`). Delivery record:
  `E:\temp\InteractiveNPCs\review-delivery.md` (stable name, overwritten per
  release). Independent verification:
  `E:\temp\InteractiveNPCs\review-independent-verification.json`.
- App: `E:\temp\InteractiveNPCs\review\interactive-npcs-control.exe`
- Game: `E:\temp\InteractiveNPCs\review\local-app-data\test-game\interactive-npcs-synthetic-target.exe`
- User's private keys (never print values, never commit/log them):
  `C:\Users\akshi\Desktop\Code Palace\Commonly used Keys.txt`
  (labels: cohere, elevenlabs ×2, assemblyai, nvidia_nim, deepgram, cartesia,
  inworld, groq, googlegemini, openrouter, cloudflare account+api, mistral).
- Hard constraints carried over: **headless only** — no flashing/blue-red
  smoke windows, no GUI launches, no aggregate native CTest from unaudited
  caches (serious epilepsy concern); hidden shell does NOT hide GUI windows.
  Shared cargo target `E:\temp\InteractiveNPCs\cargo-target` (never duplicate
  caches). Owner vault/profile stay intact. Always report both app and game
  paths in chat when delivering test builds.

## 1. The lip-sync question the user asked, answered

User: lip-sync is not playing, "weird because it was able to create lip-sync
videos before using some method." The answer, verified:

1. **The old videos were never made by the app.** `scripts/benchmarks/
   render-dense-observed-lip-proof.py` (v53) is an offline Python file
   renderer: MediaPipe landmarks + dense optical flow + source-pixel warping
   + observed oral-interior transfer. It writes an `.mp4` to disk; it has no
   live path, no broker, no budget (~55 ms mean render, `rendered-not-
   qualified`). The Wav2Lip offline video was likewise a rejected file
   render. Nothing ever played lip-sync live in-app.
2. **The in-app pipeline is a different method.** `native/mouth-worker`
   (`src/compositor.cpp` contour-warp geometry, `signal_adapter.cpp`,
   `landmark_provider.cpp`) consumes OpenSeeFace 66-point landmarks (not
   MediaPipe), oral-atlas residuals, broker WGC frames, and a 15 Hz budget
   with fail-open bypasses. v53's approach was ported in spirit
   (`5f396da` observed-mouth enrollment and follow-ups), but the v66 render
   from this pipeline (`E:\temp\InteractiveNPCs\voice-lipsync-20260905\
   moving-mara-v66-corrected-openseeface-topology\mouth-board.png`, inspected
   directly) shows real aperture motion with identity preserved AND a flat
   dark-oval mouth interior: no teeth, no tongue, pasted lower lip. The prior
   rejection is accurate; do not present v66 output as acceptable.
3. **In-app motion was structurally impossible until this session** — four
   stacked gates, all confirmed in source and owner state:
   - Loadout `lipsync` role is Disabled in every seeded loadout and no mouth
     pack was ever imported (`%APPDATA%\…review\character-mouth-packs-v1`
     does not exist). Product path needs admitted packs regardless.
   - The debug review path (`MouthWorkerSupervisor::present_project_owned_
     review_frame`, `apps/control/src-tauri/src/visual_runtime.rs:1350`)
     requires a review OpenSeeFace pack that was staged **nowhere**: the
     review bundle policy forbids weights/DLLs and no install flow offered
     mnv3. Fixed by installing to the owner profile (section 4).
   - The runtime pins for the two ONNX Runtime DLLs matched **no real
     binary** (see section 4). Any present attempt failed closed.
   - `RuntimeRouter::start` only ran the visual turn when the overlay
     preference was on; overlay defaults **false** in every preset
     (`product_preferences.rs:621-696`) and the owner never enabled it. Now
     synthetic-fixture turns attempt it in review builds (see section 5).
4. **Measured inference reality** (fresh-built `landmark_dump`, real
   installed pack, clean synthetic frames exported headlessly from the game
   binary with `--headless-export-directory`, which returns before any
   window exists): ~29 ms/frame, 66-point mouth contours produced — but only
   **1–2/40 frames accepted** (`E:\temp\InteractiveNPCs\lipsync-proof\`).
   32× `provider_face_not_found_in_seed_roi` (seed-ROI tracking loses lock,
   never reacquires in the harness path), 9× `adapter:bypass_invalid_
   packet` on found faces. So: inference works, tracking/reacquisition and
   packet acceptance are the live-motion blockers after gating, and
   compositor natural quality is open research. **Do not tell the user
   lip-sync is fixed.** Say motion may now attempt, experimentally.

## 2. Speech fix recap (prior evening, needed for context)

Text-input turns died in ~0.25 s with silent subtitles. Live probe: Groq
404 `model_not_found` for pinned `qwen/qwen3.6-27b` (retired; catalog now
lists `qwen/qwen3.8-27b`). Cartesia `sonic-3.6` + qualified stock voice was
proven healthy the same day (4.72 s clean PCM, first audio 597 ms).
Catalog r10+ rotates the starter/seeds/migration to 3.8 with the
non-reasoning gate extended; live Groq-3.8→Cartesia chain measured
request-to-speech 959 ms. Owner loadouts re-seeded (isolated review
namespace, secret scan clean); `cyberpunk-private-fast-groq` still active.
Redacted evidence: `E:\temp\InteractiveNPCs\tts-live-20260917\`.

## 3. Testing providers (the zero-spend rig — use it)

Built because the user explicitly asked for dummy voice/LLM/call-makers
under one `testing` provider, for joint testing without API spend.

- Catalog (`catalog/v1/catalog.json`, experimental, local, offline,
  credential-free, never default/fallback): provider `testing`;
  `testing.llm.echo-v1` (upstream `testing-echo-v1`); `testing.tts.synth-v1`
  (upstream `testing-synth-v1`, 24 kHz mono s16le, `stock_voice_only`).
  `LiveQualification::NotRequired` keeps them selectable; a dedicated
  catalog-test arm pins the testing contract (implemented, experimental,
  selectable, non-default, no qualification baggage).
- Runtime (`apps/runtime-host/src/simulation.rs`): ready echo LLM arm
  (`FixtureLlm` with `Testing echo: <transcript>`, 50 ms pacing, no
  credential/network, not debug-gated); synth-voice arm
  (`TestingSynthTts`: deterministic 440→660 Hz sine sweep with
  attack/decay envelopes, text-length-scaled 0.1–2 s, peak ≈8192) submitted
  to the **normal broker sink when leases exist** (audible through
  speakers) or a `testingLocal`-labeled measured sink without leases
  (mock's `deterministicMock` receipts are deliberately filtered from
  evidence at `simulation.rs:~1295`, which is why testing has its own
  sink label). Testing turns emit an explicit capability notice and record
  `testing` provider ids with `llm_provider_live=false`,
  `tts_provider_live=false` — honest by construction.
- Control plane: `selected_playback_format` maps the exact testing triple
  (no credential) to 24 kHz mono so broker leases allocate;
  `qualified_hosted_tts_route` + `hosted_tts_route_supported` accept the
  exact testing triple; benchmark evidence labels testing `Mocked`.
- UI (`providerLoadouts.ts`, `ProviderLoadoutEditor.tsx`): Testing entries
  last in LLM/TTS lists, `execution: Local` (so no credential buttons
  render), fixed `testing-voice-1` preset; new `providerLoadouts.testing.
  test.ts`.
- Seeded `testing-dummy` global loadout in the owner profile (available,
  **never auto-activated**); active setup untouched.
- Coverage: hermetic `apps/runtime-host/tests/testing_providers_turn.rs`
  (echo text, measured synth receipts, testing provenance —
  HUD state; asserts Completed lifecycle, `Testing echo:` reply, non-live
  flags, `audio_submitted`, capability notice, `testing` TTS provenance;
  no network/credentials/GPU).
  [Tail fix 2026-09-17: committed file ended mid-sentence with a pasted
  `...[truncated N chars]` read artifact; completion restored from the
  test source.]

## 9. Successor session note (2026-09-17 — stopped before joint testing)

- Brief was joint voice + testing-dummy turns, then lip-sync tracking
  reacquisition + compositor quality on synthetic Mara, headless evidence
  before any quality claim. User then said stop; update handoff only.
  No implementation, build, live-key use, or test run happened.
- This session: read `see me.md` + both handoffs (small-range reads;
  long-range output truncates); read
  `apps/runtime-host/tests/testing_providers_turn.rs` fully to restore the
  tail from source; briefly inserted then removed a stray `...`
  placeholder at old line 138 (removed).
- Correction: an early message in this session claimed HEAD `d06cecd` —
  wrong. Task brief + `git log` output show HEAD **`275a086`** on
  `feat/2.0-overhaul`. §0 still says `HEAD: d06cecd` — stale; fix on the
  next doc pass. Nothing pushed; no push/tag/release taken.
- Trap: never paste read-output truncation markers into docs; re-read
  smaller ranges instead.
- Status reaffirmed, no new evidence: Groq qwen3.8 + Cartesia speech
  fixed; testing-dummy rig hermetically verified; lip-sync structurally
  unblocked but NOT proven live (tracking 1–2/40 frames, compositor
  quality rejected). Do not claim lip-sync works.

## 10. Continuation session (2026-09-17 — pin provenance + Yunet comparison)

- **ORT pin provenance solved.** The old review-path pins (`ea37f63d…`,
  14,854,728 bytes) are byte-identical to the ONNX Runtime DLLs inside the
  leftover *Yunet* native-test pack
  (`model-packs\openseeface-yunet640-lm1-mouth-signal-native-test\…`).
  A previous agent copied Yunet-pack runtime hashes into the mnv3
  review-path pins. The corrected pins (`7788f3f3…`, 12,416,032 bytes)
  match the project's documented source — the official Microsoft 1.22.1
  zip, whose archive hash `855276cd…` is pinned in
  `packaging/model-packs/openseeface-mnv3-lm1-mouth-signal.json` (verified
  by re-download) — and agree with the in-tree `yunet_native_qualifier`
  values. Same wrong pair also fixed in `landmark_dump.cpp` and
  `headless_proof_support.hpp`.
- **Yunet vs mnv3 on identical clean frames** (42-frame headless export
  from the game binary; fresh-built `landmark_dump`; no GUI at any step):
  mnv3 = **1/42 accepted** (32× seed-ROI track loss with no reacquisition,
  9× found faces at detector conf 0.60–0.64 rejected by the hard 0.70
  floor in `signal_adapter.cpp:19-21`); Yunet640 = **42/42 accepted**,
  detector conf 0.946 mean, landmarks 0.89+, 17.6 ms mean inference,
  mouth geometry on all frames. Evidence:
  `E:\temp\InteractiveNPCs\lipsync-proof\mnv3-clean-frames.json` and
  `yunet-clean-frames.json`. The Yunet run used a clearly-labeled
  test-only pack assembly (`lipsync-proof\yunet-test-pack\`: verified
  Yunet models + verified official ORT); it is not a product artifact.
- **Pivot recommendation (not a directive):** the product path already
  centers Yunet (catalog overlay, install/admission machinery,
  `landmark_provider.cpp` YuNet tracking). The mnv3 debug review path now
  initializes but will mostly bypass on this footage. Further lip-sync
  effort likely belongs on the Yunet product path (admitted packs + actor
  lock) plus compositor natural quality — not on mnv3 tracking surgery.
  Do not lower the 0.70 confidence floor to admit 0.60 detections without
  a false-accept study; animating the wrong face violates the product's
  own identity rules.
- Review-pack resolution test added (`visual_runtime.rs` tests: negative
  always, positive env-gated via `REVIEW_OPENSEEFACE_PACK_ROOT`); the
  positive run passes against the installed owner pack (0.51 s).
- §0 HEAD corrected to `3ebbb10`.

## 11. Continuation session (2026-09-17 — review path prefers Yunet)

- Implemented the pivot: `MouthWorkerLaunchConfig.review_yunet_root`
  (portable `review-model-packs/<yunet-id>/<rev>` else config
  `model-packs/<yunet-id>/<rev>`); `resolve_yunet_review_provider` with
  Yunet detector pin (232,589 bytes) beside the shared lm1/ORT pins;
  `pack_id` now travels on the launch struct into the worker wire
  protocol (worker already admitted the Yunet pack id — no worker change);
  `resolve_preferred_review_provider` prefers installed Yunet, falls back
  to mnv3 only when no Yunet root exists, and fails closed on
  corrupt-but-present Yunet roots. Both `prepare` and `present` call
  sites use it. Install script gained `-PackVariant Yunet` (verified
  closed world incl. both license files) and installed the owner
  profile's Yunet pack (10 files verified).
- Commit `994f22d`; single `review` folder rebuilt from it (source
  clean; app SHA-256 `3ae69f05…9688b52b8`; manifest `d662ada9…1ebe6b40f`;
  all verifications passed, `launched=false`). Delivery record
  overwritten: `E:\temp\InteractiveNPCs\review-delivery.md`.
- Coverage: new env-gated Yunet resolution test passes against the
  installed pack; control lib 273/273; Clippy/fmt clean on touched code.
- Not done: live in-app motion proof (needs game + broker + turn — joint
  testing); compositor natural quality; STT testing (deliberately still
  cloud-only).
## 12. Live GUI test session (2026-09-17 � user lifted headless restriction)

Tooling (kept under `E:\temp\InteractiveNPCs\tools\`, single-folder policy):
`tauri-driver` 2.0.6 (cargo-installed) + Edge WebDriver 153
(`tools\msedgedriver\`, matches installed Edge) + `tools\shot-game.ps1`
(occlusion-proof game-window capture via PrintWindow � CopyFromScreen
catches whatever covers the game, e.g. the user's other windows; the
Transparency App warning from the 09-16 handoff applies). WebDriver session
drives the review app DOM (nav buttons, launcher, loadout manager,
composer); game frames captured per-turn with PrintWindow.

Results, all observed live (evidence: `E:\temp\InteractiveNPCs\lipsync-proof\live\`):
- App launch, onboarding dismiss, Games navigation all work under driver.
- **Launcher-spawn failure under driver**: `Launch & connect` with no game
  running reports "could not be launched". The game exe starts fine
  standalone. Suspected cause is a nested Windows Job Object conflict:
  the driver-launched app cannot assign the game to its own job. The
  user's Explorer launches never hit this (their Sep-17 turns connected).
  Do NOT treat as product bug without reproducing from an Explorer launch;
  consider surfacing the OS/Job error distinctly instead of the generic
  launch message.
- **Manual-open attach works (the 833e8be fallback, proven live)**:
  manually started game (PID 10692) attached on `Launch & connect` with
  "Synthetic WGC receipt verified, exact-window pixels".
- testing-dummy activated through the real Loadout UI (Cognition + Voice
  show Testing, "Active for next turn"); owner setup otherwise untouched.
- **Leased testing turns failed closed live** with
  `simulation_request_invalid`: control allocated broker leases for the
  exact testing triple but runtime `selected_hosted_tts_format` only knew
  hosted providers. Fixed in `14db70e` (+ leased-validation regression
  test), single `review` folder rebuilt from it (source clean, app SHA-256
  `e5995f95�738f20fa50`, all verifications passed), delivery doc identity
  block updated.
- Re-run after fix: **testing echo turn completed live** ("Testing echo:
  �" delivered, "Text committed", 15 turn events, consumed route
  testing-dummy gen 1, LLM testing-echo-v1, TTS testing-synth-v1/voice-1).
- **Lip-sync verdict: no articulation.** 10 game frames across the speech
  window vs 4 quiet frames: mouth-box mean diffs 4�6.5/255 in both sets �
  indistinguishable from idle breathing/camera motion. The app diagnostics
  for every turn say `visual turn.unavailable skipped
  visual_actor_lock_or_admission_unavailable`, i.e. no presented visual
  receipt. Precise bypass point still unknown: start_turn should run for
  synthetic turns (994f22d change), so every frame is failing open inside
  prepare/present � but visual receipts are memory-only, so the exact
  reason (capture diagnostics? atlas resolve? worker configure? envelope?
  lease? landmark/track gates?) was not captured. Next step: expose the
  latest visual receipt detail in the UI trace or a log before any further
  visual guessing.
- Game window note: the synthetic game renders Mara idle (breathing +
  blink, mouth invariant) with "MOUTH UNARTICULATED" footer; any motion
  must come from the overlay path.
Left running on the user machine when this session stopped: the review
app (showing the completed testing turn), the synthetic game window,
tauri-driver on port 4444 (WebDriver session 0a0afe41-8072; the earlier de05bb9e-499e5 session was closed
with its pre-fix build). Close or keep at the user direction.

## 13. Live retest after the validation fix (stopped mid-analysis)

- Commit `14db70e` fixed the leased-testing-turn rejection (runtime
  `selected_hosted_tts_format` lacked the testing arm) plus a
  leased-validation regression test. Single `review` folder rebuilt from
  it (source clean, app SHA-256 `e5995f95…738f20fa50`, all verifications
  passed); delivery-doc identity block updated to match.
- Live retest, all observed (evidence: `lipsync-proof/live/app-2*.png`,
  `game-turn2-*.png`, `game-quiet-*.png`, `app-dom-turn.html`):
  manually started game (PID 21052) attached with WGC receipt verified;
  testing-dummy still active from the earlier session; typed turn sent and
  **completed** ("Testing echo: …" committed, 15 turn events, consumed
  route testing-dummy gen 1, LLM testing-echo-v1, TTS
  testing-synth-v1/voice-1).
- Delivery-details disclosure in the UI: Audio "Not proven - requires live
  TTS + submission + drain receipts"; Visible speech "Not reported - no
  native visual presentation receipt". No degradation text is rendered in
  the DOM for this turn.
- Mouth verdict again negative: 10 turn-window frames vs 4 quiet frames,
  mouth-box mean diffs 4–6.5/255 in both sets — indistinguishable from
  idle breathing/camera motion. Diagnostics log
  `visual turn.unavailable skipped
  visual_actor_lock_or_admission_unavailable` on every turn, and zero
  audio/broker/playback/lease diagnostic entries anywhere (submission and
  drain are not logged on success — absence proves nothing).
- Speaker-submission question still open (agent cannot hear). Rig ready
  but unanalyzed: `tools/loopback-record.py` (sounddevice WASAPI loopback
  of the default output — PortAudio exposes no loopback device by name;
  use `WasapiSettings(loopback=True)` on WASAPI output index 16) plus a
  testing turn (turn3, ~124-char echo) sent with 6 game captures;
  `lipsync-proof/live/loopback-turn3.wav` (22 s) and `game-turn3-*.png`
  exist on disk. Next: spectrum/onset analysis for the 440–660 Hz beeps
  to prove or deny speaker submission, then frame comparison if audio is
  present. If beeps are present, the fault is purely visual; if absent,
  the broker sink path from the sidecar is broken too.
- Visual-receipt detail is currently unreachable live: `debug_latest_
  visual_presentation_receipt` is a Tauri command but the webview sets no
  `window.__TAURI__` global, so WebDriver `execute` cannot invoke it.
  To get the exact bypass reason, surface the latest visual receipt in
  the turn trace UI or a diagnostics log line, then rebuild and re-run.
- Driver caveats learned: launcher-spawn fails under driver-launched app
  (suspected nested Job Object conflict — Explorer launches unaffected;
  do not file as product bug without Explorer reproduction); always
  capture the game with PrintWindow via `tools/shot-game.ps1` (screen-rect
  capture catches whatever covers the game); dismiss toast notices before
  clicking covered buttons; textarea `value` scripts replace text.
- Running processes left as found: review app (completed testing turn
  visible), synthetic game (PID 21052), tauri-driver on port 4444
  (session 0a0afe41-8072). STT/mic path untouched; AssemblyAI not
  requalified; nothing pushed.

