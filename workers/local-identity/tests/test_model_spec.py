from __future__ import annotations

import hashlib
import json
import math
import struct
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from model_spec import (  # noqa: E402
    EMBEDDING_DIMENSIONS,
    PACK_ID,
    PACK_MANIFEST_SHA256,
    PACK_REVISION,
    REFERENCE_IMPORT_CONTRACT_VERSION,
    REQUEST_CONTRACT_VERSION,
    PackSpec,
    ReferenceImportRequest,
    SpecError,
    WgcFrameRequest,
    normalize_vector,
    pixel_mapping_name,
    tensor_bytes,
)

MANIFEST = ROOT.parents[1] / "packaging" / "model-packs" / "opencv-yunet-sface-private-evaluation.json"
DIGEST = "a" * 64


def lease() -> dict[str, object]:
    lease_id = "pixel-lease-01"
    lease_nonce = "nonce-identity-01"
    return {
        "lease_id": lease_id,
        "shared_memory_name": pixel_mapping_name(lease_id, lease_nonce),
        "lease_nonce": lease_nonce,
        "byte_length": 128 * 72 * 4,
        "width": 128,
        "height": 72,
        "stride_bytes": 128 * 4,
        "pixel_format": "b8g8r8a8_unorm",
        "content_sha256": DIGEST,
    }


def frame() -> dict[str, object]:
    return {
        "contract_version": REQUEST_CONTRACT_VERSION,
        "mode": "wgc_frame",
        "target": {
            "capture_session_id": "capture-session-01",
            "process_id": 4242,
            "window_handle": 99,
            "executable_name": "npc-review-test-game.exe",
        },
        "frame_sequence": 7,
        "device_generation": 3,
        "geometry_epoch": 2,
        "source_frame_qpc": 9001,
        "qpc_frequency": 10_000_000,
        "captured_at_ms": 1234,
        "content_sha256": DIGEST,
        "advancing_frame_verified": True,
        "overlay_capture_excluded": True,
        "protected_online_detected": False,
        "anti_cheat_detected": False,
        "pixel_lease": lease(),
    }


def reference(source_class: str = "user_private") -> dict[str, object]:
    return {
        "contract_version": REFERENCE_IMPORT_CONTRACT_VERSION,
        "mode": "reference_import",
        "game_profile_id": "eclipse-harbor",
        "subject_id": "mara-venn",
        "reference_id": "mara-private-01",
        "subject_display_name": "Mara Venn",
        "source_class": source_class,
        "source_content_sha256": DIGEST,
        "owner_user_id": "local-user-01" if source_class == "user_private" else None,
        "original_work_license": None if source_class == "user_private" else "CC0-1.0 original synthetic fixture",
        "explicit_user_consent": True,
        "local_only": True,
        "imported_at_ms": 999,
        "pixel_lease": lease(),
    }


class ManifestTests(unittest.TestCase):
    def test_checked_in_manifest_is_exact_and_model_files_are_permissively_licensed(self) -> None:
        pack = PackSpec.load(MANIFEST)
        self.assertEqual(pack.manifest_sha256, PACK_MANIFEST_SHA256)
        self.assertEqual(pack.artifact("yunet-2026may-onnx").size_bytes, 229_738)
        self.assertEqual(pack.artifact("sface-2021dec-onnx").size_bytes, 38_696_353)
        raw = json.loads(MANIFEST.read_text(encoding="utf-8"))
        self.assertTrue(raw["license"]["redistributable"])
        self.assertEqual(raw["license"]["commercial_use"], "allowed")
        self.assertIsNone(raw["resources"]["planning_resident_ram_bytes"])
        self.assertTrue(raw["resources"]["measurement"]["signed_evidence_required"])
        self.assertEqual(raw["admission"]["state"], "blocked_pending_measurement")

    def test_mutable_revision_url_is_rejected(self) -> None:
        raw = json.loads(MANIFEST.read_text(encoding="utf-8"))
        raw["artifacts"][0]["source_urls"] = ["https://github.com/opencv/opencv_zoo/raw/main/model.onnx"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(raw), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "mutable"):
                PackSpec.load(path)

    def test_sface_hash_change_is_rejected(self) -> None:
        raw = json.loads(MANIFEST.read_text(encoding="utf-8"))
        raw["artifacts"][2]["sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(raw), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "missing or changed"):
                PackSpec.load(path)

    def test_attempt_to_remove_reviewed_model_file_provenance_is_rejected(self) -> None:
        raw = json.loads(MANIFEST.read_text(encoding="utf-8"))
        raw["license"]["redistributable"] = False
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(raw), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "provenance changed"):
                PackSpec.load(path)

    def test_non_security_metadata_drift_still_changes_the_reviewed_revision(self) -> None:
        raw = json.loads(MANIFEST.read_text(encoding="utf-8"))
        raw["description"] += " Changed after review."
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(raw), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "immutable revision"):
                PackSpec.load(path)

    def test_runtime_requirement_lock_is_byte_exact(self) -> None:
        runtime_lock = ROOT / "runtime-requirements.windows-x86_64-cp312.v1.json"
        self.assertEqual(
            hashlib.sha256(runtime_lock.read_bytes()).hexdigest(),
            "e63cdbf08823ea44c6ad307f38636d9b2eaff511078d2243c253bc8dbc766e96",
        )
        raw = json.loads(runtime_lock.read_text(encoding="utf-8"))
        requirements = {item["name"]: item for item in raw["direct_runtime_requirements"]}
        self.assertEqual(requirements["opencv-python-headless"]["version"], "5.0.0.93")
        self.assertEqual(
            requirements["opencv-python-headless"]["sha256"],
            "829717b6a95554f273e49e357cee3b3a2a26b6f4842fbc1bed2b45bdd8f87e0e",
        )
        self.assertEqual(requirements["numpy"]["version"], "2.5.2")
        self.assertEqual(
            requirements["numpy"]["sha256"],
            "28ac63476ec7651484215ee7fa15a1f78b57c14621f01e392afe17b9a1390ce4",
        )


class TensorAndRequestTests(unittest.TestCase):
    def test_tensor_is_versioned_normalized_f32le(self) -> None:
        values = normalize_vector([1.0] * EMBEDDING_DIMENSIONS)
        self.assertEqual(len(values), EMBEDDING_DIMENSIONS)
        self.assertAlmostEqual(math.sqrt(math.fsum(value * value for value in values)), 1.0, places=5)
        encoded = tensor_bytes(values)
        self.assertEqual(len(encoded), EMBEDDING_DIMENSIONS * 4)
        self.assertEqual(struct.unpack("<f", encoded[:4])[0], values[0])

    def test_zero_nonfinite_and_wrong_dimensions_rejected(self) -> None:
        for values in ([0.0] * EMBEDDING_DIMENSIONS, [math.inf] + [1.0] * 127, [1.0, 2.0]):
            with self.assertRaises(SpecError):
                normalize_vector(values)

    def test_wgc_request_binds_exact_target_frame_and_digest(self) -> None:
        value = WgcFrameRequest.parse(frame())
        self.assertEqual(value.target.process_id, 4242)
        self.assertEqual(value.frame_sequence, 7)
        self.assertEqual(value.content_sha256, value.pixel_lease.content_sha256)

    def test_unsafe_wgc_evidence_fails_closed(self) -> None:
        for key, value in (
            ("advancing_frame_verified", False),
            ("overlay_capture_excluded", False),
            ("protected_online_detected", True),
            ("anti_cheat_detected", True),
        ):
            raw = frame()
            raw[key] = value
            with self.assertRaisesRegex(SpecError, "does not authorize"):
                WgcFrameRequest.parse(raw)

    def test_wgc_digest_mismatch_and_unknown_fields_fail_closed(self) -> None:
        raw = frame()
        raw["content_sha256"] = "b" * 64
        with self.assertRaisesRegex(SpecError, "digests differ"):
            WgcFrameRequest.parse(raw)
        raw = frame()
        raw["debug"] = True
        with self.assertRaises(SpecError):
            WgcFrameRequest.parse(raw)

    def test_named_mapping_must_be_local_and_bound_to_lease_secret(self) -> None:
        for mapping in (
            "Local\\npc.identity.unrelated",
            "Global\\npc.identity.unrelated",
        ):
            raw = frame()
            raw["pixel_lease"]["shared_memory_name"] = mapping
            with self.assertRaisesRegex(SpecError, "bound to the pixel lease"):
                WgcFrameRequest.parse(raw)

    def test_user_private_reference_requires_consent_owner_and_local_only(self) -> None:
        parsed = ReferenceImportRequest.parse(reference())
        self.assertEqual(parsed.owner_user_id, "local-user-01")
        for key, value in (("explicit_user_consent", False), ("owner_user_id", None), ("local_only", False)):
            raw = reference()
            raw[key] = value
            with self.assertRaises(SpecError):
                ReferenceImportRequest.parse(raw)

    def test_original_reference_requires_license_and_no_private_owner(self) -> None:
        parsed = ReferenceImportRequest.parse(reference("original_synthetic"))
        self.assertEqual(parsed.original_work_license, "CC0-1.0 original synthetic fixture")
        raw = reference("original_synthetic")
        raw["original_work_license"] = None
        with self.assertRaises(SpecError):
            ReferenceImportRequest.parse(raw)

    def test_unknown_reference_source_and_pickle_fields_are_rejected(self) -> None:
        raw = reference()
        raw["source_class"] = "web_scrape"
        with self.assertRaises(SpecError):
            ReferenceImportRequest.parse(raw)
        raw = reference()
        raw["pickle"] = "payload"
        with self.assertRaises(SpecError):
            ReferenceImportRequest.parse(raw)


if __name__ == "__main__":
    unittest.main()
