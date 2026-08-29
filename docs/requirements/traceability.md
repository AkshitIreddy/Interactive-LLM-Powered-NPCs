# Version 2 requirements traceability

Source: the 40-section 2.0 brief supplied with the repository, as superseded by the user's API-first and generic/no-mod integration decisions.  
State terms: **specified** means a decision/acceptance path exists in documentation; it does not mean production implementation has passed. **RC-blocking** means missing evidence prevents release-candidate readiness unless the user explicitly approves a recorded deferral.

## Forty-section matrix

| ID | Requirement | Design/artifact mapping | Verification evidence | Gate |
| --- | --- | --- | --- | --- |
| R01 | Audit/research and architecture before major code | `legacy/repository-audit.md`, `v1-pipeline.md`, `research/*`, ADRs | Audited revision/inventory, source ledger, architecture-spike results | RC-blocking; specified |
| R02 | Consumer desktop install → onboard → select → Start → play | Response Console + NSIS + runtime | Clean non-admin Win10/11 VM; first-run simulation; returning user starts in ≤2 primary actions | RC-blocking |
| R03 | Select desktop technology based on gaming constraints | ADR-0001, process model | Tauri release overhead/reconnect/package spike; WinUI fallback criterion | RC-blocking before shell freeze; specified |
| R04 | Polished progressive-disclosure UI/UX | `architecture/ui-architecture.md`, design tokens/components | Rendered review of all primary/empty/error/loading states, keyboard/Narrator/controller, 100–200% | RC-blocking |
| R05 | Provider-agnostic hosted LLM configuration/streaming | ADR-0004 + `LanguageModelProvider` | Contract/fixture tests for OpenAI, Gemini, Anthropic, Groq, compatible endpoint; credential UI | RC-blocking |
| R06 | Hosted STT with provider tradeoffs | ADR-0004, provider catalog | Streaming/final/cancel/noise/WER/language tests; provider disclosure accuracy | RC-blocking for one qualified hosted route; remaining adapters cannot be falsely advertised |
| R07 | Hosted TTS and multiple voices | ADR-0004, voice-binding profile data | Streaming first PCM, formats/cancel/voice discovery; deterministic background assignment; no clone assets | RC-blocking |
| R08 | Optional generic lip-sync pack manager | ADR-0005/0007 | Explicit opt-in; model/size/VRAM/license/quality disclosures; install/resume/hash/attestation/user-activation/repair/update/remove/rollback; no automatic/default/dependency/migration/game/fallback activation | RC-blocking only before advertising a local lip-sync pack |
| R09 | End-to-end conversational latency/pipelining | `data-flow.md`, benchmark strategy | Heavy-load API-route QPC traces: p50 ≤1.35 s and p95 ≤2.50 s first audio | RC-blocking |
| R10 | Performance page with reference/This PC and presets | UI architecture + resource broker | Accurate median/p95 CPU/GPU/VRAM/RAM/load/frame impact; Competitive/Fast/Balanced/Immersive/Maximum/Custom | RC-blocking |
| R11 | Major generic real-time lip-sync redesign and candidate research | ADR-0007, model ledger/benchmark strategy | Generic screen-space go/no-go measurements and rendered blind review; license/Windows evidence | Visual claims blocked; conversation RC may ship audio-only |
| R12 | Anchored, temporally stable overlay/facial technique | ADR-0008 + ADR-0007 | Anchor drift p95 ≤3% face width; outside-mask changes ≤0.1%; occlusion/stale recovery within one frame | RC-blocking for advertised screen-space capability |
| R13 | Reliable single/multi-monitor/DPI/resolution/modes | `legacy/single-monitor-root-cause.md`, ADR-0008 | Rendered matrix: negative origins, DPI, 720p–4K, ultrawide, SDR/HDR, resize, alt-tab, device loss | RC-blocking |
| R14 | Local desktop memory/RAG with distinct concepts | ADR-0006 + data flow | SQLite migration/recovery/disk-full; FTS/vector relevance/latency; source/derived separation | RC-blocking |
| R15 | Approximately 20 complete popular PC game profiles | `research/game-selection.md`, `GameProfileV2` | All 20 non-placeholder, provenance-complete, schema/replay valid; capability/fallback evidence | RC-blocking |
| R16 | Automatic Steam/Epic/GOG/common/manual discovery + generic mode | Profile design | Fixture/VM discovery, duplicates/moved installs/manual path; generic conversation/audio/subtitles | RC-blocking |
| R17 | Stable character identification using mixed evidence | Profile/identity contracts | Replays for switching/occlusion/reacquisition; no one-frame identity switch; explicit/offscreen selection | RC-blocking for advertised identity levels |
| R18 | Audit/disposition every existing feature; rethink emotion | `legacy/feature-disposition.md`, `NpcEffectsV1` | Every legacy feature row mapped; malformed/failed emotion neutral and non-blocking; webcam opt-in | RC-blocking; specified |
| R19 | Structured NPC runtime/output; optional failures isolated | ADR-0002/0003, `NpcEffectsV1` | Schema fuzz/invalid/late outputs; no game actions; stage-failure degradation and cancellation tests | RC-blocking |
| R20 | Deterministic reproducible developer environment/one command | Cargo/pnpm/uv locks, toolchain pins, `dev.ps1` | Clean clone `setup`, then `dev/test/lint/benchmark/package`; offline-resolvable release inputs | RC-blocking |
| R21 | Meaningful unit/integration/provider/profile/migration/manager/overlay/latency/recovery tests | Test suites and simulation mode | CI/local reports, coverage by requirement ID, deterministic seeds/fixtures | RC-blocking |
| R22 | Gameplay-video harness plus synthetic resource load | Replay harness and benchmark fixtures | Deterministic hashes/timelines/golden tolerances; GPU/VRAM/CPU/RAM load calibration | RC-blocking for visual profile evidence |
| R23 | User-facing diagnostics and secret-redacted export | Diagnostics UI/runtime | Mic/speaker/STT/TTS/provider/model/capture/GPU/game/overlay/latency/permission tests; canary scan | RC-blocking |
| R24 | Secure API-key storage | ADR-0009/security architecture | Windows Credential Manager lifecycle; no secret in files/logs/UI/child env/diagnostic/crash output | RC-blocking |
| R25 | Proper package; no end-user toolchains; robust upgrades/cache/downloads | NSIS + model manager | Clean VM install/uninstall/upgrade/migrations; download failure/resume/hash/disk-full/cache cleanup | RC-blocking |
| R26 | Professional README and detailed `/docs` | README lane + this documentation set | Link/lint/content review, truthful capability/benchmark tables, no stale v1 imagery | RC-blocking |
| R27 | Gifsmith synthetic demo GIF | Eclipse Harbor demo source/render manifest | Deterministic 28–32 s GIF/WebP visual review; no copyrighted gameplay/fabricated metrics | RC-blocking for presentation, not runtime |
| R28 | Extensive explained customization and overrides | UI/settings schema | Defaults/descriptions, global/game/character effective-source tests, settings migration, privacy/resource tags | RC-blocking |
| R29 | API/Offline routing, named provider/model loadouts, and performance presets | UI architecture + provider ADR | Loadout inheritance/switching and preset snapshots; no preset or loadout silently grants egress/fallback authority | RC-blocking |
| R30 | Intelligent scheduling, VRAM ceiling and FPS target | Resource broker | Optional visual lease; contention/cancel; live DXGI budget; ≤256 MiB cap overshoot for ≤2 s | RC-blocking for advertised lip-sync |
| R31 | Graceful degradation | Architecture degradation ladder | Fault matrix for webcam/GPU/lip-sync/provider/vector/STT/TTS/game; exact pre-authorized fallback only | RC-blocking |
| R32 | Structured local logging/timings; remote telemetry off | Runtime observability | Correlated QPC events, rotation/storage limits, redaction, telemetry absence/default-off test | RC-blocking |
| R33 | Clear data egress; Offline and local lip-sync make no provider calls | Security/data flow/UI disclosures | Per-feature egress snapshot; deny-all-network offline/lip-sync test; no hidden fallback | RC-blocking |
| R34 | License review and machine-readable attribution | Model/license ledger, SBOM/notices/provenance | Exact revision/license/hash review, CycloneDX SBOM, third-party notices, blocked-license tests | RC-blocking |
| R35 | Clean rewrite; import useful data only | Legacy audit/import map | No v1 runtime import/pickle deserialization; deterministic quarantine/dry-run/atomic rollback | RC-blocking; specified |
| R36 | Modern Windows target, API-first on ordinary PCs, optional 12 GB lip-sync reference, game priority | Model/resource architecture | API-only clean-machine run plus optional visual 6–8/12 GB/vendor matrix; game impact thresholds; no entire-GPU default | RC-blocking |
| R37 | Research/design/implementation/quality/presentation deliverables | Phase artifacts and RC report | Deliverable checklist is complete or user-approved deferral is explicit | RC-blocking |
| R38 | Do not release; local RC then user testing/approval | Release policy/automation guards | No push/tag/release/feed/public upload; local RC test/change/machine-test instructions | Absolute external-action gate |
| R39 | Prefer substantially better replacement, document decisions | ADR set + feature disposition | Major choices have context/decision/consequence/evidence and no legacy-only rationale | RC-blocking; specified |
| R40 | Twelve primary success criteria | Success-criteria matrix below | Each SC has evidence, no marketing substitution | RC-blocking |

## Primary success criteria

| ID | Success criterion | Acceptance evidence |
| --- | --- | --- |
| SC01 | Non-technical gamer can install/configure | Clean non-admin Windows 10/11 VM; no Python/Node/Rust/CUDA/FFmpeg; successful onboarding simulation without terminal/filesystem work. |
| SC02 | Supported games require almost no manual setup | Installed game detected/profile selected; returning configured user starts in ≤2 primary actions; manual executable only fallback. |
| SC03 | Explicit provider choice and reusable loadouts | Hosted conversation routes and named LLM/STT/TTS/retrieval loadouts expose provider/model/egress; switching is explicit; no unauthorized fallback. |
| SC04 | Dramatically lower latency | Numeric R09 thresholds under heavy load and comparative v1 trace showing removal of fixed 5-second pre-generation delay/full-render waits. |
| SC05 | More natural lip-sync | Capability-specific latency/FPS/visual gates and blind rendered preference; otherwise honestly audio-only. |
| SC06 | Overlay no longer looks pasted | Fresh-frame masked residual, anchor/outside-mask/occlusion thresholds and human review across motion/display matrix. |
| SC07 | Preserve FPS/GPU headroom | Competitive average/1%-low loss ≤2%/5%; Balanced ≤5%/10%; Immersive ≤10%/15%; audio-only overlay ≤0.5 ms p95. |
| SC08 | Single-monitor works | Foreground input + PTT + capture/overlay rendered evidence with no mirror/self-capture/frozen frame at required DPIs/resolutions. |
| SC09 | Developers reproduce reliably | Clean clone one-command setup; all locks/toolchain/model manifests verified; deterministic simulation passes. |
| SC10 | Polished real-product UI | Complete rendered state/accessibility review and successful novice onboarding study/test script. |
| SC11 | Professional README/docs | Truthful reviewed README/demo plus task-oriented docs, architecture/ADR/reference/troubleshooting and working links. |
| SC12 | Modular/evolvable architecture | Provider/worker/profile contract fixtures prove add/replace without turn-state changes; compatible IPC/schema migration tests. |

## Cross-cutting numeric RC gates

- Retrieval p95 ≤75 ms; VAD endpoint p95 ≤500 ms; barge-in to silence p95 ≤150 ms; audio/visual onset skew p95 ≤80 ms.
- English fast-preset WER ≤8% clean and ≤15% at 10 dB game-noise SNR; false rejects ≤2%; false activations ≤1/hour.
- No WASAPI underruns or >20 ms chunk gaps in a 30-minute stream.
- Four-hour/500-turn heavy-load soak with no crash, deadlock, stuck audio, device-reset failure or orphan worker.
- Screen-space lip-sync ≥30 generated FPS or validated 15 FPS temporal mode; p95 capture-to-composite ≤50 ms; pass 12 GB contention.

## Release report rule

Each row must resolve to `PASS`, `FAIL`, `NOT MEASURED`, or `USER-APPROVED DEFERRAL`, with links to immutable result artifacts. “Implemented,” vendor benchmarks, simulation-only evidence for live claims, and undocumented absence are not passing states.

The current source-identified disposition is recorded in the
[local 2.0 review evidence report](local-review-evidence-report.md). It is
deliberately not called an RC report while live and clean-machine gates remain
open.
