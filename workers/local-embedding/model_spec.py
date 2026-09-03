"""Pinned BGE Small English v1.5 model and tensor contracts.

No function in this module downloads or loads a model.  It is safe to import in
catalog validation, install planning, and fixture-only tests.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
import struct
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Sequence

PACK_SCHEMA = "npc.model-pack/v2"
PACK_ID = "bge-small-en-v1.5-onnx-fp32"
PACK_REVISION = "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a.1"
SOURCE_REVISION = "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a"
MODEL_ID = "BAAI/bge-small-en-v1.5"
MODEL_SHA256 = "828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35"
MODEL_SIZE_BYTES = 133_093_490
MODEL_URL = f"https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/{SOURCE_REVISION}/onnx/model.onnx"
TOKENIZER_SIZE_BYTES = 711_396
TOKENIZER_SHA256 = "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"
TOKENIZER_URL = f"https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/{SOURCE_REVISION}/tokenizer.json"
MODEL_CARD_SIZE_BYTES = 94_783
MODEL_CARD_SHA256 = "ddb964361a55c6e5dfca6361615854b260c9c960205d04c7520151aaa1d75837"
MODEL_CARD_URL = f"https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/{SOURCE_REVISION}/README.md"
DIMENSIONS = 384
MAX_SEQUENCE_TOKENS = 512
POOLING = "cls"
NORMALIZATION = "l2"
ELEMENT_FORMAT = "f32_le"
DISTANCE_METRIC = "cosine"
TENSOR_CONTRACT_VERSION = "npc.embedding-tensor/v1"
REQUEST_CONTRACT_VERSION = "npc.embedding-request/v1"
SELF_TEST_CONTRACT_VERSION = "npc.embedding-self-test/v1"
CHARACTER_DB_SCHEMA_VERSION = "character-db/1.0.0"
PREPROCESSING_REVISION = "bge-en-v1.5-cls-l2-tokenizer-5c38ec7-v1"
QUERY_INSTRUCTION = "Represent this sentence for searching relevant passages: "
MAX_BATCH_ITEMS = 64
MAX_TOTAL_TEXT_BYTES = 262_144
MAX_ITEM_TEXT_BYTES = 32_768
MAX_FRAME_BYTES = 1_048_576
UNIT_NORM_TOLERANCE = 1.0e-4

_HEX64 = re.compile(r"^[a-f0-9]{64}$")
_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,126}[A-Za-z0-9]$")


class SpecError(ValueError):
    """An immutable pack or embedding contract is invalid."""


@dataclass(frozen=True, slots=True)
class ArtifactSpec:
    artifact_id: str
    source_urls: tuple[str, ...]
    size_bytes: int
    sha256: str
    destination: PurePosixPath
    role: str = "other"

    @classmethod
    def parse(cls, raw: Any) -> "ArtifactSpec":
        if not isinstance(raw, dict):
            raise SpecError("artifact must be an object")
        required = {
            "id",
            "role",
            "kind",
            "archive_format",
            "source_urls",
            "size_bytes",
            "sha256",
            "destination",
            "strip_prefix",
            "required_paths",
        }
        if (
            set(raw) != required
            or raw.get("kind") != "file"
            or raw.get("archive_format") is not None
            or raw.get("strip_prefix") is not None
            or raw.get("required_paths") != []
            or raw.get("role") not in {"model_weights", "tokenizer", "license"}
        ):
            raise SpecError("artifact must contain the exact immutable file fields")
        artifact_id = raw.get("id")
        urls = raw.get("source_urls")
        size = raw.get("size_bytes")
        digest = raw.get("sha256")
        destination = raw.get("destination")
        if not isinstance(artifact_id, str) or not _ID.fullmatch(artifact_id):
            raise SpecError("artifact id is invalid")
        if not isinstance(urls, list) or not urls or not all(
            isinstance(url, str) and url.startswith("https://") for url in urls
        ):
            raise SpecError("artifact sources must be non-empty HTTPS URLs")
        if isinstance(size, bool) or not isinstance(size, int) or size <= 0:
            raise SpecError("artifact size must be positive")
        if not isinstance(digest, str) or not _HEX64.fullmatch(digest):
            raise SpecError("artifact SHA-256 must be canonical lowercase hexadecimal")
        if not isinstance(destination, str):
            raise SpecError("artifact destination must be a string")
        relative = PurePosixPath(destination)
        if relative.is_absolute() or not relative.parts or any(part in {"", ".", ".."} for part in relative.parts):
            raise SpecError("artifact destination must be a safe relative POSIX path")
        if "\\" in destination or ":" in destination:
            raise SpecError("artifact destination cannot contain a Windows path escape")
        return cls(artifact_id, tuple(urls), size, digest, relative, raw["role"])


@dataclass(frozen=True, slots=True)
class PackSpec:
    manifest_path: Path
    manifest_sha256: str
    artifacts: tuple[ArtifactSpec, ...]

    @classmethod
    def load(cls, path: Path) -> "PackSpec":
        data = path.read_bytes()
        try:
            raw = json.loads(data)
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SpecError("model pack manifest is not canonical UTF-8 JSON") from exc
        if not isinstance(raw, dict):
            raise SpecError("model pack manifest root must be an object")
        if raw.get("schema") != PACK_SCHEMA or raw.get("pack_id") != PACK_ID or raw.get("revision") != PACK_REVISION:
            raise SpecError("model pack identity does not match this worker revision")
        source = raw.get("source")
        if not isinstance(source, dict) or source.get("immutable_revision") != SOURCE_REVISION:
            raise SpecError("model pack source revision is not the reviewed immutable commit")
        capability = raw.get("capability")
        if capability != {"kind": "embedding", "scope": "generic"}:
            raise SpecError("model pack capability must be generic embedding")
        runtime = raw.get("runtime")
        if (
            not isinstance(runtime, dict)
            or runtime.get("runtime") != "onnxruntime"
            or runtime.get("immutable_revision") != "1.29.0"
            or runtime.get("abi") != "bge-bert-cls-f32-v1"
            or runtime.get("backends") != ["onnxruntime-cpu"]
            or runtime.get("network_access_after_install") is not False
        ):
            raise SpecError("model pack runtime ABI is incompatible")
        artifacts_raw = raw.get("artifacts")
        if not isinstance(artifacts_raw, list) or not artifacts_raw:
            raise SpecError("model pack has no artifacts")
        artifacts = tuple(ArtifactSpec.parse(item) for item in artifacts_raw)
        if len(artifacts) != 3:
            raise SpecError("the BGE pack must contain exactly the reviewed model, tokenizer, and model-card license files")
        if len({artifact.artifact_id for artifact in artifacts}) != len(artifacts):
            raise SpecError("artifact ids must be unique")
        if len({artifact.destination for artifact in artifacts}) != len(artifacts):
            raise SpecError("artifact destinations must be unique")
        model = next((artifact for artifact in artifacts if artifact.artifact_id == "bge-small-en-v1.5-onnx-fp32"), None)
        if (
            model is None
            or model.sha256 != MODEL_SHA256
            or model.size_bytes != MODEL_SIZE_BYTES
            or model.source_urls != (MODEL_URL,)
            or model.destination != PurePosixPath("model/model.onnx")
            or model.role != "model_weights"
        ):
            raise SpecError("the reviewed upstream ONNX artifact is missing or changed")
        tokenizer = next((artifact for artifact in artifacts if artifact.artifact_id == "bge-small-en-v1.5-tokenizer-json"), None)
        if (
            tokenizer is None
            or tokenizer.sha256 != TOKENIZER_SHA256
            or tokenizer.size_bytes != TOKENIZER_SIZE_BYTES
            or tokenizer.source_urls != (TOKENIZER_URL,)
            or tokenizer.destination != PurePosixPath("model/tokenizer.json")
            or tokenizer.role != "tokenizer"
        ):
            raise SpecError("the immutable upstream tokenizer artifact is missing or changed")
        model_card = next((artifact for artifact in artifacts if artifact.artifact_id == "bge-small-en-v1.5-model-card-license"), None)
        if (
            model_card is None
            or model_card.sha256 != MODEL_CARD_SHA256
            or model_card.size_bytes != MODEL_CARD_SIZE_BYTES
            or model_card.source_urls != (MODEL_CARD_URL,)
            or model_card.destination != PurePosixPath("licenses/BGE-MODEL-CARD.md")
            or model_card.role != "license"
        ):
            raise SpecError("the immutable upstream model-card license artifact is missing or changed")
        return cls(path.resolve(), hashlib.sha256(data).hexdigest(), artifacts)


@dataclass(frozen=True, slots=True)
class EmbeddingItem:
    input_id: str
    text: str
    source_content_sha256: str
    item_id: str | None = None
    generation: int | None = None


@dataclass(frozen=True, slots=True)
class EmbeddingRequest:
    mode: str
    purpose: str
    priority: str
    items: tuple[EmbeddingItem, ...]

    @classmethod
    def parse(cls, payload: Any) -> "EmbeddingRequest":
        if not isinstance(payload, dict):
            raise SpecError("embedding payload must be an object")
        allowed = {"contract_version", "mode", "purpose", "priority", "items"}
        if set(payload) != allowed:
            raise SpecError("embedding payload contains missing or unknown fields")
        if payload.get("contract_version") != REQUEST_CONTRACT_VERSION:
            raise SpecError("unsupported embedding request contract")
        mode = payload.get("mode")
        if mode not in {"query", "passage"}:
            raise SpecError("embedding mode must be query or passage")
        purpose = payload.get("purpose")
        if purpose not in {"memory_retrieval", "character_knowledge", "self_test"}:
            raise SpecError("embedding purpose is unsupported")
        if purpose == "self_test":
            raise SpecError("self-test is supervisor-owned and cannot enter the public infer payload")
        priority = payload.get("priority")
        if priority != "background":
            raise SpecError("local embeddings are background-only work")
        raw_items = payload.get("items")
        if not isinstance(raw_items, list) or not 1 <= len(raw_items) <= MAX_BATCH_ITEMS:
            raise SpecError(f"embedding batch size must be in 1..={MAX_BATCH_ITEMS}")
        parsed: list[EmbeddingItem] = []
        total_bytes = 0
        ids: set[str] = set()
        for raw in raw_items:
            if not isinstance(raw, dict):
                raise SpecError("embedding item must be an object")
            if set(raw) not in (
                {"input_id", "text", "source_content_sha256"},
                {"input_id", "text", "source_content_sha256", "item_id", "generation"},
            ):
                raise SpecError("embedding item contains an incomplete storage binding or unknown fields")
            input_id = raw.get("input_id")
            text = raw.get("text")
            digest = raw.get("source_content_sha256")
            if not isinstance(input_id, str) or not _ID.fullmatch(input_id) or input_id in ids:
                raise SpecError("embedding input ids must be unique opaque identifiers")
            ids.add(input_id)
            if not isinstance(text, str) or not text.strip():
                raise SpecError("embedding text must be a non-empty string")
            size = len(text.encode("utf-8"))
            if size > MAX_ITEM_TEXT_BYTES:
                raise SpecError("embedding text exceeds the per-item byte limit")
            total_bytes += size
            if total_bytes > MAX_TOTAL_TEXT_BYTES:
                raise SpecError("embedding batch exceeds the aggregate text byte limit")
            actual_digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
            if not isinstance(digest, str) or digest != actual_digest:
                raise SpecError("source_content_sha256 must match the exact UTF-8 text")
            item_id = raw.get("item_id")
            generation = raw.get("generation")
            if item_id is not None:
                if not isinstance(item_id, str) or not _ID.fullmatch(item_id):
                    raise SpecError("storage item_id must be an opaque identifier")
                if isinstance(generation, bool) or not isinstance(generation, int) or generation < 0:
                    raise SpecError("storage generation must be a non-negative integer")
            parsed.append(EmbeddingItem(input_id, text, digest, item_id, generation))
        return cls(mode, purpose, priority, tuple(parsed))

    def prepared_texts(self) -> tuple[str, ...]:
        prefix = QUERY_INSTRUCTION if self.mode == "query" else ""
        return tuple(prefix + item.text for item in self.items)


def sha256_file(path: Path, *, expected_size: int | None = None) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            size += len(chunk)
            if expected_size is not None and size > expected_size:
                raise SpecError("artifact exceeds its declared size")
            digest.update(chunk)
    return digest.hexdigest(), size


def canonical_f32(value: float) -> float:
    if not math.isfinite(value):
        raise SpecError("embedding contains a non-finite value")
    packed = struct.pack("<f", value)
    canonical = struct.unpack("<f", packed)[0]
    return 0.0 if canonical == 0.0 else canonical


def normalize_vector(values: Iterable[float], dimensions: int = DIMENSIONS) -> tuple[float, ...]:
    canonical = tuple(canonical_f32(value) for value in values)
    if len(canonical) != dimensions:
        raise SpecError(f"embedding dimensions must equal {dimensions}")
    norm = math.sqrt(math.fsum(float(value) * float(value) for value in canonical))
    if not math.isfinite(norm) or norm <= 1.0e-12:
        raise SpecError("embedding has zero or invalid norm")
    normalized = tuple(canonical_f32(value / norm) for value in canonical)
    final_norm = math.sqrt(math.fsum(float(value) * float(value) for value in normalized))
    if abs(final_norm - 1.0) > UNIT_NORM_TOLERANCE:
        raise SpecError("embedding could not be normalized within tolerance")
    return normalized


def tensor_bytes(values: Sequence[float]) -> bytes:
    return b"".join(struct.pack("<f", canonical_f32(value)) for value in values)


def tensor_payload(request: EmbeddingRequest, vectors: Sequence[Sequence[float]]) -> dict[str, Any]:
    if len(vectors) != len(request.items):
        raise SpecError("backend result count does not match the request")
    results: list[dict[str, Any]] = []
    preprocessing = f"{PREPROCESSING_REVISION}:{request.mode}"
    for item, vector in zip(request.items, vectors, strict=True):
        normalized = normalize_vector(vector)
        encoded = tensor_bytes(normalized)
        storage = None
        if item.item_id is not None:
            storage = {
                "schema_version": CHARACTER_DB_SCHEMA_VERSION,
                "item_id": item.item_id,
                "generation": item.generation,
                "model_id": MODEL_ID,
                "model_revision": SOURCE_REVISION,
                "preprocessing_revision": preprocessing,
                "dimensions": DIMENSIONS,
                "element_format": ELEMENT_FORMAT,
                "distance_metric": DISTANCE_METRIC,
                "source_content_sha256": item.source_content_sha256,
                "tensor_sha256": hashlib.sha256(encoded).hexdigest(),
                "tensor_byte_length": len(encoded),
            }
        results.append(
            {
                "input_id": item.input_id,
                "source_content_sha256": item.source_content_sha256,
                "values": list(normalized),
                "storage": storage,
            }
        )
    payload = {
        "contract_version": TENSOR_CONTRACT_VERSION,
        "model": {
            "provider": "onnxruntime-cpu",
            "model_id": MODEL_ID,
            "revision": SOURCE_REVISION,
            "pack_revision": PACK_REVISION,
            "dimensions": DIMENSIONS,
        },
        "preprocessing": {
            "revision": preprocessing,
            "maximum_tokens": MAX_SEQUENCE_TOKENS,
            "pooling": POOLING,
            "normalization": NORMALIZATION,
            "query_instruction_applied": request.mode == "query",
        },
        "element_format": ELEMENT_FORMAT,
        "distance_metric": DISTANCE_METRIC,
        "items": results,
    }
    encoded_payload = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(encoded_payload) > MAX_FRAME_BYTES:
        raise SpecError("embedding result would exceed the worker frame limit")
    return payload
