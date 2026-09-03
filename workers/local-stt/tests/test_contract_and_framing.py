from __future__ import annotations

import io
import json
from pathlib import Path
import struct
import sys
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.contract import PROTOCOL_VERSION, PcmFrame, ProtocolFault, RequestEnvelope
from npc_local_stt.framing import encode_control_frame, encode_pcm_frame, read_control_frame, read_pcm_frame


class ContractAndFramingTests(unittest.TestCase):
    def request(self) -> dict:
        return {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": "worker-1",
            "requestId": "request-1",
            "sequence": 1,
            "generation": 0,
            "deadlineUnixMs": int(time.time() * 1000) + 1000,
            "operation": "health",
            "payload": {},
        }

    def pcm_metadata(self) -> dict:
        return {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": "worker-1",
            "sessionId": "session-1",
            "generation": 0,
            "chunkId": "chunk-1",
            "chunkIndex": 0,
            "sampleRateHz": 16000,
            "channels": 1,
            "sampleFormat": "pcm_s16le",
            "sampleCount": 2,
            "capturedAtQpc": 100,
            "qpcFrequencyHz": 10000000,
        }

    def test_control_frame_round_trip(self) -> None:
        request = self.request()
        self.assertEqual(read_control_frame(io.BytesIO(encode_control_frame(request))), request)

    def test_pcm_frame_round_trip(self) -> None:
        metadata = self.pcm_metadata()
        frame = read_pcm_frame(io.BytesIO(encode_pcm_frame(metadata, b"\x01\x00\xff\xff")))
        self.assertIsNotNone(frame)
        assert frame is not None
        self.assertEqual(frame.sample_count, 2)
        self.assertEqual(frame.pcm, b"\x01\x00\xff\xff")

    def test_unknown_request_field_is_rejected(self) -> None:
        request = self.request()
        request["audio"] = "forbidden"
        with self.assertRaisesRegex(ProtocolFault, "unsupported fields"):
            RequestEnvelope.parse(request)

    def test_expired_request_is_rejected(self) -> None:
        request = self.request()
        request["deadlineUnixMs"] = 1
        with self.assertRaises(ProtocolFault) as caught:
            RequestEnvelope.parse(request)
        self.assertEqual(caught.exception.code, "deadline_exceeded")

    def test_pcm_length_must_match_sample_count(self) -> None:
        with self.assertRaises(ProtocolFault) as caught:
            PcmFrame.parse(self.pcm_metadata(), b"\x00\x00")
        self.assertEqual(caught.exception.code, "invalid_pcm_frame")

    def test_stereo_pcm_is_rejected(self) -> None:
        metadata = self.pcm_metadata()
        metadata["channels"] = 2
        with self.assertRaises(ProtocolFault):
            PcmFrame.parse(metadata, b"\x00\x00\x00\x00")

    def test_truncated_control_frame_is_rejected(self) -> None:
        with self.assertRaises(ProtocolFault) as caught:
            read_control_frame(io.BytesIO(struct.pack(">I", 9) + b"{}"))
        self.assertEqual(caught.exception.code, "invalid_frame")

    def test_zero_length_frame_is_rejected(self) -> None:
        with self.assertRaises(ProtocolFault):
            read_control_frame(io.BytesIO(struct.pack(">I", 0)))

    def test_protocol_schema_is_strict_json(self) -> None:
        schema = json.loads((ROOT / "protocol-v1.schema.json").read_text(encoding="utf-8"))
        self.assertFalse(schema["additionalProperties"])
        self.assertEqual(schema["properties"]["protocolVersion"]["const"], PROTOCOL_VERSION)
        self.assertIn("cancel", schema["properties"]["operation"]["enum"])


if __name__ == "__main__":
    unittest.main()
