from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path


SECURITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SECURITY_DIR))
from measure_packaged_telemetry_inventory import InventoryError, measure  # noqa: E402


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class PackagedTelemetryInventoryTests(unittest.TestCase):
    def candidate(self, root: Path, binary: bytes = b"ordinary packaged bytes") -> tuple[Path, Path, Path, Path]:
        repo = root / "repo"
        install = root / "installed"
        repo.mkdir()
        install.mkdir()
        (repo / "Cargo.lock").write_text("ordinary-lock", encoding="utf-8")
        (repo / "pnpm-lock.yaml").write_text("ordinary-pnpm", encoding="utf-8")
        (repo / "package.json").write_text("{}", encoding="utf-8")
        executable = install / "candidate.exe"
        executable.write_bytes(binary)
        installed_path = root / "installed.json"
        installed_path.write_text(json.dumps({
            "schema_version": 1,
            "files": [{"path": "candidate.exe", "sha256": sha(executable)}],
        }), encoding="utf-8")
        package_path = root / "package.json"
        package_path.write_text(json.dumps({
            "schema_version": 1,
            "distribution": "local-review-only",
            "source": {
                "inputs": {
                    "Cargo.lock": {"sha256": sha(repo / "Cargo.lock")},
                    "pnpm-lock.yaml": {"sha256": sha(repo / "pnpm-lock.yaml")},
                },
            },
        }), encoding="utf-8")
        return repo, install, package_path, installed_path

    def test_clean_bound_candidate_measures_zero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo, install, package, installed = self.candidate(Path(directory))
            result = measure(repo, package, installed, install)
            self.assertEqual(result["provenance"], "measured")
            self.assertEqual(result["automaticUploadEntryPoints"], 0)
            self.assertEqual(result["remoteTelemetryDestinations"], 0)

    def test_telemetry_destination_and_upload_marker_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo, install, package, installed = self.candidate(
                Path(directory), b"telemetry_upload https://sentry.io/api"
            )
            result = measure(repo, package, installed, install)
            self.assertGreater(result["automaticUploadEntryPoints"], 0)
            self.assertGreater(result["remoteTelemetryDestinations"], 0)

    def test_changed_source_or_installed_binary_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo, install, package, installed = self.candidate(Path(directory))
            (repo / "Cargo.lock").write_text("changed", encoding="utf-8")
            with self.assertRaisesRegex(InventoryError, "Cargo.lock"):
                measure(repo, package, installed, install)
            (repo / "Cargo.lock").write_text("ordinary-lock", encoding="utf-8")
            (install / "candidate.exe").write_bytes(b"changed")
            with self.assertRaisesRegex(InventoryError, "differs"):
                measure(repo, package, installed, install)

    def test_installed_path_escape_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo, install, package, installed = self.candidate(root)
            outside = root / "outside.exe"
            outside.write_bytes(b"outside")
            installed.write_text(json.dumps({
                "schema_version": 1,
                "files": [{"path": "../outside.exe", "sha256": sha(outside)}],
            }), encoding="utf-8")
            with self.assertRaisesRegex(InventoryError, "escapes"):
                measure(repo, package, installed, install)


if __name__ == "__main__":
    unittest.main()
