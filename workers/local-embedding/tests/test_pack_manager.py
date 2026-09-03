from __future__ import annotations

import hashlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from model_spec import ArtifactSpec, PackSpec  # noqa: E402
from pack_manager import LifecycleError, PackLifecycle  # noqa: E402
from gpu_lock import GpuLockBusy, ai_model_lock, externally_owned_ai_model_lock  # noqa: E402


class PackLifecycleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.model = b"reviewed-fixture-model"
        self.tokenizer = b'{"version":"fixture"}'
        artifacts = (
            ArtifactSpec(
                "fixture-model",
                ("https://example.invalid/model",),
                len(self.model),
                hashlib.sha256(self.model).hexdigest(),
                PurePosixPath("model/model.onnx"),
            ),
            ArtifactSpec(
                "fixture-tokenizer",
                ("https://example.invalid/tokenizer",),
                len(self.tokenizer),
                hashlib.sha256(self.tokenizer).hexdigest(),
                PurePosixPath("model/tokenizer.json"),
            ),
        )
        self.pack = PackSpec(self.root / "manifest.json", "a" * 64, artifacts)
        self.contents = {"fixture-model": self.model, "fixture-tokenizer": self.tokenizer}

        def fetch(artifact: ArtifactSpec, destination: io.BufferedWriter) -> tuple[str, int]:
            data = self.contents[artifact.artifact_id]
            destination.write(data)
            return hashlib.sha256(data).hexdigest(), len(data)

        self.lifecycle = PackLifecycle(self.root / "packs", self.pack, fetch=fetch)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_install_is_atomic_and_verified(self) -> None:
        result = self.lifecycle.install()
        self.assertTrue(result.changed)
        self.assertTrue(result.report.healthy)
        self.assertTrue((result.target / "model/model.onnx").is_file())

    def test_repeat_install_is_idempotent(self) -> None:
        self.lifecycle.install()
        result = self.lifecycle.install()
        self.assertFalse(result.changed)
        self.assertTrue(result.report.healthy)

    def test_checksum_failure_never_commits_target(self) -> None:
        self.contents["fixture-model"] = b"corrupt"
        with self.assertRaisesRegex(LifecycleError, "did not match"):
            self.lifecycle.install()
        self.assertFalse(self.lifecycle.target.exists())

    def test_verify_detects_corruption(self) -> None:
        self.lifecycle.install()
        (self.lifecycle.target / "model/model.onnx").write_bytes(b"corrupt")
        report = self.lifecycle.verify()
        self.assertFalse(report.healthy)
        self.assertTrue(any(issue.code in {"size_mismatch", "sha256_mismatch"} for issue in report.issues))

    def test_install_refuses_to_overwrite_unhealthy_pack(self) -> None:
        self.lifecycle.install()
        (self.lifecycle.target / "model/model.onnx").write_bytes(b"corrupt")
        with self.assertRaisesRegex(LifecycleError, "repair"):
            self.lifecycle.install()

    def test_repair_replaces_corruption_and_keeps_recovery(self) -> None:
        self.lifecycle.install()
        (self.lifecycle.target / "model/model.onnx").write_bytes(b"corrupt")
        result = self.lifecycle.repair()
        self.assertTrue(result.changed)
        self.assertTrue(result.report.healthy)
        self.assertIsNotNone(result.recovery_path)
        self.assertTrue(result.recovery_path.is_dir())

    def test_healthy_repair_is_idempotent(self) -> None:
        self.lifecycle.install()
        result = self.lifecycle.repair()
        self.assertFalse(result.changed)

    def test_remove_is_recoverable_rename(self) -> None:
        self.lifecycle.install()
        result = self.lifecycle.remove()
        self.assertTrue(result.changed)
        self.assertFalse(result.target.exists())
        self.assertTrue(result.recovery_path.is_dir())
        self.assertTrue((result.recovery_path / "model/model.onnx").is_file())

    def test_repeat_remove_is_idempotent(self) -> None:
        self.lifecycle.install()
        self.lifecycle.remove()
        result = self.lifecycle.remove()
        self.assertFalse(result.changed)

    def test_receipt_tamper_fails_verification(self) -> None:
        self.lifecycle.install()
        receipt = self.lifecycle.receipt_path
        raw = json.loads(receipt.read_text())
        raw["manifest_sha256"] = "0" * 64
        receipt.write_text(json.dumps(raw))
        report = self.lifecycle.verify()
        self.assertFalse(report.receipt_valid)

    def test_intermediate_symlink_is_rejected(self) -> None:
        self.lifecycle.install()
        real = self.lifecycle.target / "real-model"
        real.mkdir()
        (real / "model.onnx").write_bytes(self.model)
        original = self.lifecycle.target / "model"
        for child in original.iterdir():
            child.unlink()
        original.rmdir()
        try:
            original.symlink_to(real, target_is_directory=True)
        except OSError:
            self.skipTest("symlink creation is unavailable")
        report = self.lifecycle.verify()
        self.assertFalse(report.healthy)
        self.assertTrue(any(issue.code == "unsafe_destination" for issue in report.issues))

    def test_lifecycle_lock_fails_closed(self) -> None:
        self.lifecycle.root.mkdir(parents=True, exist_ok=True)
        lock = self.lifecycle.root / ".bge-small-en-v1.5-onnx-fp32.5c38ec7c405ec4b44b94cc5a9bb96e735b38267a.1.lifecycle.lock"
        lock.write_text("owned")
        with self.assertRaisesRegex(LifecycleError, "another"):
            self.lifecycle.install()


class AiModelLockTests(unittest.TestCase):
    def test_lock_transitions_no_yes_no(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gpu-use.txt"
            path.write_bytes(b"no")
            with ai_model_lock(path):
                self.assertEqual(path.read_bytes(), b"yes")
            self.assertEqual(path.read_bytes(), b"no")

    def test_busy_state_is_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gpu-use.txt"
            path.write_bytes(b"yes")
            with self.assertRaises(GpuLockBusy):
                with ai_model_lock(path):
                    self.fail("busy lock must never be entered")
            self.assertEqual(path.read_bytes(), b"yes")

    def test_exception_restores_no(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gpu-use.txt"
            path.write_bytes(b"no")
            with self.assertRaisesRegex(RuntimeError, "fixture failure"):
                with ai_model_lock(path):
                    raise RuntimeError("fixture failure")
            self.assertEqual(path.read_bytes(), b"no")

    def test_external_root_owned_lock_is_verified_without_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gpu-use.txt"
            path.write_bytes(b"yes")
            with externally_owned_ai_model_lock(path):
                self.assertEqual(path.read_bytes(), b"yes")
            self.assertEqual(path.read_bytes(), b"yes")

    def test_external_root_owned_lock_rejects_no(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gpu-use.txt"
            path.write_bytes(b"no")
            with self.assertRaises(GpuLockBusy):
                with externally_owned_ai_model_lock(path):
                    self.fail("inactive external lane must not run")


if __name__ == "__main__":
    unittest.main()
