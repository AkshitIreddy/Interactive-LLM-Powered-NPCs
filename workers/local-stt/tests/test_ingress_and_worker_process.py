from __future__ import annotations

import io
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.contract import PROTOCOL_VERSION, PcmFrame, ProtocolFault
from npc_local_stt.framing import encode_control_frame, encode_pcm_frame, read_control_frame
from npc_local_stt.ingress import PcmFrameStore, PcmReaderThread


def request(sequence: int, operation: str, payload: dict) -> dict:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "workerInstanceId": "worker-process-1",
        "requestId": f"process-{sequence}",
        "sequence": sequence,
        "generation": 0,
        "deadlineUnixMs": int(time.time() * 1000) + 30_000,
        "operation": operation,
        "payload": payload,
    }


def pcm_frame(chunk_id: str = "chunk-0") -> PcmFrame:
    return PcmFrame.parse(
        {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": "worker-process-1",
            "sessionId": "session-1",
            "generation": 0,
            "chunkId": chunk_id,
            "chunkIndex": 0,
            "sampleRateHz": 16000,
            "channels": 1,
            "sampleFormat": "pcm_s16le",
            "sampleCount": 2,
            "capturedAtQpc": 100,
            "qpcFrequencyHz": 10_000_000,
        },
        b"\x01\x00\xff\xff",
    )


class IngressAndWorkerProcessTests(unittest.TestCase):
    def test_store_rejects_duplicate_and_backpressure(self) -> None:
        store = PcmFrameStore(maximum_frames=1)
        store.put(pcm_frame("chunk-0"))
        with self.assertRaises(ProtocolFault) as duplicate:
            store.put(pcm_frame("chunk-0"))
        self.assertEqual(duplicate.exception.code, "duplicate_pcm_chunk")
        with self.assertRaises(ProtocolFault) as full:
            store.put(pcm_frame("chunk-1"))
        self.assertEqual(full.exception.code, "pcm_backpressure")
        self.assertTrue(full.exception.retryable)

    def test_reader_thread_transfers_framed_pcm_and_closes(self) -> None:
        frame = pcm_frame()
        encoded = encode_pcm_frame(
            {
                "protocolVersion": frame.protocol_version,
                "workerInstanceId": frame.worker_instance_id,
                "sessionId": frame.session_id,
                "generation": frame.generation,
                "chunkId": frame.chunk_id,
                "chunkIndex": frame.chunk_index,
                "sampleRateHz": frame.sample_rate_hz,
                "channels": frame.channels,
                "sampleFormat": frame.sample_format,
                "sampleCount": frame.sample_count,
                "capturedAtQpc": frame.captured_at_qpc,
                "qpcFrequencyHz": frame.qpc_frequency_hz,
            },
            frame.pcm,
        )
        store = PcmFrameStore()
        reader = PcmReaderThread(io.BytesIO(encoded), store)
        reader.start()
        deadline = time.monotonic() + 2
        while store.snapshot()["seenFrames"] != 1 and time.monotonic() < deadline:
            time.sleep(0.005)
        received = store.take("chunk-0")
        self.assertEqual(received.pcm, frame.pcm)
        deadline = time.monotonic() + 2
        while not store.snapshot()["closed"] and time.monotonic() < deadline:
            time.sleep(0.005)
        self.assertTrue(store.snapshot()["closed"])

    def test_fixture_worker_binary_framing_e2e(self) -> None:
        commands = [
            request(
                1,
                "handshake",
                {
                    "launchNonce": "process-nonce",
                    "supervisorPid": 1,
                    "supervisorExecutableSha256": "0" * 64,
                },
            ),
            request(2, "capabilities", {}),
            request(3, "health", {}),
            request(4, "self_test", {"level": "protocol"}),
            request(5, "shutdown", {}),
        ]
        environment = os.environ.copy()
        environment.update(
            {
                "NPC_STT_WORKER_INSTANCE_ID": "worker-process-1",
                "NPC_STT_LAUNCH_NONCE": "process-nonce",
            }
        )
        completed = subprocess.run(
            [sys.executable, str(ROOT / "worker.py"), "--fixture"],
            input=b"".join(encode_control_frame(command) for command in commands),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            timeout=15,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode("utf-8", errors="replace"))
        stream = io.BytesIO(completed.stdout)
        events: list[dict] = []
        while True:
            event = read_control_frame(stream)
            if event is None:
                break
            events.append(event)
        self.assertFalse([event for event in events if event["event"] == "error"])
        self.assertEqual({event["requestId"] for event in events if event["terminal"]}, {f"process-{i}" for i in range(1, 6)})
        descriptor = next(event for event in events if event["event"] == "descriptor")
        self.assertTrue(descriptor["payload"]["fixture"])
        self.assertEqual(descriptor["payload"]["microphoneOwnership"], "media_broker")
        self.assertEqual(descriptor["payload"]["computeBackend"], "cpu")
        self_test = next(event for event in events if event["event"] == "self_test_result")
        self.assertEqual(self_test["payload"]["modelInference"], "not_run")


if __name__ == "__main__":
    unittest.main()
