from __future__ import annotations

import ctypes
import json
from pathlib import Path
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.backend import MoonshineBridgeBackend
from npc_local_stt.contract import ProtocolFault


class FreeOnlyLibrary:
    def __init__(self) -> None:
        self.freed = 0

    def npc_stt_free_json(self, _pointer: ctypes.c_void_p) -> None:
        self.freed += 1


class BackendValidationTests(unittest.TestCase):
    def backend_with_library(self) -> tuple[MoonshineBridgeBackend, FreeOnlyLibrary]:
        backend = MoonshineBridgeBackend(Path("missing-bridge"), Path("missing-model"))
        library = FreeOnlyLibrary()
        backend._library = library  # type: ignore[assignment]
        return backend, library

    @staticmethod
    def native_pointer(value: object) -> tuple[ctypes.Array, ctypes.c_void_p]:
        buffer = ctypes.create_string_buffer(json.dumps(value).encode("utf-8"))
        return buffer, ctypes.cast(buffer, ctypes.c_void_p)

    def test_missing_bridge_fails_as_protocol_fault(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            backend = MoonshineBridgeBackend(Path(directory) / "missing.dll", Path(directory) / "model")
            with self.assertRaises(ProtocolFault) as caught:
                backend.warm()
            self.assertEqual(caught.exception.code, "bridge_unavailable")

    def test_valid_native_delta_is_bounded_and_parsed(self) -> None:
        backend, library = self.backend_with_library()
        buffer, pointer = self.native_pointer(
            {
                "lines": [
                    {
                        "utteranceId": "moonshine-1",
                        "text": "hello",
                        "startMs": 0,
                        "endMs": 250,
                        "complete": False,
                        "changed": True,
                        "upstreamLatencyMs": 7,
                        "words": [
                            {"text": "hello", "startMs": 0, "endMs": 250, "confidence": 0.9}
                        ],
                    }
                ],
                "speechStarted": True,
                "speechEnded": False,
                "analyzedAudioMs": 250,
            }
        )
        self.assertIsNotNone(buffer)
        delta = backend._parse_native_json(pointer, 3.0)
        self.assertEqual(delta.lines[0].text, "hello")
        self.assertEqual(delta.lines[0].words[0].confidence, 0.9)
        self.assertEqual(delta.analyzed_audio_ms, 250)
        self.assertEqual(library.freed, 1)

    def test_invalid_native_timing_is_rejected_after_free(self) -> None:
        backend, library = self.backend_with_library()
        buffer, pointer = self.native_pointer(
            {
                "lines": [
                    {
                        "utteranceId": "moonshine-1",
                        "text": "bad",
                        "startMs": 20,
                        "endMs": 10,
                        "complete": True,
                        "changed": True,
                        "upstreamLatencyMs": 1,
                        "words": [],
                    }
                ]
            }
        )
        self.assertIsNotNone(buffer)
        with self.assertRaises(ProtocolFault) as caught:
            backend._parse_native_json(pointer, 1.0)
        self.assertEqual(caught.exception.code, "bridge_output_invalid")
        self.assertEqual(library.freed, 1)

    def test_reversed_word_end_is_clamped_for_partial_line(self) -> None:
        backend, library = self.backend_with_library()
        buffer, pointer = self.native_pointer(
            {
                "lines": [
                    {
                        "utteranceId": "moonshine-1",
                        "text": "pending",
                        "startMs": 100,
                        "endMs": 200,
                        "complete": False,
                        "changed": True,
                        "words": [
                            {"text": "pending", "startMs": 180, "endMs": 160, "confidence": None}
                        ],
                    }
                ]
            }
        )
        self.assertIsNotNone(buffer)
        delta = backend._parse_native_json(pointer, 1.0)
        self.assertEqual(delta.lines[0].words[0].start_ms, 180)
        self.assertEqual(delta.lines[0].words[0].end_ms, 180)
        self.assertEqual(library.freed, 1)

    def test_reversed_word_end_is_clamped_for_completed_line(self) -> None:
        backend, library = self.backend_with_library()
        buffer, pointer = self.native_pointer(
            {
                "lines": [
                    {
                        "utteranceId": "moonshine-1",
                        "text": "complete",
                        "startMs": 100,
                        "endMs": 200,
                        "complete": True,
                        "changed": True,
                        "words": [
                            {"text": "complete", "startMs": 180, "endMs": 160, "confidence": None}
                        ],
                    }
                ]
            }
        )
        self.assertIsNotNone(buffer)
        delta = backend._parse_native_json(pointer, 1.0)
        self.assertEqual(delta.lines[0].words[0].start_ms, 180)
        self.assertEqual(delta.lines[0].words[0].end_ms, 180)
        self.assertEqual(library.freed, 1)


if __name__ == "__main__":
    unittest.main()
