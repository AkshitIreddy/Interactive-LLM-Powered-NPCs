from __future__ import annotations

import io
import math
import struct
import sys
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(LOCAL_TTS))

from pcm_transport import (
    MAX_CHUNK_FRAMES,
    PcmTransportError,
    PcmWriter,
    floats_to_pcm16,
    read_pcm_record,
)
from protocol import (
    MAX_FRAME_BYTES,
    EndOfStream,
    FrameWriter,
    ProtocolError,
    read_frame,
)


class ProtocolTests(unittest.TestCase):
    def test_control_frame_round_trip(self) -> None:
        stream = io.BytesIO()
        FrameWriter(stream).write({"hello": "world"})
        stream.seek(0)
        self.assertEqual(read_frame(stream), {"hello": "world"})

    def test_rejects_oversized_and_truncated_frames(self) -> None:
        with self.assertRaises(ProtocolError):
            read_frame(
                io.BytesIO(struct.pack(">I", MAX_FRAME_BYTES + 1))
            )
        with self.assertRaises(ProtocolError):
            read_frame(io.BytesIO(struct.pack(">I", 4) + b"{}"))
        with self.assertRaises(EndOfStream):
            read_frame(io.BytesIO())


class PcmTests(unittest.TestCase):
    def test_float_conversion_is_little_endian_and_bounded(self) -> None:
        pcm, clipped = floats_to_pcm16([-2.0, -1.0, 0.0, 1.0, 2.0])
        self.assertEqual(clipped, 2)
        self.assertEqual(
            struct.unpack("<hhhhh", pcm),
            (-32768, -32768, 0, 32767, 32767),
        )
        with self.assertRaises(PcmTransportError):
            floats_to_pcm16([math.nan])

    def test_writer_splits_chunks_and_binds_sample_clock(self) -> None:
        output = io.BytesIO()
        writer = PcmWriter(output)
        samples = [0.1] * (MAX_CHUNK_FRAMES + 5)
        receipts = writer.write_samples(
            stream_id="stream-1",
            request_id="request-1",
            generation=3,
            chunk_sequence=7,
            sample_start=100,
            sample_rate_hz=24000,
            samples=samples,
        )
        writer.finish(
            stream_id="stream-1",
            request_id="request-1",
            generation=3,
            final_frames=len(samples),
            status="completed",
        )
        self.assertEqual([row.frame_count for row in receipts], [2400, 5])
        self.assertEqual([row.sample_start for row in receipts], [100, 2500])
        output.seek(0)
        first, pcm = read_pcm_record(output)
        second, _ = read_pcm_record(output)
        terminal, empty = read_pcm_record(output)
        self.assertEqual(first["request_id"], "request-1")
        self.assertEqual(first["generation"], 3)
        self.assertEqual(len(pcm), 4800)
        self.assertEqual(second["sample_start"], 2500)
        self.assertEqual(terminal["record"], "end")
        self.assertEqual(empty, b"")

    def test_stream_may_finish_only_once(self) -> None:
        writer = PcmWriter(io.BytesIO())
        arguments = {
            "stream_id": "stream",
            "request_id": "request",
            "generation": 0,
            "final_frames": 0,
            "status": "cancelled",
        }
        writer.finish(**arguments)
        with self.assertRaises(PcmTransportError):
            writer.finish(**arguments)


if __name__ == "__main__":
    unittest.main()
