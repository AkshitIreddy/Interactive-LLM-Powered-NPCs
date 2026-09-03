# Installed-app evidence collector

This directory contains the fail-closed visual/function evidence workflow for the exact installed Interactive NPCs review build and the project-owned synthetic game.

The collector has three modes:

- `ValidatePlan` validates the screen, zoom, closeup, native-claim, and control-coverage contract. It cannot launch a process.
- `Preflight` reconciles the package manifest, installer, installer-smoke receipt, installed-distribution manifest and installed files, source evidence, and synthetic fixture. It cannot launch a process.
- `Capture` additionally requires a package/source-bound frozen-source receipt and the literal `ROOT_CONFIRMED_SOURCE_FROZEN` authorization token. Only this mode can launch the exact installed control executable and exact synthetic target.

Capture inventories visible windows before launch, preserves all unrelated windows, treats Transparency App windows as protected, places only the two exact task PIDs by using the secondary-monitor helper in dry-run then apply mode, and stops only processes it started. Terminal, recorder, and driver processes use hidden non-shell process creation. A task-owned visible `ConsoleWindowClass` fails the run.

Canonical screenshots come from the installed WebView through its loopback-only CDP endpoint. The display video is separately labeled as composited evidence because external dimming can alter perceived color and luminance. Every retained page is captured at normal/narrow widths and 100/150/200 percent WebView-effective scaling with a full frame and four to six closeups. These profiles are never presented as physical monitor DPI or HDR evidence: unavailable physical DPI/HDR observations are recorded as `not_measured` and are not simulated. Enabled controls without a matching exercise policy fail the run.

Successful runs are created in a never-reused timestamp-plus-nonce directory. `immutable-manifest.json` contains the SHA-256, size, and path of every other output and an evidence-root digest; files are then marked read-only. The manifest excludes its own hash to avoid circularity.

The frozen-source receipt must use this minimal shape:

```json
{
  "schema": "interactive-npcs-source-freeze/v1",
  "status": "frozen",
  "package_manifest_sha256": "<64 lowercase hex>",
  "source_candidate_digest_sha256": "<64 lowercase hex>"
}
```

Do not use `Capture` until the coordinating task has explicitly confirmed that the installed candidate and source are frozen.
