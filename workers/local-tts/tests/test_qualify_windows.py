from __future__ import annotations

import math
import sys
import threading
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
REPO = LOCAL_TTS.parents[1]
sys.path.insert(0, str(LOCAL_TTS))

from backend import FixtureBackend
from manifest import load_manifest
from qualify_windows import (
    NoDownload,
    QualificationError,
    qualification_plan,
    synthesize_evidence,
)

MANIFEST = load_manifest(
    REPO
    / "packaging"
    / "model-packs"
    / "kokoro-sherpa-onnx-v1.0-int8-windows-x64.json"
)


class QualificationHarnessTests(unittest.TestCase):
    def test_plan_is_non_admissible_and_covers_all_typed_voices(self) -> None:
        plan = qualification_plan(MANIFEST, 24)
        self.assertEqual(plan["samples"], 24)
        self.assertEqual(len(plan["typed_voice_inventory"]), 28)
        self.assertFalse(plan["downloads"])
        self.assertFalse(plan["qualified_resource_envelope_created"])
        self.assertFalse(plan["admission_allowed"])

    def test_fixture_audio_evidence_measures_first_chunk_and_full_rtf(self) -> None:
        backend = FixtureBackend(callbacks=5, frames_per_callback=480)
        backend.load(Path("/fixture"), num_threads=2)
        evidence = synthesize_evidence(
            backend,
            text="Fixture only.",
            speaker_id=3,
            cancel=threading.Event(),
        )
        self.assertEqual(evidence.frames, 2400)
        self.assertEqual(evidence.first_callback_frames, 480)
        self.assertGreater(evidence.rms, 0)
        self.assertTrue(math.isfinite(evidence.realtime_factor))
        self.assertEqual(evidence.clipped_samples, 0)
        self.assertEqual(len(evidence.pcm), evidence.frames * 2)

    def test_qualification_downloader_is_impossible(self) -> None:
        with self.assertRaises(QualificationError):
            NoDownload().fetch(object(), object(), object())


if __name__ == "__main__":
    unittest.main()

