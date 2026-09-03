# Developer setup

## Supported host

Use Windows 10 22H2 or Windows 11 x64 with PowerShell 5.1+ or PowerShell 7. Native capture, audio, Credential Manager, Job Objects, installer, and clean-machine acceptance require Windows; WSL is not certification evidence.

Install Git, Rust 1.96.1 with the `x86_64-pc-windows-msvc` target plus clippy/rustfmt, Node.js 20.20.2, pnpm 10.28.2 through Corepack, Visual Studio Build Tools with Desktop development with C++ and a compatible Windows SDK, CMake 3.24+ (and a supported generator such as Ninja) for the media broker, and WebView2 Evergreen. Optional Python is only for explicitly pinned isolated research/worker packs—never the legacy global requirements.

## Bootstrap

From the repository root:

```powershell
./dev.ps1 environment
./dev.ps1 setup
```

`environment` reports prerequisites. `setup` never installs global tools; it restores only workspaces whose 2.0 manifests exist. Use `setup -Offline` only after all package caches/inputs are available.

All Windows task children are launched with no console window and captured
stdout/stderr. A failure still propagates its exact exit code and diagnostics;
headless execution is not silent error suppression.

The restore set is deliberately broader than the root workspaces: pnpm uses
`pnpm-lock.yaml`, the reusable Rust crates/runtime use the root `Cargo.lock`,
the Tauri shell uses its nested `apps/control/src-tauri/Cargo.lock`, and the
README renderer uses `demo/readme/package-lock.json`. A cold machine should run
`setup` once with network access, then run `setup -Offline`. The second command
must restore all four graphs without network access; an incidental developer
cache is not clean-checkout evidence.

Windows setup also prepares the exact reviewed Microsoft WebView2 Evergreen
Standalone Installer in a task-owned ignored cache. The online pass may download
only the immutable resolved Microsoft GUID URL pinned by
`packaging/security/installer-toolchain-provenance.json`, then verifies its size,
SHA-256, version, and Authenticode signer. `setup -Offline` performs verification
only and fails if the cache is absent or changed. Packaging never downloads it.

The isolated README demo is restored during `setup`. Its `npm test` command does
not reinstall dependencies or hide a missing setup behind an implicit cache.

## Run the development surface

```powershell
./dev.ps1 dev
```

The canonical dev command always supplies
`packaging/windows/tauri.dev.conf.json`, so its Tauri identifier and
`app_config_dir` end in `.debug`. Debug development acknowledgement/state can
never read or write the production identifier's config directory. Review
installers use the separate `.review` identifier; production uses neither.

The current development surface evolves during the rewrite. Read command output and [IMPLEMENTATION_STATUS.md](../../IMPLEMENTATION_STATUS.md); a successful frontend preview is not evidence that native capture, hosted providers, or game integration works.

## Required checks

```powershell
./dev.ps1 lint
./dev.ps1 test
./scripts/security/scan-secrets.ps1 -IncludeUntracked
```

Use [testing and simulation](testing-and-simulation.md) for focused commands.

## Repository hygiene

The checkout began with widespread CRLF-only working-tree changes and legacy sensitive/unsafe artifacts. Do not normalize or delete unrelated material casually, load old pickles, run notebooks, or execute generated/per-character Python. Never add a real key to `apikeys.json`.

Keep all release/publishing actions local and inert unless explicit approval is given.
