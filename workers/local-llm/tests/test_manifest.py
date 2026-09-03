from __future__ import annotations

import json
import unittest
from pathlib import Path

import jsonschema

from npc_local_llm.constants import MODEL_ARTIFACT_SHA256, MODEL_ARTIFACT_SIZE, MODEL_ID, RUNTIME_ABI
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.download import validate_pinned_or_cdn_url
from npc_local_llm.manifest import ModelPack, RuntimeBundle, safe_relative_path, validate_pinned_https_url

from support import MANIFEST, ROOT, RUNTIME_BUNDLE


class ManifestTests(unittest.TestCase):
    def test_pack_matches_shared_schema_and_frozen_identity(self) -> None:
        schema = json.loads((ROOT / "packaging/model-packs/model-pack-manifest.schema.json").read_bytes())
        value = json.loads(MANIFEST.read_bytes())
        jsonschema.Draft202012Validator(schema).validate(value)
        pack = ModelPack.load(MANIFEST)
        self.assertEqual(pack.pack_id, MODEL_ID)
        self.assertEqual(pack.runtime_abi, RUNTIME_ABI)
        self.assertEqual(pack.model_artifact.size_bytes, MODEL_ARTIFACT_SIZE)
        self.assertEqual(pack.model_artifact.sha256, MODEL_ARTIFACT_SHA256)
        self.assertNotIn("main", pack.model_artifact.source_urls[0])

    def test_runtime_bundle_is_fully_pinned(self) -> None:
        bundle = RuntimeBundle.load(RUNTIME_BUNDLE)
        self.assertEqual(bundle.abi, RUNTIME_ABI)
        self.assertEqual({variant.backend for variant in bundle.variants}, {"cpu", "vulkan"})
        for variant in bundle.variants:
            self.assertEqual(len(variant.artifact.sha256), 64)
            self.assertGreater(variant.artifact.size_bytes, 1_000_000)

    def test_mutable_or_credentialed_urls_are_rejected(self) -> None:
        with self.assertRaises(LocalLlmError):
            validate_pinned_https_url("https://huggingface.co/Qwen/model/resolve/main/model.gguf")
        with self.assertRaises(LocalLlmError):
            validate_pinned_https_url("https://token@example.com/model.gguf")
        with self.assertRaises(LocalLlmError):
            validate_pinned_https_url("http://huggingface.co/model")

    def test_current_hugging_face_content_redirect_is_exactly_allowlisted(self) -> None:
        validate_pinned_or_cdn_url("https://us.aws.cdn.hf.co/xet-bridge-us/content?signature=ephemeral")
        with self.assertRaises(LocalLlmError):
            validate_pinned_or_cdn_url("https://aws.cdn.hf.co.evil.example/content")

    def test_windows_unsafe_destinations_are_rejected(self) -> None:
        for value in ("../model.gguf", "C:/model.gguf", "NUL", "dir\\file", "/absolute"):
            with self.subTest(value=value), self.assertRaises(LocalLlmError):
                safe_relative_path(value)


if __name__ == "__main__":
    unittest.main()
