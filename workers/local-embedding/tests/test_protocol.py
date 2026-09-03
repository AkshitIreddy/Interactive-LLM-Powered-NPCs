from __future__ import annotations

import hashlib
import io
import json
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from backend import DeterministicFixtureBackend  # noqa: E402
from framing import FrameWriter, FramingError, read_frame  # noqa: E402
from manifest_builder import build_manifest  # noqa: E402
from model_spec import TOKENIZER_SHA256  # noqa: E402
from protocol import Request, WorkerController  # noqa: E402
from scheduler import EmbeddingScheduler, SchedulerError  # noqa: E402


def manifest() -> dict[str, object]:
    return build_manifest(tokenizer_sha256=TOKENIZER_SHA256)


def envelope(operation: str, *, request_id: str, sequence: int, generation: int = 0, payload: dict | None = None) -> dict:
    return {
        "protocol_version": "1.0",
        "worker_instance_id": "worker-instance-01" if operation != "handshake" else "",
        "request_id": request_id,
        "sequence": sequence,
        "generation": generation,
        "deadline_unix_ms": 0,
        "operation": operation,
        "payload": payload or {},
    }


def embedding_payload(text: str = "queue pressure") -> dict[str, object]:
    return {
        "contract_version": "npc.embedding-request/v1",
        "mode": "passage",
        "purpose": "memory_retrieval",
        "priority": "background",
        "items": [
            {
                "input_id": "input-queue",
                "text": text,
                "source_content_sha256": hashlib.sha256(text.encode()).hexdigest(),
            }
        ],
    }


def decode_frames(data: bytes) -> list[dict]:
    stream = io.BytesIO(data)
    values = []
    while stream.tell() < len(data):
        values.append(read_frame(stream))
    return values


def encode_frame(value: dict) -> bytes:
    encoded = json.dumps(value, separators=(",", ":")).encode("utf-8")
    return struct.pack(">I", len(encoded)) + encoded


class FramingTests(unittest.TestCase):
    def test_round_trip(self) -> None:
        stream = io.BytesIO()
        FrameWriter(stream).write({"safe": "value"})
        stream.seek(0)
        self.assertEqual(read_frame(stream), {"safe": "value"})

    def test_zero_length_rejected(self) -> None:
        with self.assertRaises(FramingError):
            read_frame(io.BytesIO(struct.pack(">I", 0)))

    def test_non_object_rejected(self) -> None:
        encoded = b"[]"
        with self.assertRaises(FramingError):
            read_frame(io.BytesIO(struct.pack(">I", len(encoded)) + encoded))


class ProtocolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.manifest = Path(self.temporary.name) / "manifest.json"
        self.manifest.write_text(json.dumps(manifest()), encoding="utf-8")
        self.output = io.BytesIO()
        self.controller = WorkerController(
            FrameWriter(self.output),
            launch_nonce="nonce-secret",
            worker_instance_id="worker-instance-01",
            manifest_path=self.manifest,
            backend=DeterministicFixtureBackend(),
        )

    def tearDown(self) -> None:
        if self.controller.scheduler:
            self.controller.scheduler.stop()
        self.temporary.cleanup()

    def events(self) -> list[dict]:
        return decode_frames(self.output.getvalue())

    def handshake(self) -> None:
        self.controller.handle(
            envelope(
                "handshake",
                request_id="request-01",
                sequence=1,
                payload={"launch_nonce": "nonce-secret", "supervisor": "npc-runtime"},
            )
        )

    def test_handshake_binds_instance_and_reports_capabilities(self) -> None:
        self.handshake()
        event = self.events()[-1]
        self.assertEqual(event["event"], "completed")
        self.assertEqual(event["payload"]["kind"], "embedding")
        self.assertEqual(event["payload"]["capabilities"]["compute_backends"], ["cpu"])

    def test_nonce_mismatch_fails_without_entering_cold(self) -> None:
        self.controller.handle(
            envelope(
                "handshake",
                request_id="request-01",
                sequence=1,
                payload={"launch_nonce": "wrong", "supervisor": "npc-runtime"},
            )
        )
        self.assertEqual(self.events()[-1]["error"]["code"], "authentication_failed")
        self.assertEqual(self.controller.lifecycle, "starting")

    def test_handshake_is_required(self) -> None:
        self.controller.handle(envelope("health", request_id="request-01", sequence=1))
        self.assertEqual(self.events()[-1]["error"]["code"], "handshake_required")

    def test_duplicate_request_rejected(self) -> None:
        self.handshake()
        self.controller.handle(envelope("health", request_id="duplicate-id", sequence=2))
        self.controller.handle(envelope("health", request_id="duplicate-id", sequence=3))
        self.assertEqual(self.events()[-1]["error"]["code"], "duplicate_request")

    def test_non_increasing_sequence_rejected(self) -> None:
        self.handshake()
        self.controller.handle(envelope("health", request_id="request-02", sequence=1))
        self.assertEqual(self.events()[-1]["error"]["code"], "out_of_order_sequence")

    def test_health_reports_unknown_measurements_as_null(self) -> None:
        self.handshake()
        self.controller.handle(envelope("health", request_id="request-02", sequence=2))
        resources = self.events()[-1]["payload"]["resources"]
        self.assertIsNone(resources["load_millis"])
        self.assertIsNone(resources["vram"]["resident_bytes"])

    def test_infer_before_load_rejected(self) -> None:
        self.handshake()
        self.controller.handle(envelope("infer", request_id="request-02", sequence=2))
        self.assertEqual(self.events()[-1]["error"]["code"], "model_not_loaded")

    def test_rejected_queue_does_not_emit_accepted_event(self) -> None:
        self.handshake()

        class RejectingScheduler:
            def submit(self, _job):  # type: ignore[no-untyped-def]
                raise SchedulerError("worker_busy", "background embedding queue is full", retryable=True)

        self.controller.lifecycle = "loaded"
        self.controller.scheduler = RejectingScheduler()  # type: ignore[assignment]
        self.controller.handle(envelope("infer", request_id="request-02", sequence=2, payload=embedding_payload()))
        request_events = [event for event in self.events() if event["request_id"] == "request-02"]
        self.assertEqual(len(request_events), 1)
        self.assertEqual(request_events[0]["error"]["code"], "worker_busy")
        self.controller.scheduler = None

    def test_cancelled_infer_emits_no_late_tensor_or_terminal_event(self) -> None:
        self.handshake()
        backend = DeterministicFixtureBackend(delay_seconds=0.25)
        self.controller.backend = backend
        self.controller.lifecycle = "loaded"
        self.controller.scheduler = EmbeddingScheduler(backend, self.controller.telemetry, batch_window_ms=0)
        self.controller.handle(
            envelope("infer", request_id="request-02", sequence=2, payload=embedding_payload("cancel this"))
        )
        time.sleep(0.03)
        self.controller.handle(
            envelope(
                "cancel",
                request_id="request-03",
                sequence=3,
                generation=1,
                payload={"cancellation_generation": 1},
            )
        )
        self.assertTrue(self.controller.scheduler.drain(1))
        infer_events = [event for event in self.events() if event["request_id"] == "request-02"]
        self.assertEqual([event["event"] for event in infer_events], ["accepted"])
        self.assertFalse(infer_events[0]["terminal"])

    def test_cancel_is_exact_generation_barrier(self) -> None:
        self.handshake()
        self.controller.handle(
            envelope(
                "cancel",
                request_id="request-02",
                sequence=2,
                generation=1,
                payload={"cancellation_generation": 1},
            )
        )
        self.assertEqual(self.events()[-1]["payload"]["generation"], 1)
        self.controller.handle(envelope("health", request_id="request-03", sequence=3, generation=0))
        self.assertEqual(self.events()[-1]["error"]["code"], "stale_generation")

    def test_cancel_gap_rejected(self) -> None:
        self.handshake()
        self.controller.handle(
            envelope(
                "cancel",
                request_id="request-02",
                sequence=2,
                generation=2,
                payload={"cancellation_generation": 2},
            )
        )
        self.assertEqual(self.events()[-1]["error"]["code"], "generation_gap")

    def test_expired_envelope_fails_before_operation(self) -> None:
        self.handshake()
        raw = envelope("health", request_id="request-02", sequence=2)
        raw["deadline_unix_ms"] = int(time.time() * 1000) - 1
        self.controller.handle(raw)
        self.assertEqual(self.events()[-1]["error"]["code"], "deadline_exceeded")

    def test_shutdown_is_terminal_and_idempotent_cleanup(self) -> None:
        self.handshake()
        self.controller.handle(envelope("shutdown", request_id="request-02", sequence=2))
        self.assertTrue(self.controller.should_exit.is_set())
        self.assertEqual(self.controller.lifecycle, "stopped")

    def test_unknown_envelope_field_rejected(self) -> None:
        raw = envelope("health", request_id="request-01", sequence=1)
        raw["debug"] = True
        with self.assertRaises(Exception):
            Request.parse(raw)


class WorkerProcessTests(unittest.TestCase):
    def test_stdio_process_handshake_health_shutdown(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(manifest()), encoding="utf-8")
            requests = [
                envelope(
                    "handshake",
                    request_id="request-01",
                    sequence=1,
                    payload={"launch_nonce": "nonce-secret", "supervisor": "npc-runtime"},
                ),
                envelope("health", request_id="request-02", sequence=2),
                envelope("shutdown", request_id="request-03", sequence=3),
            ]
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "worker.py"),
                    "--launch-nonce",
                    "nonce-secret",
                    "--worker-instance-id",
                    "worker-instance-01",
                    "--manifest",
                    str(path),
                ],
                input=b"".join(encode_frame(value) for value in requests),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=5,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            events = decode_frames(result.stdout)
            self.assertEqual([event["event"] for event in events], ["completed", "completed", "completed"])
            self.assertEqual(events[1]["payload"]["lifecycle"], "cold")
            self.assertEqual(events[2]["payload"]["lifecycle"], "stopped")

    def test_network_audit_hook_rejects_socket_creation(self) -> None:
        code = (
            "import socket,sys;"
            f"sys.path.insert(0,{str(ROOT)!r});"
            "from worker import install_network_denial;"
            "install_network_denial();"
            "\ntry: socket.socket()\nexcept PermissionError: raise SystemExit(0)\nraise SystemExit(9)"
        )
        result = subprocess.run(
            [sys.executable, "-c", code],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))


if __name__ == "__main__":
    unittest.main()
