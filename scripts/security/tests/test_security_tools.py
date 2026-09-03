from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SECURITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SECURITY_DIR))
from lock_inventory import InventoryError, load_inventory  # noqa: E402
from reconcile_distribution import ReconciliationError, reconcile, validate_installer_legal_materials  # noqa: E402
from check_lock_licenses import ExpressionError, expression_is_allowed, is_model_pack_manifest, validate_installer_toolchain_provenance, validate_license_material_overrides  # noqa: E402
from collect_artifact_licenses import override_sources  # noqa: E402
from generate_source_evidence import windows_unaddressable  # noqa: E402


class SourceEvidenceTests(unittest.TestCase):
    def test_windows_reserved_device_paths_are_rejected(self) -> None:
        for relative in ("NUL", "nul.txt", "nested/CON", "nested/com1.log"):
            with self.subTest(relative=relative):
                self.assertTrue(windows_unaddressable(relative))
        for relative in ("NULL", "console.txt", "nested/component.log"):
            with self.subTest(relative=relative):
                self.assertFalse(windows_unaddressable(relative))


class InstallerToolchainProvenanceTests(unittest.TestCase):
    def test_reviewed_nsis_tauri_and_webview_pins_match_the_repository(self) -> None:
        repository_root = Path(__file__).resolve().parents[3]
        self.assertEqual(3, validate_installer_toolchain_provenance(repository_root))


class LockInventoryTests(unittest.TestCase):
    def fixture(self) -> Path:
        root = Path(tempfile.mkdtemp(prefix="npc-security-locks-", dir="/tmp" if Path("/tmp").is_dir() else None))
        (root / "apps/control/src-tauri").mkdir(parents=True)
        (root / "demo/readme").mkdir(parents=True)
        (root / "Cargo.lock").write_text('version = 4\n[[package]]\nname = "root-crate"\nversion = "1.0.0"\nchecksum = "' + "a" * 64 + '"\n')
        (root / "apps/control/src-tauri/Cargo.lock").write_text('version = 4\n[[package]]\nname = "tauri-crate"\nversion = "2.0.0"\nchecksum = "' + "b" * 64 + '"\n')
        (root / "pnpm-lock.yaml").write_text("lockfileVersion: '9.0'\npackages:\n  alpha@1.0.0:\n    resolution:\n      integrity: sha512-YQ==\nsnapshots:\n")
        package_lock = {
            "name": "demo", "lockfileVersion": 3,
            "packages": {"": {"name": "demo", "version": "1.0.0"}, "node_modules/bravo": {"version": "2.0.0", "integrity": "sha512-Yg==", "license": "MIT"}},
        }
        (root / "demo/readme/package-lock.json").write_text(json.dumps(package_lock))
        return root

    def test_every_lock_contributes_components_and_source_properties(self) -> None:
        root = self.fixture()
        components, sources = load_inventory(root)
        self.assertEqual([item[0] for item in __import__("lock_inventory").LOCK_SPECS], sources)
        observed = {prop["value"] for component in components for prop in component["properties"] if prop["name"] == "interactive-npcs:source-lock"}
        self.assertEqual(set(sources), observed)
        self.assertEqual({"root-crate", "tauri-crate", "alpha", "bravo"}, {item["name"] for item in components})

    def test_missing_lock_fails_closed(self) -> None:
        root = self.fixture()
        (root / "pnpm-lock.yaml").unlink()
        with self.assertRaises(InventoryError):
            load_inventory(root)

    def test_unparsed_package_key_fails_closed(self) -> None:
        root = self.fixture()
        (root / "pnpm-lock.yaml").write_text("lockfileVersion: '9.0'\npackages:\n  malformed-key:\n    resolution: {}\nsnapshots:\n")
        with self.assertRaises(InventoryError):
            load_inventory(root)

    def test_sbom_is_byte_deterministic(self) -> None:
        root = self.fixture()
        first, second = root / "first.json", root / "second.json"
        for output in (first, second):
            subprocess.run([sys.executable, str(SECURITY_DIR / "generate_lock_sbom.py"), "--root", str(root), "--out", str(output)], check=True, capture_output=True)
        self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_installer_sbom_excludes_external_optional_and_development_components(self) -> None:
        root = self.fixture()
        distribution = root / "distribution.json"
        base = {
            "name": "component", "spdx": "MIT", "distribution_class": "fixture",
            "hash_policy": "fixture", "source_reference": "source", "notice_reference": "notice",
        }
        distribution.write_text(json.dumps({
            "components": {
                "project:shipping": {**base, "redistributed": True, "scopes": ["base-installer"]},
                "external:runtime": {**base, "redistributed": False, "scopes": ["external-system"]},
                "optional:model": {**base, "redistributed": False, "scopes": ["optional-user-download"]},
                "development:tool": {**base, "redistributed": False, "scopes": ["development-only"]},
            },
            "static_files": [],
            "distribution_profiles": {"installer": {"scope": "base-installer"}, "test-game": {"scope": "review-test-game"}},
        }), encoding="utf-8")
        output = root / "installer.json"
        subprocess.run([
            sys.executable, str(SECURITY_DIR / "generate_lock_sbom.py"), "--root", str(root),
            "--distribution-ledger", str(distribution), "--out", str(output),
        ], check=True, capture_output=True)
        bom = json.loads(output.read_text())
        by_id = {
            next(prop["value"] for prop in item["properties"] if prop["name"] == "interactive-npcs:component-id"): item
            for item in bom["components"] if item["bom-ref"].startswith("urn:interactive-npcs:distribution-component:")
        }
        self.assertEqual("required", by_id["project:shipping"]["scope"])
        self.assertEqual("excluded", by_id["external:runtime"]["scope"])
        root_dependencies = next(item["dependsOn"] for item in bom["dependencies"] if item["ref"].startswith("pkg:generic/"))
        self.assertIn(by_id["project:shipping"]["bom-ref"], root_dependencies)
        for component_id in ("external:runtime", "optional:model", "development:tool"):
            self.assertNotIn(by_id[component_id]["bom-ref"], root_dependencies)

    def test_artifact_scope_and_exact_license_materials_are_path_portable(self) -> None:
        root = self.fixture()
        tree = root / "runtime.tree.txt"
        tree.write_text("root-crate v1.0.0\n", encoding="utf-8")
        scope = root / "runtime.scope.json"
        subprocess.run([
            sys.executable, str(SECURITY_DIR / "generate_artifact_scope.py"),
            "--root", str(root), "--artifact-id", "fixture-runtime", "--cargo-tree", str(tree), "--out", str(scope),
        ], check=True, capture_output=True)
        source = root / "registry/root-crate-1.0.0"
        source.mkdir(parents=True)
        (source / "Cargo.toml").write_text('[package]\nname="root-crate"\nversion="1.0.0"\n', encoding="utf-8")
        (source / "LICENSE-MIT").write_text("fixture MIT body", encoding="utf-8")
        metadata = root / "metadata.json"
        metadata.write_text(json.dumps({"packages": [{"name": "root-crate", "version": "1.0.0", "manifest_path": str(source / "Cargo.toml")}]}), encoding="utf-8")
        ledger = root / "ledger.json"
        ledger.write_text(json.dumps({"components": {"pkg:cargo/root-crate@1.0.0": "MIT"}}), encoding="utf-8")
        output = root / "license-materials"
        subprocess.run([
            sys.executable, str(SECURITY_DIR / "collect_artifact_licenses.py"),
            "--root", str(root), "--scope", str(scope), "--cargo-metadata", str(metadata),
            "--license-ledger", str(ledger), "--out", str(output),
        ], check=True, capture_output=True)
        scope_document = json.loads(scope.read_text())
        index = json.loads((output / "THIRD-PARTY-LICENSE-FILES.json").read_text())
        self.assertEqual("runtime.tree.txt", scope_document["inputs"][0]["file"])
        self.assertEqual(["pkg:cargo/root-crate@1.0.0"], list(index["components"]))
        self.assertFalse(Path(index["components"]["pkg:cargo/root-crate@1.0.0"][0]["path"]).is_absolute())

    def test_override_sources_require_exact_crate_linkage_and_all_bodies(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="npc-license-override-", dir="/tmp" if Path("/tmp").is_dir() else None))
        package = root / "registry/example-1.0.0"
        package.mkdir(parents=True)
        revision = "a" * 40
        vcs = package / ".cargo_vcs_info.json"
        vcs.write_text(json.dumps({"git": {"sha1": revision}, "path_in_vcs": "crates/example"}), encoding="utf-8")
        cargo_orig = package / "Cargo.toml.orig"
        cargo_orig.write_text('[package]\nname = "example"\nversion = "1.0.0"\nlicense = "MIT OR Apache-2.0"\n', encoding="utf-8")
        body_mit = root / "MIT.txt"
        body_apache = root / "APACHE-2.0.txt"
        notice = root / "COPYRIGHT.md"
        body_mit.write_text("exact MIT body", encoding="utf-8")
        body_apache.write_text("exact Apache body", encoding="utf-8")
        notice.write_text("See [MIT](MIT.txt)", encoding="utf-8")
        files = {
            "mit": {"path": "MIT.txt", "sha256": hashlib.sha256(body_mit.read_bytes()).hexdigest()},
            "apache": {"path": "APACHE-2.0.txt", "sha256": hashlib.sha256(body_apache.read_bytes()).hexdigest()},
            "notice": {
                "path": "COPYRIGHT.md",
                "sha256": hashlib.sha256(notice.read_bytes()).hexdigest(),
                "markdown_link_rewrites": [{"upstream": "LICENSE-MIT", "vendored": "MIT.txt"}],
            },
        }
        override = {
            "declared_spdx": "MIT OR Apache-2.0",
            "crate_source_revision": revision,
            "vcs_path": "crates/example",
            "crate_vcs_info_sha256": hashlib.sha256(vcs.read_bytes()).hexdigest(),
            "cargo_toml_orig_sha256": hashlib.sha256(cargo_orig.read_bytes()).hexdigest(),
            "license_file_ids": ["mit", "apache", "notice"],
        }
        self.assertEqual(
            [body_mit, body_apache, notice],
            override_sources(package, "pkg:cargo/example@1.0.0", "MIT OR Apache-2.0", override, files, root),
        )
        override["license_file_ids"] = ["apache", "notice"]
        with self.assertRaisesRegex(ValueError, "unresolved collected Markdown targets"):
            override_sources(package, "pkg:cargo/example@1.0.0", "MIT OR Apache-2.0", override, files, root)
        override["license_file_ids"] = ["mit", "apache", "notice"]
        vcs.write_text(json.dumps({"git": {"sha1": "b" * 40}, "path_in_vcs": "crates/example"}), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "VCS linkage differs"):
            override_sources(package, "pkg:cargo/example@1.0.0", "MIT OR Apache-2.0", override, files, root)


class SecretScannerTests(unittest.TestCase):
    def repo(self) -> Path:
        root = Path(tempfile.mkdtemp(prefix="npc-secret-fixture-", dir="/tmp" if Path("/tmp").is_dir() else None))
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.email", "security-fixture@example.invalid"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.name", "Security Fixture"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "maintenance.auto", "false"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "gc.auto", "0"], check=True)
        return root

    def scan(self, root: Path, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, str(SECURITY_DIR / "scan_secrets.py"), "--root", str(root), *args], text=True, capture_output=True)

    def test_provider_canaries_and_hashes_do_not_false_positive(self) -> None:
        root = self.repo()
        (root / "safe.md").write_text("openai canary sk-canary-ABCDEFGHIJKLMNOPQRSTUVWXYZ\nelevenlabs canary sk_canary_ABCDEFGHIJKLMNOPQRSTUVWXYZ\nsha256 = " + "a" * 64 + "\nassemblyai placeholder = REDACTED\n")
        subprocess.run(["git", "-C", str(root), "add", "safe.md"], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "safe canaries"], check=True)
        result = self.scan(root, "--include-untracked")
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)

    def test_reachable_history_reports_only_file_commit_and_rule(self) -> None:
        root = self.repo()
        synthetic = "cohere-fixture-A7b9C2d4E6f8G1h3J5k7L9m2N4p6Q8s"
        (root / "apikeys.json").write_text(json.dumps({"cohere_api_key": synthetic}))
        subprocess.run(["git", "-C", str(root), "add", "apikeys.json"], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "historical fixture"], check=True)
        (root / "apikeys.json").unlink()
        subprocess.run(["git", "-C", str(root), "add", "-u"], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-qm", "remove fixture"], check=True)
        result = self.scan(root)
        self.assertEqual(1, result.returncode)
        self.assertIn("apikeys.json", result.stdout)
        self.assertIn("blocked credential filename", result.stdout)
        self.assertNotIn(synthetic, result.stdout)

    def test_tracked_missing_file_is_scanned_from_index(self) -> None:
        root = self.repo()
        (root / "credentials.json").write_text('{"assemblyai_api_key":"fixture-A7b9C2d4E6f8G1h3J5k7L9m2N4p6"}')
        subprocess.run(["git", "-C", str(root), "add", "credentials.json"], check=True)
        (root / "credentials.json").unlink()
        result = self.scan(root, "--no-history")
        self.assertEqual(1, result.returncode)
        self.assertIn("credentials.json\tINDEX\tblocked credential filename", result.stdout)


class LicensePolicyTests(unittest.TestCase):
    def fixture(self) -> Path:
        root = LockInventoryTests().fixture()
        (root / "packaging/security").mkdir(parents=True)
        components, sources = load_inventory(root)
        ledger = {
            "schema_version": 1,
            "source_locks": sources,
            "components": {item["purl"]: "MIT" for item in components},
        }
        policy = {
            "schema_version": 1,
            "allowed_identifiers": ["MIT"],
            "disallowed_identifiers": ["GPL-3.0-or-later"],
            "reviewed_exceptions": {},
        }
        (root / "packaging/security/dependency-licenses.json").write_text(json.dumps(ledger))
        (root / "packaging/security/license-policy.json").write_text(json.dumps(policy))
        return root

    def check(self, root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, str(SECURITY_DIR / "check_lock_licenses.py"), "--root", str(root)], text=True, capture_output=True)

    def test_complete_internal_ledger_passes_without_external_tools(self) -> None:
        result = self.check(self.fixture())
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)

    def test_unknown_component_fails_closed(self) -> None:
        root = self.fixture()
        path = root / "packaging/security/dependency-licenses.json"
        ledger = json.loads(path.read_text())
        ledger["components"].pop(next(iter(ledger["components"])))
        path.write_text(json.dumps(ledger))
        result = self.check(root)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("does not exactly match locks", result.stderr)

    def test_disallowed_license_fails_closed(self) -> None:
        root = self.fixture()
        path = root / "packaging/security/dependency-licenses.json"
        ledger = json.loads(path.read_text())
        ledger["components"][next(iter(ledger["components"]))] = "GPL-3.0-or-later"
        path.write_text(json.dumps(ledger))
        result = self.check(root)
        self.assertEqual(1, result.returncode)
        self.assertIn("disallowed license", result.stdout)

    def test_permissive_or_branch_does_not_blanket_allow_restricted_identifier(self) -> None:
        self.assertEqual((True, set()), expression_is_allowed("MIT OR LGPL-2.1-or-later", {"MIT"}, {"LGPL-2.1-or-later"}))
        self.assertEqual((False, set()), expression_is_allowed("MIT AND LGPL-2.1-or-later", {"MIT"}, {"LGPL-2.1-or-later"}))

    def test_model_pack_provenance_check_excludes_catalog_and_review_documents(self) -> None:
        self.assertTrue(is_model_pack_manifest({"schema": "npc.model-pack/v2"}))
        self.assertFalse(is_model_pack_manifest({"schema": "npc.model-catalog-root/v1"}))
        self.assertFalse(is_model_pack_manifest({"schema": "npc.non-qualifying-model-review-evidence-index/v1"}))

    def test_pinned_upstream_license_override_hash_and_linkage(self) -> None:
        repo = SECURITY_DIR.parents[1]
        ledger = json.loads((repo / "packaging/security/dependency-licenses.json").read_text())["components"]
        self.assertEqual(12, validate_license_material_overrides(repo, ledger))

    def test_official_standard_override_is_exact_component_scoped(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="npc-official-license-", dir="/tmp" if Path("/tmp").is_dir() else None))
        legal = root / "docs/legal/licenses/MPL-2.0.txt"
        legal.parent.mkdir(parents=True)
        legal.write_text("official MPL fixture", encoding="utf-8")
        policy = {
            "schema_version": 1,
            "license_files": {
                "mpl": {
                    "path": "docs/legal/licenses/MPL-2.0.txt",
                    "sha256": hashlib.sha256(legal.read_bytes()).hexdigest(),
                    "source_kind": "official-standard-license-text",
                    "standard_id": "MPL-2.0",
                    "source_url": "https://www.mozilla.org/MPL/2.0/",
                    "source_revision": "MPL-2.0",
                }
            },
            "components": {
                "pkg:cargo/selectors@0.36.1": {
                    "declared_spdx": "MPL-2.0",
                    "license_file_ids": ["mpl"],
                    "crate_source_revision": "a" * 40,
                    "crate_vcs_info_sha256": "b" * 64,
                    "cargo_toml_orig_sha256": "c" * 64,
                }
            },
        }
        overrides = root / "packaging/security/license-material-overrides.json"
        overrides.parent.mkdir(parents=True)
        overrides.write_text(json.dumps(policy), encoding="utf-8")
        self.assertEqual(1, validate_license_material_overrides(root, {"pkg:cargo/selectors@0.36.1": "MPL-2.0"}))
        policy["license_files"]["mpl"]["standard_id"] = "MIT"
        overrides.write_text(json.dumps(policy), encoding="utf-8")
        with self.assertRaisesRegex(ExpressionError, "official standard license body does not match"):
            validate_license_material_overrides(root, {"pkg:cargo/selectors@0.36.1": "MPL-2.0"})


class DistributionReconciliationTests(unittest.TestCase):
    def fixture(self) -> tuple[Path, Path, Path]:
        root = Path(tempfile.mkdtemp(prefix="npc-distribution-", dir="/tmp" if Path("/tmp").is_dir() else None))
        (root / "payload.exe").write_bytes(b"project release payload")
        (root / "NOTICE.md").write_text("notice", encoding="utf-8")
        ledger = root.parent / f"{root.name}-ledger.json"
        ledger.write_text(json.dumps({
            "components": {
                "project:test": {"spdx": "MIT", "redistributed": True, "scopes": ["review-test-game"]},
                "blocked:test": {"spdx": "GPL-3.0-or-later", "redistributed": False, "scopes": ["development-only"]},
            },
            "distribution_profiles": {
                "test-game": {"scope": "review-test-game", "required_paths": ["payload.exe", "NOTICE.md", "manifest.json"], "forbidden_component_ids": ["blocked:test"]}
            },
        }), encoding="utf-8")
        manifest = root / "manifest.json"
        entries = []
        for name in ("payload.exe", "NOTICE.md"):
            entries.append({
                "path": name,
                "sha256": __import__("hashlib").sha256((root / name).read_bytes()).hexdigest(),
                "component_id": "project:test", "spdx": "MIT",
                "distribution_scope": "review-test-game", "source_reference": "source",
                "notice_reference": "NOTICE.md",
            })
        manifest.write_text(json.dumps({
            "schema_version": 1, "files": entries, "third_party_binaries": [],
            "manifest_self": {"path": "manifest.json", "hash": "excluded-to-avoid-circularity"},
        }), encoding="utf-8")
        return root, manifest, ledger

    def test_exact_distribution_passes(self) -> None:
        root, manifest, ledger = self.fixture()
        self.assertEqual("passed", reconcile(root, manifest, ledger, "test-game")["status"])

    def test_unexplained_file_fails_closed(self) -> None:
        root, manifest, ledger = self.fixture()
        (root / "surprise.dll").write_bytes(b"unknown")
        with self.assertRaisesRegex(ReconciliationError, "unexplained"):
            reconcile(root, manifest, ledger, "test-game")

    def test_hash_mismatch_fails_closed(self) -> None:
        root, manifest, ledger = self.fixture()
        (root / "payload.exe").write_bytes(b"tampered")
        with self.assertRaisesRegex(ReconciliationError, "SHA-256 mismatch"):
            reconcile(root, manifest, ledger, "test-game")

    def test_symlink_fails_closed(self) -> None:
        root, manifest, ledger = self.fixture()
        link = root / "linked"
        try:
            link.symlink_to(root / "NOTICE.md")
        except OSError:
            self.skipTest("symlink unavailable")
        with self.assertRaisesRegex(ReconciliationError, "symlink"):
            reconcile(root, manifest, ledger, "test-game")

    def test_debug_crt_marker_fails_closed_even_when_hashed(self) -> None:
        root, manifest, ledger = self.fixture()
        payload = root / "payload.exe"
        payload.write_bytes(b"PE fixture imports VCRUNTIME140D.dll")
        data = json.loads(manifest.read_text())
        data["files"][0]["sha256"] = __import__("hashlib").sha256(payload.read_bytes()).hexdigest()
        manifest.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaisesRegex(ReconciliationError, "debug CRT"):
            reconcile(root, manifest, ledger, "test-game")

    def test_extracted_installer_unexplained_file_fails_closed(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="npc-installer-", dir="/tmp" if Path("/tmp").is_dir() else None))
        legal = root / "legal"
        legal.mkdir()
        required = ["legal/LICENSE.txt", "legal/THIRD-PARTY-NOTICES.md", "legal/distribution-components.json", "legal/lockfiles.cdx.json"]
        for relative in required:
            (root / relative).write_text(relative, encoding="utf-8")
        manifest = root / "release-files.json"
        ledger = root.parent / f"{root.name}-ledger.json"
        ledger.write_text(json.dumps({
            "components": {"project:legal": {"spdx": "MIT", "redistributed": True, "scopes": ["base-installer"]}},
            "distribution_profiles": {"installer": {"scope": "base-installer", "required_install_paths": required, "forbidden_component_ids": []}},
        }), encoding="utf-8")
        files = [{
            "path": relative, "sha256": __import__("hashlib").sha256((root / relative).read_bytes()).hexdigest(),
            "component_id": "project:legal", "spdx": "MIT", "distribution_scope": "base-installer",
            "source_reference": "source", "notice_reference": "legal/THIRD-PARTY-NOTICES.md",
        } for relative in required]
        manifest.write_text(json.dumps({
            "schema_version": 1, "files": files,
            "manifest_self": {"path": "release-files.json", "hash": "excluded-to-avoid-circularity"},
        }), encoding="utf-8")
        (root / "unexplained-nsis-plugin.dll").write_bytes(b"unclassified")
        with self.assertRaisesRegex(ReconciliationError, "unexplained"):
            reconcile(root, manifest, ledger, "installer")

    def test_installed_license_corpus_sbom_and_scope_reconcile(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="npc-legal-corpus-", dir="/tmp" if Path("/tmp").is_dir() else None))
        legal = root / "product-audit/legal"
        body = legal / "packages/components/0001-example/LICENSE"
        body.parent.mkdir(parents=True)
        body.write_text("exact license body", encoding="utf-8")
        ledger = {"schema_version": 1, "components": {}, "static_files": []}
        (legal / "distribution-components.json").write_text(json.dumps(ledger), encoding="utf-8")
        scope = {"schema_version": 1, "artifact_id": "fixture", "components": {"pkg:cargo/example@1.0.0": "required"}}
        (legal / "windows-artifact-scope.json").write_text(json.dumps(scope), encoding="utf-8")
        index = {
            "schema_version": 1, "artifact_id": "fixture",
            "components": {"pkg:cargo/example@1.0.0": [{
                "path": "components/0001-example/LICENSE",
                "sha256": __import__("hashlib").sha256(body.read_bytes()).hexdigest(), "bytes": body.stat().st_size,
            }]},
        }
        index_path = legal / "packages/THIRD-PARTY-LICENSE-FILES.json"
        index_path.write_text(json.dumps(index), encoding="utf-8")
        sbom = {
            "metadata": {"properties": [
                {"name": "interactive-npcs:artifact-id", "value": "fixture"},
                {"name": "interactive-npcs:artifact-scope-resolved", "value": "true"},
                {"name": "interactive-npcs:license-material-index-sha256", "value": __import__("hashlib").sha256(index_path.read_bytes()).hexdigest()},
            ]},
            "components": [{"purl": "pkg:cargo/example@1.0.0", "scope": "required"}],
        }
        (legal / "lockfiles.cdx.json").write_text(json.dumps(sbom), encoding="utf-8")
        validate_installer_legal_materials(root, ledger)
        (body.parent / "SURPRISE.txt").write_text("unindexed", encoding="utf-8")
        with self.assertRaisesRegex(ReconciliationError, "differs from index"):
            validate_installer_legal_materials(root, ledger)


if __name__ == "__main__":
    unittest.main()
