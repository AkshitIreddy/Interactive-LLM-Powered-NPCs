from __future__ import annotations

import hashlib
import io
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
REPO = LOCAL_TTS.parents[1]
sys.path.insert(0, str(LOCAL_TTS))
sys.path.insert(0, str(Path(__file__).parent))

from manifest import Artifact
from pack_manager import (
    PackLifecycleError,
    PackManager,
    extract_verified_tar_bz2,
)
from support import FakeDownloader, fixture_manifest

MANIFEST = (
    REPO
    / "packaging"
    / "model-packs"
    / "kokoro-sherpa-onnx-v1.0-int8-windows-x64.json"
)


class PackManagerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.root = self.base / "model-packs"
        self.root.mkdir()
        archives, self.manifest = fixture_manifest(MANIFEST)
        self.downloader = FakeDownloader(archives)
        self.manager = PackManager(self.root, self.downloader)
        self.authorization = {
            "expected_manifest_sha256": self.manifest.canonical_sha256,
            "confirmed_pack_id": self.manifest.pack_id,
            "confirmed_revision": self.manifest.revision,
            "accepted_license_ids": set(
                self.manifest.license_acceptance_ids
            ),
        }

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_install_verify_tamper_repair_remove(self) -> None:
        target = self.manager.install(self.manifest, **self.authorization)
        self.assertTrue(target.is_dir())
        self.assertEqual(
            self.downloader.calls,
            ["sherpa-runtime", "kokoro-model"],
        )
        record = self.manager.verify(self.manifest)
        self.assertEqual(record["state"], "verified")

        model = target / "model" / "model.int8.onnx"
        model.write_bytes(b"tampered")
        with self.assertRaisesRegex(
            PackLifecycleError,
            "critical pack file failed verification",
        ):
            self.manager.verify(self.manifest)

        repaired = self.manager.repair(
            self.manifest,
            **self.authorization,
        )
        self.assertEqual(repaired, target)
        self.manager.verify(self.manifest)

        with self.assertRaises(PackLifecycleError):
            self.manager.remove(
                self.manifest,
                expected_manifest_sha256=self.manifest.canonical_sha256,
                confirm_remove="wrong",
            )
        self.manager.remove(
            self.manifest,
            expected_manifest_sha256=self.manifest.canonical_sha256,
            confirm_remove=self.manifest.install_key,
        )
        self.assertFalse(target.exists())

    def test_install_requires_exact_manifest_pack_revision_and_licenses(self) -> None:
        cases = [
            {**self.authorization, "expected_manifest_sha256": "0" * 64},
            {**self.authorization, "confirmed_pack_id": "another.pack"},
            {**self.authorization, "confirmed_revision": "another-revision"},
            {**self.authorization, "accepted_license_ids": set()},
        ]
        for authorization in cases:
            with self.subTest(authorization=authorization):
                with self.assertRaises(PackLifecycleError):
                    self.manager.install(self.manifest, **authorization)
        self.assertEqual(self.downloader.calls, [])

    def test_verify_rejects_unrecorded_files(self) -> None:
        target = self.manager.install(self.manifest, **self.authorization)
        (target / "surprise.dll").write_bytes(b"unrecorded")
        with self.assertRaisesRegex(
            PackLifecycleError,
            "unrecorded or omitted",
        ):
            self.manager.verify(self.manifest)

    def test_pack_root_must_be_dedicated(self) -> None:
        with self.assertRaises(PackLifecycleError):
            PackManager(self.base / "arbitrary", self.downloader)


class SafeExtractionTests(unittest.TestCase):
    def artifact(self, payload: bytes, prefix: str = "root") -> Artifact:
        return Artifact(
            artifact_id="fixture",
            url="https://github.com/example/release/file.tar.bz2",
            size_bytes=len(payload),
            sha256=hashlib.sha256(payload).hexdigest(),
            archive="tar.bz2",
            strip_prefix=prefix,
            destination="content",
            required_paths=(),
        )

    def bundle(self, entries) -> bytes:
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:bz2") as archive:
            for item, content in entries:
                item.size = len(content)
                archive.addfile(item, io.BytesIO(content))
        return output.getvalue()

    def assert_rejected(self, entries) -> None:
        payload = self.bundle(entries)
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "bad.tar.bz2"
            archive.write_bytes(payload)
            with self.assertRaises(PackLifecycleError):
                extract_verified_tar_bz2(
                    archive,
                    self.artifact(payload),
                    Path(temporary) / "stage",
                )

    def test_rejects_traversal(self) -> None:
        self.assert_rejected(
            [(tarfile.TarInfo("root/../escape"), b"bad")]
        )

    def test_rejects_symlink(self) -> None:
        item = tarfile.TarInfo("root/link")
        item.type = tarfile.SYMTYPE
        item.linkname = "target"
        self.assert_rejected([(item, b"")])

    def test_rejects_case_collisions(self) -> None:
        self.assert_rejected(
            [
                (tarfile.TarInfo("root/Model.bin"), b"a"),
                (tarfile.TarInfo("root/model.bin"), b"b"),
            ]
        )

    def test_rejects_unexpected_archive_root(self) -> None:
        self.assert_rejected(
            [(tarfile.TarInfo("different/file"), b"bad")]
        )


if __name__ == "__main__":
    unittest.main()
