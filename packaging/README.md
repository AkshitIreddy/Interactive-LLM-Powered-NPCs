# Local packaging boundary

This directory defines the review-only Windows packaging boundary for the 2.0 application. It does not contain signing credentials, public URLs, update keys, model binaries, Python, CUDA, FFmpeg, or publish automation.

Build locally from a configured Windows x64 shell:

```powershell
.\scripts\dev.ps1 package -Configuration Release
```

The command runs lint and tests, asks the pinned Tauri CLI for an NSIS bundle, requires exactly one fresh expected installer, copies it into `artifacts/package/<UTC timestamp>-<operation nonce>/`, and writes SHA-256 hashes. It never uploads, signs, tags, releases, or activates an updater.

## Boundaries

- `windows/` is a Tauri 2 release-config overlay and inert NSIS hooks.
- `runtime/` specifies files allowed in the small base installer.
- `model-packs/` specifies independently downloaded, immutable model packs.
- `updater/` documents the deliberately inactive local update-feed placeholder.
- `tuf/` reserves the future signed metadata boundary without shipping fabricated trust roots.
- `security/` records release policy; executable checks live under `scripts/security/`.

The Review and Release overlays contain exactly four x64 GUI-subsystem
sidecars: runtime, media broker, mouth worker, and subtitle presenter. The C++
sidecars are always Release-built and all four are audited for PE subsystem,
machine type, exact hash, and absence of Debug CRT imports. Product runtime uses
broker audio; the direct WASAPI qualification feature is never enabled by the
package pipeline.

The installer also carries the exact artifact-scoped legal corpus and its
binding evidence under `product-audit/`: source/component ledger, scope,
CycloneDX SBOM, every included dependency license/notice body, reviewed pinned
license overrides, subtitle policy, ONNX Runtime metadata, and sidecar/resource
hash manifests. Any missing, unmapped, or changed item aborts staging.

Any production signing identity, updater public key, endpoint, or public distribution workflow requires a separate reviewed change and explicit user approval.

Product overlays disable the base Tauri `beforeBuildCommand`. The package
pipeline builds frontend assets once through its exact hidden Node/Corepack
resolver, rejecting the extensionless/POSIX shim, before invoking Tauri.

## Installer evidence

`package.ps1` emits only pre-reconciliation local-review evidence, always sets
`immutable_release_candidate=false`, and records package-level
`promotion_supported=false`. Staged inputs are not proof of what an NSIS
executable installed. After source freeze, the opt-in smoke performs two
isolated installs, emits a deterministic closed-world installed manifest, and
invokes the distribution reconciler before app launch and again after
reinstall. Only exact four-sidecar hashes, legal/SBOM/scope agreement, no
blocked or unclassified file, supervised runtime/broker lifecycle, and clean
uninstall can produce separate `promotion_eligible=true` smoke evidence. The
installed and reinstalled model-catalog root must also declare
`production_release` trust, completed key rotation, and explicit
promotion/publication support; the automated local-review bootstrap root is
always promotion-ineligible even when every local smoke assertion passes.

The opt-in installer harness records two immutable observations: a clean pre-install snapshot and a post-uninstall snapshot taken before any emergency cleanup. Release review must require all four post-uninstall assertions—no remaining processes, install files, shortcuts, or registry entries—to be true. Cleanup activity is a separate audit object and cannot repair a failed result.

The broker is a persistent authenticated service, not a standalone executable smoke. Installer acceptance verifies its installed SHA-256, then observes it and the runtime as direct children of the installed Response Console for the bounded launch. Both process paths, hashes, parent IDs, sustained presence, and disappearance after forced parent termination are recorded. Missing broker integration is reported as `broker_supervision_observed=false` and fails closed.

Debug local-review installers use the separate `io.github.akshitireddy.interactive-npcs.review` identifier; Release continues to use the production identifier. Debug installers expose one fixed, presence-only health probe when launched with `NPC2_INSTALLER_SMOKE=1`. The app itself performs the authenticated runtime ping and broker health/diagnostics calls, atomically writes `installer-smoke-health-v1.json` beneath the isolated protected review config directory, and accepts no output path or content from the harness. Installer acceptance requires authenticated/connected states, exact observed child PIDs, non-fixture backends, protocol versions, a rendered first-run onboarding overlay observed through a bounded loopback WebView inspection session, and an advancing frame plus generated PCM from the standalone project-owned synthetic game. That fixture is outside the installer and carries an exact five-file allowlist, SHA-256 source/license/build provenance, MIT notice, and CycloneDX handoff; it contains no FFmpeg, video, model, captured game content, or third-party binary. The harness removes the task-owned review namespace after capturing evidence and proves the production namespace digest did not change.

After the installed runtime doctor creates its SQLite data directory, the harness requires `app_data_acl_private=true`: the Windows DACL must be protected, contain no inherited ACEs, and name only the current account, SYSTEM, and Administrators. Evidence contains principal names only; an identity that cannot be safely translated is recorded as `unresolved-principal`, never as a raw SID.

Each result binds the tested package manifest to the local Git HEAD and dirty state where available, plus SHA-256 values for both lockfiles, the deterministic profile corpus, provider catalog, model-pack example, and Tauri configuration. A dirty checkout is recorded rather than silently described as a reproducible commit build.

## NSIS hook configuration

`scripts/package.ps1` passes `packaging/windows/tauri.release.conf.json` to the Tauri CLI as a release overlay. Both packaging overlays require `generated-installer-inputs/installer-hooks.generated.nsh`. Packaging creates that ignored wrapper only after verifying the exact reviewed Microsoft-signed WebView2 standalone installer, and the wrapper includes the committed `packaging/windows/nsis/installer-hooks.nsh` source. Tauri uses `webviewInstallMode.type=skip`, so its mutable fwlink resolver cannot run.

The current-user installer records its install directory as the default value of `HKCU\Software\github\Interactive NPCs Response Console`. The post-uninstall hook reads that exact value and compares it to the active `$INSTDIR`. Only on an exact match does it delete the owned default value; it removes the product and `github` parent keys only with NSIS `/ifempty`. It never mutates HKLM, a mismatched product key, sibling vendor products, or non-default values. `packaging/tests/nsis-registry-symmetry.ps1` locks this install/uninstall symmetry and path resolution.
