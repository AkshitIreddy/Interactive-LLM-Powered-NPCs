# Local packaging boundary

This directory defines the review-only Windows packaging boundary for the 2.0 application. It does not contain signing credentials, public URLs, update keys, model binaries, Python, CUDA, FFmpeg, or publish automation.

Build locally from a configured Windows x64 shell:

```powershell
.\scripts\dev.ps1 package -Configuration Release
```

The command runs lint and tests, asks the pinned Tauri CLI for an NSIS bundle, copies the result into `artifacts/package/<UTC timestamp>/`, and writes SHA-256 hashes. It never uploads, signs, tags, releases, or activates an updater.

## Boundaries

- `windows/` is a Tauri 2 release-config overlay and inert NSIS hooks.
- `runtime/` specifies files allowed in the small base installer.
- `model-packs/` specifies independently downloaded, immutable model packs.
- `updater/` documents the deliberately inactive local update-feed placeholder.
- `tuf/` reserves the future signed metadata boundary without shipping fabricated trust roots.
- `security/` records release policy; executable checks live under `scripts/security/`.

Any production signing identity, updater public key, endpoint, or public distribution workflow requires a separate reviewed change and explicit user approval.

## Installer evidence

The opt-in installer harness records two immutable observations: a clean pre-install snapshot and a post-uninstall snapshot taken before any emergency cleanup. Release review must require all four post-uninstall assertions—no remaining processes, install files, shortcuts, or registry entries—to be true. Cleanup activity is a separate audit object and cannot repair a failed result.

The broker is a persistent authenticated service, not a standalone executable smoke. Installer acceptance verifies its installed SHA-256, then observes it and the runtime as direct children of the installed Response Console for the bounded launch. Both process paths, hashes, parent IDs, sustained presence, and disappearance after forced parent termination are recorded. Missing broker integration is reported as `broker_supervision_observed=false` and fails closed.

Debug installers expose one fixed, presence-only health probe when launched with `NPC2_INSTALLER_SMOKE=1`. The app itself performs the authenticated runtime ping and broker health/diagnostics calls, atomically writes `installer-smoke-health-v1.json` beneath its protected config directory, and accepts no output path or content from the harness. Installer acceptance requires authenticated/connected states, exact observed child PIDs, non-fixture backends, and protocol versions from this app-owned path. The harness removes only that fixed probe after capturing evidence.

After the installed runtime doctor creates its SQLite data directory, the harness requires `app_data_acl_private=true`: the Windows DACL must be protected, contain no inherited ACEs, and name only the current account, SYSTEM, and Administrators. Evidence contains principal names only; an identity that cannot be safely translated is recorded as `unresolved-principal`, never as a raw SID.

Each result binds the tested package manifest to the local Git HEAD and dirty state where available, plus SHA-256 values for both lockfiles, the deterministic profile corpus, provider catalog, model-pack example, and Tauri configuration. A dirty checkout is recorded rather than silently described as a reproducible commit build.

## NSIS hook configuration

`scripts/package.ps1` passes `packaging/windows/tauri.release.conf.json` to the Tauri CLI as a release overlay. Tauri resolves `bundle.windows.nsis.installerHooks` from the application config base at `apps/control/src-tauri`, so the committed value is exactly `../../../packaging/windows/nsis/installer-hooks.nsh`.

The current-user installer records its install directory as the default value of `HKCU\Software\github\Interactive NPCs Response Console`. The post-uninstall hook reads that exact value and compares it to the active `$INSTDIR`. Only on an exact match does it delete the owned default value; it removes the product and `github` parent keys only with NSIS `/ifempty`. It never mutates HKLM, a mismatched product key, sibling vendor products, or non-default values. `packaging/tests/nsis-registry-symmetry.ps1` locks this install/uninstall symmetry and path resolution.
