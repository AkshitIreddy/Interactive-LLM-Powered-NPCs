# Native vertical-slice audit — 2026-09-05

Status: native source remediation and headless regression complete
Scope: control shell native bridge, runtime host, media broker, synthetic review target
Release boundary: local review only; no push, publication, updater activation, or release

Snapshot notice: this audit preserves its September 5 sequence. Follow-up source
adds fixed-origin Cartesia, Deepgram, and Inworld runtime construction, periodic
YuNet detection with tracked-ROI reuse, and a verified v17 test game. A
production Groq Qwen → Cartesia component chain reached bridge PCM in 492.964
ms, but it did not exercise microphone/STT, `HostState`, native broker drain, an
OS speaker, or physical audibility. Optional-pack real-worker activation and the
v17 application package remain pending. See
[the reconciled review record](local-review-2026-09-05.md).

## Acceptance slice

The native path must support this sequence without a terminal or file editing:

1. locate or launch the packaged synthetic target;
2. discover its current top-level window;
3. bind the exact process instance, HWND, and executable identity;
4. ask the media broker to select that same target;
5. prove advancing exact-window WGC frames;
6. dispatch one selected-provider turn;
7. route decoded stock-voice PCM to the broker and return delivery receipts;
8. expose measured transcript, response, playback, subtitle, and degradation evidence.

Component tests are useful, but exact-window WGC requires the GUI target to own a
visible root window. The current no-desktop-interaction restriction therefore makes
the WGC and physical-playback steps explicit deferred gates rather than results a
headless test can manufacture.

## Current capabilities

### Process and console boundary

- The Tauri shell and Rust runtime host use the Windows GUI subsystem.
- The native media broker is linked as a Windows executable with
  `mainCRTStartup` and `WIN32_EXECUTABLE`.
- The synthetic target is compiled with `/target:winexe`.
- Runtime and broker children are started with `CREATE_NO_WINDOW`; the broker
  and its workers also join the shell-owned kill-on-close Job Object. The new
  synthetic launcher assigns its child to that same Job Object or terminates it.
- `CREATE_NO_WINDOW` suppresses a console. It does not hide the WinForms test-game
  window, which is intentionally visible when the user chooses Launch test game.
- Broker standard streams are null. Runtime stdout is the authenticated control
  transport while stderr is null at the shell boundary.

This is the correct PE/process-launch design. It still needs artifact-level PE
inspection in the fresh package because source settings do not prove the staged
executables use them.

### Target discovery and binding

- `discover_game_targets` enumerates visible, uncloaked, non-minimized root
  windows on Windows and applies the selected profile's process/window policy.
- A selected target is bound in memory to PID, HWND, process creation time,
  executable path, executable leaf, and policy evidence through
  `npc-game-discovery::BoundGameTarget`.
- Revalidation re-enumerates current windows and rejects a stale process,
  replaced HWND, changed executable, or policy mismatch.
- Candidate results expose the PID, HWND, executable leaf, a digest of the full
  path, title, foreground state, and client size without returning the path.

### Synthetic metadata and native broker

The local-review gate reads one fixed app-config file,
`debug-synthetic-replay-target.json`. It requires:

- fixture kind `synthetic-original-video-replay`;
- state `playing` and at least one generated/decoded frame;
- matching duplicate PID and HWND fields;
- exact executable leaf `interactive-npcs-synthetic-target.exe`;
- exact project-owned portrait digest;
- either explicit schema-1 static compatibility with the old project-owned visual
  source and `source_mouth_motion=false`, or schema 2 with the exact attested
  42-frame sequence digest, dimensions/rate, current frame index/hash, source
  motion provenance, rendered actor/blink/breathing motion, and no source-mouth or
  product lip articulation.

The debug command asks the broker to select that PID/HWND/executable leaf, then
selects the same HWND through the game-target policy and records the broker bind.
The native broker independently verifies the HWND's PID, process image leaf,
same user/session, process accessibility/protection state, and module-based
anti-cheat policy before opening exact-window WGC.

Capture evidence includes selected PID/HWND/executable leaf, device and geometry
generations, latest frame sequence/QPC, dimensions, content hashes/change count,
scope/source, and overlay/luminance exclusion flags. The control layer rejects
evidence unless it matches the immutable selection, uses exact-window WGC, and
advances beyond the previously accepted frame sequence.
The bridge exposes the accepted fixture motion mode so static compatibility cannot
be presented as the moving test-game qualification.

### Runtime/provider/audio path

- The shell supervises an authenticated, nonce-bound runtime-host connection.
- A selected provider loadout is resolved and pinned per turn. The runtime host
  has concrete hosted LLM/TTS bridges, including exact stock-voice validation,
  and emits ordered measured stage/turn evidence. Exact executable hosted TTS
  routes now include Cartesia Sonic 3.6/Greg, Deepgram Aura 2/Arcas, Inworld
  TTS 2 Flash/Dennis, ElevenLabs, and the private-evaluation NVIDIA Magpie route.
- Hosted synthesis has a five-second first-PCM deadline beginning before
  provider session setup, so DNS/TLS/WebSocket connection time is included. The
  deadline stops after the first decoded PCM and does not cap long speech. A
  pre-audio timeout reports retry/change-provider guidance and never silently
  substitutes a different voice.
- The runtime bridge forwards all but one PCM frame immediately and retains only
  the final frame required to carry the core EOS marker. It preserves byte order,
  checks cancellation between every yielded item, and bounds pre-audio timing
  metadata to 32,768 events.
- For an ordinary ready TTS route, the shell preallocates a bounded native PCM
  playback pool from the broker and sends only opaque lease data to the runtime.
  The runtime must return matching delivery receipts before the shell records
  audio as delivered.
- Trusted broker presentation evidence can populate the runtime subtitle
  context. Invalid or stale native evidence produces the explicit console-
  unavailable fallback.

## Blocking gaps

| Gap | Status and current behavior | Consequence |
| --- | --- | --- |
| Synthetic target launch/location | Closed for local review. A debug capability resolves only the co-located `local-app-data/test-game` executable, validates its review manifest and exact SHA-256/size, launches it muted with null standard streams, assigns it to the kill-on-close Job Object, then polls its own PID metadata before broker binding. | The product can offer one Launch test game action without accepting an arbitrary path. The GUI target itself remains visible by design. |
| Split selection paths | Closed for the task-owned review target. Launch/attach, exact HWND selection, broker binding, and capture verification are now one native path. Commercial game selection remains capture-blocked without trusted runtime safety evidence. | The review flow no longer advances from a selector-only success. |
| Debug-build coupling | Synthetic metadata parsing, broker selection, capture verification, and the Tauri invoke command are all behind `debug_assertions`. | A release-mode local-review build loses the only complete synthetic slice. |
| Weak launch provenance | Metadata authenticates project fixture constants and the broker checks the live executable leaf, but the broker protocol does not carry the selected process creation time or full-path digest. | The Rust selector and broker validate different identity subsets; the proof is less coherent than its UI wording implies. |
| Stable target status | Partially closed. Launch returns typed target identity and fixture motion mode; selection and capture verification return typed snapshots. Lost/relaunch remains expressed as bounded command errors rather than one persistent state machine. | The UI can gate setup on capture evidence, but it still translates a later target loss into retry guidance. |
| Turn/capture disconnect | Closed for the Eclipse Harbor review turn. Dispatch now requires a fresh advancing capture verification before consuming selected STT or starting provider work. | A review-game response cannot proceed on stale selector state alone. |
| Effective overlay/subtitle preferences | Closed. The native router pins the effective preferences for the turn, starts visual coordination only when the overlay is enabled, and authorizes subtitle presentation only when both settings and safety evidence permit it. | Saved switches now change native behavior rather than only UI text. |
| Hosted stock voice latency | Closed for a selectable low-latency route, with explicit alternatives retained. The earlier 27–36 second NVIDIA observations were transient historical evidence from before the corrected request headers. A same-day exact official/repository A/B measured NVIDIA near 0.7 seconds cold and 0.19–0.20 seconds warm. The production Cartesia adapter measured 607.3 ms first PCM on a fresh socket including 390.6 ms upgrade, then 115.2 ms on reuse. The native `RuntimeTtsBridge` measured 551.721 ms request-to-bridge-audio cold, 85.066 ms for the second sentence in the same turn, and 81.684 ms for the next turn, with only 12/28/28 µs from provider PCM to bridge emission. Inworld and Deepgram passed exact production adapter PCM/terminal gates and remain manual selectable alternatives. | The leading low-latency candidate uses a measured persistent transport rather than a unary or benchmark-only path. Cold and warm measurements remain separate, and provider/voice changes are explicit. |
| Native playback proof under headless restriction | Open by design. A private WAV receipt proves the exact selected Cohere and NVIDIA adapters, runtime TTS bridge, PCM shape, non-silence, and EOS. It explicitly sets audio delivery, native broker, physical audibility, and production delivery claims false. | No device playback claim is made without a native broker endpoint drain receipt. |
| Local whole-turn admission | Open with the missing inputs made exact. Native system telemetry and reserve settings exist, but runtime-host does not yet receive a freshness-bounded selected-process-instance snapshot plus a qualified whole-turn CPU/GPU envelope. Kokoro produced real native PCM, but its 20-sample qualification failed and no selected-game VRAM or p99 additional-memory envelope exists; observed first PCM was 3.337–6.587 seconds and cancellation drain reached 7.170 seconds. | The opt-in core `ResourceBroker` and local Kokoro route remain closed instead of admitting work with browser-authored, single-observation, or invented budgets. |
| Provider timing-to-mouth handoff | Partially closed. Cartesia phonemes and Inworld provider visemes carry typed symbol kind plus exact provider/model provenance. Only the explicit covered Cartesia phoneme table can produce canonical mouth states; unknown or ambiguous symbols produce no visual cue and retain PCM-only fallback. Cue offsets/durations are converted to source sample indices and the mouth worker accepts them only when its current playback sample lies inside the exact audio-clock window. | Metadata-arrival timing versus the real playback clock is still unmeasured under the no-desktop/audio restriction. Late timing is dropped at the authoritative native clock gate; video and audio are never delayed to rescue it. |

## V1 comparison

The immutable v1 implementation used ad-hoc Python/notebook orchestration,
screen or window mirroring, file-based voice helpers, and heavy whole-face video
generation. It had no durable exact PID/HWND/process-instance contract, no
authenticated native media boundary, no receipt-backed PCM delivery, and no
safe failure isolation. Its useful product intent was that one selected game and
character drive a complete spoken interaction. The 2.0 native boundaries are a
sound replacement, but they currently expose that intent as disconnected
commands rather than one operable transaction.

## Remediation decision

Keep the Win32 observer, process-instance revalidation, native broker policy,
WGC evidence, authenticated sidecars, Job Object, and receipt-backed audio.
The special debug sequence has been replaced with a local-review
synthetic-target service that owns four explicit operations:

1. resolve the allowlisted packaged sibling test-game path;
2. start the GUI target hidden-console with the exact app-config metadata path,
   or attach to the one already attested running instance;
3. select and broker-bind the exact observed process/window as one transaction;
4. poll one typed status until frame evidence advances or return a bounded,
   actionable missing/stale/mismatch/lost error.

The service cannot launch arbitrary paths and does not weaken commercial-game
safety policy. Moving this local-review capability from `debug_assertions` to an
explicit build feature remains appropriate before distributing a release-mode
review binary; compiler optimization level is not an authorization boundary.

Turn dispatch now consumes fresh target verification. The headless provider
qualification uses the exact selected-provider constructors, discovered stock
voice, and runtime TTS bridge, but its file receipt is deliberately separate from
broker delivery accounting. Real audio delivery still requires the broker
playback pool and endpoint-drain receipts.

## Failure tests required

- target absent, staged path absent, and duplicate live instances;
- metadata absent, partial/oversized/linked, stale, wrong executable, wrong PID,
  wrong HWND, wrong portrait digest, and zero frame count;
- PID reuse/process restart after selection;
- broker rejection for wrong HWND/PID/name, protected process, anti-cheat module,
  different user/session, or target loss;
- frame sequence unchanged, scope/source mismatch, and content not changing;
- broker/runtime child spawn with no console subsystem and hidden creation flags;
- selected loadout route mismatch, missing credential, provider cancellation,
  decoded PCM without broker receipt, and receipt without matching lease;
- closing and relaunching both apps without accepting stale selection state.

## Evidence gathered in this audit

- The final locked, offline root-workspace run passed 696 tests across 69 test
  binaries with zero failures and 15 intentional ignores. The ignored set is
  limited to credentialed live-provider/model-pack qualifications and their
  canary helpers; those live paths have separate sanitized evidence below.
  The complete log is
  `E:\temp\InteractiveNPCs\final-rust-workspace-tests-20260905.log` (SHA-256
  `943540a9eb3136863ec1b87008c3c579b4235d40215ffe46a5f6989662466d29`).
- The final locked, offline nested Tauri run passed 218 tests across four test
  binaries with zero failures and one intentional ignore: the live
  ElevenLabs-to-WASAPI audibility qualification prohibited by this review's
  headless/no-audio boundary. This run used a freshly built real native broker
  for authenticated health, audio-endpoint enumeration/selection, and clean
  shutdown. Its log is
  `E:\temp\InteractiveNPCs\final-rust-tauri-tests-20260905.log` (SHA-256
  `971d0cabe0f7499478988788fda025af58c2858afb9474b9596bed76a7f8b747`).
- The current native media broker built in Release mode with MSVC warnings as
  errors. All five noninteractive CTest suites passed; interactive WGC,
  DirectComposition, and WASAPI playback smoke remained excluded. The exact
  test broker is
  `E:\temp\InteractiveNPCs\native-builds\media-broker-final-20260905\Release\npc-media-broker.exe`
  (SHA-256
  `220e3a8e50666f82d50dbf9847513a466b8147d0442dddd76d2513db4cbc5cf5`).
  The CTest log is
  `E:\temp\InteractiveNPCs\final-native-media-broker-ctest-20260905.log`
  (SHA-256
  `61db0c407c2bd502bea558b56f4d1e5d5e44bf13465ea5b11cf47978a3c4e4ed`).
- Strict `cargo clippy` with all targets, all features, locked dependencies,
  offline resolution, and warnings denied passed for both Rust workspaces. The
  root log SHA-256 is
  `c4e5baf252e4b40b21f96418b1ccdde862cc4eb57c2454a7ffc11919ecb4ec7b`;
  the nested Tauri log SHA-256 is
  `a17cc51074f0500dc20bac5e096e13b81efbff45f5f0c66c0fc24a6d06d1a438`.
  Both Cargo format checks and `git diff --check` also pass. After the lint-only
  assertion cleanup, all 15 runtime-host broker cue/receipt tests were repeated
  and passed.
- Source inspection confirms the remaining debug-only capability boundary; the
  missing launch API identified at audit start is now implemented.
- Native launch, manifest-integrity, moving-metadata, unchanged-frame, stale
  process-instance, effective presentation-preference, and selected-route tests
  pass without starting the GUI target.
- The production hosted WebSocket adapter gate passed for all three new routes:
  Cartesia exact PCM/phonemes/cancellation/pool isolation, Inworld exact PCM plus
  44 provider visemes, and Deepgram exact PCM/terminal completion. Its sanitized
  report is
  `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\rust-production-adapter-qualification-20260905T050606Z.json`.
- A second production `RuntimeTtsBridge` headless gate used two sentences in the
  same public turn and one sentence in a second turn. It proved one fresh plus
  two safely reused Cartesia connections, exact non-empty PCM/EOS, and microsecond
  provider-to-bridge forwarding without opening an OS audio endpoint. Its
  sanitized report is
  `E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives\runtime-bridge-cartesia-20260905T0520Z.json`.
- The live common-file credentials were injected only into bounded test-process
  environments and scanned out of output. They were not imported into the
  production Windows vault or review namespace. A fresh reviewer must still use
  the native provider-account flow to save the chosen provider key and explicitly
  select its exact stock voice.
- A bounded private live headless run proved the exact Cohere selected adapter and
  NVIDIA stock-voice runtime bridge. It wrote only a WAV and sanitized receipt to
  `E:\temp\InteractiveNPCs\provider-live-2026-09-05\selected-route-headless` and
  did not open an audio endpoint.
- A bounded private AssemblyAI v3 streaming run transcribed the exact NVIDIA WAV,
  producing one exact final transcript with 16 audio chunks and 399 ms from PTT
  release to final. Current official AssemblyAI documentation still names the
  model `u3-rt-pro`.
- Legacy comparison used immutable `git show`/`git grep` evidence only; no v1
  notebook, generator, credential file, pickle, Chroma store, or generated
  Python was executed or deserialized.
