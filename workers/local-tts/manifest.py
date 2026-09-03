"""Strict worker-side parser for the canonical Kokoro model-pack manifest."""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any
from urllib.parse import urlparse

from voices import STOCK_VOICES

SCHEMA = "npc.model-pack/v2"
SCHEMA_URI = "./model-pack-manifest.schema.json"
PACK_ID = "local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64"
PACK_REVISION = "2026.08.30-r1"
RUNTIME_VERSION = "1.13.6"
RUNTIME_GIT_SHA = "1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911"
ONNXRUNTIME_VERSION = "1.27.1"
RUNTIME_REVISION = (
    f"{RUNTIME_VERSION}+{RUNTIME_GIT_SHA}+onnxruntime.{ONNXRUNTIME_VERSION}"
)
MODEL_ID = "kokoro-82m-v1.0-int8-multilang"
TOKEN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+@-]{0,127}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
ALLOWED_ARTIFACT_HOSTS = {
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
}
EXPECTED_LICENSE_IDS = frozenset(
    {
        "kokoro-model-apache-2.0",
        "kokoro-training-attribution-cc",
        "sherpa-onnx-apache-2.0",
        "onnxruntime-mit",
        "espeak-ng-gpl-3.0-or-later",
        "piper-phonemize-mit",
    }
)
EXPECTED_ACTIVATION_GATES = {
    "explicit_user_approval",
    "catalog_trust",
    "artifact_integrity",
    "license_acceptance",
    "runtime_compatibility",
    "self_test_attestation",
    "current_device_measurement",
    "whole_loadout_admission",
}


class ManifestError(ValueError):
    pass


def _object(value: Any, field: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ManifestError(f"{field} must be an object")
    return value


def _closed_object(
    value: Any,
    field: str,
    expected_fields: set[str],
) -> dict[str, Any]:
    value = _object(value, field)
    if set(value) != expected_fields:
        raise ManifestError(f"{field} fields do not match the canonical contract")
    return value


def _list(value: Any, field: str) -> list[Any]:
    if not isinstance(value, list):
        raise ManifestError(f"{field} must be an array")
    return value


def _text(value: Any, field: str, maximum: int = 4096) -> str:
    if not isinstance(value, str) or not value or len(value.encode("utf-8")) > maximum:
        raise ManifestError(f"{field} must be bounded non-empty text")
    return value


def _token(value: Any, field: str) -> str:
    value = _text(value, field, 128)
    if not TOKEN.fullmatch(value):
        raise ManifestError(f"{field} is not a stable token")
    return value


def _digest(value: Any, field: str) -> str:
    value = _text(value, field, 64)
    if not SHA256.fullmatch(value):
        raise ManifestError(f"{field} must be lowercase SHA-256")
    return value


def _relative_path(value: Any, field: str) -> str:
    value = _text(value, field, 1024).replace("\\", "/")
    path = PurePosixPath(value)
    if path.is_absolute() or not path.parts or any(
        part in ("", ".", "..") for part in path.parts
    ):
        raise ManifestError(f"{field} must be a normalized relative path")
    if ":" in path.parts[0] or value.startswith("//"):
        raise ManifestError(f"{field} must not select a device or UNC path")
    return str(path)


@dataclass(frozen=True, slots=True)
class Artifact:
    artifact_id: str
    url: str
    size_bytes: int
    sha256: str
    archive: str
    strip_prefix: str
    destination: str
    required_paths: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class CriticalFile:
    path: str
    size_bytes: int
    sha256: str


PINNED_CRITICAL_FILES = (
    CriticalFile(
        "model/model.int8.onnx",
        114_298_054,
        "77ef4f0513401d508ed7831f8504c7042df58bc75e004ec9666894590f999b1d",
    ),
    CriticalFile(
        "model/voices.bin",
        27_678_720,
        "8a77c0d397026208d22211f37670b5b3b11e03f190756b25a1d24041fced82a9",
    ),
    CriticalFile(
        "model/tokens.txt",
        687,
        "6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425",
    ),
    CriticalFile(
        "model/lexicon-us-en.txt",
        5_956_885,
        "7daaab53a181be9885b853a8582bf1838186317e5dadacbcef9c426d6fa0da14",
    ),
    CriticalFile(
        "model/espeak-ng-data/phondata",
        550_424,
        "4e0288957874029a8c3c9f41a8f517ad4bf18127046decbdd4b9d1d6807ce3a3",
    ),
    CriticalFile(
        "model/LICENSE",
        11_358,
        "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
    ),
)


@dataclass(frozen=True, slots=True)
class PackManifest:
    path: Path
    raw: dict[str, Any]
    canonical_sha256: str
    pack_id: str
    revision: str
    runtime_abi: str
    runtime_version: str
    runtime_git_sha: str
    onnxruntime_version: str
    model_id: str
    output_sample_rate_hz: int
    artifacts: tuple[Artifact, ...]
    critical_files: tuple[CriticalFile, ...]
    license_acceptance_ids: frozenset[str]
    allowed_redirect_hosts: frozenset[str]

    @property
    def install_key(self) -> str:
        return f"{self.pack_id}@{self.revision}"


def canonical_json_bytes(raw: dict[str, Any]) -> bytes:
    return json.dumps(
        raw,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def _parse_artifacts(raw: dict[str, Any]) -> tuple[Artifact, ...]:
    artifacts: list[Artifact] = []
    artifact_ids: set[str] = set()
    expected_roles = {"sherpa-runtime": "runtime", "kokoro-model": "other"}
    expected_urls = {
        "sherpa-runtime": (
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.6/"
            "sherpa-onnx-v1.13.6-win-x64-shared-MD-Release-lib.tar.bz2"
        ),
        "kokoro-model": (
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/"
            "kokoro-int8-multi-lang-v1_0.tar.bz2"
        ),
    }
    expected_facts = {
        "sherpa-runtime": {
            "size_bytes": 7_215_482,
            "sha256": "dca033829d3a7e74c127fc0d349a12257fb890fe5038a381ab1706e4b35cf0fa",
            "destination": "runtime",
            "strip_prefix": "sherpa-onnx-v1.13.6-win-x64-shared-MD-Release-lib",
            "required_paths": {
                "lib/sherpa-onnx-c-api.dll",
                "lib/onnxruntime.dll",
            },
        },
        "kokoro-model": {
            "size_bytes": 131_839_838,
            "sha256": "75654a84864be26f345f020f4070c2c019e96dd1b7f9bf6e2ffd59efac6aa5a3",
            "destination": "model",
            "strip_prefix": "kokoro-int8-multi-lang-v1_0",
            "required_paths": {
                "model.int8.onnx",
                "voices.bin",
                "tokens.txt",
                "lexicon-us-en.txt",
                "espeak-ng-data/phondata",
                "LICENSE",
            },
        },
    }
    for index, value in enumerate(_list(raw.get("artifacts"), "artifacts")):
        item = _closed_object(
            value,
            f"artifacts[{index}]",
            {
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
            },
        )
        artifact_id = _token(item["id"], f"artifacts[{index}].id")
        if artifact_id in artifact_ids:
            raise ManifestError("artifact IDs must be unique")
        artifact_ids.add(artifact_id)
        if item["role"] != expected_roles.get(artifact_id):
            raise ManifestError("combined archive role is not the audited role")
        if item["kind"] != "archive" or item["archive_format"] != "tar_bz2":
            raise ManifestError("Kokoro archives must truthfully remain tar_bz2")
        urls = _list(item["source_urls"], "artifact.source_urls")
        if len(urls) != 1:
            raise ManifestError("each candidate archive has one pinned source")
        url = _text(urls[0], "artifact.source_url", 2048)
        parsed = urlparse(url)
        if (
            url != expected_urls.get(artifact_id)
            or parsed.scheme != "https"
            or parsed.hostname != "github.com"
            or parsed.username
            or parsed.password
            or parsed.query
            or parsed.fragment
        ):
            raise ManifestError("artifact URL is not the pinned credential-free URL")
        size = item["size_bytes"]
        if (
            isinstance(size, bool)
            or not isinstance(size, int)
            or size <= 0
            or size > 2_000_000_000
        ):
            raise ManifestError("artifact size is invalid")
        parsed_artifact = Artifact(
            artifact_id=artifact_id,
            url=url,
            size_bytes=size,
            sha256=_digest(item["sha256"], "artifact.sha256"),
            archive="tar.bz2",
            strip_prefix=_relative_path(
                item["strip_prefix"],
                "artifact.strip_prefix",
            ),
            destination=_relative_path(
                item["destination"],
                "artifact.destination",
            ),
            required_paths=tuple(
                _relative_path(path, "artifact.required_path")
                for path in _list(
                    item["required_paths"],
                    "artifact.required_paths",
                )
            ),
        )
        facts = expected_facts[artifact_id]
        if (
            parsed_artifact.size_bytes != facts["size_bytes"]
            or parsed_artifact.sha256 != facts["sha256"]
            or parsed_artifact.destination != facts["destination"]
            or parsed_artifact.strip_prefix != facts["strip_prefix"]
            or set(parsed_artifact.required_paths) != facts["required_paths"]
        ):
            raise ManifestError("pinned archive identity or extraction facts drifted")
        artifacts.append(parsed_artifact)
    if artifact_ids != set(expected_roles):
        raise ManifestError("runtime and model archives are both required")
    if sum(item.size_bytes for item in artifacts) != 139_055_320:
        raise ManifestError("pinned archive byte total drifted")
    return tuple(artifacts)


def load_manifest(path: str | Path) -> PackManifest:
    source = Path(path).resolve(strict=True)
    try:
        raw = json.loads(source.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ManifestError("manifest is not valid UTF-8 JSON") from error
    raw = _closed_object(
        raw,
        "manifest",
        {
            "$schema",
            "schema",
            "pack_id",
            "revision",
            "display_name",
            "description",
            "source",
            "capability",
            "artifacts",
            "runtime",
            "hardware",
            "resources",
            "license",
            "self_test",
            "voices",
            "extensions",
            "lifecycle",
            "trust",
            "admission",
        },
    )
    if raw["$schema"] != SCHEMA_URI or raw["schema"] != SCHEMA:
        raise ManifestError("unsupported or noncanonical model-pack schema")
    pack_id = _token(raw["pack_id"], "pack_id")
    revision = _token(raw["revision"], "revision")
    if pack_id != PACK_ID or revision != PACK_REVISION:
        raise ManifestError("manifest selects an unexpected pack identity")

    source_metadata = _closed_object(
        raw["source"],
        "source",
        {"project_url", "immutable_revision", "model_card_url"},
    )
    if (
        source_metadata["project_url"]
        != "https://huggingface.co/hexgrad/Kokoro-82M"
        or source_metadata["immutable_revision"]
        != "f3ff3571791e39611d31c381e3a41a3af07b4987"
    ):
        raise ManifestError("Kokoro source identity drifted")
    capability = _closed_object(
        raw["capability"],
        "capability",
        {"kind", "scope"},
    )
    if capability != {"kind": "speech_synthesis", "scope": "generic"}:
        raise ManifestError("manifest capability must remain generic speech synthesis")

    artifacts = _parse_artifacts(raw)
    runtime = _closed_object(
        raw["runtime"],
        "runtime",
        {
            "runtime",
            "immutable_revision",
            "abi",
            "entrypoint",
            "supported_platforms",
            "supported_architectures",
            "backends",
            "network_access_after_install",
        },
    )
    if (
        runtime["runtime"] != "sherpa-onnx"
        or runtime["immutable_revision"] != RUNTIME_REVISION
        or runtime["abi"] != "npc-local-tts-sherpa-c-api-v1"
        or runtime["entrypoint"] != "workers/local-tts/worker.py"
        or set(_list(runtime["supported_platforms"], "runtime.supported_platforms"))
        != {"windows10", "windows11"}
        or runtime["supported_architectures"] != ["x86_64"]
        or runtime["backends"] != ["sherpa-onnx-1.13.6-cpu"]
        or runtime["network_access_after_install"] is not False
    ):
        raise ManifestError("runtime identity or offline boundary drifted")

    hardware = _closed_object(
        raw["hardware"],
        "hardware",
        {
            "minimum_ram_bytes",
            "recommended_ram_bytes",
            "minimum_vram_bytes",
            "recommended_vram_bytes",
            "minimum_cpu_threads",
            "accelerators",
            "required_cpu_features",
        },
    )
    if hardware != {
        "minimum_ram_bytes": 0,
        "recommended_ram_bytes": 0,
        "minimum_vram_bytes": 0,
        "recommended_vram_bytes": 0,
        "minimum_cpu_threads": 1,
        "accelerators": ["cpu"],
        "required_cpu_features": [],
    }:
        raise ManifestError("unmeasured hardware requirements must remain conservative")

    resources = _closed_object(
        raw["resources"],
        "resources",
        {
            "storage_bytes",
            "peak_install_bytes",
            "planning_resident_ram_bytes",
            "planning_resident_vram_bytes",
            "planning_load_millis",
            "planning_hardware",
            "quality_tier",
            "languages",
            "measurement",
        },
    )
    if (
        resources["storage_bytes"] != 139_055_320
        or resources["peak_install_bytes"] != 1_139_055_320
        or any(
            resources[field] is not None
            for field in (
                "planning_resident_ram_bytes",
                "planning_resident_vram_bytes",
                "planning_load_millis",
                "planning_hardware",
            )
        )
        or resources["quality_tier"] != "experimental"
        or set(_list(resources["languages"], "resources.languages"))
        != {"en-US", "en-GB"}
    ):
        raise ManifestError("resource planning facts drifted or claim measurement")
    measurement = _closed_object(
        resources["measurement"],
        "resources.measurement",
        {
            "required_schema",
            "minimum_samples",
            "current_device_fingerprint_required",
            "signed_evidence_required",
            "p99_reload_required",
            "manifest_values_not_for_admission",
        },
    )
    if measurement != {
        "required_schema": "npc.measured-resource-envelope/v1",
        "minimum_samples": 20,
        "current_device_fingerprint_required": True,
        "signed_evidence_required": True,
        "p99_reload_required": True,
        "manifest_values_not_for_admission": True,
    }:
        raise ManifestError("measurement trust requirements drifted")

    license_metadata = _closed_object(
        raw["license"],
        "license",
        {
            "spdx_expression",
            "license_name",
            "license_url",
            "attribution",
            "redistributable",
            "commercial_use",
            "derivative_use",
            "acceptance_required",
            "notices",
            "components",
        },
    )
    components = _list(license_metadata["components"], "license.components")
    component_ids = frozenset(
        _token(
            _closed_object(
                item,
                "license.component",
                {
                    "id",
                    "component",
                    "spdx_expression",
                    "license_url",
                    "immutable_source_revision",
                    "redistributable",
                    "notice_required",
                },
            )["id"],
            "license.component.id",
        )
        for item in components
    )
    if (
        component_ids != EXPECTED_LICENSE_IDS
        or license_metadata["acceptance_required"] is not True
        or license_metadata["redistributable"] is not False
        or license_metadata["commercial_use"] != "allowed"
        or license_metadata["derivative_use"] != "allowed"
        or len(_list(license_metadata["notices"], "license.notices")) < 6
    ):
        raise ManifestError("transitive license closure is incomplete")
    for component in components:
        if (
            component["notice_required"] is not True
            or component["redistributable"] is not True
        ):
            raise ManifestError("component license notice/redistribution facts drifted")

    manifest_voices = _list(raw["voices"], "voices")
    if len(manifest_voices) != len(STOCK_VOICES):
        raise ManifestError("voice catalog length drifted")
    for item, expected in zip(manifest_voices, STOCK_VOICES, strict=True):
        item = _closed_object(
            item,
            "voice",
            {
                "voice_id",
                "display_name",
                "locale",
                "stock_voice",
                "voice_cloning",
                "license_component_id",
            },
        )
        if (
            item["voice_id"] != expected.voice_id
            or item["display_name"] != expected.display_name
            or item["locale"] != expected.locale
            or item["stock_voice"] is not True
            or item["voice_cloning"] is not False
            or item["license_component_id"] != "kokoro-model-apache-2.0"
        ):
            raise ManifestError("typed voice catalog drifted from the audited order")

    extensions = _closed_object(
        raw["extensions"],
        "extensions",
        {"speech_synthesis"},
    )
    synthesis = _closed_object(
        extensions["speech_synthesis"],
        "extensions.speech_synthesis",
        {
            "output_sample_rate_hz",
            "channels",
            "sample_format",
            "incremental_pcm",
            "voice_cloning",
            "maximum_concurrent_sessions",
            "maximum_text_utf8_bytes",
            "word_timing",
            "phoneme_timing",
            "viseme_timing",
        },
    )
    if synthesis != {
        "output_sample_rate_hz": 24_000,
        "channels": 1,
        "sample_format": "pcm_s16le",
        "incremental_pcm": True,
        "voice_cloning": False,
        "maximum_concurrent_sessions": 1,
        "maximum_text_utf8_bytes": 262_144,
        "word_timing": False,
        "phoneme_timing": False,
        "viseme_timing": False,
    }:
        raise ManifestError("speech-synthesis capability facts drifted")

    self_test = _closed_object(
        raw["self_test"],
        "self_test",
        {
            "kind",
            "suite_revision",
            "allowed_runtime_backends",
            "input_fixture",
            "input_fixture_sha256",
            "expected_output_sha256",
            "timeout_millis",
        },
    )
    if (
        self_test["kind"] != "kokoro_structural_tts_v1"
        or self_test["allowed_runtime_backends"]
        != ["sherpa-onnx-1.13.6-cpu"]
        or self_test["input_fixture"]
        != "workers/local-tts/fixtures/self-test.txt"
        or self_test["input_fixture_sha256"]
        != "cf627dd34ca9f579d1a40af3b5ad95f72f90be432414f8898df036494ea75856"
        or self_test["expected_output_sha256"] is not None
    ):
        raise ManifestError("structural self-test contract drifted")

    lifecycle = _closed_object(
        raw["lifecycle"],
        "lifecycle",
        {
            "explicit_download_required",
            "automatic_download_allowed",
            "install_strategy",
            "repair_strategy",
            "remove_requires_unreferenced",
            "activation_gates",
        },
    )
    if (
        lifecycle["explicit_download_required"] is not True
        or lifecycle["automatic_download_allowed"] is not False
        or lifecycle["install_strategy"] != "verify_then_atomic_activate"
        or lifecycle["repair_strategy"] != "verify_quarantine_reinstall"
        or lifecycle["remove_requires_unreferenced"] is not True
        or set(_list(lifecycle["activation_gates"], "lifecycle.activation_gates"))
        != EXPECTED_ACTIVATION_GATES
    ):
        raise ManifestError("explicit lifecycle gates drifted")
    trust = _closed_object(
        raw["trust"],
        "trust",
        {
            "release_catalog_required",
            "immutable_revision_required",
            "artifact_sha256_required",
            "self_test_attestation_required",
            "measured_resource_envelope_required",
            "unsigned_activation_allowed",
        },
    )
    if trust != {
        "release_catalog_required": True,
        "immutable_revision_required": True,
        "artifact_sha256_required": True,
        "self_test_attestation_required": True,
        "measured_resource_envelope_required": True,
        "unsigned_activation_allowed": False,
    }:
        raise ManifestError("catalog and admission trust must fail closed")
    admission = _closed_object(
        raw["admission"],
        "admission",
        {"state", "reason", "allowed_residencies", "unknowns_fail_closed"},
    )
    if (
        admission["state"] != "blocked_pending_measurement"
        or admission["allowed_residencies"] != ["cpu_resident"]
        or admission["unknowns_fail_closed"] is not True
    ):
        raise ManifestError("unmeasured Kokoro pack must remain admission-blocked")

    digest = hashlib.sha256(canonical_json_bytes(raw)).hexdigest()
    return PackManifest(
        path=source,
        raw=raw,
        canonical_sha256=digest,
        pack_id=pack_id,
        revision=revision,
        runtime_abi=runtime["abi"],
        runtime_version=RUNTIME_VERSION,
        runtime_git_sha=RUNTIME_GIT_SHA,
        onnxruntime_version=ONNXRUNTIME_VERSION,
        model_id=MODEL_ID,
        output_sample_rate_hz=synthesis["output_sample_rate_hz"],
        artifacts=artifacts,
        critical_files=PINNED_CRITICAL_FILES,
        license_acceptance_ids=EXPECTED_LICENSE_IDS,
        allowed_redirect_hosts=frozenset(ALLOWED_ARTIFACT_HOSTS),
    )
