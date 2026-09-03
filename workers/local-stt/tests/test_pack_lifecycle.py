from __future__ import annotations

from io import BytesIO
import hashlib
import json
from pathlib import Path
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in __import__("sys").path:
    __import__("sys").path.insert(0, str(ROOT))

from npc_local_stt.pack_lifecycle import PackError, PackLifecycle, PackManifest


REQUIRED_FILES = [
    "moonshine-voice-windows-x86_64/include/moonshine-c-api.h",
    "moonshine-voice-windows-x86_64/lib/moonshine.lib",
    "models/medium-streaming-en/encoder.ort",
]


def make_archive(*, traversal: bool = False, symlink: bool = False) -> bytes:
    output = BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        files = {
            f"cli-transcriber/{REQUIRED_FILES[0]}": b"header",
            f"cli-transcriber/{REQUIRED_FILES[1]}": b"library",
            f"cli-transcriber/{REQUIRED_FILES[2]}": b"model-fixture-not-a-weight",
        }
        if traversal:
            files["cli-transcriber/../../escape"] = b"no"
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o600
            archive.addfile(info, BytesIO(data))
        if symlink:
            info = tarfile.TarInfo("cli-transcriber/linked-model")
            info.type = tarfile.SYMTYPE
            info.linkname = "models/medium-streaming-en/encoder.ort"
            archive.addfile(info)
    return output.getvalue()


class FakeResponse(BytesIO):
    status = 200

    def getcode(self) -> int:
        return self.status


def write_manifest(path: Path, archive: bytes) -> PackManifest:
    value = {
        "schemaVersion": "npc.local-model-pack/v2",
        "packId": "fixture.stt.pack",
        "revision": "fixture-v1",
        "displayName": "Fixture STT Pack",
        "kind": "stt",
        "status": "candidate_unqualified",
        "artifacts": [
            {
                "artifactId": "fixture-archive",
                "url": "https://github.com/moonshine-ai/moonshine/releases/download/v0.1.5/fixture.tar.gz",
                "bytes": len(archive),
                "sha256": hashlib.sha256(archive).hexdigest(),
                "archive": {
                    "requiredTopLevelDirectory": "cli-transcriber",
                    "maximumEntries": 16,
                    "maximumExpandedBytes": 100000,
                },
            }
        ],
        "installedLayout": {"rootDirectory": "cli-transcriber", "requiredFiles": REQUIRED_FILES},
        "license": {
            "spdx": "MIT",
            "evidence": "https://example.invalid/MIT",
            "explicitUserAcceptanceRequired": True,
        },
        "lifecycle": {
            "activationGates": [
                "explicit_license_and_download_confirmation",
                "archive_sha256_verified",
                "safe_extraction_verified",
                "self_test",
            ]
        },
    }
    path.write_text(json.dumps(value), encoding="utf-8")
    return PackManifest.load(path)


class PackLifecycleTests(unittest.TestCase):
    def lifecycle(self, temp: Path, archive: bytes) -> PackLifecycle:
        manifest = write_manifest(temp / "manifest.json", archive)
        return PackLifecycle(manifest, temp / "dedicated" / "pack-root", opener=lambda *_args, **_kwargs: FakeResponse(archive))

    def test_install_requires_explicit_confirmation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive())
            with self.assertRaises(PackError) as caught:
                lifecycle.install(confirmed=False)
            self.assertEqual(caught.exception.code, "confirmation_required")

    def test_install_verify_and_remove_fixture_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive())
            receipt = lifecycle.install(confirmed=True)
            self.assertFalse(receipt["activationAllowed"])
            verified = lifecycle.verify()
            self.assertTrue(verified["verified"])
            self.assertGreaterEqual(verified["fileCount"], 4)
            removed = lifecycle.remove()
            self.assertTrue(removed["removed"])
            self.assertFalse(lifecycle.pack_root.exists())

    def test_install_is_idempotent_after_full_verification(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive())
            lifecycle.install(confirmed=True)
            second = lifecycle.install(confirmed=True)
            self.assertTrue(second["idempotent"])

    def test_tampering_is_detected_and_repair_reinstalls(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive())
            lifecycle.install(confirmed=True)
            target = lifecycle.pack_root / "cli-transcriber" / REQUIRED_FILES[0]
            target.write_bytes(b"tampered")
            with self.assertRaises(PackError):
                lifecycle.verify()
            repaired = lifecycle.repair(confirmed=True)
            self.assertEqual(repaired["operation"], "repair")
            self.assertTrue(repaired["quarantinedPreviousInstall"])
            self.assertTrue(lifecycle.verify()["verified"])

    def test_archive_parent_traversal_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive(traversal=True))
            with self.assertRaises(PackError) as caught:
                lifecycle.install(confirmed=True)
            self.assertEqual(caught.exception.code, "unsafe_archive")
            self.assertFalse((Path(directory) / "escape").exists())

    def test_archive_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive(symlink=True))
            with self.assertRaises(PackError) as caught:
                lifecycle.install(confirmed=True)
            self.assertEqual(caught.exception.code, "unsafe_archive")

    def test_unexpected_file_after_install_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive())
            lifecycle.install(confirmed=True)
            (lifecycle.pack_root / "unexpected.dll").write_bytes(b"not-in-receipt")
            with self.assertRaises(PackError) as caught:
                lifecycle.verify()
            self.assertEqual(caught.exception.code, "install_invalid")

    def test_remove_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lifecycle = self.lifecycle(Path(directory), make_archive())
            lifecycle.install(confirmed=True)
            self.assertTrue(lifecycle.remove()["removed"])
            second = lifecycle.remove()
            self.assertFalse(second["removed"])
            self.assertTrue(second["idempotent"])

    def test_manifest_rejects_traversing_required_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            archive = make_archive()
            manifest_path = temp / "manifest.json"
            write_manifest(manifest_path, archive)
            value = json.loads(manifest_path.read_text(encoding="utf-8"))
            value["installedLayout"]["requiredFiles"][0] = "../escape"
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(PackError) as caught:
                PackManifest.load(manifest_path)
            self.assertEqual(caught.exception.code, "manifest_invalid")

    def test_wrong_digest_is_rejected_before_extraction(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = make_archive()
            lifecycle = self.lifecycle(Path(directory), archive)
            object.__setattr__(lifecycle.manifest.artifact, "sha256", "0" * 64)
            with self.assertRaises(PackError) as caught:
                lifecycle.install(confirmed=True)
            self.assertEqual(caught.exception.code, "artifact_digest_mismatch")

    def test_production_manifest_is_fail_closed_and_strongly_pinned(self) -> None:
        production = ROOT.parents[1] / "packaging" / "model-packs" / "moonshine-v2-medium-streaming-en-win-x64-v0.1.5.json"
        value = json.loads(production.read_text(encoding="utf-8"))
        self.assertEqual(value["schema"], "npc.model-pack/v2")
        self.assertEqual(value["admission"]["state"], "blocked_pending_measurement")
        self.assertFalse(value["lifecycle"]["automatic_download_allowed"])
        self.assertEqual(value["hardware"]["accelerators"], ["cpu"])
        self.assertEqual(value["artifacts"][0]["size_bytes"], 402991031)
        self.assertEqual(len(value["artifacts"][0]["sha256"]), 64)
        notice = ROOT / "THIRD_PARTY_LICENSES" / "MOONSHINE-v0.1.5-LICENSE.txt"
        self.assertEqual(
            hashlib.sha256(notice.read_bytes()).hexdigest(),
            "fa7d1174dd8af6a7cd280be20b80d10095ed4c19b5b20b61a7715c3ad790dc5f",
        )
        parsed = PackManifest.load(production)
        self.assertEqual(parsed.license_notice_path, notice.resolve())
        self.assertIsNone(value["resources"]["planning_resident_ram_bytes"])
        self.assertIsNone(value["resources"]["planning_resident_vram_bytes"])
        self.assertIsNone(value["resources"]["planning_load_millis"])
        self.assertTrue(value["admission"]["unknowns_fail_closed"])


if __name__ == "__main__":
    unittest.main()
