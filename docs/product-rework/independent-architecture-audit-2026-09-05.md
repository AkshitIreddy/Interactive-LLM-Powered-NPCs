# Independent architecture audit — 2026-09-05

Snapshot notice: “current” in this audit means the source inspected on September
5, before the follow-up implementation. The mounted UI has since been rebuilt;
Mistral/OpenRouter LLM and Cartesia/Deepgram/Inworld TTS routes are executable;
the selected Groq Qwen → Cartesia component chain reached first PCM in 492.964
ms; and YuNet now tracks between full detections with a 12-frame refresh. The
visual result remains experimental, the private YuNet pack is not yet active,
and no physical audio, installed GUI, or live-game acceptance follows. See
[the reconciled review record](local-review-2026-09-05.md).

## Audit identity and limits

This is a new source-level audit of the immutable legacy revisions and the current
`feat/2.0-overhaul` architecture. It is intentionally independent of the conclusions in
the existing handoff and legacy audit. Existing documents are treated as claims and
cross-checks, not as proof.

Legacy baselines:

- `main` at `503ef3b64a921b6a11efa9e3e0432a0c3de3b619`
- `v1.0.0` at `9996575e69cf40d719e326adfea108476d80467b`

The legacy tree was read only through `git show`, `git diff`, and tree listing. No
notebook, model, credential-bearing file, pickle, Chroma database, generated Python, or
legacy executable was run. Current 2.0 source, architecture records, profiles, and the
original-brief acceptance ledger were read from the working tree. This audit lane did
not run a GUI, game, installer, model, or release build. Therefore it can establish
implemented contracts and source gaps, but it cannot promote visual, performance,
installed-app, or live-game acceptance.

## Executive verdict

The 2.0 architecture is a sound replacement for v1 at its safety and process boundaries,
but it is not yet a working replacement for the v1 user experience. The codebase has
credible native capture, overlay, provider, memory, model-lifecycle, and fail-open
contracts. Its central product loop is still broken at three joins:

1. Authored game profiles are deliberately kept in console-only mode, so the profile
   corpus does not yet provide supported-game behavior.
2. Production actor identity is deliberately unqualified, so the visual route cannot
   safely activate for a moving character.
3. The audited mouth compositor had component evidence but no accepted moving-character
   result; the latest native output at audit time was explicitly rejected visually.

The correct response is not to restore v1's frozen-frame/SadTalker method. Keep the 2.0
boundaries and finish one rights-cleared vertical slice through them. Replace the visual
synthesis method, reconcile the local-model policy with the implemented model manager,
remove or isolate fixture UI, and qualify each bridge with live evidence.

## What v1 actually did

The tag and `main` are materially the same application. Comparing the immutable trees
shows that `main` changed documentation and the single-monitor screen-grab helper; it did
not replace the notebook interaction pipeline. The single-monitor helper moved from
virtual-screen `BitBlt` capture to locating a named window and using `PrintWindow`, while
the root notebook and orchestration modules remained unchanged.

### Observed turn path

| Stage | Actual v1 behavior | Evidence at immutable revision | Consequence |
| --- | --- | --- | --- |
| Startup/configuration | A notebook defines a player name, game name, interaction key, animation toggle, and a fixed 1920×1080 presentation path. | `main.ipynb`; `miscellaneous/Single Monitor/main.ipynb` | It is a developer-operated script, not an installed product. |
| Interaction | OpenCV mirrors a captured frame, polls the interaction key with `waitKey`, waits two seconds, records microphone input synchronously, and briefly displays the transcript. | `main.ipynb` | The game view and turn are blocking; there is no cancellable streaming pipeline. |
| Character choice | One captured frame is scanned. Multiple detected faces are reduced to the highest-confidence face. Each character directory is searched with DeepFace/Facenet512 and the lowest cosine score wins. | `functions/face_detection.py`; `functions/find_character.py` | Identity is single-frame nearest-neighbor matching without a calibrated global reject threshold, temporal lock, or multi-actor selection. |
| Context | Public and character Chroma clients plus Cohere embeddings are reconstructed during calls. Retrieval uses one result and recent conversation text. | `functions/get_public_data.py`; `functions/get_character_data.py` | The useful idea is context separation; the implementation is expensive, provider-coupled, and weakly versioned. |
| Prompt/generation | Four near-duplicate audio/video and known/background generators assemble prompts and call Cohere. Known characters include biography, response-style examples, public lore, character lore, recent conversation, and webcam emotion. | `functions/generate_*_response.py`; character data files | Character authoring is worth preserving as data. The duplicated orchestration and provider coupling are not. |
| Memory | The assistant response is appended before audible/visible delivery. At a token threshold, history is either discarded or summarized through Cohere, then the JSON log is cleared. | `functions/conversation_loader.py`; response generators | Failed playback can become false memory; compaction is destructive and not receipt-based. |
| Audio | Edge TTS is generated after the complete text response. Character scripts hard-code voices such as `en-US-EricNeural`. | character TTS scripts; response generators | Stock TTS is a useful frictionless fallback, but route selection, voice discovery, cancellation, and delivery proof are absent. |
| Animated output | Complete TTS and complete SadTalker generation finish before playback. The notebook freezes the captured game image and pastes each generated talking-head frame into a rectangular face region while audio plays. | `main.ipynb`; video generators | This is not live game lip-sync. It freezes the world, replaces too much of the face, adds long batch latency, and cannot track motion or occlusion. |
| Background actors | Video mode compares against one mutable default face and may overwrite a single background persona with inferred age/gender/race. Audio mode rotates one randomly generated background identity after ten minutes. | `functions/generate_background_video_response.py`; `functions/generate_background_audio_response.py` | One mutable fallback identity conflates unrelated actors. Demographic inference is unnecessary and should remain removed. |
| Webcam | Each turn may open camera index 0 and infer the player's emotion automatically. | webcam emotion helper; `functions/main.py` | This violates the 2.0 privacy/default-off direction and adds an unrelated fragile dependency. |
| Execution/security | API keys are read from plaintext `apikeys.json`. Generated response content is interpolated into root `temp.py`, run through a virtual-environment Python, then deleted. | retrieval and generation modules | Plaintext secrets and generated-code execution are unacceptable product mechanisms. |

### What the old README promised versus what source supports

The legacy README describes seamless use with any game, face recognition, memory,
pixel-level replacement, an audio fallback, and SadTalker animation. Source supports a
manually configured notebook demonstration of those ideas. It does not support an
installed, automatic, game-agnostic product claim. Setup requires command-line tooling,
Python, Git, download utilities, Visual Studio build tools, FFmpeg, Jupyter, model
downloads, plaintext key setup, and hand-authored character data. It also requires a
separate single-monitor file replacement. These are material gaps from the original
brief's novice install and one-monitor requirements.

## Legacy feature disposition

| Legacy capability | Decision | 2.0 replacement or removal condition |
| --- | --- | --- |
| Push-to-talk conversation | **Keep the user intent; replace the implementation.** | Native hotkey/microphone capture, cancellable streaming STT, clear listening state, and one measured ordinary turn. |
| Character biography and dialogue examples | **Keep and migrate as data.** | Versioned character/profile schema with provenance, spoiler scope, validation, and no imported executable state. |
| Public lore and character-specific retrieval lanes | **Keep the separation; replace storage/provider coupling.** | Local SQLite/FTS memory and pluggable embeddings/retrieval with migration, recovery, and relevance evidence. |
| Delivered conversation memory | **Keep the goal; replace pre-delivery writes.** | Commit NPC text only after an audio or subtitle delivery receipt. Preserve failed/cancelled turns as diagnostics, not memories. |
| Automatic face recognition | **Keep only as an optional assist.** | Explicit/manual selection stays authoritative. Automatic identity needs temporal tracking, a calibrated reject option, OCR/dialogue cues, privacy-qualified local embeddings, and reacquisition evidence. |
| Stock voices | **Keep as a first-run voice route.** | Expose deterministic assignment and preview in the voice UI; prove a selected stock or hosted route on the physical output endpoint. |
| Audio-only fallback | **Keep as a release invariant.** | Speech and subtitles must remain usable when identity, capture, overlay, model, or compositor work is unavailable. |
| SadTalker batch animation | **Remove.** | Do not wrap or optimize it for the live path. It fails latency, current-frame, tracking, and game-impact requirements by construction. |
| Frozen-frame mirror/player | **Remove.** | Present only overlay layers over the live game; never substitute the game's full frame. |
| Rectangular face paste | **Remove.** | Only a bounded, feathered, identity-bound mouth residual may be composed, and only onto the matching current presentation frame. |
| One-frame nearest-face match | **Remove.** | Actor lock must be a temporal state machine with unknown/ambiguous states and manual recovery. |
| Random/mutable background persona | **Remove.** | Every encounter needs a stable, inspectable ID; unknown actors remain unknown until deliberately assigned. |
| Automatic webcam emotion | **Remove from the default product.** | Any future perception feature must be explicit, optional, local/privacy-scoped, and separately justified. |
| Demographic inference | **Remove.** | It is unnecessary to the conversation loop and cannot determine character identity safely. |
| Plaintext key file | **Remove.** | Use Windows Credential Manager references and keep secret bytes out of WebView, logs, child environments, and exports. |
| Generated `temp.py` execution | **Remove.** | Provider calls and TTS are typed code paths with bounded inputs, cancellation, and receipts. |
| Notebook/manual setup | **Remove from the product path.** | Signed installer, first-run onboarding, repair/uninstall, and no required development toolchain. |

## Current 2.0 architecture assessment

### Decision matrix

| Subsystem | Keep | Replace/remove | Prove before product claim |
| --- | --- | --- | --- |
| Tauri/native process model | Control plane, supervised sidecars, named pipes, Job Object lifetime, console-free launch | Correct documentation that implies fully typed protobuf business messages | Crash/restart/cancellation, orphan prevention, clean install, and long-session reliability |
| Runtime turn orchestration | Explicit route snapshots, cancellation generations, timing ledger, delivery receipts, fail-open optional work | Remove fixture effects from any production route | One real selected STT → LLM → sentence → TTS/subtitle → delivered-memory turn |
| Provider layer | Provider traits, Credential Manager references, explicit egress and route selection | Remove hard-coded or hidden fallback behavior from retained product flows | Credentials, model discovery, error/cancel/rate-limit behavior, physical mic/output for each promoted route |
| Character and memory | Data-only profiles, prompt authority lanes, SQLite/FTS, delivery-gated commits | Remove any duplicate pre-delivery or UI-only conversation state | Migration, recovery, delete/export, disk-full, relevance, latency, and repeated installed-app turns |
| Game profiles | Schema, provenance, replay fixtures, capability flags | Remove “supported” language inferred only from corpus presence | Per-capability certification on the current build and executable/profile revision |
| Game discovery/targeting | Explicit HWND target and safe external observation | Remove named-window/manual-file workaround as the primary flow | Store/common/manual discovery, duplicate/moved installs, foreground changes, protected/unknown targets |
| Identity | Native-only pixels, sticky IDs/epochs, unknown state, explicit selection authority | Replace unqualified product bus; improve greedy association where replay benchmarks justify it | Detector/gallery pack, threshold calibration, multi-actor crossings, occlusion, reacquisition, manual correction |
| Capture/overlay | WGC by HWND, D3D resources, DirectComposition, fail-open display path | Remove all frozen mirror/full-frame replacement paths | Physical display/mode/DPI/HDR/device-loss/self-capture matrix |
| Subtitle delivery | Native renderer authority and committed presentation receipts | Remove fixture receipts from acceptance evidence | Live timing, typography, bidi/graphemes, scaling, contrast, safe-area and HDR review |
| Lip-sync orchestration | Actor/frame/audio clock binding, freshness, mask, lease, cancellation and current-frame invariants | Replace the rejected atlas/painted-mouth compositor result | Moving-character visual and performance gates with blind review |
| Model manager | Signed lifecycle, exact manifests, measured whole-loadout governor | Replace stale lip-sync-only ADR; remove unavailable candidates from normal UI | One qualified pack per promoted role, including license, download/repair/remove and pressure behavior |
| Product UI | Job-oriented workspace concept and local diagnostics | Remove duplicate fixture pages, inert controls, query-only onboarding, simulated state presented as product state | Native-backed persistence, narrow/accessibility/scaling review, novice first run, returning-user flow |

### Keep: native process boundaries and fail-open degradation

ADR-0001 through ADR-0004 choose a Tauri control plane, a Rust runtime host, native
sidecars, and provider traits. The source implements more than a diagram:

- the control application suppresses a Windows console in release-style startup;
- sidecars use named-pipe framing, per-user scoping, process supervision, and Windows Job
  Object ownership;
- runtime routing enters deterministic fixture mode only when supervisor health is
  explicitly `DevelopmentFixture`;
- sidecar protocol requests are versioned and bounded;
- optional visual work is fail-open and cannot block the speech/subtitle path.

These boundaries directly eliminate v1's notebook process, generated Python, global
temporary files, and frozen display loop. Preserve them.

One documentation correction is required. The sidecar IPC is not a fully protobuf-typed
application protocol. `EnvelopeV1` and `ControlRequestV1` are protobuf-framed, but the
business request is serialized into `payload_json` in
`apps/control/src-tauri/src/sidecar_protocol.rs`. This hybrid is valid if intentional,
but the architecture must state it accurately. Either keep it and document JSON schema
versioning/compatibility rules, or move high-risk business messages to typed protobuf.
Do not cite “Protobuf” alone as proof of end-to-end type safety.

### Keep: current character context and delivered-memory contracts

The current character database and memory crates are a decisive improvement. They use
SQLite with strict schemas, WAL/FTS, explicit transactions, derived embeddings, and
separate prompt-authority lanes for canon, character facts, public game context,
character-authored context, retrieved memory, session summaries, and recently delivered
turns. Delivery receipts gate NPC memory commits. This solves the most serious semantic
defect in v1: remembering responses that were never heard or shown.

Keep this design. It still needs disk-full, corruption, migration, restore, delete/export,
latency, and retrieval-relevance evidence in the installed app before R14 can pass.

### Keep the profile schema; stop equating corpus size with game support

The checked-in profile corpus is substantial and data-only. It includes stable character
IDs, biographies, prompts, voice policy, identity policy, provenance, and replay metadata.
That is useful product content and a safe successor to v1's per-character scripts.

Current control-source logic deliberately treats authored non-synthetic profiles as
“console isolated” with no game interaction. Profile metadata frequently records that
live capture or identity remains unverified. Therefore the profile-corpus PASS in R15
means schema/content coverage only. It does not prove discovery, foreground targeting,
capture, identity, subtitles, lip-sync, or a conversation in any listed commercial game.

Keep all honest capability flags. In the UI, label a profile **Ready** only when its
declared capability matrix has installed-app evidence for the current executable/build,
display mode, and route. Use **Profile available** or **Audio/subtitle unverified** for the
rest.

### Keep hosted provider bridges; prove one ordinary selected route

The runtime host has real selected-hosted-LLM adapters for OpenAI, Anthropic, Gemini,
Groq, Cohere, and NVIDIA NIM. The normal selected STT bridge currently admits only
AssemblyAI `u3-rt-pro`. TTS provider types are broader than executable credential paths:
ElevenLabs and NVIDIA NIM Magpie have construction paths, while a complete normal
selected-TTS-to-physical-output turn remains unqualified. Hosted semantic retrieval is a
specific NVIDIA `nvidia/nemotron-3-embed-1b` bridge. Cataloged providers such as generic
`openai-compatible` LLM and the other stable STT entries are not ordinary executable
routes merely because they appear in catalogs or doctor output.

Source breadth is not acceptance. The required proof is one non-fixture end-to-end turn:
foreground push-to-talk, real microphone PCM, selected STT, selected LLM, sentence
streaming, selected named voice, first PCM on the physical endpoint, bottom-center
subtitle presentation, cancellation, and receipt-gated memory. Then expand the provider
matrix. This order reveals integration faults sooner than isolated adapter tests.

Structured actions are still a gap: `apps/runtime-host/src/simulation.rs` constructs
`FixtureEffects` unconditionally. R19 must remain open until a selected LLM's structured
output is schema-validated, late/malformed actions neutralize without delaying speech,
and game actions remain outside the safe contract unless a separate approved path exists.

### ADR-0005 now records the architecture the code and brief require

The former ADR-0005 and parts of ADR-0007 stated that optional local packs were limited
to generic lip-sync and that local LLM/STT/TTS/embedding packs were outside product
policy. Current source no longer implements that narrow thesis:

- `ModelPackKindV1` includes language model, speech recognition, speech synthesis,
  embedding, vision, and lip-sync;
- manifest v2 defines role-specific extensions for all six;
- the measured local-pack selection policy admits all six generic roles;
- the governor reasons about whole-loadout p99 RAM, resident/transient VRAM, load and
  reload cost, operation latency, desktop use, live game use, and configured game reserve;
- the built-in research catalog includes candidates for each role.

The original brief also requires local/hybrid operation as an optional experience.
ADR-0005 was superseded on 2026-09-05: the base install remains API-first and model-free,
while explicit, signed, measured local packs may serve all six roles. Admission is
loadout-wide, preserves the game reserve, exposes cold/warm switching cost, and forbids
silent provider/device substitution.

Runtime-core also contains an opt-in TurnSupervisor/ResourceBroker join driven by a
host-supplied measured plan. It can reject reserve shortfalls and release leases, but the
normal runtime host does not yet receive a trusted native-stamped selected-game budget
plus a qualified whole-turn envelope. Treat this as implemented core policy, not
production resource enforcement.

Do not expose dormant catalog entries as available models. Current local resources
correctly start as `NotInstalled`, the lip-sync pack is incomplete, identity trust is
fail-closed, and installability requires signed manifests and measured envelopes. Keep
those gates. Qualify one role at a time; local STT or TTS is a better first product proof
than pretending a full local loadout is ready.

### Keep the media broker implementation; qualify its display behavior

ADR-0008 is supported by concrete native code:

- `native/media-broker/src/windows_capture.cpp` creates a Windows Graphics Capture item
  for the selected HWND and starts capture;
- `native/media-broker/src/windows_platform.cpp` implements Desktop Duplication fallback
  and shared D3D resource handles;
- `native/media-broker/src/windows_overlay.cpp` creates a composition swap chain,
  DirectComposition target and root visual, then commits it;
- `native/media-broker/src/windows_presentation_context.cpp` records advanced color/HDR
  state without guessing when evidence is unavailable.

This is the right solution to v1's one-monitor mirror and `PrintWindow` limitations. It
still requires installed source-identified proof for the full matrix: one/multiple
monitors, negative origins, move/resize, borderless/windowed/exclusive behavior, alt-tab,
minimize, protected surfaces, device loss, 100/150/200% DPI, 720p through 4K and
ultrawide, SDR/HDR, self-capture prevention, and overlay z-order/focus/click-through.
Simulated display-matrix tests document policy; they do not prove those physical modes.

### Keep the identity contract; replace the current product readiness story

The identity engine improves on v1 by assigning sticky track IDs and epochs, carrying
normalized embedding model/revision/preprocessing/source-hash metadata, preserving
unknown/ambiguous states, and defining encounter and reacquisition rules. Manual actor
picker presentation states also exist in the control source.

The product route is nevertheless deliberately disabled. The header of
`apps/control/src-tauri/src/identity_runtime.rs` says activation is withheld until Model
Manager supplies a measured v2 pack, and `AppState` constructs
`NativeActorLockBusV1::new_unqualified()`. Pixels and coordinates are withheld from the
WebView by design. This is responsible fail-closed behavior, but it means moving-character
identity is not integrated product functionality.

Before activation, improve and prove the resolver rather than copying v1's method:

1. Detect faces/heads/person regions on trusted WGC frames and emit timestamped candidate
   sets, including “none” and “ambiguous.”
2. Associate candidates with a global assignment method under crossings and occlusion;
   benchmark the current greedy association against Hungarian/min-cost matching before
   deciding it is sufficient.
3. Fuse appearance with face geometry, dialogue-box/OCR speaker evidence, screen region,
   motion continuity, and user selection. Treat profile/gallery embeddings as one cue,
   not the sole truth.
4. Calibrate per-model accept/reject/ambiguity thresholds on rights-cleared replay data.
   Never force the nearest gallery result when all matches are poor.
5. Hold a selected actor lock through short occlusion, expire it on defined evidence,
   and provide a quick manual correction path whose choice seeds the current encounter.
6. Prove multi-actor crossing, partial face, profile change, cutscene, camera pan, speaking
   off-screen, disappearance/re-entry, and wrong-person avoidance. Report identity errors
   separately from mouth-anchor errors.

### Keep the visual orchestration boundary; replace the mouth-rendering method

ADR-0007's immutable-current-frame contract is the correct answer to v1. Visual work is
bound to actor ID, track epoch, capture sequence, geometry generation, audio playback
clock, cancellation generation, resource lease, and a short deadline. Source limits
admitted signals to 15 Hz, rejects stale/incompatible work, and produces fail-open
receipts. The game frame remains authoritative.

The current visual result is not acceptable. The handoff records two distinct states:

- the source-preserving component route showed measured aperture changes but did not
  prove identity, visemes, full mouth appearance, moving-frame anchoring, or the installed
  app;
- the newer native output was visually rejected for a dark oval/hole, missing teeth, and
  upper-lip damage.

Retain the orchestration and validation contracts. Replace the renderer with a tracked,
source-conditioned deformation/residual system:

1. Build a per-actor neutral mouth reference from recent high-confidence source frames;
   never use a generic painted cavity as the primary appearance.
2. Estimate a stabilized 2D/2.5D lower-face mesh in current-frame coordinates and carry
   confidence, pose, occlusion, and lighting metadata with every proposal.
3. Drive motion from the exact playback stream using a provider-neutral canonical viseme
   timeline. Prefer provider timing events; use a bounded PCM classifier when events are
   absent; use amplitude only as the final degrade mode.
4. Warp source pixels for jaw/lip shape first. Inpaint only newly exposed interior pixels
   inside a dynamic inner-mouth mask. Synthesize teeth/tongue only when visibility and
   source evidence require them, with temporal identity and lighting consistency.
5. Use optical-flow/landmark reprojection to anchor each presentation-frame residual.
   Discard rather than stretch through large pose changes, occlusion, cuts, or stale
   frames.
6. Composite in linear color with edge-aware masks, per-frame color matching, and temporal
   regularization. Validate zero changes outside the admitted dynamic mask.
7. Run the compositor at display cadence even if semantic viseme updates are 15 Hz;
   interpolate stable geometry/controls and always sample the newest compatible game
   frame.

The first acceptance corpus should include synthetic rights-cleared dialogue with camera
translation, yaw/pitch, expression changes, hand occlusion, hair/facial hair, varied skin
tones, HDR/SDR, and two crossing actors. Score lip/phoneme alignment, anchor drift,
outside-mask change, identity switches, temporal flicker, frame age, p95 latency, FPS,
VRAM, 1% lows, and blind human preference against audio-only. If it fails, ship the
vertical slice with audio/subtitles and keep animation experimental.

### Replace the product UI's source of truth

At audit time the frontend source contained two competing generations of product surface. Newer
workspace/console components express the intended product model, while `Pages.tsx` still
imports hard-coded game, character, and model-pack collections, creates simulated session
timings, and contains inert `onChange={() => undefined}` controls. A fixture label makes a
test honest; it does not make an inert product control useful.

Use one UI application state derived from native commands and receipts. Every retained
control must mutate persisted configuration, launch a command, navigate, or show a clear
disabled reason. Remove duplicate/obsolete pages from the production route rather than
maintaining two information architectures.

Recommended five workspaces:

1. **Home** — foreground game, selected character, readiness, one primary Start/Stop
   action, current fallback, and the next fix when blocked.
2. **Games & characters** — discovery, profile capability/certification, explicit actor
   selection, identity confidence, voice assignment, and per-game overrides.
3. **Voice & intelligence** — coherent LLM/STT/TTS/retrieval loadout, credentials,
   privacy/egress, voice preview, and optional local packs with resource cost.
4. **Appearance** — subtitles first; animation only when the identity and visual pack are
   admitted, with an honest experimental state and immediate off switch.
5. **Health & support** — microphone/speaker/provider/game/capture/overlay checks,
   correlated turn timing, redacted export, repair, and recovery actions.

Onboarding should discover a target, explain cloud/local data movement, validate only the
credentials required for the chosen loadout, test microphone and voice, and end at a
real ready Home state. Query parameters and fixtures cannot be its persistence layer.

## Acceptance gaps against the original brief

The central ledger currently records 8 PASS, 7 FAIL, and 37 NOT MEASURED requirements,
with no populated evidence records in its generated gap map. Some named compile failures
in that ledger may be stale after later source changes; the acceptance conditions remain
open until a fresh source-identified full check promotes them.

### Blockers that prevent calling 2.0 a replacement

| Brief area | Current source finding | Required proof |
| --- | --- | --- |
| Clean novice install/onboarding (R02, SC01) | Installer and onboarding code exist, but this audit has no clean-machine evidence. | Non-admin Win10/11 install with no toolchain or console; configure and start without filesystem/manual steps; repair, upgrade, uninstall. |
| Coherent UI (R04, R28, SC10) | Duplicate UI generations, hard-coded collections, inert controls, and simulated sessions remain. | Single native-backed state model; normal/narrow keyboard/Narrator/controller and 100/150/200% rendered review. |
| Hosted ordinary turn (R05–R07, R29, SC03) | Real adapters and route selection exist; no source-only proof of the physical end-to-end path. | Named STT/LLM/TTS routes, explicit egress, microphone, streaming, physical output, subtitles, cancellation, receipt-gated memory. |
| Optional local/hybrid (R08, R25, R30, R34, R36) | Broad model-manager capability exists but policy ADR is stale and release packs are not qualified. | Superseding ADR, exact license/provenance/signing, signed lifecycle, measured loadout admission, game reserve, failure/recovery. |
| Performance (R09, R10, SC04, SC07) | Measurement schemas and synthetic tooling exist. | Source-identified v1/v2 game-load trace with p50/p95, first-audio latency, CPU/RAM/GPU/VRAM, average/1%-low/frame-time impact. |
| Identity (R17) | Engine/contracts exist; production actor-lock bus is unqualified and activation withheld. | Trusted frame transport, qualified detector/gallery, calibration, OCR/dialogue/manual fusion, crossing/occlusion/reacquisition E2E. |
| Lip-sync (R11, R12, SC05, SC06) | Strong fail-open contract; current renderer is unqualified and latest output is rejected. | Accepted source-conditioned moving-frame result meeting visual, anchor, outside-mask, latency, FPS, resource, and blind-review gates. |
| One-monitor/display matrix (R13, SC08) | Correct native technologies are implemented. | Installed physical matrix including DPI/HDR, negative origin, alt-tab, move/resize/device loss, self-capture, modes, and live subtitles/optional residual. |
| Generic game mode (R16, SC02) | Profiles/discovery exist; authored commercial profiles are console-isolated. | Installed discovery and a returning-user start in at most two primary actions; audio/subtitle E2E before any animation claim. |
| Structured output (R19) | Effects provider is a fixture in the runtime simulation path. | Validated selected-LLM schema; malformed/late neutralization; speech independence; explicit safe action scope. |
| Diagnostics/privacy (R23, R24, R31–R33) | Credential, diagnostics, egress, and fallback contracts exist. | Live fault matrix, actionable UI, redacted export/canary scan, Offline deny-all, no hidden provider fallback. |
| Reproducible product (R20, R21, SC09) | Previous ledger records source failures; later handoff lists substantial component passes. | Fresh clean checkout setup/build/lint/unit/integration/headless package checks on the integrated tree, then clean Windows VM. |
| Truthful presentation (R26, R27, SC11) | Docs distinguish many simulated paths, but the current product surface can still show fixtures. | New screenshots/demo from retained native UI and real vertical slice; no unsupported lip-sync/performance/game-support claim. |
| Local review candidate (R37–R40) | Release prohibition is honored; product acceptance is incomplete. | All release-blocking FAIL items cleared, explicit deferrals recorded, review app and test game generated locally, no push/release. |

### Existing passes that should remain passes

- R01: the legacy repository is inventoried, with this audit adding a second direct check.
- R03: Tauri plus native sidecars remains the right architecture.
- R15: profile corpus/schema coverage passes, while live per-profile certification remains
  explicitly separate.
- R18: webcam demographic/emotion inference is removed from the default product direction.
- R35: legacy import is quarantined and data-only.
- R38: the no-push/no-release gate is explicit.
- R39 and SC12: versioned contracts, migration posture, and architecture-decision discipline
  are worth preserving.

Do not promote any of these narrower passes into a claim that the application is ready.

## Recommended integration sequence

### Gate 1 — make the product state honest

- Select one production frontend root and remove unreachable/duplicate fixture routes.
- Replace hard-coded UI values with native snapshots and receipts.
- Persist onboarding, loadout, target, character, voice, subtitle, and privacy choices.
- Add a single readiness model that explains the exact blocker and fallback.

Exit: every visible control acts, persists, navigates, or exposes a disabled reason; a
restart restores the same native-backed state.

### Gate 2 — prove the audio/subtitle vertical slice

- Use Eclipse Harbor or another rights-cleared test game.
- Discover and bind the actual foreground HWND.
- Run native PTT microphone capture through one selected STT and one selected LLM.
- Stream sentences to one selected stock/hosted TTS route.
- Present subtitles through the native compositor and audio on the selected physical
  endpoint.
- Commit memory only on delivery receipts; prove cancellation and provider failure.

Exit: a local installed review app completes repeated turns headlessly instrumented and
visually reviewed, while the game continues moving.

### Gate 3 — qualify target and actor identity

- Integrate a signed/measured vision pack through Model Manager.
- Produce trusted detections and actor locks without exposing pixels/coordinates to the
  WebView.
- Add manual selection and OCR/dialogue fusion.
- Calibrate and test unknown/ambiguous/crossing/occlusion behavior.

Exit: the chosen actor remains correct across a moving rights-cleared sequence, with
measured false-lock and reacquisition results.

### Gate 4 — replace and qualify the mouth renderer

- Implement the source-conditioned mesh/warp/residual pipeline behind the existing worker
  ABI.
- Drive it from the exact playback-clocked viseme timeline.
- Preserve current-frame, mask, actor-lock, resource, freshness, and fail-open gates.
- Compare rendered moving sequences against audio-only in blind review.

Exit: all ADR-0007 numeric gates and the qualitative mouth-appearance review pass. If
they do not, the review candidate stays audio/subtitle-only.

### Gate 5 — product qualification

- Run current full source checks, package checks, clean clone, and clean VM install.
- Run display, device-loss, provider-fault, offline/egress, credential, diagnostics,
  resource-pressure, and long-session matrices.
- Generate new screenshots and demo media only from the accepted build.
- Prepare the local review application and exact test-game procedure without pushing,
  publishing, signing for release, or activating updates.

## Decisions to record now

1. **ADR-0005 superseded.** Base install remains model-free/API-first; optional local
   packs may cover all six generic roles after explicit selection, signed provenance,
   measured loadout-wide admission, and no silent fallback.
2. **ADR-0003 clarified.** Current IPC uses protobuf framing/envelopes plus versioned JSON
   business payloads, with separate compatibility and validation rules.
3. **ADR-0007 amended.** The immutable-current-frame and fail-open contracts remain; the
   rejected painted mouth/atlas result is not a product candidate, and the target is
   source-conditioned deformation/residual synthesis.
4. **Define profile certification levels.** Separate data-complete, console-simulated,
   audio/subtitle-qualified, identity-qualified, and animation-qualified.
5. **Declare the vertical-slice game.** Make the rights-cleared target the only route that
   can be called end-to-end until it passes; commercial profiles remain content previews.

## Stop conditions and non-goals

- Do not execute or migrate legacy generated code, pickle/Chroma state, model output, or
  plaintext credentials.
- Do not reintroduce a frozen game-frame viewer, rectangular face replacement, webcam
  default, demographic inference, injection, mods, hooks, or game-memory access.
- Do not make animation a dependency of conversation, audio, subtitles, setup, or game
  support.
- Do not call fixtures, synthetic tests, isolated component renders, profile count, or
  architecture diagrams installed-product proof.
- Do not advertise a local pack without immutable source, license, checksum, signature,
  measured resource envelope, lifecycle, and game-impact evidence.
- Do not push, release, publish a pack, activate an updater, or upload demo media as part
  of the local review effort.

## Source evidence index

Immutable legacy evidence:

- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:main.ipynb`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/main.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/face_detection.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/find_character.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/get_character_data.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/get_public_data.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/conversation_loader.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:functions/pre_conversation_loader.py`
- `503ef3b64a921b6a11efa9e3e0432a0c3de3b619:miscellaneous/Single Monitor/functions/grabscreen.py`
- `9996575e69cf40d719e326adfea108476d80467b:main.ipynb`
- `9996575e69cf40d719e326adfea108476d80467b:miscellaneous/Single Monitor/functions/grabscreen.py`

Current architecture and source evidence:

- `docs/architecture/overview.md`
- `docs/architecture/process-model.md`
- `docs/architecture/profile-and-model-manager.md`
- `docs/architecture/adr/0001-tauri-control-plane.md` through
  `docs/architecture/adr/0009-security-and-update-boundaries.md`
- `apps/control/src-tauri/src/commands.rs`
- `apps/control/src-tauri/src/runtime_router.rs`
- `apps/control/src-tauri/src/sidecar_protocol.rs`
- `apps/control/src-tauri/src/identity_runtime.rs`
- `apps/control/src-tauri/src/visual_runtime.rs`
- `apps/control/src-tauri/src/local_resources.rs`
- `apps/runtime-host/src/simulation.rs`
- `apps/runtime-host/src/llm_bridge.rs`
- `crates/character-db/`
- `crates/memory/`
- `crates/identity-engine/`
- `crates/model-manager/src/manifest.rs`
- `crates/model-manager/src/manifest_v2.rs`
- `crates/model-manager/src/selection.rs`
- `crates/model-manager/src/resource_governor.rs`
- `native/media-broker/src/windows_capture.cpp`
- `native/media-broker/src/windows_platform.cpp`
- `native/media-broker/src/windows_overlay.cpp`
- `native/media-broker/src/windows_presentation_context.cpp`
- `native/mouth-worker/src/compositor.cpp`
- `apps/control/src/Pages.tsx`
- `apps/control/src/ProductConsole.tsx`
- `apps/control/src/ProductWorkspaces.tsx`
- `profiles/games/`
- `docs/product-rework/original-brief-acceptance.md`
- `docs/product-rework/original-brief-gap-map.json`
- `IMPLEMENTATION_STATUS.md`
- `HANDOFF.md`
