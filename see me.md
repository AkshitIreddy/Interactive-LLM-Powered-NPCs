# See me — mandatory product rework handoff

## Latest continuation — 16 September 2026

Read [NEXT_AGENT_HANDOFF_2026-09-16.md](NEXT_AGENT_HANDOFF_2026-09-16.md) for the current v21 build, full user feedback, whole-app changes, provider/content/pack state, and the reopened test-game connection failure. The user reports that connection still fails even when they open the test game. It is **not fixed or live-verified**. The newer handoff distinguishes observed facts from investigation leads; historical build paths and completion statements below must not override it. Preserve the display-safety restrictions below.

## Display safety — renewed explicit instruction, 8 September 2026

The user again reported an epilepsy concern after the native media-broker smoke
suite created a flashing blue/red window. Do not run that smoke suite or any
window-creating test. Hidden terminal execution does NOT make a spawned test
window hidden. Do not run aggregate native CTest against an unaudited or stale
build cache. Use build-only checks and individually audited windowless unit
tests. Window-creating native tests must require explicit opt-in and stay out of
the normal suite. No visible capture/game/GUI/audio testing is authorized.

The Games tab must be called **Games**, not Night City: the product is intended
to support multiple games, while Cyberpunk 2077 remains the current focus.

## Current directive — re-evaluate and rebuild the product experience

This section is the current source of truth and supersedes the incremental UI
and fixture TODOs lower in this file. The user reviewed the installed app and
found that the present 2.0 work is too text-heavy, visually weak, and often
non-functional. Do not continue polishing the existing pages one card at a
time. Review the entire product, the current implementation, and the useful
behavior of version 1, then rebuild the information architecture and vertical
slice around functions that actually work.

The target is a high-quality Windows-only application for talking to visible
NPCs in safely capturable single-player games. A screen, control, metric, game,
character, memory, or capability appears only when it is backed by real data or
a clearly operable setup action. Explanatory prose supports a function; it does
not substitute for one.

### Mandatory review before further implementation

1. Read this entire file, including the historical implementation evidence
   below. Treat older completion claims as evidence to recheck, not proof that
   the product experience is acceptable.
2. Inspect every current page and trace every button, toggle, card, metric,
   list, and status to its data source, persistence path, native command, and
   tested outcome. Classify each as working, fixture-only, inert, misleading,
   or missing.
3. Read `apps/control/src/Pages.tsx`, `App.tsx`, `Onboarding.tsx`, `data.ts`,
   `ProviderSettings.tsx`, `ProviderLoadoutEditor.tsx`, and the Tauri/runtime
   bridge before redesigning the UI. Current visual density is not the only
   problem; many surfaces are hard-coded presentation fixtures.
4. Review the immutable v1 baseline `503ef3b64a921b6a11efa9e3e0432a0c3de3b619`
   through `git show` and the safe summaries in `docs/legacy/`. Do not execute
   the notebooks/generators, deserialize the face/Chroma pickles, load old
   credentials, or run model-generated `temp.py`.
5. Produce a current-screen functional audit, a v1 behavior comparison, a new
   information architecture, and an implementation/acceptance map before
   resuming broad UI construction. Every retained page must have a checkable
   job and a real empty/loading/error/ready state.
6. Build one coherent real vertical slice before restoring breadth:
   first-run onboarding → detect/select the synthetic game → select the current
   game-scoped character → select/test provider models and stock voice → run a
   real spoken turn → show subtitles/diagnostics based on measured events.

Completion criterion: a reviewer can launch the installed app on a clean local
test state and complete that vertical slice without a terminal, query-string
flags, file editing, or interpreting fixture prose.

## User-observed defects that must all remain tracked

- Opening the test app also opens a terminal/console window. Explorer launch
  must produce only the intended GUI; runtime and broker children remain hidden
  and write structured logs instead of owning visible consoles.
- The Home hero is oversized. Its tagline and huge line breaks stretch the
  page, waste the first viewport, and make the text harder to read. Replace it
  with a compact command surface whose primary state, game, character, PTT and
  start action are legible at a glance.
- The overall UI needs a complete high-quality overhaul. The requested
  direction is cyberpunk-game-inspired: strong hierarchy, controlled neon,
  dense but readable instrumentation, deliberate typography and motion, not a
  giant marketing landing page. Use original/open-licensed assets and design;
  do not copy Cyberpunk 2077 UI art or proprietary fonts.
- Authored profiles appear empty and do not work. Opening a profile must show
  the actual runtime profile data, detection/build status, supported
  characters, lore/content readiness, capabilities, fallback, and actionable
  troubleshooting. A card saying “authored package” is not a profile viewer.
- Characters is half-built. Characters from unrelated games are mixed in one
  global list; search, add, edit, preview, memory and relationship controls are
  inert or fixture copy. The page must be scoped to the selected game/profile,
  and selecting a character must drive real identity, prompt, voice, memory and
  provider configuration.
- Conversation contains invented sessions and speculative relationship/quest
  material. The app cannot claim a current quest, save state, relationship
  event or remembered action unless a verified game signal/profile adapter or
  delivered-turn record supplied it. Show only actually delivered dialogue and
  explicitly sourced context; otherwise show a useful empty state.
- The standalone synthetic game was not detected by the app during the user's
  test. Fix and verify the complete installed-app flow, not only video decode:
  launch `local-app-data\test-game\interactive-npcs-synthetic-target.exe`,
  publish matching PID/HWND metadata, surface the target in the app, select it,
  validate it in the native broker, and show advancing captured frames.
- In-game subtitles need to feel native to the selected game: correct safe-area
  placement, readable foreground depth, speaker labeling, outlines/shadows,
  collision avoidance, DPI/HDR handling and game-appropriate typography.
  Provide multiple open-licensed font/style presets plus Windows fallbacks;
  record each font's license and test non-Latin/RTL/fallback behavior.
- The current screen-space mouth-motion story is incomplete. The system must
  first determine which tracked on-screen actor the player is addressing and
  lock that identity over time. It must never animate a face selected from
  memory alone or whichever detection scores highest for one frame.
- Models is mostly prose about optional lip-sync candidates. It does not expose
  the actual model/provider workflow the user expects: choose, configure and
  test LLM, STT, TTS, stock voice, embeddings and optional local lip-sync;
  create named loadouts; change global/game/character overrides; see cost,
  privacy, latency, language, license and hardware fit; install/remove local
  packs where applicable.
- Diagnostics currently mixes a few native states with simulated checks and a
  fake timeline. It must run real bounded checks, explain failures in plain
  language, offer safe actions, reveal exact evidence provenance, and export a
  genuinely redacted bundle.
- Performance currently presents words and illustrative values rather than a
  useful tool. It must measure this PC, label unmeasured fields, compare presets,
  expose CPU/GPU/VRAM/RAM/frame impact/latency, explain degradation decisions,
  and provide repeatable benchmark actions.
- Onboarding does not appear on normal first launch. `App.tsx` currently opens
  it only when `?onboarding=1` is present. Persist a real first-run completion
  state, launch onboarding automatically when incomplete, and provide a clear
  rerun/reset action.
- Settings and Help need full rework. Several settings sections are generated
  by `SettingsPlaceholder` and toggles do nothing. Help search and quick actions
  must open real local guides or run real checks. Remove controls whose runtime
  behavior does not exist.
- Audit all screens, including Presence and every dialog/toast/command palette.
  The app currently contains many sentences describing intended architecture;
  the new UI must expose meaningful state and actions rather than architecture
  prose.

## Current implementation facts explaining the defects

- `Pages.tsx` imports `GAME_PROFILES`, `CHARACTERS`, and `MODEL_PACKS` directly
  from `data.ts`. These are presentation fixtures, not the runtime's complete
  profile/model state.
- `GAME_PROFILES` is generated from short static seeds. “Open authored profile”
  has no working profile-detail navigation/action.
- `CHARACTERS` is one hard-coded cross-game array. Character detail text,
  relationship thread and several actions are fixed/inert.
- Conversation session names and older turn counts are hard-coded. Any quest,
  relationship or remembered-action language on that surface is not live game
  evidence.
- `ModelsPage` is driven by three static candidate packs; provider/loadout
  functionality lives elsewhere and is not presented as one usable model
  workflow.
- Performance and Diagnostics contain simulated timings/health rows. The UI
  often labels them as fixtures, but the volume of fixture prose still makes
  the surfaces noisy and unhelpful.
- Audio, overlay, storage and updates settings fall through to
  `SettingsPlaceholder`; many toggles use `onChange={() => undefined}`.
- Onboarding defaults to closed unless the URL contains `onboarding=1`.
- The Home hero is a large `command-deck` with multi-line dynamic taglines and
  substantial fixture disclaimers before the useful controls.

These are root causes. A theme change alone will not fix them.

## Screen-by-screen product contract

| Surface | Required job | Acceptance evidence |
| --- | --- | --- |
| Onboarding | Configure a usable first run: hardware/privacy scan, execution mode, installed/synthetic game, provider models/voice, microphone/PTT, subtitle style, optional vision/lip-sync, test turn | Appears automatically on clean state; progress persists; final real simulation succeeds or reports one actionable blocker |
| Home | Start/stop the selected game session and show compact real state | First viewport shows selected game/character, capture/provider/audio readiness, PTT and one primary action; no oversized tagline |
| Games | Detect installations and manage real `GameProfileV2` packages | Scan works; profile opens; details come from runtime profile data; manual EXE and synthetic target paths work |
| Characters | Manage characters inside the selected game only | Game filter is mandatory; known-character reference/identity/voice/prompt data is inspectable; no cross-game global fixture list |
| Conversation | Show delivered dialogue and user-controlled memory | Empty state on fresh DB; only delivered turns appear; source/save/character scope is visible; unsupported quest facts never appear |
| Presence | Configure target identity, tracking, subtitles and optional visual response | Live target/track confidence and fallback are visible; every toggle changes a real runtime setting |
| Performance | Measure and tune the current machine | Run benchmark button produces timestamped CPU/GPU/VRAM/RAM/latency/frame-impact evidence; fixture numbers are absent |
| Models | Configure provider modalities, voices, loadouts and optional local packs | User can change/test LLM, STT, TTS, stock voice, embeddings and lip-sync; changes persist with global/game/character inheritance |
| Diagnostics | Diagnose actual app/runtime/broker/provider/capture/audio/model state | Checks execute; timestamps/provenance are visible; failures offer safe retry/open-log/repair actions; simulated timeline removed |
| Settings | Persist all supported global defaults and overrides | No placeholder sections or inert toggles; device/font/overlay/storage/privacy/update controls have tests and consequences |
| Help | Guide real tasks and troubleshooting | Search opens real local docs; microphone, diagnostics, privacy and setup actions navigate/run correctly |

Any screen that cannot meet its job should be merged into a working surface or
removed until its subsystem exists.

## Known-character identity — preserve the v1 intent, replace the method

Version 1 did have an important behavior that 2.0 has not yet made concrete:
after detecting a face, it searched each character image directory with
DeepFace/Facenet512 and used the match to choose the character-specific prompt,
voice and memory. Relevant safe evidence is in
`docs/legacy/v1-pipeline.md`, `feature-disposition.md`, and the immutable audited
revision. Review the exact old code through `git show`; do not run it or open
`representations_facenet512.pkl`.

The replacement must be an optional, user-local, provenance-aware identity
pipeline:

1. Capture the selected game HWND through WGC and detect all faces/heads in the
   current frame with a benchmarked efficient detector.
2. Maintain temporal actor tracks across frames/occlusion. Give each track a
   sticky actor ID; one weak frame cannot switch identity.
3. Determine the addressed actor from multiple signals: selected/manual target,
   screen position and persistence, dialogue/subtitle/name-tag OCR where
   available, speaking/turn timing, and face embedding similarity.
4. For known characters, compare a track against a game-scoped reference set
   using a modern pinned ONNX/native face embedding model. Compute embeddings
   from licensed, original, or user-private reference images; store versioned
   tensor data in the app database/model namespace, never pickle.
5. Use calibrated per-game thresholds, multi-frame consensus, margin from the
   second-best identity and a confidence lock. Ambiguity keeps the existing
   selected character or asks the user; it never silently chooses another face.
6. Bind prompt, lore, voice and memory only after the actor identity is locked.
   When the actor is occluded/offscreen, continue the selected identity through
   audio/subtitles until explicit evidence changes it.
7. Benchmark candidate detector/embedding/tracker combinations on the synthetic
   replay plus legally sourced per-game/user-local reference sets. Measure
   latency, VRAM, reacquisition, false match and identity-switch rates before
   selecting defaults.

This preserves the useful v1 database-recognition idea while removing its
single-frame search, unstable thresholds, unsafe pickles and serialized disk
pipeline.

## Unknown/background NPC identity — deterministic without demographic guesses

The user asked to revisit v1's unknown-NPC flow, which inferred gender, age,
race and ethnicity from a face and used those guesses to choose a name and
voice. The v1 audit shows why that was unreliable and privacy-sensitive. Do not
restore demographic classifiers or claim protected traits from pixels.

Meet the actual product need with a game-scoped background NPC system:

- Assign a stable encounter ID to the tracked unknown actor and preserve it
  across occlusion/reacquisition and repeated nearby encounters.
- Select from a curated, data-only background-NPC archetype/name/voice library
  owned by that game profile. Use reliable context such as game, locale,
  district/faction/settlement metadata supplied by the profile or user—not
  inferred race/ethnicity/age/gender.
- Seed name, persona and provider-neutral voice traits deterministically from
  the encounter ID so the same actor does not change on every frame/turn.
- Permit manual correction, naming and voice override. Keep a neutral unnamed
  identity when context is insufficient.
- Keep background encounters isolated from named-character memory and from one
  another; record expiry/merge rules explicitly.

## Subtitles and screen-space mouth motion

Subtitles and mouth motion consume the same selected actor track but remain
independent capabilities.

- Subtitle profiles define open-licensed font family/fallback, weight, size,
  outline, shadow/backplate, speaker label, alignment, animation, safe region,
  HDR color and DPI scaling. Ship several original genre presets and allow
  game/profile/character overrides. Verify fonts and licenses in notices/SBOM.
- Placement uses the tracked actor/head anchor when confident, clamps to a
  readable foreground safe area, avoids HUD/dialogue regions, and falls back to
  a stable bottom-center game-style layout when the actor is offscreen or the
  anchor is unreliable.
- Mouth motion begins only after the addressed actor is locked. The visual
  worker receives the exact actor ID, frame ID, timestamp, mouth ROI and audio
  timing. It returns a bounded residual for that same frame/track.
- The compositor applies only a feathered mouth region to a presentation copy
  of the newest compatible frame. Low confidence, stale output, occlusion,
  identity mismatch or budget pressure restores the untouched frame within one
  refresh. Memory text never selects or drives a face.
- The current offline ONNX/Wav2Lip evidence proves only that mouth shapes can
  change on a synthetic still; its soft/waxy lower face fails production visual
  quality and it is not a pack candidate.

## Synthetic game and terminal acceptance

The packaged test target is
`local-app-data\test-game\interactive-npcs-synthetic-target.exe`. It is a
double-clickable Windows GUI executable with sibling video/FFmpeg. It publishes
the exact executable basename, PID, HWND, decoded-frame count and title expected
by the debug control path.

Required end-to-end test:

1. Launch the target from Explorer; no terminal appears.
2. Launch `interactive-npcs-control.exe`; no terminal appears for the shell,
   runtime, broker or helper processes.
3. On first run, onboarding appears and offers the synthetic game.
4. Selecting it validates the exact PID/HWND/executable through the broker,
   starts WGC, and shows advancing frame diagnostics.
5. Closing/relaunching, stale metadata, wrong executable, mismatched PID/HWND,
   protected/anti-cheat state and target loss all produce explicit safe states.

Investigate the console problem at the PE/process-launch boundary. Verify the
Windows subsystem of the control/runtime/broker executables and child process
creation flags. GUI launch must use the Windows GUI subsystem or hidden child
creation as appropriate; stdout/stderr go to bounded app-owned log files.

## Rework order and release gate

1. Functional audit and rendered audit of every current screen/state.
2. Legacy behavior review and modern identity/subtitle architecture decision.
3. New information architecture and compact cyberpunk visual specimen at
   normal, narrow, 150% and 200% scaling.
4. Real onboarding plus synthetic-game detection vertical slice.
5. Runtime-backed Games, game-scoped Characters and delivered-only
   Conversation.
6. Functional provider/model/loadout/voice workflow.
7. Measured Diagnostics and Performance, then real Settings and Help.
8. Actor identity/tracking, subtitle style system and optional mouth residual.
9. Installed-app Playwright/native interaction testing, framebuffer review,
   real provider/audio tests, failure/cancellation tests and clean-state rerun.

For every page, capture and inspect 4–6 readable close-ups plus the full frame.
Exercise every visible control. A green DOM/unit test does not pass visual or
functional acceptance. Do not prepare another installer as the user-review
candidate until the vertical slice works and every remaining fixture or inert
control is either removed or visibly quarantined in an explicit developer
evidence area.

## Historical checkpoint material

Paused at the user’s request on 2026-08-29. Do not push, publish, tag, upload,
activate an updater, use provider credentials, download models, or change the
power profile while resuming this work.

## Active continuation — real speaking/lip-sync qualification

The user later explicitly resumed work and authorized their local test-provider
credentials and disposable model downloads for **private testing only**. The
public-distribution boundary above remains unchanged: do not push, publish,
tag, upload, activate updates, or bundle any test model.

The new evidence gate is stricter than the earlier fixture video: a test is not
complete unless it contains a real stock voice, a real local model render, and
an inspected video with a muxed audio stream. The existing app is still silent
and metadata-only for animation, so no existing Response Console video may be
described as real spoken/lip-synced application E2E.

### Completed in this continuation

- Hardened the one-off ElevenLabs test generator to select only verified
  `premade`/`default` stock voices, reject stale output, measure 24 kHz PCM,
  hash outputs, and round-trip the exact Mara reply through AssemblyAI. The
  real call succeeded using a non-cloned premade voice: 9.639 s, 24 kHz mono,
  peak `0.872101`, RMS `0.181025`, zero clipped samples; ASR matched the full
  reply at `0.9876` confidence and the remote transcript was deleted.
- Fixed and committed the real WGC frame-lifetime bug as
  `334000c fix(capture): own WGC texture before frame close`. The product
  broker now owns/copies a D3D texture before a WGC frame closes.
- Created the fully isolated test root:
  `C:\Users\akshi\Desktop\Code Palace\interactive llm\local-app-data\windows-local\InteractiveNPCsTests\wav2lip-qualification-20260829T163112Z`
  with a Python 3.10 venv, pinned dependencies, original synthetic face/video
  inputs, redacted provenance, hardening scripts, and GPU coordination wrappers.
- Ran the official Wav2Lip path for real: CUDA detected the RTX 4080 and
  processed the full PCM/face-detection preflight. It then correctly rejected
  the official downloaded GAN artifact because it was a TorchScript archive,
  not tensor-only state data. Do **not** bypass this with `weights_only=False`
  or `torch.jit.load`; PyTorch documents arbitrary-code-execution risk for
  untrusted/tampered TorchScript archives.
- Prepared a segregated fallback ONNX graph from a third-party conversion,
  pinned to its Hugging Face revision/content hash. It passes ONNX structural
  validation: standard-domain graph only, expected Wav2Lip inputs/outputs, and
  only standard Conv/ConvTranspose/normalization/activation operators. It is
  expressly **not** a product pack or catalog candidate.

### Current state and exact next steps

1. The real ONNX runs are now complete, CUDA-gated, and released the shared GPU
   file to `no` after each bounded interval. The accepted **offline** full
   exchange is:
   `C:\Users\akshi\Desktop\Code Palace\interactive llm\local-app-data\windows-local\InteractiveNPCsTests\wav2lip-qualification-20260829T163112Z\outputs\eclipse-harbor-full-exchange-offline.mp4`.
   It starts with an explicit typed player question, begins Mara's actual stock
   TTS at 2620 ms, and shows local ONNX mouth motion. It is visibly labeled
   original synthetic/offline/not-live-app-E2E.
2. Retain the final only as functional evidence. Blind review found the mouth
   changes clear and identity stable, but lower-face detail remains soft. The
   candidate fails polished/prod visual acceptance and must never enter a pack,
   installer, default visual mode, benchmark claim, or live-game claim.
3. Preserve the exact rejected artifacts for the adversarial record: unsafe
   official TorchScript checkpoint, short 238-frame mux, full-face blur, green
   intro, and moving-mouth-before-audio intro. Do not reuse any of them.
4. For a truthful live application test, implement the bounded Windows-only
   runtime-host qualification path: concrete ElevenLabs transport, Runtime
   Core TTS bridge, dev-only WASAPI `AudioSink`, explicit live-audio
   authorization/provider/voice selection, and an honest `lip_sync_unavailable`
   state. Production audio still requires the planned broker shared-PCM mapping.
5. Before any later CUDA experiment, re-read
   `C:\Users\akshi\Desktop\Code Palace\gpu use.txt`, require `no`, inspect
   `nvidia-smi` for compute ownership, use the test-root mutex/wrapper, and
   restore `no` in cleanup. Do not contend with another project.

### Live-audio implementation checkpoint

Committed, independently reviewed components now exist but are not yet wired
into the normal application turn:

- `30c4001` / `a2a9359`: concrete and hardened ElevenLabs WebSocket transport
  with a normal-account default, bounded messages, non-cloned stock-voice
  format validation, and a <=150 ms cancellation boundary.
- `b15da19`: Runtime Core no longer drops an audio sink before its cooperative
  cancellation receipt returns.
- `01206d0` / `b7c0022`: `devLiveTts` request shaping is allowlisted, carries
  no secret over IPC, preserves old JSON, carries trusted safety context, and
  remains truthfully fixture-only until an actual provider and output path are
  connected.
- `3d9a72b`, `0c9b2a1`, `0f17ba2`, `0cbbf14`: hosted-TTS bridge creates one
  upstream session per sentence, enforces a conservative cloud/privacy
  descriptor, emits metadata before EOS, and keeps vault-secret conversions
  zeroizing.
- `3c32280`, `413413a`, `221b54c`, `ff68c16`: feature-off developer WASAPI
  raw-speaker probe uses bounded device-rate resampling, owned SPSC handles,
  cancellation wakeups and explicit submission/drain telemetry. It is not a
  production broker replacement and does not claim physical audibility.

A manual real-provider raw-PCM speaker smoke was run through the default
Windows endpoint. The probe exited zero, but the best-effort endpoint-wide
SoundCard loopback capture selected the Bluetooth headphones and did not
correlate cleanly with the source waveform. Treat that run as **inconclusive**,
not evidence that the full reply reached physical speakers. The next rigorous
step, if requested, is the documented native process-scoped WASAPI loopback
verifier rather than a desktop/endpoint-wide capture.

The adversarial ledger for this continuation is
`artifacts/actual-ui-demo/ADVERSARIAL_REFINEMENT_LEDGER.md`.

## What is complete

- The 2.0 local Debug review build was previously packaged through its full
  no-skip gate from the clean local sanitized source and installed into the
  task-owned hands-on location:
  `C:\Users\akshi\Desktop\Code Palace\interactive llm\local-app-data\codex-localcache\InteractiveNPCsHandsOnTest\app`.
- That previous strict package passed lint, tests, security/history scans,
  license/SBOM checks, installer smoke, authenticated shell-to-runtime/broker
  supervision, 20 profile validation, and a final installed runtime doctor.
- The prior walkthrough video was audited and found **not** to be a continuous
  test-game conversation. Do not present it as one. It is capture/runtime
  evidence only:
  `artifacts/actual-ui-demo/actual-synthetic-capture-walkthrough.mp4`.
- Adversarial pass 1 found a concrete mismatch: the UI showed Mara Venn and a
  lighthouse question while the native fixture received a different transcript
  and returned a generic “Hold Resident” response.
- Commit `ef0c787 fix(simulation): align Eclipse Harbor fixture turn` corrects
  that mismatch. The synthetic Eclipse Harbor route now uses the safe generic
  game boundary, passes the exact displayed lighthouse prompt, selects Mara
  Venn, returns a matching three-sentence deterministic reply, and paces only
  deterministic fixture stages for readable recording. It does **not** claim a
  live LLM, STT, TTS, audio, or lip-sync result.
- Validation already completed for that commit:
  - Tauri control library: 56/56 passed
  - Runtime-host integration: 7/7 passed
  - Windows TypeScript typecheck: passed
  - Rust formatting: passed
- The current adversarial ledger is at:
  `artifacts/actual-ui-demo/ADVERSARIAL_REFINEMENT_LEDGER.md`.

## Current strict-gate blocker

The first strict package attempt after `ef0c787` correctly failed in nested
Tauri Clippy, before any new installer was staged:

```text
clippy::large_enum_variant
WireRequest::SimulateTurn(NativeSimulationRequest)
```

The failed output is:

`artifacts/actual-ui-demo/fixture-v4-strict-package.stdout.log`

The failure happened because `NativeSimulationRequest` gained the explicit
generic fixture selection. The prior hands-on app has not been replaced.

## In-progress uncommitted fix — preserve it

An active lane was deliberately interrupted for this pause. Its narrowly scoped
uncommitted work is in:

`apps/control/src-tauri/src/sidecar_protocol.rs`

It boxes `WireRequest::SimulateTurn` and adds a JSON wire-shape regression test,
which preserves the external protocol rather than adding an allow/suppression.
Formatting passed, but its final focused test, Clippy, and commit were blocked
by a transient host-wide `No file descriptors available (os error 24)` failure.
Do **not** discard or overwrite this edit when resuming.

## Clean release-source state

The disposable sanitized local release source is:

`artifacts/local-sanitized-release-source/tree-v3`

It includes the conversation-alignment change as commit `9613503`, but does
not yet include the uncommitted Box fix. It is intentionally separate so the
old repository history is not rewritten. Its older successful strict package
remains under `artifacts/local-sanitized-release-source/packages-strict-20260829T123116Z`.

## Next safe steps

1. Wait for the host process-limit issue to clear. Inspect `git status` and the
   `sidecar_protocol.rs` change; ensure no unrelated files are staged.
2. Run the focused nested Tauri tests and strict Clippy. Confirm the new Box
   keeps the serialized `simulate_turn` JSON shape unchanged. Run the Windows
   TypeScript typecheck again if practical.
3. Commit the Box fix atomically, then apply that commit into
   `artifacts/local-sanitized-release-source/tree-v3`.
4. Run a new **no-skip** strict package from that clean sanitized source. Do
   not use `-SkipChecks` as a substitute. Confirm the manifest reports all
   five security gates true and `local-review-only`/updater disabled.
5. Use the guarded installer-smoke process against the new installer, then
   install that exact package into the task-owned HandsOnTest location. Preserve
   `%APPDATA%\io.github.akshitireddy.interactive-npcs`, Credential Manager,
   existing debug replay metadata, and prior doctor data.
6. Record a new continuous, truthful synthetic-game conversation proof:
   - launch the original CPU-decoded Eclipse Harbor synthetic target;
   - use the currently available primary display (`DISPLAY1`) only if no
     secondary display exists;
   - show real WGC target/frame diagnostics, the readable native deterministic
     Response Spine, the exact player prompt, Mara’s matching native reply,
     and the Conversation ledger;
   - record with CPU FFmpeg/GDI capture and no GPU model inference;
   - label the result as a synthetic test game plus native deterministic
     fixture, with silent/no-lip-sync limitations visible.
7. Inspect the new video frame-by-frame (full frame plus readable close-ups),
   then commission a fresh blind critique. Continue the adversarial refinement
   loop rather than treating one green recording as completion.

## Important boundaries

- The GPU coordination file last remained `no`; no model inference was run.
- The first display is available for task windows; do not move unrelated user
  windows or open visible terminals.
- The current synthetic test cannot honestly be called live gameplay, live
  microphone/STT, live cloud inference, audible TTS, or lip-sync. Keep those
  claims out of the new video until separately proven.

## Cleanup completed after this handoff was created

The user explicitly requested a disk cleanup before work resumed. The following
confirmed rebuildable/inactive material was permanently removed after live-use
checks; it is recoverable by rebuilding or downloading from the documented
upstream sources, not from the Recycle Bin.

- `C:\Users\akshi\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\InteractiveNPCsResearch`
  — 21,107,522,025 bytes of rejected MuseTalk qualification environments,
  duplicated weights, and test output. Hash-verified evidence remains under
  `artifacts/real-weight-qualification`.
- 56 inactive `C:\Users\akshi\AppData\Local\Temp\_MEI*` PyInstaller
  extraction folders — 34,427,387,052 bytes at the final pre-delete snapshot.
  A transient icon lock was retried safely; final remaining `_MEI*` count was
  zero.
- 26 repository-local generated targets and stale copies: root/nested/per-crate
  Cargo targets, native build outputs, demo render scratch space, obsolete test
  installers, old sanitized source trees/tar snapshots, empty package folders,
  and the failed strict-package directory. This removed roughly 51 GB of
  reproducible build/cache output according to the pre-delete inventory.

Preserved deliberately: tracked source and `.git`, `tree-v3`, the last
successful strict package, `artifacts/actual-ui-demo`, installed HandsOnTest
app, offline dependency directories, real-weight evidence, uncommitted wire
fix, and this handoff. After cleanup, `C:` reported 332.76 GiB free.
# Successor-agent brief: Interactive LLM Powered NPCs 2.0

Updated: 2026-09-05 IST

Read this file before changing the repository. It is a transfer prompt and a
truth boundary, not an instruction to preserve the current design. The user
has moved this project to a more capable agent specifically so that agent can
re-audit my work, challenge my conclusions, replace weak methods, and finish a
substantially better product. Treat every current subsystem as a hypothesis
supported by evidence of varying quality. Do not continue an approach merely
because it already contains a lot of code.

## Mission

Turn the old GitHub prototype into a polished Windows desktop application that
lets ordinary PC gamers speak with game NPCs using cloud, local, or hybrid AI.
The expected experience is install, onboard, select/detect a game, configure a
provider or local models, press Start, and play. The game keeps priority over
CPU, GPU, VRAM, RAM, and latency. A useful audio/subtitle conversation must
survive the failure or absence of vision, emotion, memory enhancement, or
lip-sync.

The user expects a major 2.0 product overhaul, not a cleanup of the notebook.
They especially want:

- a much better, distinctive, consumer-grade UI and onboarding flow;
- a provider-agnostic API/local-model architecture with configurable profiles;
- a genuinely measured 5–8 second conversational-response envelope;
- natural, efficient lip-sync on arbitrary moving game characters;
- intelligent local-model VRAM admission while accounting for the game;
- automatic game discovery and useful first-class game profiles;
- character identity, lore, voice, and memory that retain the clever ideas in
  v1 without retaining its unsafe implementation;
- single-monitor reliability and a non-injecting external capture/overlay path;
- deterministic setup, tests, diagnostics, packaging, and a reviewable local
  application plus test game; and
- current research and primary-source evidence before committing to runtimes,
  models, libraries, or licenses.

Do not push, publish, tag, create a release, or enable public update/model
endpoints. Prepare and test locally, then give the user the application and
test-game paths for review. Public release requires explicit later approval.

## Repository and Git truth

- Checkout: `C:\Users\akshi\Desktop\Code Palace\interactive llm\Interactive-LLM-Powered-NPCs`
- GitHub remote: `https://github.com/AkshitIreddy/Interactive-LLM-Powered-NPCs.git`
- Working branch: `feat/2.0-overhaul`
- Original v1 branch/revision: `main` at `503ef3b64a921b6a11efa9e3e0432a0c3de3b619`
- Original v1 tag: `v1.0.0` at `9996575e69cf40d719e326adfea108476d80467b`
- First 2.0 commit after v1: `adc12a7 chore(legacy): remove unsafe prototype assets`
- Response-latency assessment checkpoint: `467d451`
- Current observed-mouth prototype checkpoint: `5f396da`

Inspect `git status`, `git log`, and the current diff before acting. Preserve
user-owned changes. Do not reset or rewrite history. If you need a browsable v1
tree, make a separate worktree below `E:\temp`; otherwise use `git show main:...`
and `git ls-tree`. Do not switch the active checkout away from the 2.0 branch.

The original full brief is represented by
`docs/product-rework/original-brief-acceptance.md`,
`docs/product-rework/original-brief-gap-map.json`, and the requirements and
traceability documents under `docs/requirements`. Re-check them against the
implementation; checked boxes and prose are not proof.

## Inspect the old version directly

Do not limit the audit to the legacy summaries. Inspect the actual code and
notebooks on `main`/`v1.0.0`, including `main.ipynb`, `functions/`, the two
Cyberpunk data trees, single-monitor experiments, face-identification data,
memory stores, per-character voices, and the SadTalker integration.

Useful durable analyses are:

- `docs/legacy/repository-audit.md`
- `docs/legacy/v1-pipeline.md`
- `docs/legacy/feature-disposition.md`
- `docs/legacy/single-monitor-root-cause.md`
- `docs/product-rework/v1-behavior-comparison.md`

Important v1 ideas worth reconsidering on their merits include face recognition
against prebuilt character references, distinct known/background NPC handling,
per-character lore and speaking examples, public versus character-specific RAG,
stable important-character voices, generated background identities, push to
talk, and an audio-only fallback. The old repository used Facenet512 reference
caches and character databases in advance; do not overlook that product idea
merely because its pickle/file-loop implementation was unsafe.

Do not revive plaintext API files inside the product, untrusted pickle/Chroma
indexes, generated `temp.py` execution, blocking notebooks, duplicated game
trees, demographic inference, automatic webcam capture, complete-response
serialization, vendored SadTalker, an opaque OpenCV game mirror, or frozen
rectangular face replacement. Preserve the intent where valuable and redesign
the mechanism.

## Current 2.0 architecture: audit, do not inherit blindly

The current tree is a large Windows-first replacement:

- `apps/control`: Tauri 2 + React control application;
- `apps/runtime-host`: Rust turn supervisor, provider routes, streaming,
  cancellation, timing evidence, memory/identity integration, and diagnostics;
- `native/media-broker`: native capture, audio, and presentation boundary;
- `native/mouth-worker`: tracked mouth residual and atlas experiments;
- `native/subtitle-renderer`: native subtitle presentation;
- `crates/`: credential vault, diagnostics, game discovery/profile, model
  manager, memory, providers, schemas, and supporting contracts;
- `catalog/` and `profiles/`: provider/model metadata and 20 game profiles;
- `demo/` and simulation fixtures: deterministic test target and replays; and
- `packaging/`: Windows package/install/reconciliation scripts.

Start with `docs/architecture/overview.md`, `docs/architecture/data-flow.md`,
`docs/architecture/process-model.md`, all ADRs, `IMPLEMENTATION_STATUS.md`, and
`HANDOFF.md`. Then verify those claims in source and tests. The present design
uses a Tauri control plane, a custom Rust runtime, authenticated local IPC,
native Windows media processes, Windows credential storage, SQLite/FTS-derived
memory, capability-driven providers, a signed-pack model manager design, WGC
capture with documented fallback, and a DirectComposition residual concept.
These choices may be sound, partially implemented, overengineered, or stale.
Judge them independently.

## UI overhaul mandate

The existing Response Console has broad functional coverage and prior headless
screenshots/tests, but the user now explicitly wants a major visual and UX
overhaul. Do not merely reskin individual cards. Audit the information
architecture and redesign the end-to-end journey: first run, game discovery,
provider setup, API/local/hybrid profiles, character and voice configuration,
lip-sync enrollment, performance budgeting, diagnostics, active-session state,
errors, repair, and help.

Build a coherent original design system with excellent typography, hierarchy,
spacing, motion, empty/loading/error states, contextual explanations, and
progressive disclosure. It should feel like a premium Windows gaming utility,
not a generic admin dashboard or AI-generated component library. Keep beginner
defaults frictionless while making advanced per-game/per-character controls
discoverable. Render and inspect the real application at wide and narrow sizes,
high contrast, scaled text, keyboard focus, loading, empty, degraded, and error
states. Use headless browser/screenshots while the display-safety restriction
below remains active. Do not claim visual quality from DOM tests alone.

## Provider, runtime, and model decisions to revisit

Research the current 2026 options again, using official documentation, papers,
model cards, repositories, licenses, and real benchmark evidence. Reassess every
hosted and local LLM, STT, TTS, embeddings, retrieval, vision, identity,
emotion, and lip-sync runtime. In particular, check whether Windows/NVIDIA
runtimes such as TensorRT-LLM, llama.cpp backends, ONNX Runtime, DirectML,
CUDA/TensorRT, or other newer stacks beat the current choices for startup,
streaming latency, memory residency, unload behavior, packaging, and game
coexistence. LM Studio's runtime list was a user reference, not a mandated
backend.

NVIDIA NIM is already a provider option and local smoke evidence exists for
chat, embeddings, ASR experiments, and Magpie TTS. Re-check current NIM terms,
quotas, model availability, stock voices, latency, and whether one key can
realistically simplify onboarding. Never describe it as unlimited from a UI
message alone. Determine whether NVIDIA currently offers an appropriate
lip-sync/face-animation service or SDK; do not assume NIM itself does.

The resource scheduler must evaluate combinations, not isolated models. If a
user chooses local LLM + STT + TTS + embeddings + visual models, model admission
must include the game's reserved VRAM/RAM/compute, persistent and peak memory,
KV cache, model overlap during streaming, load/unload cost, warm pools, CPU
fallback, and device-loss behavior. Explore clever preload, eviction, pinned
host memory, quantization, sequential residency, and process lifetime policies,
but measure them under representative game pressure. Never consume all free
VRAM simply because the models fit on an idle desktop.

## Latency truth

Commit `467d451` adds a redacted latency assessment with a 5,000 ms target and
8,000 ms ceiling. Its honest scope is finalized STT transcript to first decoded
PCM. It includes LLM time to first token, first-token-to-TTS-request delay, and
TTS time to first decoded audio. It does not yet include microphone/VAD/STT,
speaker callback, visual presentation, or a live provider/game-load benchmark,
and the 8-second status is telemetry rather than a cancellation policy.

The user means an NPC should begin responding within roughly 5–8 seconds after
the question, not that the mouth compositor may take 5–8 seconds. The visual
hot path should remain frame-rate capable while conversation work streams and
pipelines. Measure microphone to audible/visible response end to end and expose
both reference results and “This PC” benchmarks without conflating fixture,
component, desktop, game-load, or release evidence.

## Lip-sync truth and current red experiment

Lip-sync is still the largest unresolved product problem. The target is not a
static talking-head video. It must alter only the appropriate mouth/face region
of an arbitrary already-moving game character while preserving breathing,
blinks, pose, camera motion, lighting, hair, occlusion, and the fresh game
frame. It must be plausible on consumer hardware while the game is running.

The strongest retained visual direction is the Python v53 headless prototype:

- moving source: `E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1`
- male Jason API-audio fixture:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-magpie-fixture.wav`
- video:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mara-jason-imagegen-natural-aperture-v53.mp4`
- source/output board:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\source-output-mouth-board.png`
- audit:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mouth-quality-audit-v2.json`

V53 keeps the moving source exterior and transfers a one-time same-identity
observed oral interior. Its stricter audit passed containment/geometry checks,
including 0.996700 openness/aperture Spearman correlation and 0.095730 maximum
changed share outside the expanded contour. It still uses RMS aperture rather
than provider visemes, proves one open reference rather than a natural full
viseme inventory, runs as a 54.907 ms mean / 73.153 ms p95 Python inspection
renderer, and is explicitly `rendered-not-qualified`.

Commit `5f396da` ports parts of the idea into C++, adds a stricter visual audit,
single-state atlas selection, an enrollment script, and a native realistic
proof. It also corrects the OpenSeeFace 66-point topology: points 48–57 are the
five upper and five lower outer-lip samples without dedicated corners; points
58 and 62 are shared left/right corners; points 59–61 and 63–65 describe the
inner opening. Upstream OpenSeeFace notes that its 66 points differ from normal
iBUG-68, so ordinary dlib indexing is unsafe.

The latest v66 native render is rejected despite compiling and passing all six
native CTest suites:

- output:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology`
- close-up board:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology\mouth-board.png`
- report:
  `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology\headless-proof.json`
- measured failure: `0.532468` maximum upper-lip darkening and `0.4`
  articulated rows/mouth width;
- visual failure: a cleaner but still artificial dark oval/hole, missing teeth,
  distorted lip surfaces, and poor identity-preserving anatomy.

The oral-normalized enrollment atlas used by v66 is:
`E:\temp\InteractiveNPCs\review-mouth-atlas-v6-oral-normalized`. Its preview
contains intentional rectangular normalization artifacts outside the runtime
oral mask and is not itself an acceptable pasted-mouth texture.

The latest 1920x1080/250-iteration native performance run measured 6.402 ms
geometric mean, 6.323 ms direct-atlas mean, and 4.612 ms mean / 4.682 ms p50 /
7.004 ms p95 / 7.264 ms p99 for worker selection plus composition. The v66
realistic proof measured 1.756 ms compositor p95 and approximately 26.5 ms
moving OpenSeeFace inference p95 at 15 Hz. These are component timings, not
quality acceptance or end-to-end game evidence.

Study these files before deciding whether any current method survives:

- `docs/research/realtime-lipsync-overhaul-2026-09-04.md`
- `docs/research/headless-realistic-lipsync-proof-2026-09-04-v6.md`
- `scripts/benchmarks/render-dense-observed-lip-proof.py`
- `scripts/benchmarks/prepare-generated-mouth-atlas.py`
- `native/mouth-worker/src/compositor.cpp`
- `native/mouth-worker/src/signal_adapter.cpp`
- `native/mouth-worker/tools/audit_headless_mouth_quality.py`

Do not optimize or polish v66 blindly. Re-evaluate whether source-pixel warping,
observed-state oral transfer, denser landmarks, segmentation, optical flow,
3D-aware reconstruction, neural residuals, NVIDIA Maxine, a newer causal model,
or a different architecture gives the best quality/resource trade-off. Test the
strongest candidates on a clearly visible moving/blinking/breathing person or
character. Inspect enlarged mouth sequences and source/output comparisons;
numeric motion gates have previously passed visibly terrible holes, detached
slits, flat teeth, double contours, and pasted mouths.

A heavy model may be valuable as an optional one-time enrollment teacher if it
produces a tiny reviewed identity-bound pack and fully unloads before gameplay.
That was my promising idea, not a requirement. A stronger direct runtime should
replace it if measurements justify the trade. The default architecture should
remain API-first for LLM/STT/TTS and efficient/local for lip-sync only if that is
still the best researched product decision.

## Evidence and release boundaries

`IMPLEMENTATION_STATUS.md` contains many completed component checks, but major
gates remain open: live ASR qualification, natural lip-sync, generated-reference
native parity, provider-cue synchronization across real utterances, broad
face/pose/occlusion coverage, cross-process D3D transport, HDR, representative
game-load testing, selectable model packs, clean Windows VM install/repair/
upgrade/uninstall, signing, and real-game certification.

The current review executables predate v53/v66 and must not be presented as the
current lip-sync implementation:

- app: `E:\temp\InteractiveNPCs\review-app-v13-stronger-mouth\interactive-npcs-control.exe`
- test game: `E:\temp\InteractiveNPCs\review-test-game-v13\interactive-npcs-synthetic-target.exe`

Package a fresh, source-identified review build only after the source settles.
Keep package/build/model artifacts below `E:\temp`. Do not run a GUI installer
smoke or visible desktop test while the display-safety restriction remains in
force. A future fresh review layout may use:

- package root: `E:\temp\InteractiveNPCs\package-v15`
- installed/review app: `E:\temp\InteractiveNPCs\review-app-v15`
- test game: `E:\temp\InteractiveNPCs\review-v15\local-app-data\test-game`

Verify exact packaging commands against `docs/development/packaging.md` and the
scripts; do not treat these proposed directories as already created evidence.

## Machine, secrets, GPU, and display rules

- Codex is Windows-native. Keep commands and development tests hidden and
  noninteractive. Do not open, pin, focus, or move terminal/app/player windows.
- Visible white/blue flashing, beeps, and flicker caused a serious epilepsy
  concern. Desktop capture and GUI-driving are paused. Prefer headless renders,
  files, screenshots, contact sheets, logs, and process checks.
- The user's Transparency App is normally active and dims the screen; it can
  invalidate desktop-capture appearance. Repository:
  `https://github.com/AkshitIreddy/Transparency-App`.
- `C:` is space-constrained. Put model weights, extracted runtimes, build caches,
  package staging, video frames, benchmark artifacts, and other large data in
  `E:\temp`, preferably below `E:\temp\InteractiveNPCs`.
- User-supplied API keys are in
  `C:\Users\akshi\Desktop\Code Palace\Commonly used Keys.txt`. The user permits
  reading and bounded testing, but never print, copy into source/docs, log,
  commit, or include secret values in diagnostics.
- For AI-model GPU work, coordinate through
  `C:\Users\akshi\Desktop\Code Palace\gpu use.txt`: if it says `yes`, do other
  work or wait; if it says `no`, set it to `yes`, use the GPU, and restore `no`
  immediately afterward, including failure cleanup. Non-AI rendering/testing
  may use the GPU without taking this model lock.
- Keep public release and GitHub push disabled until the user explicitly
  approves them. The current branch is local work.

## Verification completed at this transfer

- `cargo test -p npc-runtime-host --lib`: 96 passed, 2 intentionally ignored.
- `cargo fmt --all -- --check`: passed.
- Native Release CMake build: passed.
- Native CTest: 6/6 passed.
- Python compilation for the visual audit, dense proof, and enrollment scripts:
  passed using the isolated OpenCV runtime under `E:\temp`.
- Latest realistic v66 native render: ran headlessly and failed the quality gate;
  it is preserved as rejected evidence.

Do not infer that the complete workspace, UI, installer, providers, game
profiles, desktop presentation, or live games were requalified on 2026-09-05.
Earlier broad verification is recorded in `IMPLEMENTATION_STATUS.md`; rerun the
relevant gates after material changes.

## Recommended successor workflow

1. Read the original brief, requirements traceability, architecture, current
   implementation status, and this transfer. Inspect both current source and
   old v1 source directly.
2. Produce an independent red-team assessment: what to keep, simplify, replace,
   delete, or prove. Distinguish implemented code from product acceptance.
3. Research current providers, Windows/NVIDIA runtimes, model/resource
   scheduling, character-identification methods, and moving-face lip-sync with
   primary evidence. Check licenses and deployability, not only demos/FPS.
4. Rework the architecture wherever evidence supports it. Preserve typed
   failure isolation, security, privacy, cancellation, and graceful degradation.
5. Overhaul the UI and actual user journey. Build and inspect rendered states;
   make it original, premium, approachable, and responsive.
6. Prototype competing lip-sync approaches headlessly on the same moving source
   and additional representative footage. Compare enlarged sequences, temporal
   stability, identity preservation, latency, CPU/GPU/VRAM, load/unload time,
   game-pressure behavior, license, and packaging complexity.
7. Integrate only the winning qualified path. Keep audio/subtitles available and
   visuals fail-open while confidence or support is inadequate.
8. Run proportionate unit, integration, simulation, visual, performance,
   security, privacy, packaging, and clean-environment checks. Record evidence
   with exact source/artifact identity and reject misleading claims.
9. Build a fresh local review app and realistic synthetic test game under
   `E:\temp`, review the result yourself headlessly first, then give the user
   exact paths and an honest list of any remaining gates.

Completion means the product is genuinely coherent and reviewable, not merely
that the existing TODO list was edited or every current method was preserved.
