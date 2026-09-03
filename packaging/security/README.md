# Release security and provenance artifacts

These files define a fail-closed local-review boundary. They do not authorize publication.

- `dependency-licenses.json` is the manually reviewed SPDX ledger for the exact merged purl set from all four committed lockfiles.
- `license-policy.json` is the canonical source-license policy; its `cargo_deny_allow` array must exactly match `deny.toml`.
- `distribution-components.json` separates source dependencies from redistributed files, statically embedded native code, installer-native binaries, install-time acquisitions, system dependencies, hosted APIs, optional user downloads, fonts, and development-only GPL tooling.
- `license-material-overrides.json` permits hash-pinned legal bodies only for exact packages whose published archives omit the body needed by the collector. Bodies come from each package's pinned primary-source revision; the sole standard-text exception is the exact `selectors@0.36.1` MPL-2.0 declaration whose pinned tree has no body, and it uses Mozilla's official MPL-2.0 text. It is not a generic SPDX-text fallback.
- `installer-toolchain-provenance.json` pins Tauri CLI 2.11.4, NSIS 3.11 binary and corresponding-source archives, every native NSIS file selected by Tauri, and `nsis-tauri-utils` 0.5.3 with exact license bodies. Generated installer/uninstaller hashes remain release-manifest evidence.
- `release-policy.json` makes installed legal resources, extracted-file reconciliation, artifact SHA-256, source identity, and Debug CRT exclusion mandatory.

Offline strict validation:

```powershell
./scripts/security/check-licenses.ps1 -Strict
./scripts/security/generate-sbom.ps1 -Strict -OutputDirectory ./artifacts/sbom
```

Direct cross-platform validation used by tests:

```text
python3 scripts/security/check_lock_licenses.py --root .
python3 scripts/security/generate_artifact_scope.py --root . --artifact-id windows-review-installer --cargo-tree <root-tree.txt> --cargo-tree <tauri-tree.txt> --pnpm-list-json <pnpm-production.json> --out <audit>/windows-artifact-scope.json
python3 scripts/security/collect_artifact_licenses.py --root . --scope <audit>/windows-artifact-scope.json --cargo-metadata <root-metadata.json> --cargo-metadata <tauri-metadata.json> --node-modules node_modules --out <audit>/legal/packages
python3 scripts/security/generate_lock_sbom.py --strict --require-artifact-scope --require-license-materials --root . --artifact-scope <audit>/windows-artifact-scope.json --license-material-index <audit>/legal/packages/THIRD-PARTY-LICENSE-FILES.json --out <audit>/legal/lockfiles.cdx.json
python3 -m unittest scripts.security.tests.test_security_tools
```

Reconcile an extracted distribution only after packaging has emitted its closed-world file manifest:

```text
python3 scripts/security/reconcile_distribution.py --kind installer --root <extracted-app> --manifest <extracted-app>/release-files.json
python3 scripts/security/reconcile_distribution.py --kind test-game --root <prepared-test-game> --manifest <prepared-test-game>/REVIEW-FIXTURE-MANIFEST.json
```

The manifest must enumerate and hash every file except itself, using the explicit `manifest_self` circularity exclusion. Each entry requires `path`, lowercase SHA-256, known `component_id`, matching `spdx`, `distribution_scope`, `source_reference`, and `notice_reference`. Unknown files, path traversal, symlinks, missing files, hash mismatches, forbidden components, or Debug CRT import markers fail the check.

The SBOM retains the complete source-lock inventory but marks only packages observed in the resolved normal build graphs as `required`; all other locked packages are `excluded` for that artifact. The license-material collector copies each required package's own license, copying, notice, copyright, or unlicense file and fails if even one required purl lacks exact material. Twelve exact crate overrides are reviewed: Nugine SIMD (2), Dropbox alloc-stdlib (1), Servo selectors (1), rust-unic (5), and webview2-rs (3). Every mapping checks its declared SPDX expression, source revision, packaged VCS metadata hash/path, original Cargo metadata hash, optional README hash, and every vendored body hash. The rust-unic copyright record has only three reversible relative-link-target rewrites; validation reconstructs the exact upstream bytes, and collection fails unless all four linked companion bodies are present. The selectors exception is additionally restricted to Mozilla's official MPL-2.0 standard text because its pinned source tree contains no body. The final extracted-file manifest is still the evidence of what actually shipped.

The release sets Tauri WebView mode to `skip` and uses a `customPinnedOfflineInstaller` NSIS hook, avoiding the locked Tauri `offlineInstaller` code path's unconditional mutable network HEAD. The hook embeds a reviewed x64 Evergreen Standalone Installer from an isolated cache. Its version, resolved Microsoft URL, size, SHA-256, and valid Microsoft Authenticode signer are pinned in `installer-toolchain-provenance.json`; package staging must reject any differing or package-time-downloaded bytes. The resulting Evergreen Runtime is installed and serviced under Microsoft terms, not treated as a project-owned lockfile library.
