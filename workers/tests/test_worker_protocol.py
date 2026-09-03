from __future__ import annotations

import io
import json
import queue
import struct
import subprocess
import sys
import threading
import time
import unittest
from pathlib import Path
from typing import Any

WORKERS = Path(__file__).resolve().parents[1]
STUBS = WORKERS / "stubs"
sys.path.insert(0, str(STUBS))

from contract import (  # noqa: E402
    MOUTH_RESIDUAL_CONTRACT_VERSION,
    MOUTH_RESIDUAL_FAIL_OPEN_REASONS,
    ContractError,
    Descriptor,
    MouthResidualProposalV1,
    MouthResidualRequestV1,
)
from framing import (  # noqa: E402
    MAX_FRAME_BYTES,
    EndOfStream,
    FrameWriter,
    InvalidJsonFrame,
    OversizedFrame,
    encode_frame,
    read_frame,
)
from lipsync_catalog import LipSyncPackCatalog  # noqa: E402


def lip_sync_payload(*, generation: int = 0) -> dict[str, Any]:
    return {
        "contract_version": MOUTH_RESIDUAL_CONTRACT_VERSION,
        "frame_lease_id": "frame-lease-000042",
        "audio_lease_id": "audio-lease-000017",
        "frame_lease_expires_qpc": 1_300_000,
        "audio_lease_expires_qpc": 1_300_000,
        "selected_encounter_id": "encounter-local-7",
        "selected_track_id": "track-local-2",
        "track_epoch": 3,
        "source_frame_sequence": 42,
        "source_capture_qpc": 1_000_000,
        "qpc_frequency_hz": 1_000_000,
        "face_region_normalized": {"x": 0.25, "y": 0.15, "width": 0.4, "height": 0.6},
        "landmark_bounds_normalized": {"x": 0.29, "y": 0.21, "width": 0.32, "height": 0.48},
        "mouth_mask_bounds_normalized": {"x": 0.38, "y": 0.55, "width": 0.14, "height": 0.1},
        "tracking_confidence": 0.94,
        "presentation_deadline_qpc": 1_200_000,
        "cancellation_generation": generation,
    }


class WorkerProcess:
    def __init__(self, descriptor_name: str) -> None:
        self.descriptor_path = WORKERS / "packs" / descriptor_name
        self.descriptor = Descriptor.load(self.descriptor_path)
        self.process = subprocess.Popen(
            [
                sys.executable,
                str(STUBS / "worker.py"),
                "--descriptor",
                str(self.descriptor_path),
                "--launch-nonce",
                "test-nonce",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        assert self.process.stdin is not None
        assert self.process.stdout is not None
        self.writer = FrameWriter(self.process.stdin)
        self.events: queue.Queue[dict[str, Any] | BaseException] = queue.Queue()
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        self.sequence = 0
        self.generation = 0
        self.closed = False

    def _read(self) -> None:
        assert self.process.stdout is not None
        while True:
            try:
                self.events.put(read_frame(self.process.stdout))
            except EndOfStream:
                return
            except BaseException as exc:
                self.events.put(exc)
                return

    def send(
        self,
        operation: str,
        payload: dict[str, Any] | None = None,
        *,
        generation: int | None = None,
        instance_id: str | None = None,
        request_id: str | None = None,
        sequence: int | None = None,
        deadline_unix_ms: int = 0,
    ) -> str:
        if sequence is None:
            self.sequence += 1
            sequence = self.sequence
        else:
            self.sequence = max(self.sequence, sequence)
        request_id = request_id or f"req-{sequence}"
        message = {
            "protocol_version": "1.0",
            "worker_instance_id": (
                "" if operation == "handshake" else self.descriptor.worker_id
            )
            if instance_id is None
            else instance_id,
            "request_id": request_id,
            "sequence": sequence,
            "generation": self.generation if generation is None else generation,
            "deadline_unix_ms": deadline_unix_ms,
            "operation": operation,
            "payload": payload or {},
        }
        self.writer.write(message)
        return request_id

    def next_event(self, timeout: float = 5.0) -> dict[str, Any]:
        value = self.events.get(timeout=timeout)
        if isinstance(value, BaseException):
            raise value
        return value

    def until_terminal(self, request_id: str, timeout: float = 5.0) -> list[dict[str, Any]]:
        deadline = time.monotonic() + timeout
        found = []
        while time.monotonic() < deadline:
            event = self.next_event(max(0.01, deadline - time.monotonic()))
            found.append(event)
            if event["request_id"] == request_id and event["terminal"]:
                return found
        raise TimeoutError(f"no terminal event for {request_id}")

    def handshake_and_load(self) -> None:
        request_id = self.send("handshake", {"launch_nonce": "test-nonce", "supervisor": "unit-test"})
        terminal = self.until_terminal(request_id)[-1]
        if terminal["event"] != "completed":
            raise AssertionError(terminal)
        self.until_terminal(self.send("warm"))
        terminal = self.until_terminal(
            self.send(
                "load",
                {"model_id": self.descriptor.default_model_id, "lease_id": "test-verified-lease"},
            )
        )[-1]
        if terminal["event"] != "completed":
            raise AssertionError(terminal)

    def close(self) -> None:
        if self.closed:
            return
        if self.process.poll() is None:
            try:
                request_id = self.send("shutdown")
                self.until_terminal(request_id, timeout=2)
            except (BrokenPipeError, TimeoutError, queue.Empty, EndOfStream):
                self.process.terminate()
            assert self.process.stdin is not None
            self.process.stdin.close()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        stderr = self.process.stderr.read().decode("utf-8", errors="replace") if self.process.stderr else ""
        if self.process.stdout:
            self.process.stdout.close()
        if self.process.stderr:
            self.process.stderr.close()
        if self.process.returncode not in {0, -15}:
            raise AssertionError(f"worker exited {self.process.returncode}: {stderr}")
        self.closed = True

    def crash(self) -> None:
        """Abruptly terminate this fixture worker without treating the exit as cleanup failure."""

        if self.closed:
            return
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=3)
        if self.process.stdin:
            self.process.stdin.close()
        if self.process.stdout:
            self.process.stdout.close()
        if self.process.stderr:
            self.process.stderr.close()
        self.closed = True


class FramingTests(unittest.TestCase):
    def test_frame_round_trip_is_canonical_and_bounded(self) -> None:
        message = {"b": "text", "a": 1}
        encoded = encode_frame(message)
        self.assertEqual(read_frame(io.BytesIO(encoded)), message)
        self.assertEqual(struct.unpack(">I", encoded[:4])[0], len(encoded) - 4)

    def test_oversized_length_is_rejected_before_payload_read(self) -> None:
        stream = io.BytesIO(struct.pack(">I", MAX_FRAME_BYTES + 1))
        with self.assertRaises(OversizedFrame):
            read_frame(stream)

    def test_non_object_json_is_rejected(self) -> None:
        body = json.dumps([1, 2, 3]).encode()
        with self.assertRaises(InvalidJsonFrame):
            read_frame(io.BytesIO(struct.pack(">I", len(body)) + body))

    def test_non_finite_numbers_are_rejected(self) -> None:
        body = b'{"value":NaN}'
        with self.assertRaises(InvalidJsonFrame):
            read_frame(io.BytesIO(struct.pack(">I", len(body)) + body))


class DescriptorTests(unittest.TestCase):
    def test_all_descriptors_are_safe_development_packs(self) -> None:
        descriptors = sorted((WORKERS / "packs").glob("*.stub-pack.json"))
        self.assertEqual(len(descriptors), 7)
        kinds = []
        for path in descriptors:
            descriptor = Descriptor.load(path)
            kinds.append(descriptor.kind)
            self.assertTrue(descriptor.raw["development_stub"])
            self.assertFalse(descriptor.raw["bundles_third_party"])
            self.assertFalse(descriptor.capabilities["network_access"])
            self.assertEqual(descriptor.raw["installation_owner"], "model_manager")
            self.assertIn("not a model benchmark", descriptor.resources["evidence"].lower())
        self.assertEqual(sorted(set(kinds)), ["embedding", "lip_sync", "llm", "stt", "tts", "vision"])

    def test_descriptor_rejects_ambiguous_third_party_bundling(self) -> None:
        raw = json.loads((WORKERS / "packs" / "llamacpp.stub-pack.json").read_text(encoding="utf-8"))
        raw["bundles_third_party"] = True
        with self.assertRaises(ContractError):
            Descriptor.validate(raw)

    def test_protocol_contract_documents_are_machine_readable(self) -> None:
        schema = json.loads((WORKERS / "protocol" / "worker-pack-v1.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
        lip_sync_schema = json.loads(
            (WORKERS / "protocol" / "lipsync-pack-catalog-v1.schema.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            lip_sync_schema["properties"]["selection_policy"]["properties"]["automatic_download"]["const"],
            False,
        )
        residual_schema = json.loads(
            (WORKERS / "protocol" / "mouth-residual-v1.schema.json").read_text(encoding="utf-8")
        )
        self.assertEqual(residual_schema["$defs"]["request"]["properties"]["contract_version"]["const"], MOUTH_RESIDUAL_CONTRACT_VERSION)
        constraints = residual_schema["$defs"]["residualConstraints"]["properties"]
        self.assertFalse(constraints["full_frame_replacement"]["const"])
        self.assertFalse(constraints["static_avatar_source"]["const"])
        self.assertEqual(residual_schema["$defs"]["freshness"]["properties"]["maximum_displayed_frames"]["const"], 1)
        self.assertEqual(
            set(residual_schema["$defs"]["failOpenReason"]["enum"]),
            MOUTH_RESIDUAL_FAIL_OPEN_REASONS,
        )
        proto = (WORKERS / "protocol" / "worker-control-v1.proto").read_text(encoding="utf-8")
        for contract in (
            "RequestEnvelopeV1",
            "EventEnvelopeV1",
            "CapabilitiesV1",
            "ResourceEstimateV1",
            "GenericLipSyncRequestV1",
            "MouthPatchProposalV1",
            "MouthResidualFailOpenV1",
            "MouthResidualConstraintsV1",
        ):
            self.assertIn(f"message {contract}", proto)
        pack_notes = (WORKERS / "packs" / "README.md").read_text(encoding="utf-8")
        self.assertIn("ModelPackManifestV1", pack_notes)
        self.assertIn("no third-party code or model data", pack_notes)

    def test_stub_runtime_denies_socket_creation(self) -> None:
        script = (
            "import sys; "
            f"sys.path.insert(0, {str(STUBS)!r}); "
            "from worker import install_network_denial; install_network_denial(); "
            "import socket; socket.socket()"
        )
        result = subprocess.run([sys.executable, "-c", script], capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"worker network access is disabled", result.stderr)


class LifecycleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.worker = WorkerProcess("llamacpp.stub-pack.json")

    def tearDown(self) -> None:
        self.worker.close()

    def test_handshake_capabilities_health_warm_load_unload_are_idempotent(self) -> None:
        handshake = self.worker.send("handshake", {"launch_nonce": "test-nonce"})
        event = self.worker.until_terminal(handshake)[-1]
        capabilities = event["payload"]["capabilities"]
        self.assertEqual(capabilities["worker_kind"], "llm")
        self.assertFalse(capabilities["network_access"])
        self.assertFalse(capabilities["third_party_payloads_bundled"])

        for operation in ("capabilities", "health", "warm", "warm"):
            self.assertEqual(self.worker.until_terminal(self.worker.send(operation))[-1]["event"], "completed")

        payload = {"model_id": "fixture.llm.echo-v1", "lease_id": "lease-1"}
        first = self.worker.until_terminal(self.worker.send("load", payload))[-1]
        second = self.worker.until_terminal(self.worker.send("load", payload))[-1]
        self.assertFalse(first["payload"]["already_loaded"])
        self.assertTrue(second["payload"]["already_loaded"])

        for _ in range(2):
            unloaded = self.worker.until_terminal(self.worker.send("unload"))[-1]
            self.assertEqual(unloaded["payload"]["lifecycle"], "warm")

    def test_handshake_authentication_and_ordering_are_enforced(self) -> None:
        early = self.worker.send("health")
        self.assertEqual(self.worker.until_terminal(early)[-1]["error"]["code"], "handshake_required")
        wrong = self.worker.send("handshake", {"launch_nonce": "wrong"})
        self.assertEqual(self.worker.until_terminal(wrong)[-1]["error"]["code"], "authentication_failed")
        valid = self.worker.send("handshake", {"launch_nonce": "test-nonce"})
        self.assertEqual(self.worker.until_terminal(valid)[-1]["event"], "completed")
        duplicate = self.worker.send("health", request_id="duplicate-id")
        self.worker.until_terminal(duplicate)
        duplicate_again = self.worker.send("health", request_id="duplicate-id")
        self.assertEqual(self.worker.until_terminal(duplicate_again)[-1]["error"]["code"], "duplicate_request")

    def test_cancel_advances_generation_and_stale_requests_are_rejected(self) -> None:
        self.worker.handshake_and_load()
        infer = self.worker.send(
            "infer",
            {"prompt": " ".join(["slow"] * 200), "max_tokens": 256, "fixture_event_delay_ms": 10},
        )
        accepted = self.worker.next_event()
        self.assertEqual((accepted["request_id"], accepted["event"]), (infer, "accepted"))
        self.worker.generation = 1
        cancel = self.worker.send("cancel", generation=1)
        events = self.worker.until_terminal(cancel)
        cancel_index = next(index for index, event in enumerate(events) if event["request_id"] == cancel)
        self.assertTrue(events[cancel_index]["payload"]["advanced"])

        stale = self.worker.send("infer", {"prompt": "late"}, generation=0)
        stale_event = self.worker.until_terminal(stale)[-1]
        self.assertEqual(stale_event["error"]["code"], "stale_generation")

        health_id = self.worker.send("health", generation=1)
        after_barrier = self.worker.until_terminal(health_id)
        self.assertFalse(any(event["request_id"] == infer for event in after_barrier))

        retry_cancel = self.worker.send("cancel", generation=1)
        retried = self.worker.until_terminal(retry_cancel)[-1]
        self.assertFalse(retried["payload"]["advanced"])

    def test_deadline_and_instance_binding_are_enforced(self) -> None:
        self.worker.handshake_and_load()
        expired = self.worker.send("health", deadline_unix_ms=int(time.time() * 1000) - 1)
        self.assertEqual(self.worker.until_terminal(expired)[-1]["error"]["code"], "deadline_exceeded")
        wrong_instance = self.worker.send("health", instance_id="worker-someone-else")
        self.assertEqual(self.worker.until_terminal(wrong_instance)[-1]["error"]["code"], "instance_mismatch")

    def test_crashed_worker_is_replaced_without_replaying_the_abandoned_request(self) -> None:
        self.worker.handshake_and_load()
        abandoned_id = self.worker.send(
            "infer",
            {"prompt": "abandoned before durable delivery", "max_tokens": 32, "fixture_event_delay_ms": 1000},
            request_id="abandoned-before-crash",
        )
        accepted = self.worker.next_event()
        self.assertEqual((accepted["request_id"], accepted["event"], accepted["terminal"]), (abandoned_id, "accepted", False))
        self.worker.crash()

        replacement = WorkerProcess("llamacpp.stub-pack.json")
        try:
            replacement.handshake_and_load()
            recovered_id = replacement.send(
                "infer",
                {"prompt": "explicit retry after restart"},
                request_id="recovered-after-crash",
            )
            recovered_events = replacement.until_terminal(recovered_id)
            self.assertTrue(any(event["event"] == "llm_result" for event in recovered_events))
            self.assertEqual(recovered_events[-1]["event"], "completed")
            self.assertTrue(all(event["request_id"] != abandoned_id for event in recovered_events))
        finally:
            replacement.close()


class ModalityTests(unittest.TestCase):
    CASES = [
        ("llamacpp.stub-pack.json", {"prompt": "Hello there"}, "llm_result"),
        ("moonshine.stub-pack.json", {"transcript_hint": "Can you hear me?", "language": "en"}, "transcript"),
        ("whispercpp.stub-pack.json", {"transcript_hint": "Bonjour tout le monde", "language": "fr"}, "transcript"),
        ("kokoro.stub-pack.json", {"text": "A short line.", "sample_rate_hz": 16000}, "tts_result"),
        ("onnx-embedding.stub-pack.json", {"texts": ["alpha", "beta"], "dimensions": 8}, "embedding_result"),
        ("onnx-vision.stub-pack.json", {"frame_digest": "sha256:fixture", "labels": ["face", "speaker"]}, "vision_result"),
        ("experimental-lipsync.stub-pack.json", lip_sync_payload(), "lip_sync_result"),
    ]

    def test_all_modality_workers_emit_deterministic_results(self) -> None:
        for descriptor, payload, expected_event in self.CASES:
            with self.subTest(descriptor=descriptor):
                worker = WorkerProcess(descriptor)
                try:
                    worker.handshake_and_load()
                    first_id = worker.send("infer", payload)
                    first_events = worker.until_terminal(first_id)
                    first = next(event["payload"] for event in first_events if event["event"] == expected_event)

                    second_id = worker.send("infer", payload)
                    second_events = worker.until_terminal(second_id)
                    second = next(event["payload"] for event in second_events if event["event"] == expected_event)
                    self.assertEqual(first, second)
                    self.assertTrue(first["deterministic"])
                finally:
                    worker.close()

    def test_generic_lipsync_proposal_is_exact_frame_bound_metadata_only(self) -> None:
        worker = WorkerProcess("experimental-lipsync.stub-pack.json")
        try:
            worker.handshake_and_load()
            request_id = worker.send("infer", lip_sync_payload())
            events = worker.until_terminal(request_id)
            proposal = next(event["payload"] for event in events if event["event"] == "mouth_patch_proposal")
            self.assertEqual(proposal["source_frame_sequence"], 42)
            self.assertEqual(proposal["source_capture_qpc"], 1_000_000)
            self.assertEqual(proposal["cancellation_generation"], 0)
            self.assertEqual(proposal["contract_version"], MOUTH_RESIDUAL_CONTRACT_VERSION)
            self.assertEqual(proposal["frame_lease_id"], "frame-lease-000042")
            self.assertEqual(proposal["audio_lease_id"], "audio-lease-000017")
            self.assertEqual(proposal["selected_encounter_id"], "encounter-local-7")
            self.assertEqual(proposal["selected_track_id"], "track-local-2")
            self.assertEqual(proposal["track_epoch"], 3)
            self.assertEqual(
                proposal["landmark_bounds_normalized"],
                {"x": 0.29, "y": 0.21, "width": 0.32, "height": 0.48},
            )
            self.assertEqual(
                proposal["mask_bounds_normalized"],
                {"x": 0.38, "y": 0.55, "width": 0.14, "height": 0.1},
            )
            self.assertEqual(proposal["tracking_confidence"], 0.94)
            self.assertGreaterEqual(proposal["residual_confidence"], 0.0)
            self.assertLessEqual(proposal["residual_confidence"], 1.0)
            self.assertEqual(proposal["output_semantics"], "additive_rgba_mouth_residual")
            self.assertTrue(proposal["no_pixels_inline"])
            self.assertTrue(proposal["metadata_only"])
            self.assertFalse(proposal["presentable"])
            self.assertFalse(proposal["image_modified"])
            self.assertIsNone(proposal["patch_lease_id"])
            self.assertTrue(proposal["freshness"]["requires_exact_source_frame_sequence"])
            self.assertTrue(proposal["freshness"]["discard_if_source_advanced"])
            self.assertTrue(proposal["freshness"]["discard_if_track_epoch_changed"])
            self.assertTrue(proposal["freshness"]["discard_if_generation_changed"])
            self.assertTrue(proposal["freshness"]["restore_unmodified_on_rejection"])
            self.assertEqual(proposal["freshness"]["valid_source_frame_sequence"], 42)
            self.assertEqual(proposal["freshness"]["discard_at_or_after_frame_sequence"], 43)
            self.assertEqual(proposal["freshness"]["maximum_source_frame_advance"], 0)
            self.assertEqual(proposal["freshness"]["maximum_displayed_frames"], 1)
            self.assertEqual(proposal["fail_open"], {"use_unmodified_source_frame": True, "reasons": ["metadata_only_stub"]})
            self.assertEqual(
                proposal["residual_constraints"],
                {
                    "full_frame_replacement": False,
                    "static_avatar_source": False,
                    "base_frame_mutation": False,
                    "alpha_outside_mask_zero": True,
                    "mask_must_remain_inside_landmarks": True,
                },
            )
            serialized = json.dumps(proposal, sort_keys=True)
            for forbidden in ("image_b64", "audio_b64", "frame_path", "audio_path"):
                self.assertNotIn(forbidden, serialized)
            self.assertNotIn("pixels", proposal)
            self.assertNotIn("image", proposal)
            self.assertNotIn("audio", proposal)
        finally:
            worker.close()

    def test_generic_lipsync_rejects_inline_media_paths_and_invalid_regions(self) -> None:
        invalid_payloads = []
        inline = lip_sync_payload()
        inline["image_b64"] = "AA=="
        invalid_payloads.append(inline)
        path = lip_sync_payload()
        path["frame_lease_id"] = "C:\\captured\\frame.png"
        invalid_payloads.append(path)
        outside = lip_sync_payload()
        outside["mouth_mask_bounds_normalized"] = {"x": 0.1, "y": 0.1, "width": 0.1, "height": 0.1}
        invalid_payloads.append(outside)
        replacement = lip_sync_payload()
        replacement["full_frame_replacement"] = True
        invalid_payloads.append(replacement)
        static_avatar = lip_sync_payload()
        static_avatar["avatar_image_lease_id"] = "avatar-static-1"
        invalid_payloads.append(static_avatar)

        for payload in invalid_payloads:
            with self.subTest(payload=payload):
                worker = WorkerProcess("experimental-lipsync.stub-pack.json")
                try:
                    worker.handshake_and_load()
                    terminal = worker.until_terminal(worker.send("infer", payload))[-1]
                    self.assertEqual(terminal["error"]["code"], "invalid_payload")
                finally:
                    worker.close()

    def test_generic_lipsync_rejects_stale_deadlines_leases_and_generation_mismatch(self) -> None:
        cases: list[tuple[dict[str, Any], str]] = []
        expired_frame = lip_sync_payload()
        expired_frame["presentation_deadline_qpc"] = expired_frame["source_capture_qpc"]
        cases.append((expired_frame, "stale_source_frame"))
        expired_lease = lip_sync_payload()
        expired_lease["frame_lease_expires_qpc"] = expired_lease["presentation_deadline_qpc"] - 1
        cases.append((expired_lease, "stale_media_lease"))
        wrong_generation = lip_sync_payload(generation=1)
        cases.append((wrong_generation, "generation_mismatch"))

        for payload, code in cases:
            with self.subTest(code=code):
                worker = WorkerProcess("experimental-lipsync.stub-pack.json")
                try:
                    worker.handshake_and_load()
                    terminal = worker.until_terminal(worker.send("infer", payload))[-1]
                    self.assertEqual(terminal["error"]["code"], code)
                finally:
                    worker.close()

    def test_mouth_residual_type_rejects_missing_epoch_confidence_and_bad_containment(self) -> None:
        valid = lip_sync_payload()
        parsed = MouthResidualRequestV1.parse(valid)
        self.assertEqual((parsed.selected_track_id, parsed.track_epoch), ("track-local-2", 3))

        invalid_payloads = []
        missing_epoch = lip_sync_payload()
        del missing_epoch["track_epoch"]
        invalid_payloads.append(missing_epoch)
        bad_confidence = lip_sync_payload()
        bad_confidence["tracking_confidence"] = 1.01
        invalid_payloads.append(bad_confidence)
        bad_landmarks = lip_sync_payload()
        bad_landmarks["landmark_bounds_normalized"] = {"x": 0.1, "y": 0.1, "width": 0.8, "height": 0.8}
        invalid_payloads.append(bad_landmarks)
        wrong_version = lip_sync_payload()
        wrong_version["contract_version"] = "npc.mouth-residual/v2"
        invalid_payloads.append(wrong_version)

        for payload in invalid_payloads:
            with self.subTest(payload=payload):
                with self.assertRaises(ContractError):
                    MouthResidualRequestV1.parse(payload)

        with self.assertRaises(ContractError):
            MouthResidualProposalV1(
                request=parsed,
                residual_confidence=0.9,
                fail_open_reasons=("unsupported_reason",),
            )
        with self.assertRaises(ContractError):
            MouthResidualProposalV1(
                request=parsed,
                residual_confidence=0.9,
                fail_open_reasons=("metadata_only_stub",),
                patch_lease_id="residual-lease-1",
                presentable=True,
                metadata_only=True,
            )

    def test_generic_lipsync_obeys_cancel_barrier_and_rebinds_new_generation(self) -> None:
        worker = WorkerProcess("experimental-lipsync.stub-pack.json")
        try:
            worker.handshake_and_load()
            worker.generation = 1
            cancel_id = worker.send("cancel", generation=1)
            self.assertTrue(worker.until_terminal(cancel_id)[-1]["payload"]["advanced"])

            stale_id = worker.send("infer", lip_sync_payload(generation=0), generation=0)
            self.assertEqual(worker.until_terminal(stale_id)[-1]["error"]["code"], "stale_generation")

            rebound_id = worker.send("infer", lip_sync_payload(generation=1), generation=1)
            rebound = worker.until_terminal(rebound_id)
            proposal = next(event for event in rebound if event["event"] == "mouth_patch_proposal")
            self.assertEqual(proposal["generation"], 1)
            self.assertEqual(proposal["payload"]["cancellation_generation"], 1)
        finally:
            worker.close()

    def test_large_inline_embedding_is_rejected_instead_of_overflowing_frame(self) -> None:
        worker = WorkerProcess("onnx-embedding.stub-pack.json")
        try:
            worker.handshake_and_load()
            request_id = worker.send("infer", {"texts": ["x"] * 256, "dimensions": 4096})
            terminal = worker.until_terminal(request_id)[-1]
            self.assertEqual(terminal["error"]["code"], "inline_result_too_large")
        finally:
            worker.close()


class LipSyncCatalogTests(unittest.TestCase):
    def setUp(self) -> None:
        self.path = WORKERS / "packs" / "lipsync-pack-catalog-v1.development.json"
        self.catalog = LipSyncPackCatalog.load(self.path)

    def test_catalog_is_non_downloading_model_free_and_game_agnostic(self) -> None:
        raw = self.catalog.raw
        self.assertTrue(raw["development_descriptor"])
        self.assertFalse(raw["downloadable_catalog"])
        self.assertFalse(raw["contains_download_locations"])
        self.assertFalse(raw["contains_model_payloads"])
        self.assertFalse(raw["network_access"])
        self.assertFalse(raw["game_specific_adapters"])
        self.assertFalse(raw["selection_policy"]["automatic_selection"])
        self.assertFalse(raw["selection_policy"]["automatic_download"])
        self.assertEqual(raw["selection_policy"]["fallback_chain"], [])
        self.assertEqual(
            {entry["id"] for entry in raw["candidates"]},
            {
                "audio2face-3d-regression-v2.3",
                "nvidia-ar-sdk-lipsync",
                "musetalk-1.5",
                "tracked-mouth-warp",
                "ditto",
                "latentsync",
            },
        )
        serialized = json.dumps(raw).lower()
        self.assertNotIn("https://", serialized)
        self.assertNotIn("http://", serialized)

    def test_selection_requires_explicit_user_installed_verified_qualified_pack(self) -> None:
        blocked = self.catalog.selection_eligibility(
            "musetalk-1.5",
            mode="live",
            user_initiated=False,
            installed=False,
            verified=False,
            qualified=False,
            platform="windows_x64",
        )
        self.assertFalse(blocked.eligible)
        self.assertEqual(
            blocked.reasons,
            (
                "user_initiation_required",
                "live_selection_not_allowed",
                "pack_not_installed",
                "pack_not_verified",
                "pack_not_qualified",
            ),
        )
        eligible = self.catalog.selection_eligibility(
            "musetalk-1.5",
            mode="offline",
            user_initiated=True,
            installed=True,
            verified=True,
            qualified=True,
            platform="windows_x64",
        )
        self.assertTrue(eligible.eligible)
        self.assertEqual(eligible.reasons, ())

    def test_private_deferred_and_offline_only_candidates_enforce_status(self) -> None:
        common = {
            "user_initiated": True,
            "installed": True,
            "verified": True,
            "qualified": True,
            "platform": "windows_x64",
        }
        nvidia = self.catalog.selection_eligibility("nvidia-ar-sdk-lipsync", mode="live", **common)
        self.assertEqual(nvidia.reasons, ("private_access_required",))
        nvidia_with_access = self.catalog.selection_eligibility(
            "nvidia-ar-sdk-lipsync", mode="live", private_access_confirmed=True, **common
        )
        self.assertTrue(nvidia_with_access.eligible)
        audio2face = self.catalog.selection_eligibility(
            "audio2face-3d-regression-v2.3", mode="live", **common
        )
        self.assertIn("live_selection_not_allowed", audio2face.reasons)
        self.assertIn(
            "live_selection_not_allowed",
            self.catalog.selection_eligibility("ditto", mode="live", **common).reasons,
        )
        self.assertIn(
            "live_selection_not_allowed",
            self.catalog.selection_eligibility("latentsync", mode="live", **common).reasons,
        )
        self.assertTrue(self.catalog.selection_eligibility("latentsync", mode="offline", **common).eligible)

    def test_catalog_rejects_automatic_download_and_unexpected_download_fields(self) -> None:
        raw = json.loads(self.path.read_text(encoding="utf-8"))
        raw["selection_policy"]["automatic_download"] = True
        with self.assertRaises(ContractError):
            LipSyncPackCatalog.validate(raw)
        raw = json.loads(self.path.read_text(encoding="utf-8"))
        raw["download_url"] = "https://example.invalid/model"
        with self.assertRaises(ContractError):
            LipSyncPackCatalog.validate(raw)


if __name__ == "__main__":
    unittest.main()
