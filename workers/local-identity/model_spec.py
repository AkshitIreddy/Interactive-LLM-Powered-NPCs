"""Immutable model and data contracts for the local identity observation worker.

The worker produces detector observations only.  It never selects a character;
`npc-identity-engine` remains authoritative for tracks, consensus, ambiguity,
manual correction, and offscreen continuity.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
import struct
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

PROTOCOL_VERSION = "1.0"
REQUEST_CONTRACT_VERSION = "npc.identity-observation-request/v1"
OBSERVATION_CONTRACT_VERSION = "npc.identity-observations/v1"
REFERENCE_IMPORT_CONTRACT_VERSION = "npc.identity-reference-import-request/v1"
PORTABLE_REFERENCE_SCHEMA_VERSION = 1
MAX_FRAME_BYTES = 1_048_576
MAX_SHARED_IMAGE_BYTES = 64 * 1024 * 1024
MAX_IMAGE_EDGE = 8_192
MAX_FACES = 64

PACK_ID = "opencv-yunet-sface-private-eval"
PACK_REVISION = "zoo-47534e27-opencv-5.0.0.93"
PACK_MANIFEST_SHA256 = "a4af4874af77c4dc517fe41990371e96e68a4469ee6817102876b7b074302e67"
SOURCE_REVISION = "47534e27c9851bb1128ccc0102f1145e27f23f98"
DETECTOR_ID = "opencv-zoo-yunet-2026may"
DETECTOR_REVISION = "sha256:ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0"
EMBEDDING_PROVIDER = "opencv-zoo"
EMBEDDING_MODEL_ID = "sface-2021dec-mobilefacenet"
EMBEDDING_REVISION = "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"
EMBEDDING_DIMENSIONS = 128
PREPROCESSING = "opencv-face-recognizer-sf-aligncrop-bgr-112x112-l2-f32-v1"

YUNET_SHA256 = "ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0"
YUNET_SIZE_BYTES = 229_738
SFACE_SHA256 = "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"
SFACE_SIZE_BYTES = 38_696_353

_SHA256 = re.compile(r"^[0-9a-f]{64}$")
_OPAQUE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,254}[A-Za-z0-9]$")


class SpecError(ValueError):
    pass


def opaque(value: Any, name: str, *, maximum: int = 256) -> str:
    if not isinstance(value, str) or len(value) > maximum or not _OPAQUE.fullmatch(value):
        raise SpecError(f"{name} must be a bounded opaque identifier")
    return value


def sha256(value: Any, name: str) -> str:
    if not isinstance(value, str) or not _SHA256.fullmatch(value):
        raise SpecError(f"{name} must be a lowercase SHA-256 digest")
    return value


def pixel_mapping_name(lease_id: str, lease_nonce: str) -> str:
    """Derive the only mapping name the worker will open for a lease.

    Binding the unguessable local-session name to both opaque lease values
    prevents a valid worker request from being redirected at an unrelated
    named mapping. The broker must still apply a same-user/SID read-only ACL.
    """

    binding = hashlib.sha256(f"{lease_id}\0{lease_nonce}".encode("utf-8")).hexdigest()
    return f"Local\\npc.identity.{binding}"


def uint(value: Any, name: str, *, minimum: int = 0, maximum: int = 2**63 - 1) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise SpecError(f"{name} must be an integer in {minimum}..={maximum}")
    return value


def finite(value: Any, name: str, *, minimum: float | None = None, maximum: float | None = None) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        raise SpecError(f"{name} must be finite")
    result = float(value)
    if minimum is not None and result < minimum or maximum is not None and result > maximum:
        raise SpecError(f"{name} is outside its allowed range")
    return result


@dataclass(frozen=True, slots=True)
class ArtifactSpec:
    artifact_id: str
    size_bytes: int
    sha256: str
    destination: PurePosixPath


@dataclass(frozen=True, slots=True)
class PackSpec:
    path: Path
    manifest_sha256: str
    artifacts: tuple[ArtifactSpec, ...]

    @classmethod
    def load(cls, path: Path) -> "PackSpec":
        if path.is_symlink() or not path.is_file():
            raise SpecError("pack manifest must be a regular file")
        data = path.read_bytes()
        try:
            raw = json.loads(data)
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SpecError("pack manifest must be UTF-8 JSON") from exc
        if not isinstance(raw, dict):
            raise SpecError("pack manifest root must be an object")
        required = {
            "$schema", "schema", "pack_id", "revision", "display_name",
            "description", "source", "capability", "artifacts", "runtime",
            "hardware", "resources", "license", "self_test", "voices",
            "extensions", "lifecycle", "trust", "admission",
        }
        if set(raw) != required:
            raise SpecError("pack manifest has missing or unknown fields")
        if (
            raw.get("$schema") != "./model-pack-manifest.schema.json"
            or raw.get("schema") != "npc.model-pack/v2"
            or raw.get("pack_id") != PACK_ID
            or raw.get("revision") != PACK_REVISION
            or raw.get("capability") != {"kind": "vision", "scope": "generic"}
        ):
            raise SpecError("pack identity differs from the reviewed private-evaluation revision")
        source = raw.get("source")
        if not isinstance(source, dict) or source.get("immutable_revision") != SOURCE_REVISION:
            raise SpecError("pack source differs from the reviewed immutable revision")
        runtime = raw.get("runtime")
        if (
            not isinstance(runtime, dict)
            or runtime.get("runtime") != "opencv-python-headless"
            or runtime.get("immutable_revision") != "cp312-opencv-5.0.0.93-numpy-2.5.2"
            or runtime.get("abi") != "npc-face-observation-v1"
            or runtime.get("backends") != ["opencv-dnn-cpu"]
            or runtime.get("network_access_after_install") is not False
        ):
            raise SpecError("pack runtime ABI differs from the reviewed worker")
        license_record = raw.get("license")
        if not isinstance(license_record, dict) or license_record.get("redistributable") is not True:
            raise SpecError("reviewed YuNet and SFace model-file redistribution provenance changed")
        if license_record.get("commercial_use") != "allowed" or license_record.get("derivative_use") != "allowed":
            raise SpecError("reviewed YuNet and SFace Apache/MIT permissions changed")
        resources = raw.get("resources")
        measurement = resources.get("measurement") if isinstance(resources, dict) else None
        if (
            not isinstance(measurement, dict)
            or measurement.get("minimum_samples") != 20
            or measurement.get("current_device_fingerprint_required") is not True
            or measurement.get("signed_evidence_required") is not True
            or measurement.get("p99_reload_required") is not True
            or measurement.get("manifest_values_not_for_admission") is not True
            or resources.get("planning_resident_ram_bytes") is not None
            or resources.get("planning_resident_vram_bytes") is not None
            or resources.get("planning_load_millis") is not None
        ):
            raise SpecError("pack measurement gate is incomplete or contains unmeasured claims")
        trust = raw.get("trust")
        admission = raw.get("admission")
        if (
            not isinstance(trust, dict)
            or trust.get("unsigned_activation_allowed") is not False
            or trust.get("measured_resource_envelope_required") is not True
            or not isinstance(admission, dict)
            or admission.get("state") != "blocked_pending_measurement"
            or admission.get("unknowns_fail_closed") is not True
        ):
            raise SpecError("pack trust or admission gate was weakened")
        raw_artifacts = raw.get("artifacts")
        if not isinstance(raw_artifacts, list) or not raw_artifacts:
            raise SpecError("pack artifacts are missing")
        artifacts: list[ArtifactSpec] = []
        destinations: set[str] = set()
        for item in raw_artifacts:
            if not isinstance(item, dict) or set(item) != {
                "id", "role", "kind", "archive_format", "source_urls",
                "size_bytes", "sha256", "destination", "strip_prefix",
                "required_paths",
            }:
                raise SpecError("pack artifact shape is invalid")
            artifact_id = opaque(item.get("id"), "artifact id", maximum=128)
            if (
                item.get("kind") != "file"
                or item.get("archive_format") is not None
                or item.get("strip_prefix") is not None
                or item.get("required_paths") != []
            ):
                raise SpecError("identity pack accepts file artifacts only")
            urls = item.get("source_urls")
            if not isinstance(urls, list) or not urls or any(not isinstance(url, str) or not url.startswith("https://") for url in urls):
                raise SpecError("artifact URLs must be non-empty HTTPS URLs")
            if any("/main/" in url or "/master/" in url or url.endswith(("/main", "/master", "/latest")) for url in urls):
                raise SpecError("artifact URLs must not use mutable revisions")
            size = uint(item.get("size_bytes"), "artifact size", minimum=1, maximum=512 * 1024 * 1024)
            digest = sha256(item.get("sha256"), "artifact digest")
            destination_raw = item.get("destination")
            if not isinstance(destination_raw, str) or "\\" in destination_raw:
                raise SpecError("artifact destination must be a portable relative path")
            destination = PurePosixPath(destination_raw)
            if destination.is_absolute() or not destination.parts or any(part in {"", ".", ".."} for part in destination.parts):
                raise SpecError("artifact destination escapes the pack root")
            folded = destination.as_posix().casefold()
            if folded in destinations:
                raise SpecError("artifact destinations must be unique")
            destinations.add(folded)
            artifacts.append(ArtifactSpec(artifact_id, size, digest, destination))
        by_id = {artifact.artifact_id: artifact for artifact in artifacts}
        expected = {
            "yunet-2026may-onnx": (YUNET_SIZE_BYTES, YUNET_SHA256),
            "sface-2021dec-onnx": (SFACE_SIZE_BYTES, SFACE_SHA256),
        }
        for artifact_id, (size, digest) in expected.items():
            artifact = by_id.get(artifact_id)
            if artifact is None or (artifact.size_bytes, artifact.sha256) != (size, digest):
                raise SpecError(f"reviewed artifact {artifact_id} is missing or changed")
        manifest_sha256 = hashlib.sha256(data).hexdigest()
        if manifest_sha256 != PACK_MANIFEST_SHA256:
            raise SpecError("pack manifest bytes differ from the reviewed immutable revision")
        return cls(path.resolve(), manifest_sha256, tuple(artifacts))

    def artifact(self, artifact_id: str) -> ArtifactSpec:
        value = next((item for item in self.artifacts if item.artifact_id == artifact_id), None)
        if value is None:
            raise SpecError(f"required artifact {artifact_id} is missing")
        return value


@dataclass(frozen=True, slots=True)
class CaptureTarget:
    capture_session_id: str
    process_id: int
    window_handle: int
    executable_name: str

    @classmethod
    def parse(cls, raw: Any) -> "CaptureTarget":
        if not isinstance(raw, dict) or set(raw) != {"capture_session_id", "process_id", "window_handle", "executable_name"}:
            raise SpecError("capture target fields are invalid")
        executable = raw.get("executable_name")
        if not isinstance(executable, str) or not executable.lower().endswith(".exe") or any(char in executable for char in "/\\:"):
            raise SpecError("capture target executable must be a basename ending in .exe")
        return cls(
            opaque(raw.get("capture_session_id"), "capture session id"),
            uint(raw.get("process_id"), "process id", minimum=1, maximum=2**32 - 1),
            uint(raw.get("window_handle"), "window handle", minimum=1),
            executable,
        )

    def payload(self) -> dict[str, object]:
        return {
            "capture_session_id": self.capture_session_id,
            "process_id": self.process_id,
            "window_handle": self.window_handle,
            "executable_name": self.executable_name,
        }


@dataclass(frozen=True, slots=True)
class PixelLease:
    lease_id: str
    shared_memory_name: str
    lease_nonce: str
    byte_length: int
    width: int
    height: int
    stride_bytes: int
    pixel_format: str
    content_sha256: str

    @classmethod
    def parse(cls, raw: Any) -> "PixelLease":
        required = {"lease_id", "shared_memory_name", "lease_nonce", "byte_length", "width", "height", "stride_bytes", "pixel_format", "content_sha256"}
        if not isinstance(raw, dict) or set(raw) != required:
            raise SpecError("pixel lease fields are invalid")
        lease_id = opaque(raw.get("lease_id"), "pixel lease id")
        lease_nonce = opaque(raw.get("lease_nonce"), "pixel lease nonce")
        name = raw.get("shared_memory_name")
        if name != pixel_mapping_name(lease_id, lease_nonce):
            raise SpecError("shared memory name is not bound to the pixel lease")
        width = uint(raw.get("width"), "image width", minimum=1, maximum=MAX_IMAGE_EDGE)
        height = uint(raw.get("height"), "image height", minimum=1, maximum=MAX_IMAGE_EDGE)
        stride = uint(raw.get("stride_bytes"), "image stride", minimum=width * 4, maximum=MAX_IMAGE_EDGE * 4)
        byte_length = uint(raw.get("byte_length"), "image byte length", minimum=1, maximum=MAX_SHARED_IMAGE_BYTES)
        if byte_length != stride * height:
            raise SpecError("image lease byte length must equal stride times height")
        if raw.get("pixel_format") != "b8g8r8a8_unorm":
            raise SpecError("identity worker accepts only b8g8r8a8_unorm pixels")
        return cls(
            lease_id,
            name,
            lease_nonce,
            byte_length,
            width,
            height,
            stride,
            "b8g8r8a8_unorm",
            sha256(raw.get("content_sha256"), "pixel content digest"),
        )


@dataclass(frozen=True, slots=True)
class WgcFrameRequest:
    target: CaptureTarget
    frame_sequence: int
    device_generation: int
    geometry_epoch: int
    source_frame_qpc: int
    qpc_frequency: int
    captured_at_ms: int
    content_sha256: str
    advancing_frame_verified: bool
    overlay_capture_excluded: bool
    protected_online_detected: bool
    anti_cheat_detected: bool
    pixel_lease: PixelLease

    @classmethod
    def parse(cls, raw: Any) -> "WgcFrameRequest":
        required = {
            "contract_version", "mode", "target", "frame_sequence", "device_generation",
            "geometry_epoch", "source_frame_qpc", "qpc_frequency", "captured_at_ms",
            "content_sha256", "advancing_frame_verified", "overlay_capture_excluded",
            "protected_online_detected", "anti_cheat_detected", "pixel_lease",
        }
        if not isinstance(raw, dict) or set(raw) != required or raw.get("contract_version") != REQUEST_CONTRACT_VERSION or raw.get("mode") != "wgc_frame":
            raise SpecError("WGC observation request fields are invalid")
        for flag in ("advancing_frame_verified", "overlay_capture_excluded", "protected_online_detected", "anti_cheat_detected"):
            if not isinstance(raw.get(flag), bool):
                raise SpecError(f"{flag} must be boolean")
        digest = sha256(raw.get("content_sha256"), "WGC content digest")
        lease = PixelLease.parse(raw.get("pixel_lease"))
        if digest != lease.content_sha256:
            raise SpecError("WGC and pixel-lease content digests differ")
        if not raw["advancing_frame_verified"] or not raw["overlay_capture_excluded"] or raw["protected_online_detected"] or raw["anti_cheat_detected"]:
            raise SpecError("WGC safety evidence does not authorize identity inference")
        return cls(
            CaptureTarget.parse(raw.get("target")),
            uint(raw.get("frame_sequence"), "frame sequence", minimum=1),
            uint(raw.get("device_generation"), "device generation", minimum=1),
            uint(raw.get("geometry_epoch"), "geometry epoch", minimum=1),
            uint(raw.get("source_frame_qpc"), "source frame QPC", minimum=1),
            uint(raw.get("qpc_frequency"), "QPC frequency", minimum=1),
            uint(raw.get("captured_at_ms"), "capture time"),
            digest,
            raw["advancing_frame_verified"], raw["overlay_capture_excluded"],
            raw["protected_online_detected"], raw["anti_cheat_detected"], lease,
        )


@dataclass(frozen=True, slots=True)
class ReferenceImportRequest:
    game_profile_id: str
    subject_id: str
    reference_id: str
    subject_display_name: str
    source_class: str
    source_content_sha256: str
    owner_user_id: str | None
    original_work_license: str | None
    explicit_user_consent: bool
    local_only: bool
    imported_at_ms: int
    pixel_lease: PixelLease

    @classmethod
    def parse(cls, raw: Any) -> "ReferenceImportRequest":
        required = {
            "contract_version", "mode", "game_profile_id", "subject_id", "reference_id",
            "subject_display_name", "source_class", "source_content_sha256", "owner_user_id",
            "original_work_license", "explicit_user_consent", "local_only", "imported_at_ms",
            "pixel_lease",
        }
        if not isinstance(raw, dict) or set(raw) != required or raw.get("contract_version") != REFERENCE_IMPORT_CONTRACT_VERSION or raw.get("mode") != "reference_import":
            raise SpecError("reference import fields are invalid")
        source_class = raw.get("source_class")
        consent = raw.get("explicit_user_consent")
        local_only = raw.get("local_only")
        owner = raw.get("owner_user_id")
        license_name = raw.get("original_work_license")
        if not isinstance(consent, bool) or not isinstance(local_only, bool) or not local_only:
            raise SpecError("identity references must be explicitly local-only")
        if source_class == "user_private":
            if not consent or not isinstance(owner, str) or license_name is not None:
                raise SpecError("user-private references require consent and a local owner")
            owner = opaque(owner, "owner user id")
        elif source_class == "original_synthetic":
            if owner is not None or not isinstance(license_name, str) or not license_name.strip() or len(license_name) > 512:
                raise SpecError("original references require a bounded license and no private owner")
        else:
            raise SpecError("reference source class is unsupported")
        display_name = raw.get("subject_display_name")
        if not isinstance(display_name, str) or not display_name.strip() or len(display_name) > 256:
            raise SpecError("subject display name is invalid")
        digest = sha256(raw.get("source_content_sha256"), "reference source digest")
        lease = PixelLease.parse(raw.get("pixel_lease"))
        if digest != lease.content_sha256:
            raise SpecError("reference and pixel-lease digests differ")
        return cls(
            opaque(raw.get("game_profile_id"), "game profile id"),
            opaque(raw.get("subject_id"), "subject id"),
            opaque(raw.get("reference_id"), "reference id"),
            display_name,
            source_class,
            digest,
            owner,
            license_name,
            consent,
            local_only,
            uint(raw.get("imported_at_ms"), "reference import time"),
            lease,
        )


def normalize_vector(values: Iterable[float]) -> tuple[float, ...]:
    canonical: list[float] = []
    for value in values:
        numeric = finite(value, "embedding value")
        rounded = struct.unpack("<f", struct.pack("<f", numeric))[0]
        canonical.append(0.0 if rounded == 0.0 else rounded)
    if len(canonical) != EMBEDDING_DIMENSIONS:
        raise SpecError(f"SFace embedding must have {EMBEDDING_DIMENSIONS} dimensions")
    norm = math.sqrt(math.fsum(value * value for value in canonical))
    if not math.isfinite(norm) or norm <= 1.0e-12:
        raise SpecError("embedding has zero or invalid norm")
    return tuple(struct.unpack("<f", struct.pack("<f", value / norm))[0] for value in canonical)


def tensor_bytes(values: Iterable[float]) -> bytes:
    return b"".join(struct.pack("<f", value) for value in values)


def model_payload() -> dict[str, object]:
    return {
        "provider": EMBEDDING_PROVIDER,
        "model_id": EMBEDDING_MODEL_ID,
        "revision": EMBEDDING_REVISION,
        "dimensions": EMBEDDING_DIMENSIONS,
    }
