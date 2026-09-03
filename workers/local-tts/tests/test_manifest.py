from __future__ import annotations

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

LOCAL_TTS = Path(__file__).resolve().parents[1]
REPO = LOCAL_TTS.parents[1]
sys.path.insert(0, str(LOCAL_TTS))

from manifest import ManifestError, load_manifest
from voices import STOCK_VOICES

MANIFEST = (
    REPO
    / "packaging"
    / "model-packs"
    / "kokoro-sherpa-onnx-v1.0-int8-windows-x64.json"
)


class ManifestTests(unittest.TestCase):
    def test_real_candidate_is_strict_and_activation_blocked(self) -> None:
        manifest = load_manifest(MANIFEST)
        self.assertEqual(
            manifest.pack_id,
            "local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64",
        )
        self.assertEqual(manifest.runtime_version, "1.13.6")
        self.assertEqual(manifest.onnxruntime_version, "1.27.1")
        self.assertEqual(manifest.output_sample_rate_hz, 24000)
        self.assertEqual(len(STOCK_VOICES), 28)
        self.assertEqual([voice.speaker_id for voice in STOCK_VOICES], list(range(28)))
        self.assertEqual(manifest.raw["schema"], "npc.model-pack/v2")
        self.assertEqual(
            manifest.raw["admission"]["state"],
            "blocked_pending_measurement",
        )
        self.assertIsNone(
            manifest.raw["resources"]["planning_resident_ram_bytes"]
        )
        self.assertTrue(
            manifest.raw["resources"]["measurement"]["p99_reload_required"]
        )

    def mutate(self, callback) -> None:
        raw = json.loads(MANIFEST.read_text(encoding="utf-8"))
        callback(raw)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "manifest.json"
            path.write_text(json.dumps(raw), encoding="utf-8")
            with self.assertRaises(ManifestError):
                load_manifest(path)

    def test_rejects_activation_claim_without_measurement(self) -> None:
        self.mutate(
            lambda raw: raw["admission"].update(
                {"state": "eligible_after_external_admission"}
            )
        )

    def test_rejects_silent_or_automatic_download(self) -> None:
        self.mutate(
            lambda raw: raw["lifecycle"].update(
                {"automatic_download_allowed": True}
            )
        )

    def test_rejects_voice_reorder_or_cloning(self) -> None:
        def change(raw):
            raw["voices"][0]["locale"] = "en-GB"
            raw["voices"][0]["voice_cloning"] = True

        self.mutate(change)

    def test_rejects_mutable_or_credentialed_artifact_url(self) -> None:
        self.mutate(
            lambda raw: raw["artifacts"][0].update(
                {
                    "source_urls": [
                        "https://user:secret@example.com/latest?token=x"
                    ]
                }
            )
        )

    def test_rejects_archive_digest_or_tar_bz2_relabeling(self) -> None:
        def change(raw):
            raw["artifacts"][0]["sha256"] = "0" * 64
            raw["artifacts"][0]["archive_format"] = "tar"

        self.mutate(change)

    def test_rejects_missing_gpl_runtime_disclosure(self) -> None:
        def change(raw):
            raw["license"]["components"] = [
                item
                for item in raw["license"]["components"]
                if item["id"] != "espeak-ng-gpl-3.0-or-later"
            ]

        self.mutate(change)

    def test_requires_acceptance_of_every_transitive_license_notice(self) -> None:
        self.mutate(
            lambda raw: raw["license"].update({"acceptance_required": False})
        )


if __name__ == "__main__":
    unittest.main()
