from __future__ import annotations

import sys
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(LOCAL_TTS))

from measurement import MeasurementError, Observation, build_unsigned_report


def complete_observations() -> list[Observation]:
    rows: list[Observation] = []
    voices = ("af_heart", "bf_emma", "am_fenrir")
    profiles = ("short", "medium", "long")
    for index in range(20):
        rows.append(Observation("load", 100 + index, 300_000_000 + index))
        rows.append(Observation("reload", 80 + index, 300_000_000 + index))
        rows.append(
            Observation(
                "synthesis",
                200 + index,
                340_000_000 + index,
                voice_id=voices[index % len(voices)],
                text_profile=profiles[index % len(profiles)],
                first_pcm_millis=30 + index / 10,
                audio_duration_millis=1000 + index,
            )
        )
    rows.append(Observation("cancel", 12, 340_000_000))
    return rows


class MeasurementTests(unittest.TestCase):
    def test_projection_has_frozen_reload_field_and_is_never_admissible(self) -> None:
        report = build_unsigned_report(
            pack_id="local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64",
            revision="2026.08.30-r1",
            manifest_sha256="a" * 64,
            runtime_revision="sherpa-onnx/1.13.6",
            observations=complete_observations(),
        )
        placement = report["placement_projection"]
        self.assertIn("p99_reload_millis", placement)
        self.assertEqual(placement["resident_vram_bytes"], 0)
        self.assertEqual(report["sample_count"], 20)
        self.assertFalse(report["admission"]["admission_allowed"])
        self.assertFalse(
            report["admission"]["qualified_resource_envelope_created"]
        )

    def test_incomplete_or_gpu_claimed_observations_fail_closed(self) -> None:
        with self.assertRaises(MeasurementError):
            build_unsigned_report(
                pack_id="pack",
                revision="revision",
                manifest_sha256="b" * 64,
                runtime_revision="runtime",
                observations=complete_observations()[:10],
            )
        bad = complete_observations()
        bad[0] = Observation("load", 10, 100, resident_vram_bytes=1)
        with self.assertRaises(MeasurementError):
            build_unsigned_report(
                pack_id="pack",
                revision="revision",
                manifest_sha256="b" * 64,
                runtime_revision="runtime",
                observations=bad,
            )


if __name__ == "__main__":
    unittest.main()

