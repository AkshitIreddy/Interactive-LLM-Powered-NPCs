"""Strict parsing for the checked-in model and runtime trust records."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any
from urllib.parse import urlsplit

from .constants import (
    ALLOWED_DOWNLOAD_HOSTS,
    MAX_DOWNLOAD_ARTIFACTS,
    MODEL_ARTIFACT_SHA256,
    MODEL_ARTIFACT_SIZE,
    MODEL_ID,
    MODEL_REVISION,
    PACK_SCHEMA,
    RUNTIME_ABI,
    RUNTIME_BUNDLE_SCHEMA,
    RUNTIME_COMMIT,
    RUNTIME_RELEASE,
)
from .errors import LocalLlmError, invalid

_ID = re.compile(r"^[a-z0-9][a-z0-9.-]{1,94}[a-z0-9]$")
_SHA256 = re.compile(r"^[a-f0-9]{64}$")
_COMMIT = re.compile(r"^[a-f0-9]{40}$")
_WINDOWS_RESERVED = re.compile(
    r"^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$", re.IGNORECASE
)


def _object(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise invalid(f"{name} must be an object", field=name)
    return value


def _array(value: Any, name: str) -> list[Any]:
    if not isinstance(value, list):
        raise invalid(f"{name} must be an array", field=name)
    return value


def _string(value: Any, name: str, *, maximum: int = 4_096) -> str:
    if not isinstance(value, str) or not value or len(value.encode("utf-8")) > maximum:
        raise invalid(f"{name} must be a bounded non-empty string", field=name)
    return value


def _positive_int(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0 or value > 2**63 - 1:
        raise invalid(f"{name} must be a positive integer", field=name)
    return value


def safe_relative_path(value: str, name: str = "destination") -> PurePosixPath:
    if "\\" in value or "\x00" in value or ":" in value:
        raise invalid(f"{name} contains an unsafe Windows path component", field=name)
    path = PurePosixPath(value)
    if path.is_absolute() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
        raise invalid(f"{name} must be a normalized relative path", field=name)
    for part in path.parts:
        if part.endswith((" ", ".")) or _WINDOWS_RESERVED.match(part):
            raise invalid(f"{name} contains a reserved Windows path component", field=name)
    return path


def validate_pinned_https_url(value: str, name: str = "source_url") -> str:
    parsed = urlsplit(value)
    hostname = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or parsed.username or parsed.password or parsed.fragment:
        raise invalid(f"{name} must be a credential-free HTTPS URL", field=name)
    if hostname not in ALLOWED_DOWNLOAD_HOSTS:
        raise invalid(f"{name} host is not in the artifact allowlist", field=name)
    if hostname == "huggingface.co":
        parts = parsed.path.split("/")
        try:
            revision = parts[parts.index("resolve") + 1]
        except (ValueError, IndexError) as error:
            raise invalid(f"{name} must use a commit-pinned Hub resolve URL", field=name) from error
        if not _COMMIT.fullmatch(revision):
            raise invalid(f"{name} Hub revision must be a full commit hash", field=name)
    elif hostname == "github.com" and "/releases/download/" not in parsed.path:
        raise invalid(f"{name} GitHub artifact must be an immutable release asset", field=name)
    return value


@dataclass(frozen=True, slots=True)
class Artifact:
    artifact_id: str
    kind: str
    source_urls: tuple[str, ...]
    size_bytes: int
    sha256: str
    destination: PurePosixPath

    @classmethod
    def parse(cls, value: Any) -> "Artifact":
        item = _object(value, "artifact")
        allowed = {
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
        if set(item) != allowed:
            raise invalid("artifact fields do not match npc.model-pack/v2")
        artifact_id = _string(item["id"], "artifact.id", maximum=128)
        _string(item["role"], "artifact.role", maximum=32)
        kind = _string(item["kind"], "artifact.kind", maximum=16)
        if kind not in {"file", "archive"}:
            raise invalid("artifact.kind is unsupported", field="artifact.kind")
        archive_format = item["archive_format"]
        strip_prefix = item["strip_prefix"]
        required_paths = _array(item["required_paths"], "artifact.required_paths")
        if kind == "file" and (archive_format is not None or strip_prefix is not None or required_paths):
            raise invalid("file artifact extraction fields must be empty", field="artifact.kind")
        if kind == "archive" and archive_format not in {"zip", "tar", "tar_gz"}:
            raise invalid("archive artifact format is unsupported", field="artifact.archive_format")
        sources = _array(item["source_urls"], "artifact.source_urls")
        if not 1 <= len(sources) <= 8:
            raise invalid("artifact.source_urls has an invalid length", field="artifact.source_urls")
        source_urls = tuple(validate_pinned_https_url(_string(url, "source_url", maximum=2_048)) for url in sources)
        sha256 = _string(item["sha256"], "artifact.sha256", maximum=64).lower()
        if not _SHA256.fullmatch(sha256):
            raise invalid("artifact.sha256 must be a lowercase SHA-256 digest", field="artifact.sha256")
        return cls(
            artifact_id=artifact_id,
            kind=kind,
            source_urls=source_urls,
            size_bytes=_positive_int(item["size_bytes"], "artifact.size_bytes"),
            sha256=sha256,
            destination=safe_relative_path(_string(item["destination"], "artifact.destination", maximum=512)),
        )


@dataclass(frozen=True, slots=True)
class ModelPack:
    path: Path
    pack_id: str
    revision: str
    runtime: str
    runtime_abi: str
    minimum_runtime_revision: str
    artifacts: tuple[Artifact, ...]
    self_test: dict[str, Any]

    @classmethod
    def load(cls, path: Path) -> "ModelPack":
        try:
            raw = path.read_bytes()
            value = json.loads(raw)
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
            raise LocalLlmError("invalid_manifest", "model-pack manifest could not be decoded") from error
        item = _object(value, "manifest")
        if item.get("schema") != PACK_SCHEMA:
            raise LocalLlmError("invalid_manifest", "model-pack schema is unsupported")
        pack_id = _string(item.get("pack_id"), "pack_id", maximum=96)
        if not _ID.fullmatch(pack_id):
            raise invalid("pack_id is invalid", field="pack_id")
        artifacts_raw = _array(item.get("artifacts"), "artifacts")
        if not 1 <= len(artifacts_raw) <= MAX_DOWNLOAD_ARTIFACTS:
            raise invalid("artifacts has an invalid length", field="artifacts")
        artifacts = tuple(Artifact.parse(entry) for entry in artifacts_raw)
        ids = [artifact.artifact_id.casefold() for artifact in artifacts]
        destinations = [str(artifact.destination).casefold() for artifact in artifacts]
        if len(set(ids)) != len(ids) or len(set(destinations)) != len(destinations):
            raise LocalLlmError("invalid_manifest", "artifact identities or destinations collide")
        runtime = _object(item.get("runtime"), "runtime")
        result = cls(
            path=path,
            pack_id=pack_id,
            revision=_string(item.get("revision"), "revision", maximum=128),
            runtime=_string(runtime.get("runtime"), "runtime.runtime", maximum=128),
            runtime_abi=_string(runtime.get("abi"), "runtime.abi", maximum=256),
            minimum_runtime_revision=_string(runtime.get("immutable_revision"), "runtime.immutable_revision", maximum=256),
            artifacts=artifacts,
            self_test=_object(item.get("self_test"), "self_test"),
        )
        result.validate_frozen_qwen_pack()
        return result

    def validate_frozen_qwen_pack(self) -> None:
        if self.pack_id != MODEL_ID or self.revision != MODEL_REVISION:
            raise LocalLlmError("manifest_identity_mismatch", "manifest does not identify the supported Qwen pack")
        if self.runtime != "llama.cpp" or self.runtime_abi != RUNTIME_ABI:
            raise LocalLlmError("runtime_abi_mismatch", "manifest runtime ABI does not match this worker")
        if RUNTIME_RELEASE not in self.minimum_runtime_revision or RUNTIME_COMMIT not in self.minimum_runtime_revision:
            raise LocalLlmError("runtime_revision_mismatch", "manifest runtime revision does not match this worker")
        matches = [artifact for artifact in self.artifacts if artifact.artifact_id.endswith("q4-k-m-gguf")]
        if len(matches) != 1:
            raise LocalLlmError("manifest_artifact_mismatch", "manifest must contain exactly one supported GGUF artifact")
        model = matches[0]
        if model.kind != "file" or model.size_bytes != MODEL_ARTIFACT_SIZE or model.sha256 != MODEL_ARTIFACT_SHA256:
            raise LocalLlmError("manifest_artifact_mismatch", "GGUF identity does not match this worker")

    @property
    def model_artifact(self) -> Artifact:
        return next(artifact for artifact in self.artifacts if artifact.sha256 == MODEL_ARTIFACT_SHA256)


@dataclass(frozen=True, slots=True)
class RuntimeVariant:
    variant_id: str
    backend: str
    artifact: Artifact


@dataclass(frozen=True, slots=True)
class RuntimeBundle:
    path: Path
    abi: str
    release_tag: str
    source_commit: str
    entrypoint: str
    variants: tuple[RuntimeVariant, ...]

    @classmethod
    def load(cls, path: Path) -> "RuntimeBundle":
        try:
            value = json.loads(path.read_bytes())
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
            raise LocalLlmError("invalid_runtime_bundle", "runtime bundle could not be decoded") from error
        item = _object(value, "runtime_bundle")
        if item.get("schema") != RUNTIME_BUNDLE_SCHEMA:
            raise LocalLlmError("invalid_runtime_bundle", "runtime bundle schema is unsupported")
        variants: list[RuntimeVariant] = []
        for raw_variant in _array(item.get("variants"), "variants"):
            variant = _object(raw_variant, "variant")
            raw_artifact = _object(variant.get("artifact"), "variant.artifact")
            artifact = Artifact.parse(
                {
                    "id": _string(variant.get("id"), "variant.id", maximum=128),
                    "role": "runtime",
                    "kind": raw_artifact.get("kind"),
                    "archive_format": "zip",
                    "source_urls": [raw_artifact.get("url")],
                    "size_bytes": raw_artifact.get("size_bytes"),
                    "sha256": raw_artifact.get("sha256"),
                    "destination": f"runtime/{variant.get('id')}.zip",
                    "strip_prefix": None,
                    "required_paths": [],
                }
            )
            variants.append(
                RuntimeVariant(
                    variant_id=_string(variant.get("id"), "variant.id", maximum=128),
                    backend=_string(variant.get("backend"), "variant.backend", maximum=32),
                    artifact=artifact,
                )
            )
        result = cls(
            path=path,
            abi=_string(item.get("abi"), "abi", maximum=256),
            release_tag=_string(item.get("release_tag"), "release_tag", maximum=64),
            source_commit=_string(item.get("source_commit"), "source_commit", maximum=40),
            entrypoint=_string(item.get("entrypoint"), "entrypoint", maximum=128),
            variants=tuple(variants),
        )
        if (
            result.abi != RUNTIME_ABI
            or result.release_tag != RUNTIME_RELEASE
            or result.source_commit != RUNTIME_COMMIT
            or result.entrypoint.casefold() != "llama-server.exe"
            or {variant.backend for variant in result.variants} != {"cpu", "vulkan"}
        ):
            raise LocalLlmError("runtime_abi_mismatch", "runtime trust record does not match this worker")
        return result

    def variant(self, variant_id: str) -> RuntimeVariant:
        for variant in self.variants:
            if variant.variant_id == variant_id:
                return variant
        raise LocalLlmError("unsupported_runtime_backend", "runtime backend is not approved")
