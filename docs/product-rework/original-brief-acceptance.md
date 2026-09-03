# Original 2.0 brief acceptance ledger

<!-- acceptance-evidence-run:start -->

## Synchronized evidence-run identity

- Status map: `docs/product-rework/original-brief-gap-map.json`
- Evidence run: `artifacts/acceptance/acceptance-evidence-run-v1.json`

The R01-R40 and SC01-SC12 status cells in this document are synchronized from the single status map above. The generated evidence run binds that map to the exact source-candidate, package, installed-distribution, and result-artifact hashes. Isolated crate, adapter, fixture, or simulation tests are supporting evidence only and cannot promote a product, installed, live, rendered, device, performance, privacy, or end-to-end row.

<!-- acceptance-evidence-run:end -->

Status: active source-of-truth checklist  
Refreshed: 2026-08-30  
Original source: the 1,123-line, 40-section 2.0 brief supplied by the maintainer  
Audited base commit: `7acf5026b7f9b46170b49bd8f7567a5abbacd873`  
Audited worktree: dirty and concurrently changing; no dirty-tree digest has been
promoted as immutable evidence

This ledger preserves the central product-path checklist while restoring
independent traceability for all forty original sections and all twelve primary
success criteria. A broad implementation, architecture document, catalog row,
fixture, vendor result, or older package does not make a live acceptance row
pass by implication.

## Result vocabulary and evidence rule

- `PASS`: the exact scoped outcome has source-identified evidence at the path
  named in the row.
- `FAIL`: current evidence directly contradicts the outcome or a required gate
  is known to fail.
- `NOT MEASURED`: useful implementation may exist, but the required clean,
  live, rendered, device, performance, or end-to-end evidence is absent.
- `USER-APPROVED DEFERRAL`: the maintainer explicitly accepted a named deferral.
  Silence, implementation difficulty, or a narrower demo is not a deferral.

Before changing a row to `PASS`, record the exact commit or packaged-source
digest, command, result artifact, environment, and any live-provider/model
revision. Current-tree test output is not transferable across later edits.

## Later maintainer direction

These decisions refine delivery order and release gates; they do not silently
erase original requirements:

- Conversation is **API-first**. Hosted LLM, STT, TTS, and retrieval routes are
  the default product path.
- Local LLM, STT, TTS, embeddings, and vision are optional packs. They are not
  RC-blocking unless the product advertises or exposes them as selectable, but
  any advertised Fully Local mode must pass the original local-mode gates.
- Generic local lip-sync is the priority local-AI path, remains optional, and
  must be explicitly selected after qualification. Audio/subtitles must remain
  useful when no lip-sync pack qualifies.
- The generic visual boundary is non-injecting external capture plus a bounded
  current-frame mouth residual. Per-game rig injection, hooks, DLLs, frozen
  faces, and full-frame talking-head replacement are not product defaults.
- No push, tag, public release, package publication, signing, update-feed
  activation, or public upload is authorized without explicit maintainer
  approval after local review.

## Central vertical-slice checklist

This is the retained high-leverage product checklist. Its rows do not substitute
for the independent R01-R40 and SC01-SC12 gates below.

### Product path

- [ ] A clean Windows install opens onboarding automatically with no console.
- [ ] The app detects/selects the task-owned synthetic game and proves advancing
  WGC frames for the exact PID/HWND/executable.
- [ ] The player selects a game-scoped character database entry and can inspect
  its biography, dialogue style, lore scopes, identity evidence, voice intent,
  and delivered memory.
- [ ] A real PTT or typed turn runs through a selected LLM and stock TTS route,
  reaches the selected audio endpoint, displays subtitles, and commits only the
  delivered portion.
- [ ] Failures degrade to typed input, audio-only, subtitles, or an explicitly
  authorized manual provider retry without inventing success.

### Provider and profile path

- [ ] Named global, game, and character loadouts configure LLM, STT, TTS,
  retrieval/embeddings, stock voice, and optional lip-sync independently.
- [x] NVIDIA NIM is tested as an optional one-account route for each modality it
  actually exposes; unsupported modalities remain visibly separate.
- [x] The UI does not call NVIDIA development endpoints unlimited or suitable
  for production unless current terms and observed headers prove that claim.
- [x] Credential values stay in Windows Credential Manager and never enter the
  WebView, IPC payloads, logs, reports, screenshots, or source files.
- [ ] Route changes are snapshotted per turn; fallbacks require explicit prior
  authorization and a user action after failure.

The NVIDIA checkbox is route-specific: current evidence covers chat,
embeddings, bounded Magpie transport, blocked Nemotron ASR after timeout, and
unavailable reranking. It is not a blanket vision, animation, entitlement,
reliability, or production claim. The credential checkbox remains subject to a
fresh current-source secret/IPC/log/export scan before R24 can pass.

### Local models and game-first resource policy

- [ ] Optional local LLM, STT, TTS, embeddings, vision, and lip-sync packs expose
  immutable source, checksum, license, installed size, RAM, resident VRAM, p99
  transient VRAM, load time, and measured latency on this PC.
- [ ] Selecting more than one local role runs a co-residency fit check that
  subtracts current game/desktop VRAM, a configured game reserve, transient
  workspaces, and a safety margin before activation.
- [x] Unknown or unmeasured resource envelopes block activation instead of
  guessing that a model fits.
- [ ] The user can set a soft VRAM ceiling and game-reserve target. The product
  describes these as scheduling targets, never impossible hard FPS guarantees.
- [ ] Idle model policies support keep-warm, CPU-resident/GPU-cold, and unload;
  the app compares reload cost against expected idle time before evicting.
- [ ] Interactive speech and LLM work outrank embeddings and vision; stale
  lip-sync frames are dropped rather than queued; pressure disables optional
  perception before speech.

The checked unknown-envelope rule is currently a verified core/UI policy, not
installed-pack activation evidence. R08, R30, and R36 remain open.

### Character identity learned from v1

- [ ] Each game profile owns its character database: stable ID, aliases,
  biography, personality, dialogue examples/style, scoped lore, prompt rules,
  voice intent, reference provenance, and memory namespace.
- [ ] Known-character recognition uses versioned tensor embeddings from
  licensed/original/user-private reference images, never legacy pickle files.
- [ ] Face/head detections become sticky actor tracks. Identity requires
  multi-frame consensus, a calibrated threshold, a margin from runner-up, and
  supporting explicit/OCR/dialogue evidence.
- [ ] Ambiguity preserves the current explicit selection or asks the player; a
  single highest-confidence face never silently switches character.
- [ ] Background NPCs receive stable encounter IDs and deterministic
  game-profile archetype/name/voice choices without inferring protected traits
  from pixels.
- [ ] Prompt construction keeps world lore, character biography, character
  knowledge, delivered recent dialogue, long-term summaries, and uncertain
  public information as typed sources with provenance and spoiler scopes.

### Generic low-latency lip-sync

- [ ] The user explicitly selects which qualified local lip-sync pack to
  download; no large model is bundled or silently installed.
- [ ] Candidate selection is based on measured Windows latency/FPS, p95 frame
  age, VRAM, game frame impact, temporal stability, identity preservation,
  camera/head motion, occlusion, lighting, license, and cancellation.
- [ ] Lip-sync consumes the exact locked actor ID, frame ID/timestamp, mouth ROI,
  and delivered audio timing. It cannot select a character from memory text.
- [ ] The compositor applies only a bounded current-frame mouth residual with a
  dynamic mask. Stale, occluded, mismatched, cancelled, or over-budget output
  reveals the untouched current frame within one refresh.
- [ ] NVIDIA Audio2Face-style blendshapes are treated only as an audio-to-motion
  signal unless a project-owned mapper can anchor them to arbitrary captured
  game pixels; static-avatar output is not presented as generic game support.
- [ ] Full-frame talking-head video, frozen-face replacement, and per-game rig
  injection remain rejected product defaults.

### Engineering, quality, and presentation

- [ ] Reproducible setup/dev/test/lint/benchmark/package commands work from a
  clean checkout with pinned lockfiles and no manual notebook steps.
- [ ] Unit, integration, provider, migration, model-manager, capture/compositor,
  failure/cancellation, crash-recovery, and deterministic replay tests pass.
- [ ] Reference and this-PC benchmarks report p50/p95 pipeline timings plus
  CPU/GPU/VRAM/RAM and frame impact with hardware and provenance.
- [ ] Every retained UI control has a command, persistence effect, navigation
  outcome, or visible disabled reason; no fixture values masquerade as state.
- [ ] Every retained screen is driven, captured, and visually inspected at
  normal/narrow and 100/150/200% scaling before review.
- [ ] README, local docs, screenshots, and a truthful Gifsmith demo describe
  only proven behavior.
- [ ] A local review candidate and exact test instructions are prepared, but no
  push, release, package publication, updater activation, or public upload
  occurs without explicit user approval.

## R01-R40 traceability

| ID | Status | Later-direction note | Current evidence path | Exact open condition |
| --- | --- | --- | --- | --- |
| R01 | PASS | None | `docs/legacy/repository-audit.md`; `docs/legacy/v1-pipeline.md`; `docs/research/`; `docs/architecture/adr/` | Re-audit only when the legacy baseline or architecture thesis changes. |
| R02 | NOT MEASURED | API-first is the default onboarding route | `apps/control/src/ProductConsole.tsx`; `scripts/installer-smoke.ps1`; `docs/product-rework/verification.md` | Clean non-admin Win10/11 install, automatic onboarding, no console, complete configure/Start/play flow. |
| R03 | PASS | Tauri control plane plus native sidecars remains the selected architecture | `docs/architecture/adr/0001-tauri-control-plane.md`; `docs/architecture/process-model.md` | Reopen if release-build game impact or sidecar reliability violates the documented fallback criteria. |
| R04 | NOT MEASURED | Five job-oriented surfaces supersede the old subsystem-page inventory | `apps/control/src/ProductConsole.tsx`; `apps/control/src/product.css`; `artifacts/frontend-native-integration-qa/` | Final native rendered states, keyboard/Narrator/controller, 100/150/200%, novice onboarding, and human review. |
| R05 | NOT MEASURED | Hosted LLMs are the primary route | `crates/providers-llm/`; `crates/provider-loadouts/`; `apps/control/src/ProviderLoadoutEditor.tsx` | Current source builds; selected hosted LLM streams through an ordinary turn with credential/model/egress evidence. |
| R06 | NOT MEASURED | Hosted STT is primary; local STT is optional | `crates/providers-stt/`; `docs/guides/providers-and-credentials.md` | Real microphone/PTT, final/cancel behavior, WER/noise/language matrix, and honest route tradeoffs. |
| R07 | NOT MEASURED | Hosted stock voices are primary; local TTS is optional | `crates/providers-tts/`; `apps/runtime-host/src/tts_bridge.rs`; `docs/product-rework/verification.md` | Ordinary selected route, first PCM, physical endpoint/loopback, cancellation, voice discovery/assignment, and quality evidence. |
| R08 | NOT MEASURED | Optional local conversation packs; local lip-sync is the priority pack | `crates/model-manager/`; `apps/control/src-tauri/src/local_resources.rs`; `workers/packs/` | Qualified selectable packs with immutable source/license/checksum/resource measurements and full lifecycle; no advertised Fully Local claim before then. |
| R09 | NOT MEASURED | Measure the API-first route first | `crates/product-benchmark/`; `docs/research/benchmark-strategy.md`; `scripts/benchmarks/` | Source-identified live game-load QPC pipeline report and v1 comparison; synthetic percentiles are not results. |
| R10 | NOT MEASURED | Performance may live under Diagnostics/Voice rather than a separate page | `crates/product-benchmark/`; `apps/control/src/ProductWorkspaces.tsx` | Reference and This-PC results, feature contribution, presets, live resources, frame impact, and usable UI. |
| R11 | NOT MEASURED | Generic local screen-space lip-sync only | `docs/architecture/adr/0007-native-and-screen-space-lipsync.md`; `workers/packs/lipsync-pack-catalog-v1.development.json` | One candidate passes Windows/license/quality/latency/FPS/VRAM/game-impact/cancellation/blind-review gates. |
| R12 | NOT MEASURED | Non-injecting current-frame mouth residual | `native/mouth-worker/`; `native/media-broker/`; `docs/architecture/adr/0008-capture-and-overlay.md` | Qualified model-driven app E2E plus anchor drift, outside-mask, occlusion, motion, and fail-open rendered evidence. |
| R13 | NOT MEASURED | No user workaround for one-monitor systems | `docs/legacy/single-monitor-root-cause.md`; `native/media-broker/` | Installed single/multi-monitor, negative origin, DPI, 720p-4K, ultrawide, SDR/HDR, modes, alt-tab/device-loss/self-capture matrix. |
| R14 | NOT MEASURED | Retrieval may be hosted, but delivered memory remains local | `crates/memory/`; `crates/character-db/src/prompt.rs`; `docs/architecture/adr/0006-sqlite-memory.md` | Release latency/relevance, migration/recovery/disk-full/backup evidence and user memory lifecycle UI. |
| R15 | PASS | Profiles are external/non-injecting and may have capability-specific certification | `profiles/games/`; `fixtures/profile-replays/v1/`; `docs/research/game-selection.md` | Live capability certification remains per profile and must not be inferred from this corpus PASS. |
| R16 | NOT MEASURED | Generic mode remains experimental | `crates/game-discovery/`; `apps/control/src/ProductWorkspaces.tsx` | Store/common/manual discovery in installed UI, moved/duplicate installs, supported-state clarity, and generic conversation/audio/subtitles E2E. |
| R17 | NOT MEASURED | Explicit selection remains authoritative; automatic identity is optional | `crates/identity-engine/`; `crates/character-db/` | Qualified live detector/gallery, trusted WGC transport, OCR/dialogue support, calibrated thresholds, and occlusion/reacquisition E2E. |
| R18 | PASS | Webcam perception remains opt-in/off; demographic inference is removed | `docs/legacy/feature-disposition.md`; `crates/protocol/src/effects.rs`; `crates/runtime-core/src/types.rs` | New legacy discoveries require a new disposition row. Live structured emotion belongs to R19. |
| R19 | NOT MEASURED | Game actions remain outside the current safe product contract | `crates/runtime-core/`; `crates/providers-llm/`; `apps/runtime-host/src/simulation.rs` | Selected hosted LLM produces validated structured output; malformed/late optional effects neutralize without delaying speech. |
| R20 | FAIL | None | `scripts/dev.ps1`; `scripts/test-setup-reproducibility.ps1`; `Cargo.lock`; `pnpm-lock.yaml` | Current audited tree must compile/test, then a clean Windows clone must pass setup/dev/test/lint/benchmark/package, including offline second pass. |
| R21 | FAIL | None | `package.json`; workspace test manifests; `docs/product-rework/verification.md` | Current audited tree has a Tauri import compile failure and subtitle manifest/test mismatch; complete source-identified quality gate must pass. |
| R22 | NOT MEASURED | Synthetic rights-cleared input is preferred | `tools/sim/`; `scripts/synthetic-game-replay.ps1`; `tools/game-load/` | Video/deterministic visual corpus drives detector/tracker/compositor goldens; calibrated live resource contention and soak evidence. |
| R23 | NOT MEASURED | Diagnostics remain local and content-light | `crates/diagnostics/`; `apps/control/src-tauri/src/diagnostics_v2.rs`; `apps/control/src/ProductConsole.tsx` | Provider/mic/speaker/STT/TTS/model/capture/GPU/game/overlay/latency/permission matrix and actionable, redacted export E2E. |
| R24 | NOT MEASURED | Native credential prompt and Windows Credential Manager only | `crates/credential-vault/`; `apps/control/src-tauri/src/credential_prompt.rs`; `scripts/security/scan-secrets.ps1` | Fresh source-identified vault lifecycle plus source/history/IPC/child-env/log/export/crash/screenshot canary scan after integration. |
| R25 | NOT MEASURED | Optional packs download after install; no model is silently bundled | `packaging/`; `scripts/package.ps1`; `scripts/installer-smoke.ps1`; `crates/model-manager/` | Clean Win10/11 no-toolchain install, repair/upgrade/migrations/cache/download failure/resume/hash/uninstall certification and production trust/signing readiness. |
| R26 | NOT MEASURED | Docs must distinguish implemented, simulated, measured, and planned | `README.md`; `docs/`; `scripts/check-doc-links.ps1` | Final current-source content/link/capability review and screenshots/demo aligned to the accepted product flow. |
| R27 | FAIL | Synthetic Eclipse Harbor remains the rights-cleared demo source | `demo/readme/`; `docs/assets/demo/` | Re-render from the current retained UI/installed flow; do not depict unsupported lip-sync/performance as current behavior even when illustrative. |
| R28 | NOT MEASURED | Advanced settings may be progressively disclosed | `apps/control/src/ProductConsole.tsx`; `apps/control/src/ProductWorkspaces.tsx`; `docs/reference/settings.md` | Requested settings inventory, defaults/explanations, global/game/character effective sources, persistence, migration, accessibility, and runtime consequences. |
| R29 | NOT MEASURED | API-first preset is default; local/hybrid presets only when packs qualify | `crates/provider-loadouts/`; `apps/control/src/ProviderLoadoutEditor.tsx` | Cloud/Hybrid/Fully Local/Performance/Immersive preset snapshots, inheritance, switching, and no hidden egress/fallback authority. |
| R30 | NOT MEASURED | Schedule optional local packs behind live game reserve | `crates/runtime-core/src/resource_broker.rs`; `crates/system-telemetry/`; `apps/control/src-tauri/src/local_resources.rs` | Integrated live DXGI/NVML admission, queue priorities, idle/reload policy, CPU fallback, pressure response, game contention, and bounded ceiling overshoot. |
| R31 | NOT MEASURED | Audio/subtitles are the dependable fallback; provider changes are manual-only | `crates/runtime-core/`; `crates/provider-loadouts/`; `tools/sim/` | Connected fault matrix for microphone/GPU/lip-sync/provider/vector/STT/TTS/game plus visible recovery actions and invariant route snapshots. |
| R32 | NOT MEASURED | Remote telemetry remains absent/default-off | `crates/diagnostics/`; `apps/control/src-tauri/src/diagnostics_v2.rs` | Integrated correlated timings, verbosity, rotation/storage, crash recovery, redaction, and packaged telemetry-absence proof. |
| R33 | NOT MEASURED | API-first requires explicit per-route egress; Offline must genuinely deny provider traffic | `crates/diagnostics/src/egress.rs`; `docs/architecture/security.md`; `docs/guides/voices-memory-and-privacy.md` | Per-data-class UI snapshot and packaged OS deny-all tests for Offline and local visual routes; no hidden fallback. |
| R34 | FAIL | Every optional pack requires its own exact license/provenance | `packaging/security/`; `docs/research/model-license-ledger.md`; `docs/legal/third-party-notices.md` | Current source dependencies can be checked, but production pack licenses/notices, approved catalog/TUF provenance, trust roots, and signing custody are absent. |
| R35 | PASS | Clean 2.0 architecture takes priority; useful data import is quarantined and explicit | `docs/legacy/import-archive-map.md`; `docs/legacy/feature-disposition.md`; `crates/character-db/src/import.rs` | Preserve no-execution/no-pickle/atomic-import invariants when adding migrations. |
| R36 | NOT MEASURED | Ordinary PCs use APIs; 12 GB is a strong optional-local reference, not a requirement | `crates/system-telemetry/`; `crates/model-manager/`; `docs/getting-started/requirements.md` | API-only clean-machine run plus optional 6-8/12 GB/vendor matrix and controlled game-impact/headroom evidence. |
| R37 | FAIL | Release phasing remains mandatory | This ledger; `docs/requirements/local-review-evidence-report.md`; `IMPLEMENTATION_STATUS.md` | Research/design artifacts exist, but implementation, live quality, benchmark, clean-VM, visual, lip-sync, and current presentation deliverables are incomplete. |
| R38 | PASS | Absolute no-release gate remains in force | `packaging/security/release-policy.json`; `IMPLEMENTATION_STATUS.md`; local-only branch state | Prepare a source-identified local candidate and instructions; external action still requires explicit approval. |
| R39 | PASS | Prefer better 2.0 replacements without needless rewrites | `docs/architecture/adr/`; `docs/legacy/feature-disposition.md` | New major decisions must add context, alternatives, evidence, consequences, and fallback. |
| R40 | FAIL | Later direction changes route priority, not the twelve success outcomes | SC01-SC12 table below | All twelve criteria must be PASS or explicitly deferred; currently they are not. |

## SC01-SC12 primary success criteria

| ID | Status | Current evidence path | Exact open condition |
| --- | --- | --- | --- |
| SC01 | NOT MEASURED | `scripts/installer-smoke.ps1`; `apps/control/src/ProductConsole.tsx` | Clean non-admin Win10/11 novice install/configure with no Python/Node/Rust/CUDA/FFmpeg/terminal/filesystem work. |
| SC02 | NOT MEASURED | `profiles/games/`; `crates/game-discovery/` | Installed supported game detected/profile selected and returning user starts in at most two primary actions. |
| SC03 | NOT MEASURED | `crates/provider-loadouts/`; `apps/control/src/ProviderLoadoutEditor.tsx` | Ordinary hosted turn consumes explicit named LLM/STT/TTS/retrieval/voice routes with egress display and no unauthorized fallback. |
| SC04 | NOT MEASURED | `crates/product-benchmark/`; `docs/research/benchmark-strategy.md` | Controlled v1-v2 heavy-load trace proves dramatically lower end-to-end first-audio latency. |
| SC05 | NOT MEASURED | `docs/architecture/adr/0007-native-and-screen-space-lipsync.md`; `workers/packs/` | Qualified candidate passes latency/FPS/visual/contention/license gates and blind rendered preference; otherwise ship honestly audio-only. |
| SC06 | NOT MEASURED | `native/mouth-worker/`; `native/media-broker/` | Live fresh-frame residual passes anchor/outside-mask/occlusion/motion/display thresholds and human review. |
| SC07 | NOT MEASURED | `tools/game-load/`; `crates/product-benchmark/` | Controlled average/1%-low/frame-time/GPU/VRAM impact meets declared preset thresholds. |
| SC08 | NOT MEASURED | `docs/legacy/single-monitor-root-cause.md`; `native/media-broker/` | Foreground PTT plus capture/subtitles/optional overlay works on one monitor without mirror/self-capture/frozen frame at required DPIs/resolutions. |
| SC09 | FAIL | `scripts/dev.ps1`; lockfiles; current audit results | Current source must build/test, then clean-clone one-command setup and deterministic simulation must pass. |
| SC10 | NOT MEASURED | `apps/control/src/`; `artifacts/frontend-native-integration-qa/` | Complete current native rendered/accessibility matrix and novice onboarding study/test script. |
| SC11 | NOT MEASURED | `README.md`; `docs/`; `demo/readme/` | Final current-product README/docs/screenshots/demo review with working links and no presentation beyond proven behavior. |
| SC12 | PASS | `crates/protocol/`; provider/worker/profile contracts; migration/cancellation tests | Keep compatibility/migration tests when adapters, workers, and profiles evolve. |

## Current known blocking evidence

The latest headless audit of the dirty worktree found:

- Response Console: 73 frontend tests passed.
- `npc-character-db`: 12 tests passed.
- `npc-identity-engine`: 24 tests passed.
- `npc-product-benchmark`: 9 tests passed.
- `npc-system-telemetry`: 12 tests passed.
- `npc-subtitle-engine`: 14 of 15 tests failed because the checked-in style
  manifest uses `none_in_renderer`, while the Rust enum accepts only
  `clamp_to_subtitle_peak` or `relative_to_reference_white`.
- The Tauri control library did not compile because
  `apps/control/src-tauri/src/local_resources.rs` imported missing
  `npc_model_manager::verify_release_manifest_files_v1` (`E0432`).

These results identify immediate integration blockers, not permanent status.
R20, R21, SC09, and any installed-source claim require a fresh full check after
the integrated tree is stable.

## Promotion rule

No local package is a review candidate while any `FAIL` remains. Every `NOT
MEASURED` row must gain source-identified evidence or an explicit
`USER-APPROVED DEFERRAL`. The final report must bind the exact source tree,
locks, catalogs, profiles, toolchains, package hashes, environment, raw results,
and retained-screen evidence. No external release action follows automatically.
