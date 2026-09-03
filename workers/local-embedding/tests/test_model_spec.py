from __future__ import annotations

import hashlib
import json
import math
import struct
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from model_spec import (  # noqa: E402
    DIMENSIONS,
    MODEL_ID,
    MODEL_SHA256,
    QUERY_INSTRUCTION,
    REQUEST_CONTRACT_VERSION,
    TOKENIZER_SHA256,
    EmbeddingRequest,
    PackSpec,
    SpecError,
    normalize_vector,
    tensor_bytes,
    tensor_payload,
)
from manifest_builder import build_manifest  # noqa: E402


def item(text: str = "A quiet village forge", *, input_id: str = "input-01", storage: bool = False) -> dict[str, object]:
    value: dict[str, object] = {
        "input_id": input_id,
        "text": text,
        "source_content_sha256": hashlib.sha256(text.encode()).hexdigest(),
    }
    if storage:
        value.update(item_id="memory-01", generation=7)
    return value


def payload(*items: dict[str, object], mode: str = "passage") -> dict[str, object]:
    return {
        "contract_version": REQUEST_CONTRACT_VERSION,
        "mode": mode,
        "purpose": "memory_retrieval",
        "priority": "background",
        "items": list(items) or [item()],
    }


class RequestContractTests(unittest.TestCase):
    def test_valid_passage_request(self) -> None:
        request = EmbeddingRequest.parse(payload(item(storage=True)))
        self.assertEqual(request.mode, "passage")
        self.assertEqual(request.items[0].generation, 7)

    def test_query_instruction_is_exact_and_query_only(self) -> None:
        query = EmbeddingRequest.parse(payload(item(), mode="query"))
        passage = EmbeddingRequest.parse(payload(item(), mode="passage"))
        self.assertEqual(query.prepared_texts()[0], QUERY_INSTRUCTION + item()["text"])
        self.assertEqual(passage.prepared_texts()[0], item()["text"])

    def test_source_digest_binds_exact_utf8(self) -> None:
        raw = item("café")
        raw["source_content_sha256"] = hashlib.sha256("cafe".encode()).hexdigest()
        with self.assertRaisesRegex(SpecError, "exact UTF-8"):
            EmbeddingRequest.parse(payload(raw))

    def test_unknown_top_level_field_rejected(self) -> None:
        raw = payload(item())
        raw["debug"] = True
        with self.assertRaises(SpecError):
            EmbeddingRequest.parse(raw)

    def test_partial_storage_binding_rejected(self) -> None:
        raw = item()
        raw["item_id"] = "memory-01"
        with self.assertRaises(SpecError):
            EmbeddingRequest.parse(payload(raw))

    def test_duplicate_input_ids_rejected(self) -> None:
        with self.assertRaises(SpecError):
            EmbeddingRequest.parse(payload(item(input_id="same-id"), item("other", input_id="same-id")))

    def test_empty_text_rejected(self) -> None:
        with self.assertRaises(SpecError):
            EmbeddingRequest.parse(payload(item("   ")))

    def test_foreground_priority_rejected(self) -> None:
        raw = payload(item())
        raw["priority"] = "interactive"
        with self.assertRaisesRegex(SpecError, "background-only"):
            EmbeddingRequest.parse(raw)

    def test_public_self_test_rejected(self) -> None:
        raw = payload(item())
        raw["purpose"] = "self_test"
        with self.assertRaisesRegex(SpecError, "supervisor-owned"):
            EmbeddingRequest.parse(raw)

    def test_more_than_64_items_rejected(self) -> None:
        items = [item(str(index), input_id=f"input-{index:02}") for index in range(65)]
        with self.assertRaises(SpecError):
            EmbeddingRequest.parse(payload(*items))


class TensorContractTests(unittest.TestCase):
    def test_normalization_outputs_unit_f32(self) -> None:
        vector = normalize_vector([1.0] * DIMENSIONS)
        self.assertEqual(len(vector), DIMENSIONS)
        self.assertAlmostEqual(math.sqrt(math.fsum(value * value for value in vector)), 1.0, places=5)

    def test_zero_vector_rejected(self) -> None:
        with self.assertRaisesRegex(SpecError, "zero"):
            normalize_vector([0.0] * DIMENSIONS)

    def test_non_finite_rejected(self) -> None:
        with self.assertRaisesRegex(SpecError, "non-finite"):
            normalize_vector([math.inf] + [1.0] * (DIMENSIONS - 1))

    def test_dimension_mismatch_rejected(self) -> None:
        with self.assertRaisesRegex(SpecError, "dimensions"):
            normalize_vector([1.0, 2.0])

    def test_tensor_bytes_are_little_endian_f32(self) -> None:
        encoded = tensor_bytes([1.0, -2.5])
        self.assertEqual(encoded, struct.pack("<ff", 1.0, -2.5))

    def test_payload_maps_to_character_db_tensor_metadata(self) -> None:
        request = EmbeddingRequest.parse(payload(item(storage=True)))
        result = tensor_payload(request, [[1.0] * DIMENSIONS])
        self.assertEqual(result["contract_version"], "npc.embedding-tensor/v1")
        self.assertEqual(result["model"]["model_id"], MODEL_ID)
        storage = result["items"][0]["storage"]
        self.assertEqual(storage["schema_version"], "character-db/1.0.0")
        self.assertEqual(storage["element_format"], "f32_le")
        self.assertEqual(storage["tensor_byte_length"], DIMENSIONS * 4)
        self.assertEqual(len(storage["tensor_sha256"]), 64)

    def test_query_and_passage_preprocessing_spaces_are_versioned(self) -> None:
        query = tensor_payload(EmbeddingRequest.parse(payload(item(), mode="query")), [[1.0] * DIMENSIONS])
        passage = tensor_payload(EmbeddingRequest.parse(payload(item(), mode="passage")), [[1.0] * DIMENSIONS])
        self.assertNotEqual(query["preprocessing"]["revision"], passage["preprocessing"]["revision"])
        self.assertTrue(query["preprocessing"]["query_instruction_applied"])
        self.assertFalse(passage["preprocessing"]["query_instruction_applied"])

    def test_payload_vector_count_must_match(self) -> None:
        with self.assertRaises(SpecError):
            tensor_payload(EmbeddingRequest.parse(payload(item())), [])


class ManifestContractTests(unittest.TestCase):
    def _manifest(self) -> dict[str, object]:
        return build_manifest(tokenizer_sha256=TOKENIZER_SHA256)

    def _load(self, raw: dict[str, object]) -> PackSpec:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(raw), encoding="utf-8")
            return PackSpec.load(path)

    def test_reviewed_manifest_identity_loads(self) -> None:
        pack = self._load(self._manifest())
        self.assertEqual(pack.artifacts[0].sha256, MODEL_SHA256)

    def test_mutable_source_revision_rejected(self) -> None:
        raw = self._manifest()
        raw["source"]["immutable_revision"] = "main"  # type: ignore[index]
        with self.assertRaises(SpecError):
            self._load(raw)

    def test_model_hash_change_rejected(self) -> None:
        raw = self._manifest()
        raw["artifacts"][0]["sha256"] = "0" * 64  # type: ignore[index]
        with self.assertRaises(SpecError):
            self._load(raw)

    def test_missing_tokenizer_rejected(self) -> None:
        raw = self._manifest()
        raw["artifacts"] = raw["artifacts"][:1]  # type: ignore[index]
        with self.assertRaisesRegex(SpecError, "model, tokenizer, and model-card license"):
            self._load(raw)

    def test_mutable_tokenizer_url_rejected(self) -> None:
        raw = self._manifest()
        raw["artifacts"][1]["source_urls"] = ["https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json"]  # type: ignore[index]
        with self.assertRaisesRegex(SpecError, "tokenizer"):
            self._load(raw)

    def test_artifact_roles_cannot_be_swapped(self) -> None:
        raw = self._manifest()
        raw["artifacts"][0]["role"] = "tokenizer"  # type: ignore[index]
        raw["artifacts"][1]["role"] = "model_weights"  # type: ignore[index]
        with self.assertRaises(SpecError):
            self._load(raw)

    def test_destination_traversal_rejected(self) -> None:
        raw = self._manifest()
        raw["artifacts"][0]["destination"] = "../model.onnx"  # type: ignore[index]
        with self.assertRaises(SpecError):
            self._load(raw)


if __name__ == "__main__":
    unittest.main()
