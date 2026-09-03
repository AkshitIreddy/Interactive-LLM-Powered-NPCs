"""Deterministic final-manifest builder for the gated BGE pack.

This module has no network or model-loading path.  It deliberately requires the
tokenizer's observed SHA-256, so a placeholder production manifest cannot be
created accidentally while the real-model lane is still queued.
"""

from __future__ import annotations

import re
from typing import Any

from backend import PINNED_ONNXRUNTIME_VERSION
from model_spec import (
    MODEL_ID,
    MODEL_CARD_SHA256,
    MODEL_CARD_SIZE_BYTES,
    MODEL_CARD_URL,
    MODEL_SHA256,
    MODEL_SIZE_BYTES,
    MODEL_URL,
    PACK_ID,
    PACK_REVISION,
    SOURCE_REVISION,
    TOKENIZER_SIZE_BYTES,
    TOKENIZER_SHA256,
    TOKENIZER_URL,
)

_SHA256 = re.compile(r"^[a-f0-9]{64}$")
SELF_TEST_FIXTURE_PATH = "workers/local-embedding/fixtures/self-test.request.json"
SELF_TEST_FIXTURE_SHA256 = "3abdba8b0018a4f553a96cbd55662e1d803cc38bb5e24040337309e695e45f03"
INSTALLED_ARTIFACT_BYTES = MODEL_SIZE_BYTES + TOKENIZER_SIZE_BYTES + MODEL_CARD_SIZE_BYTES
PEAK_TRANSACTIONAL_ARTIFACT_BYTES = INSTALLED_ARTIFACT_BYTES * 2


def _digest(value: str | None, name: str, *, nullable: bool) -> str | None:
    if value is None and nullable:
        return None
    if not isinstance(value, str) or not _SHA256.fullmatch(value) or value == "0" * 64:
        raise ValueError(f"{name} must be a non-zero canonical lowercase SHA-256")
    return value


def build_manifest(
    *,
    tokenizer_sha256: str,
    planning_resident_ram_bytes: int | None = None,
    planning_load_millis: int | None = None,
    planning_hardware: str | None = None,
) -> dict[str, Any]:
    tokenizer_digest = _digest(tokenizer_sha256, "tokenizer_sha256", nullable=False)
    if tokenizer_digest != TOKENIZER_SHA256:
        raise ValueError("tokenizer_sha256 does not match the reviewed immutable tokenizer")
    for value, name in (
        (planning_resident_ram_bytes, "planning_resident_ram_bytes"),
        (planning_load_millis, "planning_load_millis"),
    ):
        if value is not None and (isinstance(value, bool) or not isinstance(value, int) or value < 0):
            raise ValueError(f"{name} must be a non-negative integer or null")
    if planning_hardware is not None and (not isinstance(planning_hardware, str) or not planning_hardware.strip()):
        raise ValueError("planning_hardware must be a non-empty string or null")

    return {
        "$schema": "./model-pack-manifest.schema.json",
        "schema": "npc.model-pack/v2",
        "pack_id": PACK_ID,
        "revision": PACK_REVISION,
        "display_name": "BGE Small English v1.5 (local CPU embeddings)",
        "description": (
            "Optional English memory and character-knowledge embeddings using the immutable official FP32 ONNX export. "
            "Activation requires a matching signed machine qualification envelope."
        ),
        "source": {
            "project_url": f"https://huggingface.co/{MODEL_ID}",
            "immutable_revision": SOURCE_REVISION,
            "model_card_url": f"https://huggingface.co/{MODEL_ID}/blob/{SOURCE_REVISION}/README.md",
        },
        "capability": {"kind": "embedding", "scope": "generic"},
        "artifacts": [
            {
                "id": "bge-small-en-v1.5-onnx-fp32",
                "role": "model_weights",
                "kind": "file",
                "archive_format": None,
                "source_urls": [MODEL_URL],
                "size_bytes": MODEL_SIZE_BYTES,
                "sha256": MODEL_SHA256,
                "destination": "model/model.onnx",
                "strip_prefix": None,
                "required_paths": [],
            },
            {
                "id": "bge-small-en-v1.5-tokenizer-json",
                "role": "tokenizer",
                "kind": "file",
                "archive_format": None,
                "source_urls": [TOKENIZER_URL],
                "size_bytes": TOKENIZER_SIZE_BYTES,
                "sha256": tokenizer_digest,
                "destination": "model/tokenizer.json",
                "strip_prefix": None,
                "required_paths": [],
            },
            {
                "id": "bge-small-en-v1.5-model-card-license",
                "role": "license",
                "kind": "file",
                "archive_format": None,
                "source_urls": [MODEL_CARD_URL],
                "size_bytes": MODEL_CARD_SIZE_BYTES,
                "sha256": MODEL_CARD_SHA256,
                "destination": "licenses/BGE-MODEL-CARD.md",
                "strip_prefix": None,
                "required_paths": [],
            },
        ],
        "runtime": {
            "runtime": "onnxruntime",
            "immutable_revision": PINNED_ONNXRUNTIME_VERSION,
            "abi": "bge-bert-cls-f32-v1",
            "entrypoint": "workers/local-embedding/worker.py",
            "supported_platforms": ["windows10", "windows11"],
            "supported_architectures": ["x86_64"],
            "backends": ["onnxruntime-cpu"],
            "network_access_after_install": False,
        },
        "hardware": {
            "minimum_ram_bytes": 1_073_741_824,
            "recommended_ram_bytes": 2_147_483_648,
            "minimum_vram_bytes": 0,
            "recommended_vram_bytes": 0,
            "minimum_cpu_threads": 1,
            "accelerators": ["cpu"],
            "required_cpu_features": [],
        },
        "resources": {
            "storage_bytes": INSTALLED_ARTIFACT_BYTES,
            "peak_install_bytes": PEAK_TRANSACTIONAL_ARTIFACT_BYTES,
            "planning_resident_ram_bytes": planning_resident_ram_bytes,
            "planning_resident_vram_bytes": 0,
            "planning_load_millis": planning_load_millis,
            "planning_hardware": planning_hardware,
            "quality_tier": "fast",
            "languages": ["en"],
            "measurement": {
                "required_schema": "npc.measured-resource-envelope/v1",
                "minimum_samples": 20,
                "current_device_fingerprint_required": True,
                "signed_evidence_required": True,
                "p99_reload_required": True,
                "manifest_values_not_for_admission": True,
            },
        },
        "license": {
            "spdx_expression": "MIT",
            "license_name": "MIT License",
            "license_url": f"https://huggingface.co/BAAI/bge-small-en-v1.5/blob/{SOURCE_REVISION}/README.md",
            "attribution": "BAAI, BGE Small English v1.5",
            "redistributable": True,
            "commercial_use": "allowed",
            "derivative_use": "allowed",
            "acceptance_required": False,
            "notices": [
                "Model license is declared MIT by the immutable upstream model card.",
                "ONNX Runtime, NumPy, tokenizers, and transitive runtime notices ship separately.",
            ],
            "components": [
                {
                    "id": "bge-small-en-v1.5",
                    "component": "BAAI BGE Small English v1.5 model and tokenizer",
                    "spdx_expression": "MIT",
                    "license_url": f"https://huggingface.co/BAAI/bge-small-en-v1.5/blob/{SOURCE_REVISION}/README.md",
                    "immutable_source_revision": SOURCE_REVISION,
                    "redistributable": True,
                    "notice_required": True,
                }
            ],
        },
        "self_test": {
            "kind": "embedding_semantic_ordering",
            "suite_revision": "bge-small-en-v1.5-hidden-v1",
            "allowed_runtime_backends": ["onnxruntime-cpu"],
            "input_fixture": SELF_TEST_FIXTURE_PATH,
            "input_fixture_sha256": SELF_TEST_FIXTURE_SHA256,
            # Exact float bytes may differ across otherwise qualified CPU
            # kernels. The hidden test enforces semantic ordering, dimensions,
            # finiteness and normalization; its observed digest is bound into
            # the device-specific signed resource envelope, not this portable
            # catalog manifest.
            "expected_output_sha256": None,
            "timeout_millis": 30000,
        },
        "voices": [],
        "extensions": {
            "embedding": {
                "dimensions": 384,
                "maximum_tokens": 512,
                "pooling": "cls",
                "normalized": True,
            }
        },
        "lifecycle": {
            "explicit_download_required": True,
            "automatic_download_allowed": False,
            "install_strategy": "verify_then_atomic_activate",
            "repair_strategy": "verify_quarantine_reinstall",
            "remove_requires_unreferenced": True,
            "activation_gates": [
                "explicit_user_approval",
                "catalog_trust",
                "artifact_integrity",
                "license_acceptance",
                "runtime_compatibility",
                "self_test_attestation",
                "current_device_measurement",
                "whole_loadout_admission",
            ],
        },
        "trust": {
            "release_catalog_required": True,
            "immutable_revision_required": True,
            "artifact_sha256_required": True,
            "self_test_attestation_required": True,
            "measured_resource_envelope_required": True,
            "unsigned_activation_allowed": False,
        },
        "admission": {
            "state": "blocked_pending_measurement",
            "reason": (
                "CPU embedding pack remains blocked until this exact manifest is catalog-trusted, the hidden self-test is "
                "attested, and at least 20 signed current-device load/reload/operation samples admit the whole game loadout."
            ),
            "allowed_residencies": ["cpu_resident"],
            "unknowns_fail_closed": True,
        },
    }
