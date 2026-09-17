# Successor handoff — lip-sync investigation, testing providers, speech fix (2026-09-17)

This is a context handoff for another AI agent, not a work plan. Challenge every
conclusion below against source and measurements; nothing here obliges you to
keep an approach. Read `see me.md` and `NEXT_AGENT_HANDOFF_2026-09-16.md` first
for product mission and earlier history.

## 0. Where things stand

- Branch: `feat/2.0-overhaul`. HEAD: `3ebbb10`.
- Remote `origin` has `feat/2.0-overhaul` at **`4874ca9` only**, pushed per one
  explicit user request (plus a declined PR prompt — do not open one
  unasked). Every commit after `4874ca9` is **local-only**:
  `833e8be` connection fallback/errors, `2ae2464` docs, `7456296` Groq model
  rotation, `b22c246` + `e073c84` distribution hash re-pins, `93bc3ed` docs,
  `f073ad2` testing providers + lip-sync unblockers, `fee23ee` docs,
  `d06cecd` visual-pack resolution tests. Do not push, tag, release, or
  publish anything without a fresh explicit instruction.
- Single review package (no more v24-style versioned folders):
  `E:\temp\InteractiveNPCs\review` (source `e073c84` clean, app SHA-256
  `cc209227…f5878f727`, manifest `297dcafc…d4a639b476`, all verifications
  passed, `launched=false`). Delivery record:
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