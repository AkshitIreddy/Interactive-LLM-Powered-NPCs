from __future__ import annotations

import ctypes
import inspect
import sys
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(LOCAL_TTS))

import sherpa_backend
from sherpa_backend import SherpaOnnxBackend


class SherpaAbiTests(unittest.TestCase):
    def test_pinned_struct_field_order_matches_1_13_6_header(self) -> None:
        self.assertEqual(
            [name for name, _ in sherpa_backend._Kokoro._fields_],
            [
                "model",
                "voices",
                "tokens",
                "data_dir",
                "length_scale",
                "dict_dir",
                "lexicon",
                "lang",
            ],
        )
        self.assertEqual(
            [name for name, _ in sherpa_backend._GenerationConfig._fields_],
            [
                "silence_scale",
                "speed",
                "sid",
                "reference_audio",
                "reference_audio_len",
                "reference_sample_rate",
                "reference_text",
                "num_steps",
                "extra",
            ],
        )
        self.assertGreater(
            ctypes.sizeof(sherpa_backend._ModelConfig),
            ctypes.sizeof(sherpa_backend._Kokoro),
        )

    def test_production_adapter_has_no_downloader_or_fixture_switch(self) -> None:
        source = inspect.getsource(SherpaOnnxBackend)
        self.assertNotIn("urllib", source)
        self.assertNotIn("requests", source)
        self.assertNotIn("FixtureBackend", source)
        self.assertIn('provider = b"cpu"', source)


if __name__ == "__main__":
    unittest.main()
