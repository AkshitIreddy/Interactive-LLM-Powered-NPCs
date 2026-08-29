# Local Windows packaging

Packaging is review-only. It cannot publish, tag, release, sign, upload, or activate an update feed.

## Build

From a configured Windows x64 environment:

```powershell
./dev.ps1 package -Configuration Release
```

The command runs checks, requests an NSIS bundle from the pinned Tauri toolchain, copies output under `artifacts/package/<UTC timestamp>/`, and writes `package-manifest.json` with SHA-256 hashes. `-SkipChecks` is unsuitable for an RC.

## Base-installer boundary

The base package must not contain model weights, Python, CUDA, FFmpeg, secrets, signing keys, public endpoints, or fabricated trust roots. `packaging/runtime/layout.json` describes the allowed runtime layout; optional packs are separate verified downloads.

## Security artifacts

Before an RC:

```powershell
./scripts/security/scan-secrets.ps1 -IncludeUntracked
./scripts/security/generate-sbom.ps1 -Strict
./scripts/security/check-licenses.ps1 -Strict
```

Verify package files against the generated digest list on clean Windows 10/11 VMs without developer toolchains. Exercise install, first run, repair, rollback, uninstall, offline mode, and no-orphan-process behavior.

## Signing and updates

The repository contains only inert policy/configuration placeholders. Production signing identity, updater trust root, endpoint, and feed activation require a separate review and explicit approval. A local unsigned RC must say so; users must never be told to ignore an unexpected security warning.

See `packaging/README.md` and `packaging/security/release-policy.json` for the machine-readable boundary.
