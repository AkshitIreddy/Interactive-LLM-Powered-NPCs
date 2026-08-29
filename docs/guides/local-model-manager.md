# Optional Lip-sync Pack Manager

Conversation is API-first. The Model Manager is reserved for optional, immutable **generic screen-space lip-sync** packs after the base app is installed. It does not install local LLM, STT, TTS, or retrieval models. Its trusted lifecycle core is implemented in `crates/model-manager`; no public downloads, catalog keys, or qualified lip-sync packs ship yet.

## A pack must declare

- kind (`generic_screen_space_lipsync` only);
- upstream source and immutable revision;
- archive/files, byte sizes, SHA-256 hashes, and runtime ABI;
- backend/device/driver constraints and input/output formats;
- measured download/installed size, RAM, VRAM, load, latency, game impact, and visual-quality envelope;
- license, attribution, redistribution status, and use restrictions;
- self-test, activation, rollback, repair, update, and removal metadata.

## Installation lifecycle

`Available → Downloading → Verifying → Staging → Self-testing → Active`

Downloads are resumable and size-limited. Archives are extracted into staging with traversal/link checks. A pack becomes active only after metadata/hash verification, self-test, attestation, and a user activation choice; activation is atomic. A profile, game detection, update, migration, dependency, or degradation rule cannot activate it. Failure preserves the previous active pack and exposes a retry/repair reason.

## Recommendations

Recommendations use current hardware, live DXGI budget/headroom, a configured game reserve, measured warm and p99 RAM/VRAM workspace, runtime backend, selected game/performance mode, frame-time target and load/unload cost. The GPU model or total VRAM alone is insufficient. The current scheduler may admit at most one optional local visual lease and revokes or unloads it before affecting hosted conversation or audio/subtitles. Every pack stays visibly experimental until its gates pass. The user must explicitly select the model and confirm its exact size, resource envelope, license/use terms, quality evidence, and download. A pack can never be installed or activated automatically as a default, dependency, migration, profile/game requirement, or fallback.

Local LLM, STT, TTS and embedding models remain outside the current product policy. If that scope changes, the application must first add a measured co-residency matrix that classifies every selected combination as safe resident, serialized/cold-load only, CPU-only, conflicting or unverified. It must account for the running game's reserve and model p99 workspace, serialize incompatible GPU stages, disclose switching latency, and never silently change device or provider.

## Repair, update, remove

- **Repair** re-verifies files and replaces damaged content through staging.
- **Update** installs a new immutable version alongside the old one, self-tests, then atomically switches.
- **Rollback** restores the last compatible installed version.
- **Remove** checks shared references before deleting and reports reclaimed space.
- **Clean cache** removes inactive download/staging content, never an active/shared pack.

Network failure, ETag change, wrong size/hash, disk full, interruption, unsafe archive entry, ABI mismatch, failed self-test, and signature/TUF failure must be explicit states—not partial success.

## Current implementation boundary

The crate implements `ModelPackManifestV1` validation, Ed25519 signed-catalog verification, version rollback/equivocation/revocation policy, strong ETag/If-Range resumable HTTPS journals, bounded streaming size/SHA-256 verification, safe ZIP/TAR/TAR.GZ extraction, and persistent activation/repair/rollback/reference-counted removal. Archive handling rejects traversal, links, devices, collisions, expansion-limit violations, and Windows reparse-point paths. Storage uses same-volume staging, immutable version directories, interprocess locks, durable journals, and atomic active pointers.

No approved public pack catalog, TUF root, third-party runtime, or model weight ships in this checkout. Response Console lifecycle wiring, lip-sync-specific self-tests, final key provisioning, and release artifact/license review remain integration gates. The implemented network and filesystem machinery therefore does not authorize a download by itself.

## License gate

Permissive redistributable lip-sync packs can enter a signed catalog after review. Research-only, personal-use, non-commercial, private-access, or no-redistribution weights must not be repackaged. A direct-from-upstream flow is allowed only when exact terms permit the intended use and the UI clearly segregates it from official redistributable packs.

Current research uses Audio2Face-3D regression v2.3 only as a possible audio-to-animation coefficient source for a project-owned tracked 2D mouth residual; it does not connect to a native game rig. NVIDIA Maxine AR SDK LipSync is a separate access-controlled direct-video experiment pending exact access, license, Windows, latency, VRAM, quality and game-impact qualification. MuseTalk 1.5 is offline/comparator-only after its measured batch path took approximately 102 seconds for 1.579 seconds of output. EfficientSync and FlashLips are paper watchlist entries only. None is a qualified, downloadable or selectable pack.

In every lane the captured game frame is immutable. A worker may return only a bounded mouth residual keyed to the exact actor, frame, timestamp and cancellation generation. The compositor applies an accepted residual to a presentation copy of the newest compatible frame; stale, occluded, wrong-identity, out-of-mask or over-budget work is discarded and the untouched current frame is shown.
