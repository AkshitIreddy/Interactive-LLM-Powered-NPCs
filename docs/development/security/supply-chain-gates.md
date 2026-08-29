# Offline supply-chain gates

Release packaging uses committed inputs and fails closed. Its authoritative checks do not install tools or contact registries:

- `scan-secrets.ps1 -IncludeUntracked` scans non-ignored worktree files, index blobs (including tracked files missing from disk), and reachable Git history. Reports contain only a path, commit/source label, and rule.
- `check-licenses.ps1 -Strict` requires all four locks, an exact lock-to-license ledger, allowed license identifiers, and model-pack source/license provenance.
- `generate-sbom.ps1 -Strict` emits a deterministic CycloneDX lock inventory and source-evidence sidecar. The BOM covers the root and Tauri Cargo locks, pnpm lock, and README-demo npm lock.
- Source evidence records HEAD, dirty state, a deterministic tracked-plus-nonignored-untracked digest, and hashes for dependency locks, the game-profile corpus/schema, and provider catalog. Ignored credentials and build output are excluded; `.secrets` is also denied explicitly.

Run the focused offline tests with:

```powershell
python -m unittest discover -s scripts/security/tests -v
./scripts/security/scan-secrets.ps1 -IncludeUntracked
./scripts/security/check-licenses.ps1 -Strict
./scripts/security/generate-sbom.ps1 -Strict
```

`tools/security-tools.json` pins optional enrichment tools. They are never auto-installed and are used only when `-Enrich` is explicitly supplied. Absence of an optional enricher does not weaken the complete internal lock inventory.

`scripts/package.ps1 -SkipChecks` remains a deliberate developer escape hatch. Its manifest is permanently classified `unchecked-development-package`, with `immutable_release_candidate: false`; it cannot be treated as release-candidate evidence. A dirty checked package is classified `dirty-local-review`.

Refreshing `dependency-licenses.json` is a review operation, not a package step. After dependency changes, restore the exact locked packages, run `build_license_ledger.py` offline, inspect the diff and license exceptions, then rerun the strict checker. Unknown metadata or a stale/extra ledger entry fails.
