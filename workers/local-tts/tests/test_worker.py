from __future__ import annotations

import io
import sys
import time
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
REPO = LOCAL_TTS.parents[1]
sys.path.insert(0, str(LOCAL_TTS))

from backend import FixtureBackend
from manifest import load_manifest
from pcm_transport import PcmWriter, read_pcm_record
from protocol import FrameWriter, PROTOCOL_VERSION, read_frame
from worker import WorkerController

MANIFEST = load_manifest(
    REPO
    / "packaging"
    / "model-packs"
    / "kokoro-sherpa-onnx-v1.0-int8-windows-x64.json"
)


def request(
    sequence: int,
    operation: str,
    payload: dict,
    *,
    generation: int = 0,
    instance: str = "worker-test-1",
) -> dict:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "worker_instance_id": "" if operation == "handshake" else instance,
        "request_id": f"request-{sequence}",
        "sequence": sequence,
        "generation": generation,
        "deadline_unix_ms": int(time.time() * 1000) + 10_000,
        "operation": operation,
        "payload": payload,
    }


class ControllerHarness:
    def __init__(self, backend: FixtureBackend | None = None) -> None:
        self.control = io.BytesIO()
        self.pcm = io.BytesIO()
        self.backend = backend or FixtureBackend(callbacks=6)
        self.controller = WorkerController(
            manifest=MANIFEST,
            pack_root=Path("/fixture/pack"),
            backend=self.backend,
            control_writer=FrameWriter(self.control),
            pcm_writer=PcmWriter(self.pcm),
            launch_nonce="nonce-test",
            worker_instance_id="worker-test-1",
        )

    def handle(self, raw: dict) -> None:
        self.controller.handle(raw)

    def wait_idle(self) -> None:
        deadline = time.time() + 3
        while self.controller.jobs and time.time() < deadline:
            time.sleep(0.01)
        if self.controller.jobs:
            raise AssertionError("fixture synthesis did not drain")

    def events(self) -> list[dict]:
        self.control.seek(0)
        rows = []
        while self.control.tell() < len(self.control.getvalue()):
            rows.append(read_frame(self.control))
        return rows

    def pcm_records(self) -> list[tuple[dict, bytes]]:
        self.pcm.seek(0)
        rows = []
        while self.pcm.tell() < len(self.pcm.getvalue()):
            rows.append(read_pcm_record(self.pcm))
        return rows


class WorkerTests(unittest.TestCase):
    def ready(self, harness: ControllerHarness) -> None:
        harness.handle(
            request(
                1,
                "handshake",
                {"launch_nonce": "nonce-test", "supervisor": "test"},
            )
        )
        harness.handle(
            request(
                2,
                "load",
                {
                    "pack_id": MANIFEST.pack_id,
                    "revision": MANIFEST.revision,
                    "lease_id": "lease-1",
                    "num_threads": 2,
                },
            )
        )

    def test_handshake_discovery_load_and_streaming_synthesis(self) -> None:
        harness = ControllerHarness()
        harness.handle(
            request(
                1,
                "handshake",
                {"launch_nonce": "nonce-test", "supervisor": "test"},
            )
        )
        harness.handle(request(2, "discover_voices", {}))
        harness.handle(
            request(
                3,
                "load",
                {
                    "pack_id": MANIFEST.pack_id,
                    "revision": MANIFEST.revision,
                    "lease_id": "lease-1",
                },
            )
        )
        harness.handle(
            request(
                4,
                "synthesize",
                {
                    "audio_stream_id": "audio-1",
                    "text": "Hello there.",
                    "voice_id": "af_heart",
                    "speed": 1.0,
                },
            )
        )
        harness.wait_idle()
        events = harness.events()
        self.assertEqual(events[0]["payload"]["lifecycle"], "cold")
        discovery = next(
            event
            for event in events
            if event["request_id"] == "request-2"
        )
        self.assertEqual(discovery["payload"]["count"], 28)
        self.assertTrue(
            all(
                voice["voice_cloning"] is False
                for voice in discovery["payload"]["voices"]
            )
        )
        result = next(event for event in events if event["event"] == "tts_result")
        self.assertEqual(result["payload"]["frames"], 2880)
        self.assertEqual(
            result["payload"]["timing"]["kind"],
            "exact_pcm_sample_clock",
        )
        self.assertEqual(
            result["payload"]["visemes"]["availability"],
            "unavailable",
        )
        self.assertNotIn("Hello there.", repr(events))
        records = harness.pcm_records()
        self.assertEqual(records[-1][0]["record"], "end")
        self.assertEqual(records[-1][0]["status"], "completed")
        self.assertEqual(
            sum(
                row[0].get("frame_count", 0)
                for row in records
                if row[0]["record"] == "chunk"
            ),
            2880,
        )

    def test_self_test_reports_structural_evidence_not_golden_claim(self) -> None:
        harness = ControllerHarness()
        self.ready(harness)
        harness.handle(
            request(
                3,
                "self_test",
                {"audio_stream_id": "self-test-audio"},
            )
        )
        harness.wait_idle()
        result = next(
            event
            for event in harness.events()
            if event["event"] == "self_test_result"
        )
        self.assertTrue(result["payload"]["self_test_passed"])
        self.assertTrue(result["payload"]["non_silent"])
        self.assertEqual(result["payload"]["clipped_samples"], 0)
        self.assertFalse(
            result["payload"]["resource_envelope_admission_allowed"]
        )

    def test_cancel_advances_generation_and_ends_pcm_cancelled(self) -> None:
        harness = ControllerHarness(
            FixtureBackend(
                callback_delay_seconds=0.03,
                callbacks=50,
                frames_per_callback=240,
            )
        )
        self.ready(harness)
        harness.handle(
            request(
                3,
                "synthesize",
                {
                    "audio_stream_id": "audio-cancel",
                    "text": "A deliberately interruptible fixture.",
                    "voice_id": "bf_emma",
                },
            )
        )
        time.sleep(0.06)
        harness.handle(request(4, "cancel", {}, generation=1))
        harness.wait_idle()
        events = harness.events()
        cancel = next(
            event
            for event in events
            if event["request_id"] == "request-4"
        )
        self.assertEqual(cancel["payload"]["current_generation"], 1)
        self.assertNotIn(
            "tts_result",
            [
                event["event"]
                for event in events
                if event["request_id"] == "request-3"
            ],
        )
        self.assertEqual(
            harness.pcm_records()[-1][0]["status"],
            "cancelled",
        )

    def test_rejects_unknown_voice_duplicate_stream_and_bad_nonce(self) -> None:
        bad = ControllerHarness()
        bad.handle(
            request(
                1,
                "handshake",
                {"launch_nonce": "wrong", "supervisor": "test"},
            )
        )
        self.assertEqual(
            bad.events()[0]["error"]["code"],
            "authentication_failed",
        )

        harness = ControllerHarness()
        self.ready(harness)
        harness.handle(
            request(
                3,
                "synthesize",
                {
                    "audio_stream_id": "audio-bad",
                    "text": "Text",
                    "voice_id": "user-cloned-voice",
                },
            )
        )
        self.assertEqual(
            harness.events()[-1]["error"]["code"],
            "unknown_voice",
        )

    def test_unload_requires_cancelled_and_drained_job(self) -> None:
        harness = ControllerHarness(
            FixtureBackend(
                callback_delay_seconds=0.05,
                callbacks=20,
            )
        )
        self.ready(harness)
        harness.handle(
            request(
                3,
                "synthesize",
                {
                    "audio_stream_id": "audio-busy",
                    "text": "Text",
                    "voice_id": "af_bella",
                },
            )
        )
        harness.handle(request(4, "unload", {}))
        self.assertEqual(
            harness.events()[-1]["error"]["code"],
            "worker_busy",
        )
        harness.handle(request(5, "cancel", {}, generation=1))
        harness.wait_idle()
        harness.handle(request(6, "unload", {}, generation=1))
        self.assertEqual(harness.events()[-1]["payload"]["lifecycle"], "cold")

    def test_parent_disconnect_path_cancels_and_drains_before_unload(self) -> None:
        harness = ControllerHarness(
            FixtureBackend(
                callback_delay_seconds=0.02,
                callbacks=50,
                frames_per_callback=240,
            )
        )
        self.ready(harness)
        harness.handle(
            request(
                3,
                "synthesize",
                {
                    "audio_stream_id": "audio-disconnect",
                    "text": "A parent disconnect fixture.",
                    "voice_id": "af_heart",
                },
            )
        )
        self.assertTrue(harness.controller.cancel_and_drain(2.0))
        self.assertFalse(harness.controller.jobs)
        self.assertEqual(harness.pcm_records()[-1][0]["status"], "cancelled")


if __name__ == "__main__":
    unittest.main()
