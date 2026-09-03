from __future__ import annotations

import hashlib
import io
import json
import math
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from backend import DeterministicFixtureBackend, FaceObservation  # noqa: E402
from framing import FrameWriter, FramingError, read_frame  # noqa: E402
from model_spec import (  # noqa: E402
    PACK_ID,
    PACK_REVISION,
    REFERENCE_IMPORT_CONTRACT_VERSION,
    REQUEST_CONTRACT_VERSION,
    normalize_vector,
    pixel_mapping_name,
)
from protocol import Request, WorkerController  # noqa: E402

MANIFEST = ROOT.parents[1] / "packaging" / "model-packs" / "opencv-yunet-sface-private-evaluation.json"
DIGEST = "a" * 64


def face(seed: int, x: float = 10.0, confidence: float = 0.95) -> FaceObservation:
    values = [0.0] * 128
    values[seed] = 1.0
    return FaceObservation("fixture-local", x, 12.0, 30.0, 40.0, confidence, normalize_vector(values))


def envelope(operation: str, sequence: int, *, request_id: str | None = None, generation: int = 0, payload: dict | None = None) -> dict:
    return {
        "protocol_version": "1.0",
        "worker_instance_id": "" if operation == "handshake" else "identity-worker-01",
        "request_id": request_id or f"request-{sequence:02}",
        "sequence": sequence,
        "generation": generation,
        "deadline_unix_ms": 0,
        "operation": operation,
        "payload": payload or {},
    }


def lease() -> dict[str, object]:
    lease_id = "pixel-lease-01"
    lease_nonce = "pixel-secret-lease-nonce"
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


def frame_payload() -> dict[str, object]:
    return {
        "contract_version": REQUEST_CONTRACT_VERSION,
        "mode": "wgc_frame",
        "target": {"capture_session_id": "capture-session-01", "process_id": 4242, "window_handle": 99, "executable_name": "npc-review-test-game.exe"},
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


def reference_payload() -> dict[str, object]:
    return {
        "contract_version": REFERENCE_IMPORT_CONTRACT_VERSION,
        "mode": "reference_import",
        "game_profile_id": "eclipse-harbor",
        "subject_id": "mara-venn",
        "reference_id": "mara-private-01",
        "subject_display_name": "Mara Venn",
        "source_class": "user_private",
        "source_content_sha256": DIGEST,
        "owner_user_id": "local-user-01",
        "original_work_license": None,
        "explicit_user_consent": True,
        "local_only": True,
        "imported_at_ms": 999,
        "pixel_lease": lease(),
    }


def decode(data: bytes) -> list[dict]:
    stream = io.BytesIO(data)
    values = []
    while stream.tell() < len(data):
        values.append(read_frame(stream))
    return values


def encode(value: dict) -> bytes:
    data = json.dumps(value, separators=(",", ":")).encode()
    return struct.pack(">I", len(data)) + data


class ProtocolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.artifact_root = Path(self.temp.name) / PACK_ID / PACK_REVISION
        self.artifact_root.mkdir(parents=True)
        self.output = io.BytesIO()
        self.backend = DeterministicFixtureBackend({DIGEST: [face(0), face(1, x=60.0)]})
        self.controller = WorkerController(
            FrameWriter(self.output),
            launch_nonce="launch-secret",
            worker_instance_id="identity-worker-01",
            manifest_path=MANIFEST,
            backend=self.backend,
        )

    def tearDown(self) -> None:
        if self.controller.scheduler is not None:
            self.controller.scheduler.stop()
        self.temp.cleanup()

    def events(self) -> list[dict]:
        return decode(self.output.getvalue())

    def wait_terminal(self, request_id: str, timeout: float = 2.0) -> list[dict]:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            events = [event for event in self.events() if event["request_id"] == request_id]
            if any(event["terminal"] for event in events):
                return events
            time.sleep(0.005)
        self.fail(f"no terminal event for {request_id}")

    def handshake(self) -> None:
        self.controller.handle(envelope("handshake", 1, payload={"launch_nonce": "launch-secret", "supervisor": "npc-runtime"}))

    def load(self, sequence: int = 2, generation: int = 0) -> None:
        self.controller.handle(envelope("load", sequence, generation=generation, payload={
            "lease_id": "model-lease-01",
            "pack_id": PACK_ID,
            "revision": PACK_REVISION,
            "manifest_sha256": self.controller.pack.manifest_sha256,
            "artifact_root": str(self.artifact_root),
            "backend": "cpu",
            "cpu_threads": 2,
            "explicit_user_confirmation": True,
            "activation_mode": "private_evaluation",
            "verified_catalog_admission_sha256": None,
        }))

    def test_handshake_reports_non_authoritative_no_trait_contract(self) -> None:
        self.handshake()
        payload = self.events()[-1]["payload"]
        self.assertFalse(payload["character_selection"])
        self.assertFalse(payload["demographic_inference"])
        self.assertEqual(payload["output_authority"], "untrusted_observations_native_revalidation_required")

    def test_wrong_nonce_and_pre_handshake_operation_fail(self) -> None:
        self.controller.handle(envelope("health", 1))
        self.assertEqual(self.events()[-1]["error"]["code"], "handshake_required")
        other = WorkerController(FrameWriter(io.BytesIO()), launch_nonce="secret", worker_instance_id="identity-worker-01", manifest_path=MANIFEST, backend=DeterministicFixtureBackend())
        other.handle(envelope("handshake", 1, payload={"launch_nonce": "wrong", "supervisor": "npc-runtime"}))
        self.assertEqual(other.lifecycle, "starting")

    def test_successful_launch_nonce_is_consumed_and_cannot_be_replayed(self) -> None:
        self.handshake()
        replay = envelope(
            "handshake",
            2,
            request_id="replayed-handshake",
            payload={"launch_nonce": "launch-secret", "supervisor": "npc-runtime"},
        )
        replay["worker_instance_id"] = "identity-worker-01"
        self.controller.handle(replay)
        self.assertEqual(self.events()[-1]["error"]["code"], "authentication_failed")
        self.assertEqual(self.controller.lifecycle, "cold")

    def test_load_requires_explicit_activation_confirmation(self) -> None:
        self.handshake()
        payload = {
            "lease_id": "model-lease-01", "pack_id": PACK_ID, "revision": PACK_REVISION,
            "manifest_sha256": self.controller.pack.manifest_sha256, "artifact_root": str(self.artifact_root),
            "backend": "cpu", "cpu_threads": 2, "explicit_user_confirmation": False,
            "activation_mode": "private_evaluation", "verified_catalog_admission_sha256": None,
        }
        self.controller.handle(envelope("load", 2, payload=payload))
        self.assertEqual(self.events()[-1]["error"]["code"], "explicit_activation_confirmation_required")

    def test_qualified_catalog_load_requires_verified_admission_digest(self) -> None:
        self.handshake()
        payload = {
            "lease_id": "model-lease-01", "pack_id": PACK_ID, "revision": PACK_REVISION,
            "manifest_sha256": self.controller.pack.manifest_sha256,
            "artifact_root": str(self.artifact_root), "backend": "cpu", "cpu_threads": 2,
            "explicit_user_confirmation": True, "activation_mode": "qualified_catalog",
            "verified_catalog_admission_sha256": None,
        }
        self.controller.handle(envelope("load", 2, payload=payload))
        self.assertEqual(self.events()[-1]["error"]["code"], "verified_catalog_admission_required")
        payload["verified_catalog_admission_sha256"] = "c" * 64
        self.controller.handle(envelope("load", 3, request_id="qualified-load", payload=payload))
        result = self.events()[-1]["payload"]
        self.assertEqual(result["activation_mode"], "qualified_catalog")
        self.assertEqual(result["verified_catalog_admission_sha256"], "c" * 64)

    def test_wgc_infer_emits_exact_rust_shaped_observations_without_secrets(self) -> None:
        self.handshake(); self.load()
        self.controller.handle(envelope("infer", 3, request_id="infer-frame", payload=frame_payload()))
        events = self.wait_terminal("infer-frame")
        self.assertEqual([event["event"] for event in events], ["accepted", "identity_observations", "completed"])
        result = events[1]["payload"]
        self.assertEqual(result["authority"], "untrusted_worker_observations")
        self.assertTrue(result["native_revalidation_required"])
        self.assertEqual(result["frame_sequence"], 7)
        self.assertEqual(len(result["observations"]), 2)
        embedding = result["observations"][0]["embedding"]
        self.assertEqual(embedding["model"]["dimensions"], 128)
        self.assertEqual(embedding["metadata"]["source_frame_index"], 7)
        self.assertEqual(embedding["metadata"]["source_digest_sha256"], DIGEST)
        serialized = json.dumps(result)
        self.assertNotIn("shared_memory_name", serialized)
        self.assertNotIn("pixel-secret-lease-nonce", serialized)

    def test_reference_import_requires_exactly_one_face_and_emits_f32le(self) -> None:
        self.handshake(); self.load()
        self.backend.frames[DIGEST] = [face(0)]
        self.controller.handle(envelope("infer", 3, request_id="infer-reference", payload=reference_payload()))
        events = self.wait_terminal("infer-reference")
        result = events[1]["payload"]["portable_reference_import"]
        self.assertEqual(result["transport"], "f32_le")
        self.assertEqual(len(result["tensor_f32le"]), 512)
        self.assertEqual(hashlib.sha256(bytes(result["tensor_f32le"])).hexdigest(), result["tensor_sha256"])
        self.assertTrue(result["provenance"]["explicit_user_consent"])
        self.assertEqual(result["provenance"]["source_class"], "user_private")

    def test_reference_with_zero_or_multiple_faces_rejects_ambiguity(self) -> None:
        self.handshake(); self.load()
        for index, faces in enumerate(([], [face(0), face(1)]), start=3):
            self.backend.frames[DIGEST] = faces
            request_id = f"reference-{index}"
            self.controller.handle(envelope("infer", index, request_id=request_id, payload=reference_payload()))
            events = self.wait_terminal(request_id)
            self.assertEqual(events[-1]["error"]["code"], "reference_face_ambiguous")

    def test_queue_depth_one_rejects_second_request(self) -> None:
        self.handshake(); self.load()
        block = threading.Event()
        self.backend.block = block
        self.controller.handle(envelope("infer", 3, request_id="first-infer", payload=frame_payload()))
        self.controller.handle(envelope("infer", 4, request_id="second-infer", payload=frame_payload()))
        second = self.wait_terminal("second-infer")
        self.assertEqual(second[-1]["error"]["code"], "worker_busy")
        block.set()
        self.wait_terminal("first-infer")

    def test_cancel_suppresses_late_result_and_advances_exact_generation(self) -> None:
        self.handshake(); self.load()
        block = threading.Event()
        self.backend.block = block
        self.controller.handle(envelope("infer", 3, request_id="cancelled-infer", payload=frame_payload()))
        self.controller.handle(envelope("cancel", 4, request_id="cancel-01", generation=1, payload={"cancellation_generation": 1}))
        cancel = self.wait_terminal("cancel-01")
        self.assertEqual(cancel[-1]["payload"]["generation"], 1)
        block.set()
        time.sleep(0.03)
        events = [event for event in self.events() if event["request_id"] == "cancelled-infer"]
        self.assertEqual([event["event"] for event in events], ["accepted"])
        self.controller.handle(envelope("health", 5, request_id="stale-health", generation=0))
        self.assertEqual(self.events()[-1]["error"]["code"], "stale_generation")

    def test_result_that_finishes_after_deadline_is_never_published(self) -> None:
        self.handshake(); self.load()
        block = threading.Event()
        self.backend.block = block
        request = envelope("infer", 3, request_id="late-infer", payload=frame_payload())
        request["deadline_unix_ms"] = int(time.time() * 1000) + 40
        self.controller.handle(request)
        time.sleep(0.06)
        block.set()
        events = self.wait_terminal("late-infer")
        self.assertEqual([event["event"] for event in events], ["accepted", "error"])
        self.assertEqual(events[-1]["error"]["code"], "deadline_exceeded")

    def test_duplicate_sequence_unknown_fields_and_expired_deadline_fail(self) -> None:
        self.handshake()
        self.controller.handle(envelope("health", 1, request_id="bad-sequence"))
        self.assertEqual(self.events()[-1]["error"]["code"], "out_of_order_sequence")
        raw = envelope("health", 2)
        raw["debug"] = True
        with self.assertRaises(Exception):
            Request.parse(raw)
        raw = envelope("health", 2, request_id="expired")
        raw["deadline_unix_ms"] = int(time.time() * 1000) - 1
        self.controller.handle(raw)
        self.assertEqual(self.events()[-1]["error"]["code"], "deadline_exceeded")

    def test_health_marks_resources_unknown_not_fake_zero(self) -> None:
        self.handshake()
        self.controller.handle(envelope("health", 2))
        resources = self.events()[-1]["payload"]["resources"]
        self.assertIsNone(resources["resident_ram_bytes"])
        self.assertIsNone(resources["p99_operation_millis"])
        self.assertEqual(resources["measurement"], "this_pc_qualification_required")


class ProcessLifecycleTests(unittest.TestCase):
    def test_hidden_worker_can_restart_cleanly_without_loading_models(self) -> None:
        frames = encode(envelope("handshake", 1, payload={"launch_nonce": "process-secret", "supervisor": "npc-runtime"}))
        frames += encode(envelope("shutdown", 2))
        for _ in range(2):
            completed = subprocess.run(
                [sys.executable, str(ROOT / "worker.py"), "--launch-nonce", "process-secret", "--worker-instance-id", "identity-worker-01", "--manifest", str(MANIFEST)],
                input=frames,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=5,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
            events = decode(completed.stdout)
            self.assertEqual([event["event"] for event in events], ["completed", "completed"])
            self.assertEqual(events[-1]["payload"]["lifecycle"], "stopped")


class FramingTests(unittest.TestCase):
    def test_round_trip_and_bounds(self) -> None:
        stream = io.BytesIO()
        FrameWriter(stream).write({"value": 1})
        stream.seek(0)
        self.assertEqual(read_frame(stream), {"value": 1})
        with self.assertRaises(FramingError):
            read_frame(io.BytesIO(struct.pack(">I", 0)))


if __name__ == "__main__":
    unittest.main()
