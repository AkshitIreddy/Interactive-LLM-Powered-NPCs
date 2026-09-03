# Developer command surface

Run commands from the repository root on Windows PowerShell 5.1+ or PowerShell 7. The repository currently pins Node.js 20.20.2 and pnpm 10.28.2 in `package.json`.

```powershell
.\scripts\dev.ps1 environment
.\scripts\dev.ps1 setup [-Offline]
.\scripts\dev.ps1 dev
.\scripts\dev.ps1 lint
.\scripts\dev.ps1 test
.\scripts\dev.ps1 benchmark [-Quick] [-OutputDirectory <path>]
.\scripts\dev.ps1 package [-Configuration Debug|Release] [-SkipChecks] [-OutputDirectory <path>]
```

The scripts never install global tools. They discover checked-in 2.0 manifests, use pinned workspace commands, and identify missing prerequisites. `setup` restores the pnpm workspace, the root Rust workspace, the separately locked nested Tauri workspace, and the isolated README demo npm workspace. Its first online run warms the exact locked inputs; `setup -Offline` is the fail-closed second-pass proof that all four dependency graphs are cached. Optional Python-worker and native CMake workspaces are touched only when their manifests exist. On Windows, nested Tauri validation prepares project-owned binaries before Clippy so a clean source archive receives the same external-binary validation as a populated checkout. Disposable native outputs use repository-keyed short paths beneath `%LOCALAPPDATA%\InteractiveNPCs\build\{mb,mw,st}`, keeping components, checkouts, and stale source/toolchain caches isolated. `package` is Windows x64 only and produces a local NSIS review artifact with SHA-256 hashes; it does not sign or publish it. Its last stdout object is compact JSON containing the exact app, installer, manifest, stage, and (for Debug) synthetic-game paths.

Windows setup, build, lint, test, benchmark, and package children use the shared
captured launcher with `CreateNoWindow=true`, hidden window style, redirected
stdout/stderr, exact argument quoting, and checked exit propagation. This keeps
task commands from opening console windows. The separately authorized final
installer/app/test-game GUI launches are not disguised as headless checks.

The root `typecheck` and `build` package scripts use exact Corepack dispatch with
an explicit `@npc2/control` filter. They never use recursive `pnpm -r` (which
would include the root and re-enter the same script), and they never depend on a
separate `pnpm.cmd` shim.
Every other root JavaScript alias follows the same explicit Corepack + workspace
filter contract. Interactive `dev`, `tauri`, and `sim` aliases are source-checked
by the regression but are not launched during headless test/package gates.
`test-root-workspace-script-dispatch.ps1` runs both once through the exact hidden
Corepack resolver with a bounded timeout. The same launcher restores standard
Windows executable extensions when a WSL-started shell inherits `PATHEXT=.CPL`,
so workspace-local `tsc.cmd`, `vite.cmd`, and `tauri.cmd` remain resolvable.

Prove setup dispatch without downloading anything:

```powershell
.\scripts\test-setup-reproducibility.ps1
```

The regression uses disposable fake tools to require an online locked restore and
an offline restore for pnpm, both Cargo lockfiles, the demo npm lockfile, and the
pinned WebView2 standalone-installer cache dispatch.

Record the exact native build environment without recording installation paths:

```powershell
.\scripts\toolchain-evidence.ps1 `
  -RequireNative `
  -OutputPath .\artifacts\toolchain-evidence.json
```

The report hashes the observed Node, Rust, CMake, MSVC compiler and Windows SDK
tools plus the exact Microsoft-signed WebView2 standalone-installer input. It
classifies the package as `pinned-input-non-byte-reproducible-local-review`:
WebView acquisition is eliminated, but compiler/linker and NSIS output bytes are
not promised identical across independent environments.

Synchronize the two acceptance documents from their single status map and emit
an exact in-progress source identity with:

```powershell
.\scripts\generate-acceptance-evidence.ps1 -Mode InProgress -UpdateDocuments
```

The status source is
`docs/product-rework/original-brief-gap-map.json`. The generator requires all
R01-R40 and SC01-SC12 rows exactly once, validates the declared totals and every
named evidence hash, and prevents a non-static `PASS` from relying on source-only
or isolated-test evidence. `-Mode Frozen` additionally requires the exact
package manifest, installer, installed-distribution manifest, and at least one
live/rendered/matrix result artifact:

```powershell
.\scripts\generate-acceptance-evidence.ps1 `
  -Mode Frozen `
  -PackageManifestPath <package-manifest.json> `
  -InstallerPath <installer.exe> `
  -InstalledDistributionManifestPath <installed-distribution.json> `
  -InstallerSmokeResultPath <installer-smoke-result.json> `
  -EvidenceArtifactPath <result.json>,<rendered-or-live-result.json> `
  -UpdateDocuments
```

The ignored `artifacts/acceptance/acceptance-evidence-run-v1.json` binds the
synchronized status map to the exact current source-candidate digest and every
supplied artifact hash. Missing files, stale hashes, duplicate/missing rows, or
isolated-test promotion fail closed before a frozen run is emitted.

Only after packaging, installation, and installed-distribution reconciliation,
validate measured privacy evidence with
`validate-installed-privacy-proof.ps1`. It binds the proof and retained WFP
capture evidence to the exact package manifest, installer, installed manifest,
installed control executable, and release-candidate ID. It never generates
evidence or accepts fixture and test-harness provenance.

Validate sidecar copying, checkout isolation and the deep-path FileTracker regression with:

```powershell
.\scripts\test-prepare-sidecars-path.ps1
.\scripts\test-prepare-sidecars-path.ps1 -BuildNative
```

The first command is a fast four-binary PE/path-contract check using disposable
x64 GUI fixtures. `-BuildNative` additionally runs the real Release sidecar
build and audit. The Rust runtime follows the requested app profile but uses
default features only; the media broker owns product audio. Broker, mouth
worker, and subtitle presenter are Release-built even for Debug review.

The native test suites use the same checkout-keyed policy without sharing build
trees between components. Validate both the media-broker test path and the inert
game-load Windows smoke path with:

```powershell
.\scripts\test-short-cmake-build-paths.ps1
.\scripts\test-short-cmake-build-paths.ps1 -BuildNative
```

The native variant performs real MSVC builds from a deep clean-source fixture;
the game-load portion remains a dry run and never applies a workload.

`benchmark` always writes a strict simulated pipeline report containing p50,
p95 and p99 for all twelve required metrics. That report tests aggregation and
is never labelled live acceptance evidence. A separate instrumented Windows
`live + measured` JSONL input is still required for product performance claims.

Security hooks:

```powershell
.\scripts\security\scan-secrets.ps1 -IncludeUntracked
.\scripts\security\generate-sbom.ps1 [-Strict]
.\scripts\security\check-licenses.ps1 [-Strict]
```

`package` goes further than the source-lock command: it resolves the actual root,
nested-Tauri, and production-pnpm runtime graphs offline, collects every exact
included package license/notice body, binds that corpus and artifact scope into
CycloneDX, and installs the hashed result under `product-audit/legal/`.

`packaging/windows/nsis/installer-hooks.nsh` is the reviewed lifecycle and pinned
WebView2 template. The product overlays require an ignored generated wrapper under
`src-tauri/generated-installer-inputs`; packaging creates it only after exact
hash/version/Authenticode verification and removes it after Tauri consumes it.

The installer lifecycle smoke test is deliberately separate and opt-in:

```powershell
.\scripts\installer-smoke.ps1 `
  -PackageManifestPath .\artifacts\package\<timestamp>\package-manifest.json `
  -AcknowledgeLocalInstall
```

It accepts only an unsigned Debug package whose manifest says `local-review-only`, uses the isolated `io.github.akshitireddy.interactive-npcs.review` identifier, and has no updater. Release packaging continues to inherit the production identifier from `apps/control/src-tauri/tauri.conf.json`; `packaging/tests/review-namespace-isolation.ps1` fails if that boundary regresses. The smoke refuses to replace an existing installation or review namespace, installs beneath a unique `%LOCALAPPDATA%\InteractiveNPCsInstallerSmoke\<operation-id>` directory by default (or an explicit short `-InstallerSmokeRoot`, useful for long-path and external-volume review), verifies the installed executable and all four sidecar hashes, then emits a deterministic closed-world manifest and runs strict installed legal/SBOM/scope/unclassified-file reconciliation. It repeats the exact manifest proof after reinstall before running the installed runtime doctor and persistent runtime/broker lifecycle. The broker is never invoked standalone. Only a checked clean-source package with every reconciliation and lifecycle assertion true, plus an installed model-catalog root explicitly marked for production trust with rotation complete and promotion/publication enabled, emits separate `promotion_eligible=true` smoke evidence. A bootstrap catalog can pass local review but can never be promoted.

The bounded launch uses a temporary loopback-only WebView inspection port to prove that a genuinely empty review app-data namespace renders the non-dismissible first-run onboarding overlay. It also launches the stable project-owned synthetic game from `..\local-app-data\test-game`, verifies its executable, distribution-manifest, CycloneDX, notices, source, license and build-receipt hashes, then requires advancing generated-frame plus generated-PCM PID/HWND metadata. No video or codec binary is loaded. The secondary-monitor helper is applied only to the exact task-owned PIDs. During app observation, the harness verifies the installed supervised child set and its exact paths, hashes, parent IDs, authenticated health, and parent-death shutdown; the broker is never invoked as a standalone smoke executable.

Before final evidence, the harness performs one complete uninstall/reinstall cycle and verifies that sidecar hashes stay identical. The final normal uninstall must leave no process, install root, shortcut, or registry residue. The task-owned `.review` roaming and local/WebView app-data namespaces are removed, while before/after identities prove that both production namespaces were untouched. The last stdout object is compact JSON with `status`, result/installer/test-game paths, onboarding evidence, reinstall evidence, and namespace isolation, so CI or another agent can parse pass/fail without scraping prose.

The result snapshots exact process, file, shortcut, and registry targets before install and immediately after the normal uninstaller. `no_remaining_processes`, `no_install_files`, `no_shortcuts`, and `no_registry_entries` are computed before emergency cleanup. Any false value permanently fails the smoke. Narrow emergency cleanup may then restore the test host, but every terminated PID or removed target is recorded and never changes failed evidence to passed evidence. Results also record the package-manifest hash, Git HEAD/dirty state, lock hashes, profile-corpus digest, catalog hash, model-manifest hash, and Tauri-config hash.

Validate manifest policy, target snapshots, and result/source-identity generation without installing anything:

```powershell
.\scripts\installer-smoke.ps1 `
  -PackageManifestPath .\artifacts\package\<timestamp>\package-manifest.json `
  -PreflightOnly `
  -ResultPath $env:TEMP\interactive-npcs-installer-preflight.json
```

The harness never searches for or deletes unrelated installations. Older smoke-result files that do not contain all four post-uninstall booleans and both snapshots are not evidence of uninstall cleanliness.

For the final installed privacy run only, add
`-CaptureInstalledPrivacyProof` plus the release-candidate ID, application
version, telemetry-inventory output, proof output, and capture-evidence output
paths. This opt-in hook runs after the exact reinstall reconciliation and before
normal uninstall. It calls `capture-installed-privacy-proof.ps1`, which requires
an elevated host, pre-enabled Filtering Platform failure auditing, temporary
Windows Defender Firewall per-program deny rules, and fresh native receipts for
both `offline_mode` and `local_lip_sync`. All child processes are hidden and all
evidence is bounded; the runner never packages, installs, signs, publishes, or
promotes. `packaging/tests/installed-privacy-capture-contract.ps1` checks the
fail-closed ordering without installing or launching anything.

Prepare or refresh the stable synthetic review target independently with:

```powershell
.\scripts\windows\prepare-review-test-game.ps1
```

Debug manifests intentionally bind the absolute hashes and paths of this
host-local review fixture; run the preparation command again after moving the
checkout or reviewing on another Windows account. The NSIS installer itself
does not redistribute the synthetic game. The fixture folder also contains its
exact fail-closed file allowlist, MIT notice, and CycloneDX handoff. It contains
no FFmpeg/ffprobe executable, MP4, model, captured game content, or third-party
binary.

The generated review paths are:

- app build: `apps\control\src-tauri\target\debug\interactive-npcs-control.exe`
- NSIS installer: `apps\control\src-tauri\target\debug\bundle\nsis\Interactive NPCs Response Console_2.0.0-alpha.1_x64-setup.exe`
- synthetic test game: `..\local-app-data\test-game\interactive-npcs-synthetic-target.exe`

Print the current absolute paths and copyable launch commands as one JSON object:

```powershell
.\scripts\windows\review-paths.ps1 -RequireAll
```

Independently verify the prepared folder and reject any missing, changed,
symlinked, path-traversing, duplicate, or unknown file with:

```powershell
.\scripts\windows\verify-review-test-game.ps1 `
  -Directory ..\local-app-data\test-game
```

## Synthetic game replay

Use the repository-owned Eclipse Harbor generator as a deterministic,
rights-cleared top-level Windows capture target without opening a real game:

```powershell
.\scripts\synthetic-game-replay.ps1 -PlaceOnSecondMonitor
```

The launcher compiles a task-local `interactive-npcs-synthetic-target.exe` beneath
the ignored `artifacts/synthetic-replay/` directory using the exact Roslyn and
.NET Framework reference hashes declared in `SOURCE-MANIFEST.json`. It performs
a second compile and requires bit-for-bit identical executable bytes. The player
generates a moving WinForms/GDI scene and quiet loop-safe PCM ambience in memory,
uses only Windows inbox APIs at runtime, and writes
`capture-target.json` containing stable `pid`, `window_handle`, and
`executable_basename` fields (plus compatibility aliases), its window title,
source identity, renderer/audio identity, and no-third-party policy. The Tauri debug bridge can read that JSON
path after the launcher returns and pass those three fields to its allowlisted
capture-target command.
It never requests NVIDIA compute. When monitor placement is requested it
invokes the installed `prefer-second-monitor` helper first in dry-run mode and
then for the exact player PID only; a primary-display fallback is allowed.

Run the non-GUI compile, provenance, two-distinct-frame, PCM measurement and
deterministic-rebuild checks with:

```powershell
.\scripts\test-synthetic-game-replay.ps1
.\scripts\test-prepare-review-test-game.ps1
```

Useful bounded recording options are `-ExitAfterSeconds <n>`, `-NoLoop`,
`-Mute`, `-Wait`, `-WindowTitle <name>`, and `-MetadataPath <path>`. This fixture proves
capture-target plumbing only. It is synthetic replay evidence, not live-game
certification or a game-load performance benchmark.
