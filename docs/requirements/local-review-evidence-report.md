# Local 2.0 review evidence report

Snapshot date: 2026-08-29  
Distribution state: local, unsigned, unpublished development tree; one Debug `unchecked-development-package` staged for maintainer testing only  
Release status: **not a release candidate**

This report applies the result vocabulary required by
[`traceability.md`](traceability.md): `PASS`, `FAIL`, `NOT MEASURED`, or
`USER-APPROVED DEFERRAL`. `PASS` means the stated acceptance evidence exists
for the scope of that row. Source code, a green unit test, a vendor claim, or a
deterministic simulation does not substitute for live evidence when the row
requires it.

No row uses `USER-APPROVED DEFERRAL`; no release-gate deferral has been
requested or granted. The current PC is on its single Balanced power profile,
with other agents and possible GPU contention, so elapsed build/test time is not
performance evidence.

## Verified local functional evidence

These checks passed against the 2026-08-29 development tree:

| Surface | Result | Scope boundary |
| --- | --- | --- |
| Rust workspace | PASS | Full locked workspace tests, root formatting, and strict Clippy. |
| Response Console | PASS | 28 tests, TypeScript typecheck, and production Vite build. |
| Documentation | PASS | 152 local links validated. |
| Workers | PASS | 22 protocol and pack tests. No third-party runtime or model weight was executed. |
| Profiles | PASS | 20/20 authored profiles pass schema, semantic, provenance, and replay validation; none claims live certification. |
| Simulation | PASS | 15 checks across seven deterministic scenarios. Values are virtual regression evidence, not benchmarks. |
| Native | PASS | Media broker CTest 2/2 and the game-load harness inert/dry-run path. No live capture, display matrix, GPU contention, or performance claim. |
| Security and supply chain | PASS | Current-tree secret scan, strict license/provenance validation, and deterministic complete CycloneDX SBOM generation. Reachable history remains separately blocked by the historical `apikeys.json` filename. |
| Local installer smoke | PASS | The unsigned Debug `unchecked-development-package` passed authenticated runtime/media-broker health, protected AppData ACLs, direct child topology, parent-death cleanup, normal uninstall, and file/shortcut/registry residue checks. Doctor was degraded only because Debug permits the intentionally unsigned development catalog. One active display made secondary-display placement a correct no-op. |

The staged test installer and its manifest are under
`artifacts/test-app/<UTC timestamp>/`; the redacted smoke result is
`artifacts/installer-smoke-final.json`. Both are mutable local-review artifacts,
not immutable release evidence.

## RC-blocking gaps

- The current tree passes secret scanning, but the checked release pipeline
  fails closed on the reachable historical `apikeys.json` filename. Rewriting
  Git history requires explicit user approval.
- Production catalog/TUF trust roots, approved production metadata, a production
  pack runner, exact model-pack qualification and license evidence, and signing/
  update infrastructure are not provisioned.
- No optional generic lip-sync pack has passed model quality, Windows runtime,
  storage/RAM/VRAM, latency, game-impact, GPU-contention, license, and self-test
  qualification. No model is bundled or selectable as a production pack.
- Clean Windows 10/11 VM installation, live-game capture/identity/lip-sync,
  performance/latency/WER/soak, monitor/DPI/HDR/display-mode, accessibility, and
  offline deny-all-network acceptance remain incomplete.
- Consequently, the staged Debug package is not an RC and cannot be published,
  signed, used by an update feed, or represented as production-ready.

## Forty-requirement status

| ID | Status | Current evidence and disposition |
| --- | --- | --- |
| R01 | PASS | Baseline revision/CRLF proof, [legacy audit](../legacy/repository-audit.md), [v1 pipeline](../legacy/v1-pipeline.md), [import/archive map](../legacy/import-archive-map.md), research ledger, risks, and nine ADRs are present. |
| R02 | NOT MEASURED | The local Debug NSIS smoke passes install, authenticated child health, parent-death cleanup, and clean uninstall, but no clean non-admin Windows 10/11 novice onboarding run exists. |
| R03 | NOT MEASURED | Tauri packaging, authenticated sidecar recovery, and Job Object tests exist; release-build frame impact beside game load is not measured. |
| R04 | NOT MEASURED | Rendered dark/light/high-contrast/responsive fixture review is local evidence; Narrator, controller-on-WebView2, and true Windows 100–200% acceptance remain incomplete until the final visual manifest is recorded. |
| R05 | PASS | OpenAI, Gemini, Anthropic, Groq, OpenAI-compatible, and Cohere LLM adapters have deterministic stream/cancel/schema/privacy tests; credential values remain behind the native Windows Credential Manager boundary. This is adapter regression evidence, not live route qualification. |
| R06 | NOT MEASURED | Hosted STT contracts exist, but the required live streaming WER/noise/language matrix has not run. Local STT packs are no longer a product target. |
| R07 | NOT MEASURED | Hosted TTS contracts, voice intent/binding, cancellation, and zeroizing secret tests exist; no live first-PCM/voice-quality route qualification exists. Local TTS packs are no longer a product target. |
| R08 | NOT MEASURED | Verified download/extraction/storage machinery exists, but it is now reserved for optional generic lip-sync. No approved TUF root/catalog or qualified attested lip-sync pack exists, and no automatic/default/dependency/migration/game/fallback activation is authorized. |
| R09 | NOT MEASURED | Streaming/cancellation semantics are simulated; no heavy-load QPC end-to-end latency trace meets the numeric gate. |
| R10 | NOT MEASURED | Performance controls and illustrative fixtures exist; Reference/This PC resource and frame-impact results do not. |
| R11 | NOT MEASURED | Generic screen-space candidate research and worker seams exist; no candidate has passed license, Windows, latency, contention, and blind-review gates. Native-rig/mod integration is no longer a product path. |
| R12 | NOT MEASURED | Fresh-frame residual/fail-open contracts exist; anchor drift, outside-mask change, occlusion, and stale-frame thresholds have not been measured on live imagery. |
| R13 | NOT MEASURED | WGC/WASAPI/DirectComposition Windows smoke tests exist; the full monitor/DPI/resolution/SDR/HDR/game-mode matrix does not. |
| R14 | NOT MEASURED | SQLite WAL/STRICT/FTS/hybrid retrieval, migration, import, backup, retention, and source/derived separation tests pass; release retrieval latency and disk-fault evidence are not complete. |
| R15 | PASS | Twenty substantive, provenance-bearing profiles pass strict schema/semantic validation. Twenty versioned replays and their SHA-256 ledger verify profile hashes, detection, explicit offscreen selection, conversation/subtitle/memory/audio routes, and six risk refusals; no profile claims live certification. |
| R16 | PASS | Steam/Epic/GOG/common/manual discovery contracts and hostile-path tests cover automatic discovery; a separate experimental Generic Game runtime contract requires manual game/executable/character selection, provides conversation/memory/audio/subtitles, has no game actions or executable integration, and blocks protected/anti-cheat contexts. |
| R17 | NOT MEASURED | Identity evidence/fallback contracts and deterministic offscreen selection exist; stable actor IDs across live occlusion/reacquisition and one-frame switch prevention are not measured. |
| R18 | PASS | [Feature disposition](../legacy/feature-disposition.md) maps the prototype; malformed effects neutralize without delaying speech; demographic inference is removed and webcam perception is opt-in/off by default. |
| R19 | PASS | Typed non-executable effects, bounded turn state, cancellation generation, late-event rejection, worker quarantine, and optional-lane degradation have deterministic tests. Game actions are outside the product contract. |
| R20 | NOT MEASURED | Toolchains and locks are pinned and the canonical PowerShell surface covers all workspaces, but a clean offline-resolvable clone run has not been recorded. |
| R21 | PASS | The full locked Rust workspace, root format/strict Clippy, 28 frontend tests/typecheck/build, 22 worker tests, 20/20 profile validation/replays, 15 simulation checks across seven scenarios, native CTest 2/2, inert game-load path, installer smoke, and current-tree security/supply-chain gates pass. This does not promote rows that require live numeric, display, model-pack, clean-VM, or visual evidence. |
| R22 | NOT MEASURED | Deterministic simulation and a bounded D3D12/CPU/RAM load harness exist; no gameplay-video replay corpus or calibrated visual-profile evidence run exists. |
| R23 | NOT MEASURED | Redacted diagnostic report/UI/error paths and credential canaries exist; the complete real-device/provider/capture/permission matrix has not run. |
| R24 | PASS | Windows Credential Manager, allowlisted references, redaction, native entry, namespace integration, protected AppData ACLs, canary tests, and the current-tree secret scan pass. Reachable-history policy remains a separate packaging blocker. |
| R25 | NOT MEASURED | The local Debug `unchecked-development-package` passed install/health/supervision/uninstall smoke and Model Manager failure tests exist; clean Windows 10/11 install/repair/upgrade/rollback/cache/removal certification does not. |
| R26 | PASS | README, tutorials, how-to guides, references, architecture/ADRs, privacy/security, profile authoring, benchmark policy, and troubleshooting exist; the deterministic gate validates 152 local links. |
| R27 | PASS | The original Eclipse Harbor/Mara Gifsmith demo is 30.20 seconds, copyright-safe, explicitly illustrative, visually reviewed, and deterministically verified with a zero-MSE loop seam. |
| R28 | NOT MEASURED | Settings descriptions and privacy/resource consequences exist; complete global/game/character effective-source and migration acceptance is not recorded. |
| R29 | NOT MEASURED | The contract now requires API/Offline routing and explicit named provider/model loadouts with inheritance and no silent fallback. Final UI/runtime acceptance for the superseding decision is not recorded. |
| R30 | NOT MEASURED | Resource-broker state tests cover GPU leases, fixed degradation, and the transient cap; live optional-lip-sync/game contention evidence is absent. Local LLM scheduling is no longer a release requirement. |
| R31 | PASS | Deterministic fault scenarios exercise provider, animation-worker, low-VRAM, interruption, vector and offline fallbacks without unauthorized route changes. |
| R32 | FAIL | Content-light diagnostic events exist, but the complete production structured log sink, rotation/storage limits, QPC timing transport, and telemetry-absence test are not integrated. |
| R33 | NOT MEASURED | Egress policies fail closed in unit/simulation tests; packaged Offline behavior and optional lip-sync isolation behind OS deny-all networking have not passed. |
| R34 | FAIL | Strict source dependency license/provenance validation and deterministic complete CycloneDX generation pass for the current tree. RC remains blocked because approved exact model/runtime pack licenses, pack notices, production catalog/TUF provenance, trust roots, and signing custody are missing. |
| R35 | PASS | The active 1.x notebooks, generated-code execution, per-character Python voices, Chroma/pickle stores, duplicated game tree, SadTalker, and plaintext-key workflow are removed with a reversible Git/import map. |
| R36 | NOT MEASURED | API-first conversation removes the local-model hardware requirement, but the API-only clean-machine case and optional lip-sync 6–8/12 GB/vendor game-impact matrix are not measured. |
| R37 | FAIL | Local implementation artifacts are extensive, but live API-route performance, capture, generic lip-sync, clean-VM, and certification deliverables are incomplete. |
| R38 | PASS | The work remains on a local branch; no push, PR, tag, package publication, update feed, GitHub release, signing, or public asset hosting occurred. |
| R39 | PASS | ADRs and the legacy disposition record context, decisions, consequences, evidence, fallbacks, and rejected prototype patterns. |
| R40 | FAIL | The twelve success criteria below are not all `PASS`; this blocks an RC. |

## Twelve-success-criterion status

| ID | Status | Current evidence and disposition |
| --- | --- | --- |
| SC01 | NOT MEASURED | A local developer-machine Debug installer smoke passed, but no clean Windows 10/11 non-admin novice install/onboarding run without developer tools exists. |
| SC02 | NOT MEASURED | Store scanning exists, but supported-game zero/low-setup behavior and two-action return flow are not live-certified. |
| SC03 | NOT MEASURED | Hosted provider fixtures exist; named multi-provider/model loadout switching, inheritance, egress display, and no-silent-fallback acceptance are not complete. |
| SC04 | NOT MEASURED | The synchronous v1 waits are removed architecturally; no controlled comparative end-to-end latency trace exists. |
| SC05 | NOT MEASURED | No lip-sync candidate has passed the visual, latency, FPS, contention, and license gates. |
| SC06 | NOT MEASURED | No live masked-mouth residual has passed anchor/outside-mask/occlusion review. |
| SC07 | NOT MEASURED | No controlled game-impact run exists; the Balanced-profile inert load-harness pass is functional evidence only. |
| SC08 | NOT MEASURED | Native single-window smoke exists, but no real single-monitor foreground game session proves capture/PTT/overlay without self-capture. |
| SC09 | NOT MEASURED | Locks and one-command tooling exist; no clean clone/offline reproducibility artifact exists. |
| SC10 | NOT MEASURED | The UI has rendered local review evidence; Narrator/controller/200%/novice acceptance is not complete. |
| SC11 | PASS | The rewritten README, original demo, task-oriented docs, architecture/ADRs/reference/troubleshooting, and local-link validation are present and capability claims are explicitly bounded. |
| SC12 | PASS | Versioned provider/worker/profile/IPC contracts and deterministic replacement/migration/cancellation tests demonstrate modular boundaries without a general agent framework. |

## Numeric release gates

Every numeric conversation-latency, retrieval, VAD, barge-in, audio/visual skew,
FPS impact, VRAM overshoot, WER, false activation/rejection, WASAPI continuity,
identity/compositing, lip-sync, four-hour/500-turn soak, and clean-machine gate is
`NOT MEASURED` for release purposes. Synthetic and virtual-time values remain
useful regression fixtures but are not substituted here.

## Promotion rule

This report may call the build a local RC only after every `FAIL` is corrected
and every `NOT MEASURED` row is either backed by source-identified evidence or
changed to `USER-APPROVED DEFERRAL` by an explicit user decision. A fresh final
report must include the exact source-tree digest, lock/catalog/profile hashes,
toolchains, and immutable result paths.
