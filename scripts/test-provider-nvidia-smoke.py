#!/usr/bin/env python3
"""Offline unit tests for bounded NVIDIA Magpie voice selection."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_PATH = Path(__file__).with_name("provider-nvidia-smoke.py")
SPEC = importlib.util.spec_from_file_location("provider_nvidia_smoke", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
provider_nvidia_smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(provider_nvidia_smoke)


class VoiceSelectionTests(unittest.TestCase):
    STOCK = [
        "Magpie-Multilingual.EN-US.Aria",
        "Magpie-Multilingual.EN-US.Jason",
        "Magpie-Multilingual.EN-US.Leo",
        "Magpie-Multilingual.EN-US.Ray",
        "Magpie-Multilingual.EN-US.Sofia",
    ]

    def test_default_prefers_documented_stock_male_order(self) -> None:
        self.assertEqual(
            provider_nvidia_smoke.select_stock_en_us_voice(self.STOCK),
            "Magpie-Multilingual.EN-US.Jason",
        )
        without_jason = [voice for voice in self.STOCK if not voice.endswith(".Jason")]
        self.assertEqual(
            provider_nvidia_smoke.select_stock_en_us_voice(without_jason),
            "Magpie-Multilingual.EN-US.Leo",
        )
        without_jason_or_leo = [
            voice
            for voice in without_jason
            if not voice.endswith(".Leo")
        ]
        self.assertEqual(
            provider_nvidia_smoke.select_stock_en_us_voice(without_jason_or_leo),
            "Magpie-Multilingual.EN-US.Ray",
        )

    def test_default_uses_deterministic_stock_fallback(self) -> None:
        voices = [
            "Magpie-Multilingual.EN-US.Sofia",
            "Magpie-Multilingual.EN-US.Aria",
        ]
        self.assertEqual(
            provider_nvidia_smoke.select_stock_en_us_voice(
                provider_nvidia_smoke.stock_en_us_voices(voices)
            ),
            "Magpie-Multilingual.EN-US.Aria",
        )

    def test_explicit_discovered_stock_voice_is_accepted(self) -> None:
        self.assertEqual(
            provider_nvidia_smoke.select_stock_en_us_voice(
                self.STOCK, "Magpie-Multilingual.EN-US.Sofia"
            ),
            "Magpie-Multilingual.EN-US.Sofia",
        )

    def test_explicit_voice_must_be_discovered_stock_en_us(self) -> None:
        with self.assertRaisesRegex(
            ValueError, "tts_voice_not_discovered_stock_en_us"
        ):
            provider_nvidia_smoke.select_stock_en_us_voice(
                self.STOCK, "Magpie-Multilingual.EN-GB.Jason"
            )

    def test_zeroshot_and_cloning_identifiers_are_rejected(self) -> None:
        for voice in (
            "Magpie-ZeroShot-Multilingual.Male",
            "Magpie_Zero_Shot.EN-US.Custom",
            "Magpie-Multilingual.EN-US.VoiceClone",
            "Magpie-Multilingual.EN-US.Cloning-Custom",
        ):
            with self.subTest(voice=voice), self.assertRaisesRegex(
                ValueError, "tts_voice_cloning_not_allowed"
            ):
                provider_nvidia_smoke.select_stock_en_us_voice(self.STOCK, voice)

    def test_discovery_filter_excludes_non_english_and_cloning_voices(self) -> None:
        voices = self.STOCK + [
            "Magpie-Multilingual.EN-GB.Jason",
            "Magpie-ZeroShot.EN-US.Custom",
            "Magpie-Multilingual.EN-US.Clone-Custom",
            "Magpie-Multilingual.EN-US.Jason",
        ]
        self.assertEqual(
            provider_nvidia_smoke.stock_en_us_voices(voices), sorted(self.STOCK)
        )

    def test_tts_smoke_uses_explicit_discovered_voice_without_network(self) -> None:
        discovered = json.dumps({"voices": self.STOCK}).encode()
        calls = [
            (200, discovered, {}),
            (200, b"synthetic-wave-bytes", {"content-type": "audio/wav"}),
        ]
        metrics = {"silent": False, "clippedSamples": 0}
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(
            provider_nvidia_smoke, "call_bytes", side_effect=calls
        ) as call_bytes, mock.patch.object(
            provider_nvidia_smoke,
            "multipart",
            return_value=(b"synthetic-multipart", "multipart/form-data; boundary=test"),
        ) as multipart, mock.patch.object(
            provider_nvidia_smoke, "measure_wav", return_value=metrics
        ):
            result = provider_nvidia_smoke.magpie_tts_smoke(
                "synthetic-test-token",
                Path(directory) / "fixture.wav",
                "Magpie-Multilingual.EN-US.Jason",
            )

        self.assertTrue(result["ok"])
        self.assertEqual(result["voice"], "Magpie-Multilingual.EN-US.Jason")
        self.assertEqual(call_bytes.call_count, 2)
        self.assertEqual(
            multipart.call_args.args[0]["voice"],
            "Magpie-Multilingual.EN-US.Jason",
        )

    def test_tts_smoke_rejects_requested_zeroshot_before_synthesis(self) -> None:
        discovered = json.dumps(
            {"voices": self.STOCK + ["Magpie-ZeroShot-Multilingual.Male"]}
        ).encode()
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(
            provider_nvidia_smoke,
            "call_bytes",
            return_value=(200, discovered, {}),
        ) as call_bytes:
            result = provider_nvidia_smoke.magpie_tts_smoke(
                "synthetic-test-token",
                Path(directory) / "fixture.wav",
                "Magpie-ZeroShot-Multilingual.Male",
            )

        self.assertFalse(result["ok"])
        self.assertEqual(result["status"], "tts_voice_cloning_not_allowed")
        call_bytes.assert_called_once()

    def test_cli_accepts_tts_voice(self) -> None:
        args = provider_nvidia_smoke.parse_args(
            ["--tts-voice", "Magpie-Multilingual.EN-US.Jason"]
        )
        self.assertEqual(args.tts_voice, "Magpie-Multilingual.EN-US.Jason")


if __name__ == "__main__":
    unittest.main()
