# Interactive LLM Powered NPCs 2.0 implementation status

This file is the durable source of truth for the local 2.0 overhaul. A checked
item requires implementation evidence; prose progress alone does not count.

## Independent local review rebuild — 2026-09-05, reconciled 2026-09-07

The current source contains the independently audited UI, native, provider, and
moving-mouth rebuild. See [the current review record](docs/product-rework/local-review-2026-09-05.md)
for measured findings, final headless checks, artifact receipt paths, and exact
remaining gates. Historical checked items below
retain their original scope and dates; they do not qualify the changed UI,
moving-mouth renderer, newly staged artifacts, or current provider performance.
No new release, installed-app, live-game, natural-lip-sync, or physical
audio-delivery acceptance is implied by this rebuild. The September 5 audit
documents are dated snapshots. Their unresolved Cartesia route, every-frame
face-detector, pre-redesign UI, and earlier renderer findings are superseded by
the follow-up evidence summarized here and in the current review record.

## Safety and workspace invariants

- [x] Keep this nested checkout as the canonical repository root.
- [x] Create the local-only `feat/2.0-overhaul` branch.
- [x] Confirm the pre-overhaul tracked diff is CRLF-only (`--ignore-cr-at-eol` exits 0 at `503ef3b`).
- [x] Disable repository-local automatic CRLF conversion and add explicit attributes.
- [x] Preserve the complete legacy audit, feature disposition, and reversible import/archive map.
- [x] Remove executable profile code, generated `temp.py`, plaintext-key workflow, pickle/Chroma caches, duplicate game tree, notebooks, and vendored SadTalker from the active product tree.
- [x] Keep all work local; do not push, publish, tag, release, or activate update feeds.

## Product and platform

- [x] Tauri 2 + React Response Console shell with every planned area and onboarding state.
- [x] Rust runtime with explicit turn lifecycle, cancellation, provider contracts, degradation, and telemetry.
- [x] Versioned SID-restricted IPC protocol and supervised worker boundaries.
- [x] Windows media broker architecture for WASAPI, WGC, Desktop Duplication, DirectComposition, DPI/HDR, and recovery.
- [x] SQLite memory with migrations, FTS, rebuildable embeddings, provenance, retention, backup, and import.
- [x] Model Manager with signed manifests, verified resumable installs, repair, removal, rollback, and license policy.
- [x] Hosted provider catalog covering LLM, STT, TTS, and retrieval routes, including separately gated NVIDIA NIM chat, embedding, ASR, Magpie TTS, and unavailable rerank records. Prior local conversation-worker records are implementation scaffolding, not a supported download path.
- [x] Experimental generic single-player mode and explicit audio/subtitle fallback.
- [x] Lock the supported game-integration strategy to non-injecting external capture/overlay; no profile requires a mod, script extender, hook, DLL, or executable adapter.

## Game support

- [x] Validate `GameProfileV2` schema and semantic safety rules.
- [x] Author and validate all 20 non-placeholder profiles.
- [x] Implement Steam, Epic, GOG, common-path, and manual executable detection contracts.
- [x] Block protected online/anti-cheat ambiguity for risk-gated profiles.
- [x] Preserve explicit-name/offscreen NPC conversation with full lore, memory, model, and voice behavior.

## Quality, delivery, and presentation

- [x] Deterministic simulation, provider mocks, fault injection, and canonical event logs.
- [x] Unit, integration, migration, profile, security, privacy, accessibility, and packaging tests.
- [x] Synthetic resource-pressure and latency benchmark harness with power-profile metadata.
- [x] Render and inspect the Response Console at wide, narrow, high-contrast, error, loading, and active states.
- [x] Rewrite README and documentation; add ADRs, diagrams, troubleshooting, legal/provenance, and contributor setup.
- [x] Generate and visually inspect the copyright-safe Gifsmith simulated demo.
- [x] Stage and smoke-test a local unsigned Debug installer on this development machine. Its manifest permanently classifies it as an `unchecked-development-package`; it is not RC evidence.
- [ ] Finish and reconcile the fresh installer-free `review-v17` application after the source freeze. The source-built v17 sibling test game is already verified, but the application package and final manifest remain pending. Older v9, v13, v15, and v16 artifacts are historical evidence for earlier source and renderer states; none qualifies the current renderer, installed desktop behavior, or physical presentation.
- [ ] Produce a source-identified local RC and pass or explicitly defer every RC gate.

## Benchmark environment note

The current development machine is in its single Balanced power profile, and
other agents may be running or using the GPU. Functional and deterministic
results remain valid. No elapsed build/test time from this pass is a performance
or release benchmark. Canonical runs must record the Windows/G-Helper profile,
boost state, temperatures, clocks, driver, power source, and competing workload.

## 2026-09-07 reconciliation snapshot

These are source-freeze checks, not the final package receipt. The generated
`review-v17` manifest is authoritative for the later application build.

- [x] The final locked/offline Rust workspace run passed 712 tests across 71
  groups, with 16 explicitly gated tests ignored and no failures. Root and
  nested formatting passed, followed by locked/offline all-target, all-feature
  Clippy with warnings denied.
- [x] The source freeze passed the credential scan across 1,110 tracked and
  untracked files and all 40 packaging security tests.
- [x] The final nested Tauri library run passed 244 tests with one env-gated
  real-catalog activation test ignored by default; that exact opt-in test passed
  separately against the private catalog. The final frontend suite passed 162
  tests, plus typecheck, production build, formatting, and wide/logical-320
  headless inspection with no overflow or browser errors.

- [x] The four reconciled control-backend groups passed 238 cumulative nested
  Tauri tests before commit; the effective-profile forwarding regression passed
  again after its source correction.
- [x] The pre-activation integrated Rust workspace passed 709 tests across 70
  test groups, with 16 explicitly gated tests ignored; strict all-target,
  all-feature Clippy passed.
- [x] The frontend suite passed 158 tests across 21 files before the final
  activation-control edit. Wide and logical-320 headless render audits passed
  after correcting Diagnostics overflow. Final UI tests and renders remain due
  after that edit.
- [x] The earlier integrated source passed the credential scan across 1,071
  files, source-hygiene validation, and 40 packaging security tests; the final
  1,110-file scan is recorded above.
- [x] The existing Release native build passed all eight non-GUI CTest suites.
  The final native source-freeze run also passed all eight suites.
- [x] The v17 synthetic test-game pack rebuilt byte-for-byte with SHA-256
  `92cdd41c1a451a0dcdbeb1eb05f7a2292b59b418bd89804cdc51398c9f114918`.

## Historical 2026-09-03 verification snapshot

The 2026-09-03 local pass produced the following functional evidence. Counts
refer to that dated source snapshot and do not satisfy live-game, clean-VM,
performance, display-matrix, model-pack, or production-signing gates:

- [x] Full locked Rust workspace tests pass; root formatting and strict Clippy pass.
- [x] Response Console: 124 frontend tests, TypeScript typecheck, and production Vite build pass.
- [x] Documentation link validation passes for 777 local links across 150 files.
- [x] Worker protocol/pack suite passes 30 tests; deterministic benchmark harness passes 21 tests.
- [x] All 20 authored profiles pass schema, semantic, provenance, and replay validation.
- [x] Deterministic simulation passes 22 tests and 127 assertions across 13 scenarios.
- [x] Native media broker non-GUI CTest passes 5/5; display/playback/input/identity Windows smoke executables are deliberately not run. The synthetic rendering and prepared review-game contract checks pass headlessly, with no live-game or performance claim.
- [x] Current-tree secret scanning, strict license/provenance validation, and deterministic complete CycloneDX SBOM generation pass.
- [x] The local installer smoke passes application health, protected AppData ACLs, direct runtime/media-broker child topology, parent-death cleanup, normal uninstall, and file/shortcut/registry residue checks. Runtime doctor is intentionally degraded only because Debug permits the unsigned development catalog. One active display was detected, so secondary-display placement correctly remained a no-op.

## Production integration gates still open

- [x] Connect the packaged Tauri shell to the authenticated runtime-host named pipe; retain the fixture bridge only for unbundled UI development.
- [x] Exercise real WGC frame delivery, event-driven WASAPI streaming, and DirectComposition residual presentation in the media broker.
- [x] Implement real HTTPS resume, verified extraction, same-volume filesystem activation, rollback, repair, and Ed25519 catalog verification in Model Manager.
- [x] Build the safe D3D12/CPU/RAM synthetic game-pressure executable with bounded manifests and no power-setting mutations.
- [x] Run bounded hosted-provider smoke tests with maintainer-supplied Cohere, ElevenLabs, and AssemblyAI credentials; no credential values are present in this repository or tracked artifacts.
- [x] Run bounded synthetic NVIDIA NIM smoke tests for chat, 2048-dimensional embeddings, and stock-voice Magpie HTTP TTS with a maintainer-supplied key; these are functional probes, not latency, quality, reliability, or release benchmarks.
- [x] Live-qualify the concrete fixed-origin NVIDIA Magpie Riva gRPC transport with authorized stock Aria synthesis: TLS 105 ms, discovery 261 ms/86 voices, first audio 816 ms, 948 ms total, and 59,392 non-silent unclipped PCM bytes. Normal app-turn selection, physical endpoint delivery, reliability, entitlement, and game-load behavior remain separate gates.
- [x] Live-qualify one production-adapter speech-first chain: selected Groq
  `qwen/qwen3.6-27b` reached a validated spoken field in 252.777 ms and selected
  Cartesia Sonic 3.6/Greg reached `RuntimeTtsBridge` first PCM at 492.964 ms from
  the original LLM request. The run produced 172,800 PCM bytes in 40 chunks and
  55 alignment events. This is not microphone/STT, normal `HostState`, native
  broker drain, OS speaker, physical audibility, or provider-wide latency proof.
- [ ] Live-qualify NVIDIA Nemotron streaming ASR over gRPC before that experimental route becomes selectable; keep hosted reranking unavailable until an eligible route passes.
- [ ] Qualify the source-preserving lip-sync route that replaces the rejected v8,
  v16, v32, v37/v5, v66, and later experiments. The September 7
  Cyberpunk/Misty moderate-OH comparison is accepted as the latest local-review
  artifact. It preserves v14's exact recorded native admission-event digest,
  all 20 source-identical frames, and zero changes outside dynamic residual
  bounds. Schema 3 selects its atlas state from smoothed coefficients with
  contact/silence resets; state 7 alone uses a more restrained generated
  same-identity reference, while states 0–6 and every alpha byte remain exact.
  The result replaces the fish-like open circle with a modest oval/teeth
  opening, but a visible photometric/pasted seam remains. The anatomy is a
  private generated hypothesis, not teeth observed in the game. This is an
  experimental offline replay, not natural animation, installed-app behavior,
  live capture, game-load, or end-to-end latency proof. The optional YuNet/LM1
  pack has a strict measured manifest. An isolated real activation run imported
  the exact private catalog, recovered from a deliberate missing-worker failure,
  then used a fresh hidden authenticated worker to load/unload the provider and
  activate the exact 17,849,614-byte inventory matching the attested tree. The
  first r4 receipt measured 785 ms and the final persisted r5 receipt measured
  302 ms; neither is a p99 or universal cold-load result. This proves the
  provider-load lifecycle only; it does not install the pack in normal user
  state or authorize live rendering. Ordinary visual targets remain disabled
  and fail open.
- [x] Preserve exact provider viseme timing through ordinary playback. TTS duration metadata is normalized to eleven canonical classes, bounded to 128 ordered cues, authenticated as producer command 5 without consuming audio frames, stored under exact stream identity, carried additively in command 29, and resolved against the playback sample clock with 50 ms anticipation and 80 ms release. Invalid or absent visual metadata leaves audio accounting unchanged and falls back to local PCM drive.
- [ ] Complete cross-process D3D-handle/ACL synchronization, OS shared-memory mapping, audio-format conversion, and qualified HDR shaders for the final media transport.
- [x] Implement the explicit provider-load self-test and activation UI, then
  prove inactive import, nonce-scoped failure/retry, fresh authenticated hidden
  worker load/unload, exact active inventory, duplicate rejection, and stale
  runtime-admission revocation against the private YuNet/LM1 catalog in an
  isolated review state. This does not authorize live rendering. Public pack
  endpoints and production trust remain disabled.
- [ ] Qualify eligible generic local lip-sync packs and expose an explicit user choice with exact model/revision, download/storage size, RAM/VRAM, backend, license, measured quality, game impact, and experimental caveats. No pack is bundled, available, downloaded, or activated automatically; activation requires attestation and user choice, never a default, dependency, migration, game/profile requirement, or fallback.
- [ ] Obtain live-game evidence for external capture, manual/generic target selection, subtitles, and any advertised generic screen-space visual capability. No current profile claims live certification.
- [ ] Run clean Windows 10/11 VM install, repair, upgrade, rollback, and uninstall certification.
- [ ] Run controlled reference benchmarks with recorded power, thermal, driver, and competing-workload state; the current Balanced-profile functional pass is noncanonical for performance.
- [ ] Resolve the release scan blocker caused by the reachable historical `apikeys.json` filename. The active tree is clean, but history must not be rewritten without explicit user approval.
- [ ] Provision production catalog/TUF trust roots, an approved production pack runner, model-pack qualification evidence, and Authenticode/update signing infrastructure.
