# Roadmap to 2.0

This is intended work, not shipped capability. [CHANGELOG.md](CHANGELOG.md) records completed changes. Gates are sequential.

## 1. Preserve and classify the prototype

Record the checkout; distinguish line-ending changes; map duplicate game trees, notebooks, Chroma/pickle stores, SadTalker, secrets, lore, and conversations; finish the 40-section requirements traceability matrix; keep unsafe generated-code paths disabled.

**Exit:** reversible migration/archive map and no unexplained baseline changes.

## 2. Prove the architecture

Validate Tauri/WebView2 impact, named-pipe streaming, WGC/DirectComposition, worker recovery, installer rollback, and generic screen-space animation. Complete architecture, security, licensing, risk, and decision records.

**Exit:** evidence-backed architecture; WinUI 3 is used only if the Tauri gate fails.

## 3. Foundation and deterministic slice

Pin toolchains; implement IPC, migrations, credential references, timing, fixtures, simulation, Response Console states, and packaging skeleton; demonstrate an API-powered simulated streaming turn and an offline deterministic fixture run.

**Exit:** clean-machine deterministic simulation.

## 4. Stabilize the external-integration and profile contracts

Build Skyrim SE, Cyberpunk 2077, and Baldur's Gate 3 data/capture slices; validate external capture, manual/offscreen selection, lore/memory, subtitles, provider mixing, and the optional generic visual path. Freeze versioned profile and external-media contracts after migrations and replays pass. Per-game mods, hooks, DLL injection, script extenders, and native-rig adapters are out of scope.

## 5. Complete runtime and 20 profiles

Finish hosted providers, memory, scheduling, degradation, presence, and optional generic visuals. Complete all profiles in [Supported games](docs/getting-started/supported-games.md), keeping risk-gated games offline.

**Exit:** profiles are non-placeholder, provenance-complete, schema-valid, replay-verified, and truthfully capability-labeled.

## 6. Model Manager, security, packaging

Use signed/TUF metadata only for optional generic lip-sync packs; add explicit selection, resumable staged downloads, safe extraction, self-test, attested user activation, rollback, repair/removal, and model/size/VRAM/license/quality disclosures. Complete isolation, provenance, SBOM, fuzzing, offline tests; validate a model-free non-admin installer on clean Windows 10/11. No automatic download or activation through defaults, dependencies, migrations, games/profiles, or fallback.

## 7. Local release candidate

Finish documentation, diagrams, rendered screenshots, and original synthetic demo. Run latency, impact, reliability, capture, security, privacy, profile, model-manager, accessibility, and clean-install gates. Produce a local RC and evidence for review.

## Approval-only actions

Pushing release work, opening a release PR, creating remote tags, publishing any installer/package/catalog, activating an update feed, creating a GitHub release, or publicly hosting demo assets requires explicit approval at the time.

## Not promised

Protected/online bypass; compatibility with every game; reliable Generic Game identity/actions; performer imitation; extracted voice cloning; silent provider fallback; or benchmarks without reproducible evidence.
