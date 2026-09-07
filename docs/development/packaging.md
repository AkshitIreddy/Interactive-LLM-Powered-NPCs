# Local Windows packaging

Packaging is review-only. It cannot publish, tag, release, sign, upload, or activate an update feed.

## Build

From a configured Windows x64 environment:

```powershell
./dev.ps1 package -Configuration Release
```

The command runs checks, requests an NSIS bundle from the pinned Tauri toolchain, copies the one fresh expected installer under `artifacts/package/<UTC timestamp>-<operation nonce>/`, and writes `package-manifest.json` with SHA-256 hashes. It clears only validated repository-owned NSIS output roots first, so stale installers cannot be attributed to the current build. `-SkipChecks` is unsuitable for an RC.

## Installer-free local review directory

The private review handoff can be built as one co-located directory without
running an installer or launching the desktop application:

```powershell
$env:NPC_LARGE_ARTIFACT_ROOT = 'E:\temp\InteractiveNPCs'
./scripts/windows/prepare-portable-review.ps1 `
  -DestinationRoot 'E:\temp\InteractiveNPCs\review-v18' `
  -StableTestGameDirectory `
    'E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game' `
  -ReviewedMouthAtlasReceiptPath `
    'E:\temp\InteractiveNPCs\review-mouth-atlas-v80-native-compatible\reviewed-artifact-receipt.v1.json' `
  -PrivateReviewModelCatalogDirectory `
    'E:\temp\InteractiveNPCs\private-review-catalog-yunet-20260907-r3'
```

Run the non-mutating preflight while source or vertical-slice work is still in
progress:

```powershell
./scripts/windows/prepare-portable-review.ps1 `
  -DestinationRoot 'E:\temp\InteractiveNPCs\review-v18' `
  -StableTestGameDirectory `
    'E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game' `
  -ReviewedMouthAtlasReceiptPath `
    'E:\temp\InteractiveNPCs\review-mouth-atlas-v80-native-compatible\reviewed-artifact-receipt.v1.json' `
  -PrivateReviewModelCatalogDirectory `
    'E:\temp\InteractiveNPCs\private-review-catalog-yunet-20260907-r3' `
  -PreflightOnly
```

The builder compiles a fresh Debug review-namespace shell with Tauri's
`--no-bundle` path, audits the four GUI-subsystem sidecars, stages the exact
legal/SBOM resource corpus, and copies the independently built, verified
six-file synthetic-game package from
`E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game`.
That closed world contains one hash-bound project-owned `.inpcseq` camera
sequence and no codec/runtime binary. The separate two-file
`review-mouth-atlas` payload is accepted only through
`-ReviewedMouthAtlasReceiptPath`. The receipt binds the exact atlas and texture
sizes and SHA-256 values, supported schema/representation pair, identity
revision, game profile, character, enrollment status, and honest quality
boundary. It is copied to `review-evidence` while only the two bound payload
files enter the runtime atlas directory. The existing v80 receipt classifies
the exact Eclipse Harbor/Mara atlas as a legacy private fixture with natural
quality still open. It cannot be relabeled for Misty or another character. A
schema 4 receipt must use `normalized-oral-strip-v1` and include a
`reviewed-private` semantic enrollment plus matching review-evidence digest;
the package check does not qualify an unreviewed atlas. The
pipeline also copies the consolidated engineering
review into `review-evidence`, rewriting its source-relative links to absolute
checkout paths before binding the result in the outer manifest. It scans the
current tracked and untracked source
without traversing the known legacy history, records the complete dirty-file
manifest and content hashes, and never starts an executable. It refuses to
replace an existing destination.

`-MouthAtlasDirectory` remains as a compatibility input for older automation.
It resolves `reviewed-artifact-receipt.v1.json` inside that directory and then
runs the same receipt validation. Passing it together with
`-ReviewedMouthAtlasReceiptPath` is rejected. Run the receipt contract checks
without creating or deleting files using:

```powershell
./scripts/windows/test-reviewed-mouth-atlas-receipt.ps1
```

After a character-specific atlas has a reviewed-private receipt, pass one or
more receipt paths through `-ReviewedCharacterMouthPackReceiptPaths`. Each pack
is staged as an independent three-file closed world under
`review-character-mouth-packs/<game>/<character>`. Duplicate identities, a
duplicate of the main fixture atlas, legacy receipts, unreviewed enrollment,
and missing or extra files are rejected. These files are supplied for the
ordinary character-workspace import flow; packaging records `enabled: false`
and does not activate or qualify them.

`-PrivateReviewModelCatalogDirectory` is an opt-in local-review overlay. The
builder accepts only the independently verified YuNet v17 receipt and its exact
four-file loader closed world: the review trust root, signed catalog, canonical
YuNet manifest, and signed current-device measurement envelope. It places those
files at `packaging/model-packs`, where the app's normal catalog resolver reads
them, instead of merging them with the repository catalog. The verification
receipt and import context remain under `review-evidence`; model weights,
runtime libraries, signing material, installed state, and activation state are
not copied. The overlay is two-of-two signed, has production trust disabled,
requires rotation before release, and supports neither promotion nor
publication. Omit the parameter to stage the checked repository bootstrap
metadata.

Independently verify the closed-world file and directory lists, hashes,
alternate-data-stream absence, PE subsystem, blocked model/runtime payloads,
source identity, private-catalog boundary, and synthetic-game provenance with:

```powershell
./scripts/windows/verify-portable-review.ps1 `
  -Directory 'E:\temp\InteractiveNPCs\review-v18'
```

This is unsigned, installer-free, local-review evidence. It is not installed
distribution, desktop presentation, live-game, signing, or release evidence.

Review and release overlays disable Tauri's inherited bare-Corepack
`beforeBuildCommand`. `package.ps1` builds the frontend once through the exact
hidden `node.exe + corepack.js` invocation, or a resolved checked
`corepack.cmd` fallback, before Tauri starts. The extensionless/POSIX Corepack
shim is never an allowed packaging input.

Packaging also refuses to start while the local-review evidence report and the
authoritative original-brief ledger disagree on any R01-R40 or SC01-SC12
status. Regenerate evidence from the current source/artifact paths instead of
carrying a historical `PASS` into a new candidate.

Before a source freeze, record the exact local native toolchain:

```powershell
./scripts/toolchain-evidence.ps1 `
  -RequireNative `
  -OutputPath ./artifacts/toolchain-evidence.json
./scripts/test-toolchain-evidence.ps1
```

This proves which Node, Rust, CMake, Visual Studio/MSVC and Windows SDK binaries
were used. It also verifies the exact Microsoft-signed Evergreen Standalone
Installer cached by `dev.ps1 setup`. Packaging disables Tauri's mutable fwlink
route and embeds those reviewed bytes through a generated fail-closed NSIS hook;
it performs no WebView acquisition. This still does not claim a bit-for-bit
reproducible installer: native compiler/linker and NSIS outputs are not configured
for identical bytes across independent environments.

Prepare or verify the isolated cache before packaging:

```powershell
./scripts/dev.ps1 setup          # immutable resolved URL, then exact verification
./scripts/dev.ps1 setup -Offline # verification only; missing cache fails
./scripts/test-webview2-offline-installer.ps1
```

## Base-installer boundary

The base package must not contain model weights, Python, CUDA, FFmpeg, secrets, signing keys, public endpoints, or fabricated trust roots. `packaging/runtime/layout.json` describes the allowed runtime layout; optional packs are separate verified downloads.

Review and Release overlays bundle exactly four audited x64 GUI-subsystem
children: `npc-runtime`, `npc-media-broker`, `npc-mouth-worker`, and
`npc-subtitle-presenter`. All C++ children are Release-built even for the Debug
review app and are rejected if they import a Debug CRT. The runtime is built
without the qualification-only `dev-wasapi-audio` feature; broker audio is the
ordinary product route. The source-test Tauri config intentionally keeps only
the two always-available children so a clean checkout does not require staged
native outputs before ordinary nested-workspace checks.

The Debug review game is an independent host-local fixture, never an NSIS
payload. `scripts/windows/prepare-review-test-game.ps1` compiles the project-owned
Windows GUI source with the exact Roslyn 9.0.302 host/compiler and .NET Framework
reference hashes declared in `scripts/synthetic-game-replay/SOURCE-MANIFEST.json`.
The build runs twice and fails unless both executable hashes are identical. The
target generates moving GDI frames and loop-safe PCM in memory; it bundles no
video, codec, model, voice, captured game material, or third-party binary.

The prepared folder contains exactly six files. `REVIEW-FIXTURE-MANIFEST.json`
enumerates every other file with SHA-256, component ID, SPDX expression,
distribution scope/class, source reference, and notice reference, and records
its explicit circular-self-hash exclusion. `review-test-game.cdx.json` and
`THIRD-PARTY-NOTICES.md` are the SBOM/notices handoff. Run
`scripts/windows/verify-review-test-game.ps1` before smoke testing; it rejects
unknown files, directories, reparse points, unsafe/duplicate paths, source or
license drift, file tampering, and any third-party-binary declaration.

## Security artifacts

Before an RC:

```powershell
./scripts/security/scan-secrets.ps1 -IncludeUntracked
./scripts/security/generate-sbom.ps1 -Strict
./scripts/security/check-licenses.ps1 -Strict
```

Verify package files against the generated digest list on clean Windows 10/11 VMs without developer toolchains. Exercise install, first run, repair, rollback, uninstall, offline mode, and no-orphan-process behavior.

Packaging additionally resolves the actual root Cargo, nested Tauri Cargo, and
production pnpm graphs with locked offline commands. It collects the exact
LICENSE/COPYING/NOTICE bytes for every included component, applies only reviewed
hash-pinned upstream-license overrides, and binds the artifact scope and license
index into CycloneDX. Tauri installs that corpus, the source/component ledger,
subtitle policy, runtime metadata, and exact sidecar/resource manifests beneath
`product-audit/`. Missing inputs or an unreviewed license gap abort packaging.

The package manifest is deliberately `pre-reconciliation-local-review` with
`immutable_release_candidate: false` and package-level
`promotion_supported: false`. Pre-build resource staging proves intent, not
the contents of an NSIS executable. After source freeze, the opt-in installer
smoke installs the exact package into an isolated root, constructs a
deterministic closed-world manifest, invokes the distribution reconciler, and
repeats the proof after reinstall. It rejects missing/tampered/extra files,
Debug CRTs, blocked components, reparse points, incomplete legal resources, or
artifact-scope/SBOM/license divergence. Only a fully clean lifecycle result has
`promotion_eligible: true`, and only when the installed/reinstalled model
catalog also declares production trust, completed rotation, and explicit
promotion/publication support. A bootstrap trust root remains local-review
only. The original immutable package manifest remains a truthful
pre-reconciliation record.

## Signing and updates

The repository contains only inert policy/configuration placeholders. Production signing identity, updater trust root, endpoint, and feed activation require a separate review and explicit approval. A local unsigned RC must say so; users must never be told to ignore an unexpected security warning.

See `packaging/README.md` and `packaging/security/release-policy.json` for the machine-readable boundary.
