from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from build_manifest import main  # noqa: E402
from manifest_builder import (  # noqa: E402
    INSTALLED_ARTIFACT_BYTES,
    PEAK_TRANSACTIONAL_ARTIFACT_BYTES,
    build_manifest,
)
from model_spec import PackSpec, TOKENIZER_SHA256, TOKENIZER_SIZE_BYTES, TOKENIZER_URL  # noqa: E402


class ManifestBuilderTests(unittest.TestCase):
    def test_builds_complete_unqualified_manifest_without_invented_measurements(self) -> None:
        manifest = build_manifest(tokenizer_sha256=TOKENIZER_SHA256)
        self.assertEqual(manifest["resources"]["storage_bytes"], INSTALLED_ARTIFACT_BYTES)
        self.assertEqual(manifest["resources"]["peak_install_bytes"], PEAK_TRANSACTIONAL_ARTIFACT_BYTES)
        self.assertIsNone(manifest["resources"]["planning_resident_ram_bytes"])
        self.assertIsNone(manifest["resources"]["planning_load_millis"])
        self.assertIsNone(manifest["self_test"]["expected_output_sha256"])
        tokenizer = manifest["artifacts"][1]
        self.assertEqual(tokenizer["size_bytes"], TOKENIZER_SIZE_BYTES)
        self.assertEqual(tokenizer["source_urls"], [TOKENIZER_URL])

    def test_placeholder_digest_is_rejected(self) -> None:
        for digest in ("", "unknown", "0" * 64, "1" * 64, "A" * 64):
            with self.subTest(digest=digest):
                with self.assertRaisesRegex(ValueError, "tokenizer_sha256"):
                    build_manifest(tokenizer_sha256=digest)

    def test_cli_emits_manifest_accepted_by_worker_parser(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "bge-small-en-v1.5-onnx-fp32.json"
            self.assertEqual(main(["--tokenizer-sha256", TOKENIZER_SHA256, "--out", str(output)]), 0)
            PackSpec.load(output)
            raw = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(raw["pack_id"], "bge-small-en-v1.5-onnx-fp32")


if __name__ == "__main__":
    unittest.main()
