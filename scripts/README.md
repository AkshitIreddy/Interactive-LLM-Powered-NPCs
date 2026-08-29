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

The scripts never install global tools. They discover checked-in 2.0 manifests, use pinned workspace commands, and identify missing prerequisites. `setup` restores JavaScript, Rust, optional Python-worker, and native CMake workspaces only when their manifests exist. On Windows, `lint` builds the two ignored project-owned sidecars immediately before nested Tauri Clippy so a clean source archive receives the same external-binary validation as a populated checkout. The native broker's disposable CMake output uses a repository-keyed short path beneath `%LOCALAPPDATA%\InteractiveNPCs\build\mb`; this leaves Visual Studio FileTracker enough path headroom even when the checkout is deeply nested, while keeping different checkouts and stale source/toolchain caches isolated. `package` is Windows x64 only and produces a local NSIS review artifact with SHA-256 hashes; it does not sign or publish it.

Validate sidecar copying, checkout isolation and the deep-path FileTracker regression with:

```powershell
.\scripts\test-prepare-sidecars-path.ps1
.\scripts\test-prepare-sidecars-path.ps1 -BuildNative
```

The first command is a fast path-contract check. `-BuildNative` additionally places a clean broker source fixture beneath a deliberately deep checkout, then performs a real Visual Studio configure and build using the shortened binary directory.

Security hooks:

```powershell
.\scripts\security\scan-secrets.ps1 -IncludeUntracked
.\scripts\security\generate-sbom.ps1 [-Strict]
.\scripts\security\check-licenses.ps1 [-Strict]
```

`-Strict` makes missing pinned audit generators fail instead of warn. CI may call the non-strict form while a new workspace is being bootstrapped, but a release candidate must use strict mode.

`packaging/windows/nsis/installer-hooks.nsh` is an inert reviewed hook template. It is deliberately not wired into the generic Tauri overlay because Tauri resolves hook paths from the application config directory; the desktop shell should reference or copy it into its `src-tauri` package once that final path is stable.

The installer lifecycle smoke test is deliberately separate and opt-in:

```powershell
.\scripts\installer-smoke.ps1 `
  -PackageManifestPath .\artifacts\package\<timestamp>\package-manifest.json `
  -AcknowledgeLocalInstall
```

It accepts only an unsigned Debug package whose manifest says `local-review-only` and has no updater. It refuses to replace an existing installation, installs beneath a unique `%LOCALAPPDATA%\InteractiveNPCsInstallerSmoke\<operation-id>` directory, verifies the installed executable, both project-sidecar hashes, catalog, model-manifest contract, and exactly 20 profiles, then runs the installed runtime doctor. The persistent broker is never invoked standalone. During the bounded app launch, the harness requires Tauri to maintain exactly one runtime child and one broker child from the installed paths, both with the shell PID as parent, for the full observation window. It then terminates the shell and requires both children to disappear through parent-death supervision before silently uninstalling.

The result snapshots exact process, file, shortcut, and registry targets before install and immediately after the normal uninstaller. `no_remaining_processes`, `no_install_files`, `no_shortcuts`, and `no_registry_entries` are computed before emergency cleanup. Any false value permanently fails the smoke. Narrow emergency cleanup may then restore the test host, but every terminated PID or removed target is recorded and never changes failed evidence to passed evidence. Results also record the package-manifest hash, Git HEAD/dirty state, lock hashes, profile-corpus digest, catalog hash, model-manifest hash, and Tauri-config hash.

Validate manifest policy, target snapshots, and result/source-identity generation without installing anything:

```powershell
.\scripts\installer-smoke.ps1 `
  -PackageManifestPath .\artifacts\package\<timestamp>\package-manifest.json `
  -PreflightOnly `
  -ResultPath $env:TEMP\interactive-npcs-installer-preflight.json
```

The harness never searches for or deletes unrelated installations. Older smoke-result files that do not contain all four post-uninstall booleans and both snapshots are not evidence of uninstall cleanliness.

## Synthetic game replay

Use the repository-owned Eclipse Harbor MP4 as a deterministic, rights-cleared
top-level Windows capture target without opening a real game:

```powershell
.\scripts\synthetic-game-replay.ps1 -PlaceOnSecondMonitor
```

The launcher compiles a task-local `interactive-npcs-synthetic-target.exe` beneath
the ignored `artifacts/synthetic-replay/` directory. The player software-decodes
the MP4 with FFmpeg (`-hwaccel none`), renders through WinForms/GDI, and writes
`capture-target.json` containing stable `pid`, `window_handle`, and
`executable_basename` fields (plus compatibility aliases), its window title,
source identity, and decode policy. The Tauri debug bridge can read that JSON
path after the launcher returns and pass those three fields to its allowlisted
capture-target command.
It never requests NVIDIA compute. When monitor placement is requested it
invokes the installed `prefer-second-monitor` helper first in dry-run mode and
then for the exact player PID only; a primary-display fallback is allowed.

Run the non-GUI compile, provenance, and one-frame software-decode check with:

```powershell
.\scripts\test-synthetic-game-replay.ps1
```

Useful bounded recording options are `-ExitAfterSeconds <n>`, `-NoLoop`,
`-Wait`, `-WindowTitle <name>`, and `-MetadataPath <path>`. This fixture proves
capture-target plumbing only. It is synthetic replay evidence, not live-game
certification or a game-load performance benchmark.
