from __future__ import annotations

import math
from pathlib import Path
import struct
import sys
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.backend import FixtureMoonshineBackend
from npc_local_stt.contract import PROTOCOL_VERSION, PcmFrame
from npc_local_stt.runtime import WorkerRuntime


INSTANCE = "worker-fixture-1"
NONCE = "launch-nonce-fixture"


def request(sequence: int, operation: str, payload: dict, generation: int = 0, *, request_id: str | None = None) -> dict:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "workerInstanceId": INSTANCE,
        "requestId": request_id or f"request-{sequence}",
        "sequence": sequence,
        "generation": generation,
        "deadlineUnixMs": int(time.time() * 1000) + 10000,
        "operation": operation,
        "payload": payload,
    }


def handshake(runtime: WorkerRuntime, sequence: int = 1) -> list[dict]:
    return runtime.handle(
        request(
            sequence,
            "handshake",
            {
                "launchNonce": NONCE,
                "supervisorPid": 123,
                "supervisorExecutableSha256": "a" * 64,
            },
        )
    )


def pcm_frame(chunk_index: int, *, generation: int = 0, voice: bool = True, chunk_id: str | None = None) -> PcmFrame:
    sample_count = 4000
    if voice:
        samples = [int(math.sin(i * 2.0 * math.pi * 220.0 / 16000.0) * 10000) for i in range(sample_count)]
    else:
        samples = [0] * sample_count
    pcm = struct.pack(f"<{sample_count}h", *samples)
    return PcmFrame.parse(
        {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": INSTANCE,
            "sessionId": "session-1",
            "generation": generation,
            "chunkId": chunk_id or f"chunk-{chunk_index}",
            "chunkIndex": chunk_index,
            "sampleRateHz": 16000,
            "channels": 1,
            "sampleFormat": "pcm_s16le",
            "sampleCount": sample_count,
            "capturedAtQpc": 100000 + chunk_index * 1000,
            "qpcFrequencyHz": 10000000,
        },
        pcm,
    )


class RuntimeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runtime = WorkerRuntime(
            worker_instance_id=INSTANCE,
            launch_nonce=NONCE,
            backend=FixtureMoonshineBackend("Welcome to the citadel"),
        )

    def boot_loaded(self) -> None:
        self.assertEqual(handshake(self.runtime)[-1]["event"], "completed")
        self.assertEqual(self.runtime.handle(request(2, "warm", {}))[-1]["event"], "completed")
        result = self.runtime.handle(
            request(
                3,
                "load",
                {
                    "packId": "fixture.moonshine",
                    "revision": "fixture-v1",
                    "modelLeaseId": "lease-1",
                },
            )
        )
        self.assertEqual(result[-1]["event"], "completed")

    def start_session(self, sequence: int = 4, generation: int = 0) -> None:
        result = self.runtime.handle(
            request(
                sequence,
                "session_start",
                {
                    "sessionId": "session-1",
                    "inputSource": "supervisor_microphone_pcm",
                    "turnMode": "push_to_talk",
                    "sampleRateHz": 16000,
                    "channels": 1,
                    "sampleFormat": "pcm_s16le",
                    "wordTimestamps": True,
                },
                generation,
            )
        )
        self.assertEqual(result[-1]["event"], "completed")

    def commit(self, sequence: int, frame: PcmFrame, generation: int = 0) -> list[dict]:
        self.runtime.feed_pcm_for_test(frame)
        return self.runtime.handle(
            request(
                sequence,
                "pcm_commit",
                {"sessionId": "session-1", "chunkId": frame.chunk_id},
                generation,
            )
        )

    def test_authenticated_full_ptt_flow_emits_partial_final_and_words(self) -> None:
        self.boot_loaded()
        self.start_session()
        partial = self.commit(5, pcm_frame(0))
        self.assertIn("partial_transcript", [event["event"] for event in partial])
        final = self.runtime.handle(
            request(6, "session_end", {"sessionId": "session-1", "reason": "ptt_key_up"})
        )
        final_event = next(event for event in final if event["event"] == "final_transcript")
        self.assertEqual(final_event["payload"]["text"], "Welcome to the citadel")
        self.assertEqual([word["text"] for word in final_event["payload"]["words"]], ["Welcome", "to", "the", "citadel"])
        self.assertTrue(final[-1]["terminal"])
        self.assertEqual(final[-1]["payload"]["reason"], "ptt_key_up")

    def test_vad_can_complete_after_three_silent_chunks(self) -> None:
        self.boot_loaded()
        self.start_session()
        self.commit(5, pcm_frame(0))
        self.commit(6, pcm_frame(1, voice=False))
        self.commit(7, pcm_frame(2, voice=False))
        result = self.commit(8, pcm_frame(3, voice=False))
        self.assertIn("final_transcript", [event["event"] for event in result])
        self.assertIn("utterance_ended", [event["event"] for event in result])

    def test_wrong_launch_nonce_fails_closed(self) -> None:
        bad = request(
            1,
            "handshake",
            {"launchNonce": "wrong", "supervisorPid": 123, "supervisorExecutableSha256": "a" * 64},
        )
        result = self.runtime.handle(bad)
        self.assertEqual(result[0]["error"]["code"], "authentication_failed")

    def test_duplicate_request_is_rejected(self) -> None:
        handshake(self.runtime)
        first = self.runtime.handle(request(2, "health", {}, request_id="same"))
        self.assertEqual(first[-1]["event"], "completed")
        duplicate = self.runtime.handle(request(3, "health", {}, request_id="same"))
        self.assertEqual(duplicate[0]["error"]["code"], "duplicate_request")

    def test_out_of_order_control_sequence_is_rejected(self) -> None:
        handshake(self.runtime)
        result = self.runtime.handle(request(1, "health", {}, request_id="out-of-order"))
        self.assertEqual(result[0]["error"]["code"], "out_of_order_sequence")

    def test_pcm_chunk_index_must_be_contiguous(self) -> None:
        self.boot_loaded()
        self.start_session()
        result = self.commit(5, pcm_frame(1))
        self.assertEqual(result[0]["error"]["code"], "pcm_out_of_order")

    def test_pcm_generation_must_match(self) -> None:
        self.boot_loaded()
        self.start_session()
        result = self.commit(5, pcm_frame(0, generation=1))
        self.assertEqual(result[0]["error"]["code"], "stale_generation")

    def test_cancel_is_generation_barrier_and_retry_is_idempotent(self) -> None:
        self.boot_loaded()
        self.start_session()
        cancelled = self.runtime.handle(request(5, "cancel", {"reason": "barge_in"}, generation=1))
        self.assertEqual(cancelled[0]["event"], "cancelled")
        retry = self.runtime.handle(request(6, "cancel", {"reason": "barge_in"}, generation=1))
        self.assertTrue(retry[0]["payload"]["idempotent"])
        stale = self.runtime.handle(request(7, "health", {}, generation=0))
        self.assertEqual(stale[0]["error"]["code"], "stale_generation")

    def test_generation_gap_is_rejected(self) -> None:
        self.boot_loaded()
        result = self.runtime.handle(request(4, "cancel", {"reason": "deadline"}, generation=2))
        self.assertEqual(result[0]["error"]["code"], "generation_gap")

    def test_model_cannot_unload_during_active_session(self) -> None:
        self.boot_loaded()
        self.start_session()
        result = self.runtime.handle(request(5, "unload", {}))
        self.assertEqual(result[0]["error"]["code"], "worker_busy")

    def test_health_has_resource_hooks_without_raw_audio_or_transcript(self) -> None:
        self.boot_loaded()
        result = self.runtime.handle(request(4, "health", {}))
        health = result[0]["payload"]
        self.assertEqual(health["metrics"]["evidenceClass"], "fixture")
        self.assertIsNone(health["metrics"]["gpu"]["loadedVramMiB"])
        self.assertIn("external", health["metrics"]["gpu"]["telemetrySource"])
        rendered = str(health).lower()
        self.assertNotIn("pcm_s16", rendered)
        self.assertNotIn("welcome", rendered)


if __name__ == "__main__":
    unittest.main()
