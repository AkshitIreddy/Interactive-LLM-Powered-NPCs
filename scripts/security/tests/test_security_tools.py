from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SECURITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SECURITY_DIR))
from lock_inventory import InventoryError, load_inventory  # noqa: E402


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


if __name__ == "__main__":
    unittest.main()
