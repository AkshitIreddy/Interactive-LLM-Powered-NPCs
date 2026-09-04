# Interactive LLM Powered NPCs 2.0 implementation status

This file is the durable source of truth for the local 2.0 overhaul. A checked
item requires implementation evidence; prose progress alone does not count.

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
- [ ] Reconcile the current local review installer with the full-contour, sample-clocked lip-sync build. The prior v9 package passed closed-world reconciliation but its mouth compositor was later rejected visually. A clean-source `573f84a` Debug-review installer and v13 loose review directory now contain the current control/runtime, the stronger-jaw mouth worker, and warning-as-error-built, PE-audited native sidecars with matching nested manifests. Neither was launched or installed, so v13 is still unchecked development/package-build evidence rather than installed-distribution or desktop-presentation evidence.
- [ ] Produce a source-identified local RC and pass or explicitly defer every RC gate.

## Benchmark environment note

The current development machine is in its single Balanced power profile, and
other agents may be running or using the GPU. Functional and deterministic
results remain valid. No elapsed build/test time from this pass is a performance
or release benchmark. Canonical runs must record the Windows/G-Helper profile,
boost state, temperatures, clocks, driver, power source, and competing workload.

## Latest local verification snapshot

The 2026-09-03 local pass produced the following functional evidence. Counts
refer to the current development tree and do not satisfy live-game, clean-VM,
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
- [ ] Live-qualify NVIDIA Nemotron streaming ASR over gRPC before that experimental route becomes selectable; keep hosted reranking unavailable until an eligible route passes.
- [x] Replace and requalify the rejected headless mouth proofs. The first passed numeric gates but failed visual review with a detached procedural slit, flat teeth strip, and female Aria audio on a male subject. User review then rejected v16 because its symmetric cavity cut through the upper lip. The current v32 candidate uses explicit stock male Jason audio, the complete ordered OpenSeeFace mouth contour, a source-derived contact seam, exact upper-lip surface locking, stronger curved lower-jaw opening, distinct broad spectral PCM shapes, and conservative gated oral-depth hints. It measured 34.817 ms p95 moving OpenSeeFace inference at adaptive 10 Hz and 3.043 ms p95 compositing at 960x720/30 FPS with 0 GPU VRAM, 32 changed adjacent source frames, 39 distinct output digests, 28 frames above the material mouth-motion gate, a 19.2% maximum articulated aperture relative to mouth width, and zero protected-upper-lip darkening. This qualifies only the isolated CPU component; WGC/broker/DirectComposition presentation, selectable-pack activation, broader face/camera quality, commercial-game footage, and live-game certification remain open.
- [x] Preserve exact provider viseme timing through ordinary playback. TTS duration metadata is normalized to eleven canonical classes, bounded to 128 ordered cues, authenticated as producer command 5 without consuming audio frames, stored under exact stream identity, carried additively in command 29, and resolved against the playback sample clock with 50 ms anticipation and 80 ms release. Invalid or absent visual metadata leaves audio accounting unchanged and falls back to local PCM drive.
- [ ] Complete cross-process D3D-handle/ACL synchronization, OS shared-memory mapping, audio-format conversion, and qualified HDR shaders for the final media transport.
- [ ] Provision approved TUF roots/catalog metadata and wire the Model Manager lifecycle into the Response Console; public pack endpoints remain disabled.
- [ ] Qualify eligible generic local lip-sync packs and expose an explicit user choice with exact model/revision, download/storage size, RAM/VRAM, backend, license, measured quality, game impact, and experimental caveats. No pack is bundled, available, downloaded, or activated automatically; activation requires attestation and user choice, never a default, dependency, migration, game/profile requirement, or fallback.
- [ ] Obtain live-game evidence for external capture, manual/generic target selection, subtitles, and any advertised generic screen-space visual capability. No current profile claims live certification.
- [ ] Run clean Windows 10/11 VM install, repair, upgrade, rollback, and uninstall certification.
- [ ] Run controlled reference benchmarks with recorded power, thermal, driver, and competing-workload state; the current Balanced-profile functional pass is noncanonical for performance.
- [ ] Resolve the release scan blocker caused by the reachable historical `apikeys.json` filename. The active tree is clean, but history must not be rewritten without explicit user approval.
- [ ] Provision production catalog/TUF trust roots, an approved production pack runner, model-pack qualification evidence, and Authenticode/update signing infrastructure.
