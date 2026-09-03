# Local Windows review guide — blocked pending final package

Snapshot: 2026-08-30  
Status: **STALE / NON-AUTHORITATIVE — DO NOT LAUNCH FROM THIS FILE**

The previous guide named an older source-tree Debug executable and installer.
Those paths are not the current review handoff: the executable predates the
latest source changes, its directory does not contain all four required
sidecars, and the named installer is not present. All old launch instructions,
app paths, installer paths, and binary claims have therefore been removed.

Do not infer a supported app, installer, installation, or end-to-end result
from this placeholder. The project-owned synthetic game fixture may be verified
independently by the repository tooling, but its final user-facing path and hash
will be published together with the app only after the complete review bundle
is frozen.

## Regeneration gate

This guide may become authoritative only after one exact source freeze produces
all of the following:

1. a four-sidecar local-review package containing the control app,
   `npc-runtime`, `npc-media-broker`, `npc-mouth-worker`, and
   `npc-subtitle-presenter`;
2. a package manifest bound to the frozen source candidate and the canonical
   acceptance gap map, ledger, and evidence report;
3. a hash-verified installer selected from that exact package run;
4. a closed-world installed-distribution manifest with no missing or unknown
   files;
5. a passing two-install installer-smoke result bound to the package and
   installed-distribution hashes;
6. a freshly verified project-owned synthetic test-game manifest; and
7. final current-source capability text that keeps unqualified microphone STT,
   face recognition, lip-sync, live-game certification, HDR presentation, and
   production/public NVIDIA trial use explicitly unavailable.

At that point this file must be generated from the exact evidence above and
must include Windows paths, SHA-256 hashes, launch order, review steps,
limitations, and cleanup instructions for those artifacts only. Handwritten or
historical paths must not be restored.

Until regeneration, use the
[canonical acceptance ledger](original-brief-acceptance.md) for status and the
[verification plan](verification.md) for required evidence. Neither document is
a substitute for a runnable final package.
