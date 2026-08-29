"""Validated Worker Control v1 request and descriptor types."""

from __future__ import annotations

import json
import math
import re
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = "1.0"
MOUTH_RESIDUAL_CONTRACT_VERSION = "npc.mouth-residual/v1"
MAX_TEXT_BYTES = 262_144
MAX_INLINE_BINARY_BYTES = 524_288
MAX_BATCH_ENTRIES = 256
MAX_EMBEDDING_DIMENSIONS = 4_096
MAX_EVENTS_PER_REQUEST = 4_096
OPERATIONS = frozenset(
    {"handshake", "capabilities", "health", "warm", "load", "unload", "infer", "cancel", "shutdown"}
)
KINDS = frozenset({"llm", "stt", "tts", "embedding", "vision", "lip_sync"})
_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{1,95}[a-z0-9]$")
_OPAQUE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,126}[A-Za-z0-9]$")

MOUTH_RESIDUAL_FAIL_OPEN_REASONS = frozenset(
    {
        "metadata_only_stub",
        "low_confidence",
        "track_mismatch",
        "track_epoch_mismatch",
        "source_frame_advanced",
        "cancellation_generation_changed",
        "presentation_deadline_expired",
        "media_lease_expired",
        "landmarks_invalid",
        "mask_out_of_bounds",
        "occluded",
        "residual_unavailable",
        "worker_error",
    }
)


class ContractError(Exception):
    def __init__(self, code: str, message: str, *, retryable: bool = False, details: dict[str, Any] | None = None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.retryable = retryable
        self.details = details or {}


def _bounded_string(value: Any, name: str, *, maximum: int = 256, allow_empty: bool = False) -> str:
    if not isinstance(value, str):
        raise ContractError("invalid_request", f"{name} must be a string")
    size = len(value.encode("utf-8"))
    if (not allow_empty and size == 0) or size > maximum:
        raise ContractError("invalid_request", f"{name} has an invalid length")
    return value


def _nonnegative_int(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value > (2**63 - 1):
        raise ContractError("invalid_request", f"{name} must be a non-negative integer")
    return value


def validate_positive_int(value: Any, name: str) -> int:
    try:
        parsed = _nonnegative_int(value, name)
    except ContractError as exc:
        raise ContractError("invalid_payload", exc.message) from exc
    if parsed == 0:
        raise ContractError("invalid_payload", f"{name} must be positive")
    return parsed


def validate_generation(value: Any, name: str = "cancellation_generation") -> int:
    try:
        return _nonnegative_int(value, name)
    except ContractError as exc:
        raise ContractError("invalid_payload", exc.message) from exc


def validate_confidence(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ContractError("invalid_payload", f"{name} must be a finite number")
    parsed = float(value)
    if not 0.0 <= parsed <= 1.0:
        raise ContractError("invalid_payload", f"{name} must be between 0 and 1")
    return round(parsed, 8)


def validate_opaque_id(value: Any, name: str) -> str:
    """Validate a correlation identity that cannot be interpreted as a path or URL."""

    try:
        parsed = _bounded_string(value, name, maximum=128)
    except ContractError as exc:
        raise ContractError("invalid_payload", exc.message) from exc
    if not _OPAQUE_ID.fullmatch(parsed) or parsed in {".", ".."}:
        raise ContractError(
            "invalid_payload",
            f"{name} must be an opaque identifier, never a path, URL, handle, or inline payload",
        )
    return parsed


def validate_normalized_region(value: Any, name: str) -> dict[str, float]:
    if not isinstance(value, dict) or set(value) != {"x", "y", "width", "height"}:
        raise ContractError(
            "invalid_payload",
            f"{name} must contain exactly x, y, width, and height",
        )
    parsed: dict[str, float] = {}
    for field in ("x", "y", "width", "height"):
        item = value[field]
        if isinstance(item, bool) or not isinstance(item, (int, float)) or not math.isfinite(item):
            raise ContractError("invalid_payload", f"{name}.{field} must be a finite number")
        parsed[field] = float(item)
    if parsed["x"] < 0 or parsed["y"] < 0 or parsed["width"] <= 0 or parsed["height"] <= 0:
        raise ContractError("invalid_payload", f"{name} must have a positive area inside the normalized frame")
    epsilon = 1e-9
    if parsed["x"] + parsed["width"] > 1.0 + epsilon or parsed["y"] + parsed["height"] > 1.0 + epsilon:
        raise ContractError("invalid_payload", f"{name} extends outside the normalized frame")
    return {field: round(parsed[field], 8) for field in ("x", "y", "width", "height")}


def validate_contained_region(
    inner: dict[str, float], outer: dict[str, float], inner_name: str, outer_name: str
) -> None:
    epsilon = 1e-8
    if (
        inner["x"] + epsilon < outer["x"]
        or inner["y"] + epsilon < outer["y"]
        or inner["x"] + inner["width"] > outer["x"] + outer["width"] + epsilon
        or inner["y"] + inner["height"] > outer["y"] + outer["height"] + epsilon
    ):
        raise ContractError("invalid_payload", f"{inner_name} must be contained by {outer_name}")


@dataclass(frozen=True, slots=True)
class MouthResidualRequestV1:
    """Strict control-plane metadata for a residual over one live source frame.

    Media stays in runtime-owned leases. This type deliberately has no field for
    an avatar image, full replacement frame, pixels, path, URL, or raw handle.
    """

    frame_lease_id: str
    audio_lease_id: str
    frame_lease_expires_qpc: int
    audio_lease_expires_qpc: int
    selected_encounter_id: str
    selected_track_id: str
    track_epoch: int
    source_frame_sequence: int
    source_capture_qpc: int
    qpc_frequency_hz: int
    face_region_normalized: dict[str, float]
    landmark_bounds_normalized: dict[str, float]
    mouth_mask_bounds_normalized: dict[str, float]
    tracking_confidence: float
    presentation_deadline_qpc: int
    cancellation_generation: int

    @classmethod
    def parse(cls, payload: Any, *, allow_fixture_delay: bool = False) -> "MouthResidualRequestV1":
        if not isinstance(payload, dict):
            raise ContractError("invalid_payload", "mouth-residual payload must be an object")
        allowed = {
            "contract_version",
            "frame_lease_id",
            "audio_lease_id",
            "frame_lease_expires_qpc",
            "audio_lease_expires_qpc",
            "selected_encounter_id",
            "selected_track_id",
            "track_epoch",
            "source_frame_sequence",
            "source_capture_qpc",
            "qpc_frequency_hz",
            "face_region_normalized",
            "landmark_bounds_normalized",
            "mouth_mask_bounds_normalized",
            "tracking_confidence",
            "presentation_deadline_qpc",
            "cancellation_generation",
        }
        if allow_fixture_delay:
            allowed.add("fixture_event_delay_ms")
        unknown = sorted(set(payload) - allowed)
        if unknown:
            raise ContractError(
                "invalid_payload",
                "mouth-residual payload contains unsupported or inline media fields",
                details={"fields": unknown[:16]},
            )
        if payload.get("contract_version") != MOUTH_RESIDUAL_CONTRACT_VERSION:
            raise ContractError("invalid_payload", "unsupported mouth-residual contract version")

        frame_lease_id = validate_opaque_id(payload.get("frame_lease_id"), "frame_lease_id")
        audio_lease_id = validate_opaque_id(payload.get("audio_lease_id"), "audio_lease_id")
        encounter_id = validate_opaque_id(payload.get("selected_encounter_id"), "selected_encounter_id")
        track_id = validate_opaque_id(payload.get("selected_track_id"), "selected_track_id")
        track_epoch = validate_generation(payload.get("track_epoch"), "track_epoch")
        frame_sequence = validate_positive_int(payload.get("source_frame_sequence"), "source_frame_sequence")
        capture_qpc = validate_positive_int(payload.get("source_capture_qpc"), "source_capture_qpc")
        qpc_frequency = validate_positive_int(payload.get("qpc_frequency_hz"), "qpc_frequency_hz")
        deadline = validate_positive_int(payload.get("presentation_deadline_qpc"), "presentation_deadline_qpc")
        frame_expiry = validate_positive_int(payload.get("frame_lease_expires_qpc"), "frame_lease_expires_qpc")
        audio_expiry = validate_positive_int(payload.get("audio_lease_expires_qpc"), "audio_lease_expires_qpc")
        generation = validate_generation(payload.get("cancellation_generation"))
        confidence = validate_confidence(payload.get("tracking_confidence"), "tracking_confidence")
        face = validate_normalized_region(payload.get("face_region_normalized"), "face_region_normalized")
        landmarks = validate_normalized_region(
            payload.get("landmark_bounds_normalized"), "landmark_bounds_normalized"
        )
        mask = validate_normalized_region(
            payload.get("mouth_mask_bounds_normalized"), "mouth_mask_bounds_normalized"
        )
        validate_contained_region(landmarks, face, "landmark_bounds_normalized", "face_region_normalized")
        validate_contained_region(mask, landmarks, "mouth_mask_bounds_normalized", "landmark_bounds_normalized")
        if deadline <= capture_qpc:
            raise ContractError("stale_source_frame", "presentation deadline must follow source capture time")
        if frame_expiry < deadline or audio_expiry < deadline:
            raise ContractError("stale_media_lease", "frame and audio leases must remain valid through presentation")
        return cls(
            frame_lease_id,
            audio_lease_id,
            frame_expiry,
            audio_expiry,
            encounter_id,
            track_id,
            track_epoch,
            frame_sequence,
            capture_qpc,
            qpc_frequency,
            face,
            landmarks,
            mask,
            confidence,
            deadline,
            generation,
        )


@dataclass(frozen=True, slots=True)
class MouthResidualProposalV1:
    """A mouth-only proposal whose rejection always reveals the live frame."""

    request: MouthResidualRequestV1
    residual_confidence: float
    fail_open_reasons: tuple[str, ...]
    patch_lease_id: str | None = None
    presentable: bool = False
    metadata_only: bool = True
    deterministic: bool = False

    def __post_init__(self) -> None:
        validate_confidence(self.residual_confidence, "residual_confidence")
        if not self.fail_open_reasons or any(
            reason not in MOUTH_RESIDUAL_FAIL_OPEN_REASONS for reason in self.fail_open_reasons
        ):
            raise ContractError("invalid_payload", "mouth-residual fail-open reason is unsupported")
        if len(set(self.fail_open_reasons)) != len(self.fail_open_reasons):
            raise ContractError("invalid_payload", "mouth-residual fail-open reasons must be unique")
        if self.patch_lease_id is not None:
            validate_opaque_id(self.patch_lease_id, "patch_lease_id")
        if self.metadata_only and (self.patch_lease_id is not None or self.presentable):
            raise ContractError("invalid_payload", "metadata-only mouth residuals cannot be presentable or leased")

    def to_payload(self) -> dict[str, Any]:
        request = self.request
        return {
            "contract_version": MOUTH_RESIDUAL_CONTRACT_VERSION,
            "proposal_kind": "external_current_frame_mouth_residual",
            "output_semantics": "additive_rgba_mouth_residual",
            "frame_lease_id": request.frame_lease_id,
            "audio_lease_id": request.audio_lease_id,
            "selected_encounter_id": request.selected_encounter_id,
            "selected_track_id": request.selected_track_id,
            "track_epoch": request.track_epoch,
            "source_frame_sequence": request.source_frame_sequence,
            "source_capture_qpc": request.source_capture_qpc,
            "qpc_frequency_hz": request.qpc_frequency_hz,
            "cancellation_generation": request.cancellation_generation,
            "tracking_confidence": request.tracking_confidence,
            "residual_confidence": round(self.residual_confidence, 8),
            "landmark_bounds_normalized": request.landmark_bounds_normalized,
            "mask_bounds_normalized": request.mouth_mask_bounds_normalized,
            "freshness": {
                "presentation_deadline_qpc": request.presentation_deadline_qpc,
                "frame_lease_expires_qpc": request.frame_lease_expires_qpc,
                "audio_lease_expires_qpc": request.audio_lease_expires_qpc,
                "valid_source_frame_sequence": request.source_frame_sequence,
                "discard_at_or_after_frame_sequence": request.source_frame_sequence + 1,
                "maximum_source_frame_advance": 0,
                "maximum_displayed_frames": 1,
                "requires_exact_source_frame_sequence": True,
                "discard_if_source_advanced": True,
                "discard_if_track_epoch_changed": True,
                "discard_if_generation_changed": True,
                "restore_unmodified_on_rejection": True,
            },
            "fail_open": {
                "use_unmodified_source_frame": True,
                "reasons": list(self.fail_open_reasons),
            },
            "residual_constraints": {
                "full_frame_replacement": False,
                "static_avatar_source": False,
                "base_frame_mutation": False,
                "alpha_outside_mask_zero": True,
                "mask_must_remain_inside_landmarks": True,
            },
            "patch_lease_id": self.patch_lease_id,
            "presentable": self.presentable,
            "metadata_only": self.metadata_only,
            "no_pixels_inline": True,
            "image_modified": False,
            "deterministic": self.deterministic,
        }


@dataclass(frozen=True, slots=True)
class Request:
    protocol_version: str
    worker_instance_id: str
    request_id: str
    sequence: int
    generation: int
    deadline_unix_ms: int
    operation: str
    payload: dict[str, Any]

    @classmethod
    def parse(cls, raw: dict[str, Any]) -> "Request":
        allowed = {
            "protocol_version",
            "worker_instance_id",
            "request_id",
            "sequence",
            "generation",
            "deadline_unix_ms",
            "operation",
            "payload",
        }
        unknown = sorted(set(raw) - allowed)
        if unknown:
            raise ContractError("invalid_request", "request contains unknown fields", details={"fields": unknown[:16]})
        version = _bounded_string(raw.get("protocol_version"), "protocol_version", maximum=16)
        if version != PROTOCOL_VERSION:
            raise ContractError("unsupported_protocol", f"protocol {version!r} is not supported")
        instance_id = _bounded_string(
            raw.get("worker_instance_id", ""), "worker_instance_id", maximum=128, allow_empty=True
        )
        request_id = _bounded_string(raw.get("request_id"), "request_id", maximum=128)
        operation = _bounded_string(raw.get("operation"), "operation", maximum=32)
        if operation not in OPERATIONS:
            raise ContractError("unsupported_operation", f"operation {operation!r} is not supported")
        payload = raw.get("payload", {})
        if not isinstance(payload, dict):
            raise ContractError("invalid_request", "payload must be an object")
        sequence = _nonnegative_int(raw.get("sequence"), "sequence")
        generation = _nonnegative_int(raw.get("generation"), "generation")
        deadline = _nonnegative_int(raw.get("deadline_unix_ms", 0), "deadline_unix_ms")
        if deadline and int(time.time() * 1000) >= deadline:
            raise ContractError("deadline_exceeded", "request deadline has expired", retryable=True)
        return cls(version, instance_id, request_id, sequence, generation, deadline, operation, payload)


@dataclass(frozen=True, slots=True)
class Descriptor:
    raw: dict[str, Any]
    path: Path

    @property
    def pack_id(self) -> str:
        return self.raw["pack_id"]

    @property
    def worker_id(self) -> str:
        return self.raw["worker"]["id"]

    @property
    def kind(self) -> str:
        return self.raw["worker"]["kind"]

    @property
    def engine(self) -> str:
        return self.raw["worker"]["engine"]

    @property
    def capabilities(self) -> dict[str, Any]:
        return self.raw["capabilities"]

    @property
    def resources(self) -> dict[str, Any]:
        return self.raw["resource_estimate"]

    @property
    def default_model_id(self) -> str:
        return self.raw["development"]["default_model_id"]

    @classmethod
    def load(cls, path: str | Path) -> "Descriptor":
        descriptor_path = Path(path).resolve()
        try:
            raw = json.loads(
                descriptor_path.read_text(encoding="utf-8"),
                parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"non-finite number {value}")),
            )
        except (OSError, json.JSONDecodeError, RecursionError, ValueError) as exc:
            raise ContractError("invalid_descriptor", "worker descriptor cannot be read") from exc
        cls.validate(raw)
        return cls(raw, descriptor_path)

    @staticmethod
    def validate(raw: Any) -> None:
        if not isinstance(raw, dict):
            raise ContractError("invalid_descriptor", "descriptor must be an object")
        required = {
            "schema_version",
            "pack_id",
            "display_name",
            "development_stub",
            "bundles_third_party",
            "installation_owner",
            "protocol",
            "worker",
            "capabilities",
            "resource_estimate",
            "development",
            "production_pack_requirements",
        }
        missing = sorted(required - set(raw))
        if missing:
            raise ContractError("invalid_descriptor", "descriptor is missing fields", details={"fields": missing})
        unknown = sorted(set(raw) - required)
        if unknown:
            raise ContractError("invalid_descriptor", "descriptor contains unknown fields", details={"fields": unknown})
        if raw["schema_version"] != "npc.worker-pack/v1":
            raise ContractError("invalid_descriptor", "unsupported descriptor schema")
        if not isinstance(raw["display_name"], str) or not 1 <= len(raw["display_name"]) <= 128:
            raise ContractError("invalid_descriptor", "display name is malformed")
        for name in ("pack_id",):
            value = raw[name]
            if not isinstance(value, str) or not _ID.fullmatch(value):
                raise ContractError("invalid_descriptor", f"{name} is malformed")
        if raw["development_stub"] is not True or raw["bundles_third_party"] is not False:
            raise ContractError("invalid_descriptor", "development descriptors cannot bundle third-party payloads")
        if raw["installation_owner"] != "model_manager":
            raise ContractError("invalid_descriptor", "Model Manager must own production installation")
        protocol = raw["protocol"]
        if not isinstance(protocol, dict) or protocol.get("version") != PROTOCOL_VERSION:
            raise ContractError("invalid_descriptor", "descriptor protocol is incompatible")
        if set(protocol) != {"version", "encodings", "transports", "max_frame_bytes", "network_access"}:
            raise ContractError("invalid_descriptor", "descriptor protocol fields are incompatible")
        if (
            protocol.get("max_frame_bytes") != 1_048_576
            or protocol.get("network_access") is not False
            or set(protocol.get("encodings", []))
            != {"length_delimited_json", "length_delimited_protobuf"}
            or set(protocol.get("transports", [])) != {"stdio", "supervisor_created_named_pipe"}
        ):
            raise ContractError("invalid_descriptor", "descriptor safety limits are incompatible")
        worker = raw["worker"]
        if not isinstance(worker, dict) or set(worker) != {"id", "kind", "engine"}:
            raise ContractError("invalid_descriptor", "worker must be an object")
        if not isinstance(worker.get("id"), str) or not _ID.fullmatch(worker["id"]):
            raise ContractError("invalid_descriptor", "worker id is malformed")
        if worker.get("kind") not in KINDS:
            raise ContractError("invalid_descriptor", "worker kind is unsupported")
        if not isinstance(worker.get("engine"), str) or not worker["engine"]:
            raise ContractError("invalid_descriptor", "worker engine is missing")
        capabilities = raw["capabilities"]
        required_capabilities = {
            "input_modalities",
            "output_modalities",
            "compute_backends",
            "streaming_output",
            "cancellation",
            "network_access",
            "experimental",
        }
        if (
            not isinstance(capabilities, dict)
            or not required_capabilities.issubset(capabilities)
            or capabilities.get("network_access") is not False
            or capabilities.get("cancellation") is not True
            or not isinstance(capabilities.get("streaming_output"), bool)
            or not isinstance(capabilities.get("experimental"), bool)
        ):
            raise ContractError("invalid_descriptor", "worker must declare no network access")
        for field in ("input_modalities", "output_modalities", "compute_backends"):
            values = capabilities.get(field)
            if (
                not isinstance(values, list)
                or not values
                or len(values) > 64
                or any(not isinstance(value, str) or not value or len(value) > 128 for value in values)
                or len(values) != len(set(values))
            ):
                raise ContractError("invalid_descriptor", f"capability {field} is malformed")
        resources = raw["resource_estimate"]
        resource_fields = (
            "idle_ram_mib",
            "loaded_ram_mib",
            "peak_ram_mib",
            "loaded_vram_mib",
            "peak_vram_mib",
            "cpu_threads_recommended",
            "concurrent_requests",
        )
        if (
            not isinstance(resources, dict)
            or set(resources) != {*resource_fields, "estimate_class", "evidence"}
            or any(
            isinstance(resources.get(field), bool) or not isinstance(resources.get(field), int) or resources[field] < 0
            for field in resource_fields
            )
        ):
            raise ContractError("invalid_descriptor", "resource estimate is malformed")
        if resources["cpu_threads_recommended"] < 1 or resources["concurrent_requests"] < 1:
            raise ContractError("invalid_descriptor", "resource scheduling values must be positive")
        if (
            resources["loaded_ram_mib"] < resources["idle_ram_mib"]
            or resources["peak_ram_mib"] < resources["loaded_ram_mib"]
            or resources["peak_vram_mib"] < resources["loaded_vram_mib"]
        ):
            raise ContractError("invalid_descriptor", "resource estimate ordering is invalid")
        if resources.get("estimate_class") != "deterministic_stub_ceiling":
            raise ContractError("invalid_descriptor", "stub resource estimate must be explicitly classified")
        if "not a model benchmark" not in str(resources.get("evidence", "")).lower():
            raise ContractError("invalid_descriptor", "stub estimates must disclaim model benchmarking")
        development = raw["development"]
        if (
            not isinstance(development, dict)
            or set(development) != {"entrypoint", "default_model_id", "purpose"}
            or development.get("entrypoint") != "workers/stubs/worker.py"
            or development.get("purpose") != "orchestration_and_protocol_testing_only"
            or not isinstance(development.get("default_model_id"), str)
            or not development["default_model_id"]
        ):
            raise ContractError("invalid_descriptor", "development model fixture is missing")
        requirements = raw["production_pack_requirements"]
        if (
            not isinstance(requirements, dict)
            or set(requirements)
            != {"third_party_payloads_bundled_here", "installed_by", "required_artifact_roles", "activation_gates"}
            or requirements.get("third_party_payloads_bundled_here") is not False
            or requirements.get("installed_by") != "model_manager"
        ):
            raise ContractError("invalid_descriptor", "production pack ownership is ambiguous")
        for field in ("required_artifact_roles", "activation_gates"):
            values = requirements.get(field)
            if not isinstance(values, list) or not values or any(not isinstance(value, str) or not value for value in values):
                raise ContractError("invalid_descriptor", f"production pack {field} is malformed")


def validate_text(value: Any, field: str = "text", *, allow_empty: bool = False) -> str:
    return _bounded_string(value, field, maximum=MAX_TEXT_BYTES, allow_empty=allow_empty)


def validate_string_list(value: Any, field: str, *, maximum: int = MAX_BATCH_ENTRIES) -> list[str]:
    if not isinstance(value, list) or not value or len(value) > maximum:
        raise ContractError("invalid_payload", f"{field} must contain 1 to {maximum} strings")
    return [validate_text(item, f"{field}[]") for item in value]
