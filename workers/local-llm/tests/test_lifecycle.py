from __future__ import annotations

import io
import tempfile
import unittest
import zipfile
from pathlib import Path

from npc_local_llm.archive import secure_extract_zip
from npc_local_llm.download import MemoryArtifactFetcher
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.lifecycle import ModelPackLifecycle

from support import fixture_pack


class LifecycleTests(unittest.TestCase):
    def test_explicit_install_verify_repair_and_remove(self) -> None:
        payloads = {"model": b"model-bytes", "license": b"license-bytes"}
        pack = fixture_pack(payloads)
        with tempfile.TemporaryDirectory() as temporary:
            lifecycle = ModelPackLifecycle(Path(temporary), MemoryArtifactFetcher(payloads))
            with self.assertRaisesRegex(LocalLlmError, "explicit user"):
                lifecycle.install(pack, explicit_user_confirmation=False)
            receipt = lifecycle.install(pack, explicit_user_confirmation=True)
            self.assertEqual(receipt.unit_id, "fixture-pack")
            self.assertEqual(lifecycle.verify(pack).transaction_id, receipt.transaction_id)
            model = Path(temporary) / "packs/fixture-pack/fixture-revision/files/model.bin"
            model.write_bytes(b"corrupt")
            repaired = lifecycle.repair(pack, explicit_user_confirmation=True)
            self.assertNotEqual(repaired.transaction_id, receipt.transaction_id)
            self.assertEqual(model.read_bytes(), b"model-bytes")
            with self.assertRaisesRegex(LocalLlmError, "active or referenced"):
                lifecycle.remove(
                    pack,
                    explicit_user_confirmation=True,
                    require_unreferenced=True,
                    is_referenced=lambda _pack, _revision: True,
                )
            self.assertTrue(
                lifecycle.remove(
                    pack,
                    explicit_user_confirmation=True,
                    require_unreferenced=True,
                    is_referenced=lambda _pack, _revision: False,
                )
            )
            self.assertFalse(
                lifecycle.remove(
                    pack,
                    explicit_user_confirmation=True,
                    require_unreferenced=True,
                    is_referenced=lambda _pack, _revision: False,
                )
            )

    def test_failed_digest_leaves_no_installed_tree(self) -> None:
        payloads = {"model": b"good"}
        pack = fixture_pack(payloads)
        with tempfile.TemporaryDirectory() as temporary:
            lifecycle = ModelPackLifecycle(Path(temporary), MemoryArtifactFetcher({"model": b"evil"}))
            with self.assertRaises(LocalLlmError):
                lifecycle.install(pack, explicit_user_confirmation=True)
            self.assertFalse((Path(temporary) / "packs/fixture-pack").exists())


class ArchiveTests(unittest.TestCase):
    def _archive(self, members: dict[str, bytes]) -> Path:
        temporary = tempfile.NamedTemporaryFile(suffix=".zip", delete=False)
        temporary.close()
        path = Path(temporary.name)
        with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, payload in members.items():
                archive.writestr(name, payload)
        self.addCleanup(path.unlink, missing_ok=True)
        return path

    def test_safe_runtime_archive_extracts(self) -> None:
        archive = self._archive({"bin/llama-server.exe": b"fixture-exe", "LICENSE": b"MIT"})
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "payload"
            members, expanded = secure_extract_zip(archive, output)
            self.assertEqual(members, 2)
            self.assertEqual(expanded, len(b"fixture-exeMIT"))
            self.assertEqual((output / "bin/llama-server.exe").read_bytes(), b"fixture-exe")

    def test_traversal_and_case_collision_fail_closed(self) -> None:
        for members in (
            {"../evil.exe": b"evil"},
            {"BIN/a.dll": b"one", "bin/A.dll": b"two"},
            {"NUL": b"device"},
        ):
            archive = self._archive(members)
            with tempfile.TemporaryDirectory() as temporary, self.assertRaises(LocalLlmError):
                secure_extract_zip(archive, Path(temporary) / "payload")

    def test_exact_archive_identity_and_hard_ceiling_fail_closed(self) -> None:
        archive = self._archive({"runtime/llama-server.exe": b"fixture"})
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(LocalLlmError):
                secure_extract_zip(
                    archive,
                    Path(temporary) / "wrong-count",
                    expected_member_count=2,
                    expected_expanded_bytes=7,
                )
            with self.assertRaises(LocalLlmError):
                secure_extract_zip(
                    archive,
                    Path(temporary) / "above-hard-ceiling",
                    maximum_expanded_bytes=2 * 1_073_741_824 + 1,
                )


if __name__ == "__main__":
    unittest.main()
