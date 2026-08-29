"""Fail-closed validation for non-downloadable local lip-sync qualification plans."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping

try:  # Supports direct execution and package imports in test harnesses.
    from .contract import ContractError
    from .lipsync_catalog import LipSyncPackCatalog
except ImportError:
    from contract import ContractError
    from lipsync_catalog import LipSyncPackCatalog


SCHEMA_VERSION = "npc.lipsync-qualification-plan/v1"
PLAN_ID = "qualify.audio2face-3d-regression-v2.3-mark.windows-x64"
CANDIDATE_ID = "audio2face-3d-regression-v2.3"
MODEL_REVISION = "5451728e07378df93b04523279e134a9993ae71b"

_IMMUTABLE_REVISION = re.compile(r"^[0-9a-f]{40}$")
_FORBIDDEN_FIELD_FRAGMENT = re.compile(
    r"(?:url|uri|endpoint|credential|secret|token|api[_-]?key|password|authorization)",
    re.IGNORECASE,
)
_FORBIDDEN_VALUE_FRAGMENT = re.compile(r"(?:https?://|hf://|s3://|file://)", re.IGNORECASE)

_MOUTH_COEFFICIENTS = frozenset(
    {
        "jawForward",
        "jawLeft",
        "jawOpen",
        "jawRight",
        "mouthClose",
        "mouthDimpleLeft",
        "mouthDimpleRight",
        "mouthFrownLeft",
        "mouthFrownRight",
        "mouthFunnel",
        "mouthLeft",
        "mouthLowerDownLeft",
        "mouthLowerDownRight",
        "mouthPressLeft",
        "mouthPressRight",
        "mouthPucker",
        "mouthRight",
        "mouthRollLower",
        "mouthRollUpper",
        "mouthShrugLower",
        "mouthShrugUpper",
        "mouthSmileLeft",
        "mouthSmileRight",
        "mouthStretchLeft",
        "mouthStretchRight",
        "mouthUpperUpLeft",
        "mouthUpperUpRight",
        "tongueOut",
    }
)

_EXPECTED_SELECTION = {
    "contains_download_locations": False,
    "contains_model_payloads": False,
    "automatic_download": False,
    "default_candidate": False,
    "live_selectable": False,
    "offline_selectable": False,
    "activation_state": "blocked_pending_qualification",
}

_EXPECTED_SOURCE = {
    "publisher": "NVIDIA",
    "model_family": "Audio2Face-3D",
    "model_variant": "Mark",
    "model_version": "2.3",
    "model_revision": MODEL_REVISION,
    "revision_kind": "immutable_git_commit",
    "access": "public_ungated",
    "sdk_source_revision_policy": "exact_commit_required_before_build",
    "model_license": "NVIDIA Open Model License",
    "sdk_license": "MIT",
    "license_qualification": "pending_exact_text_and_redistribution_review",
}

_EXPECTED_INPUT = {
    "modality": "speech_pcm",
    "sample_rate_hz": 16000,
    "channels": 1,
    "sample_format": "signed_16_bit_little_endian",
    "streaming": True,
}

_EXPECTED_OUTPUT = {
    "modality": "timestamped_arkit_blendshape_coefficients",
    "coefficient_namespace": "arkit_52",
    "coefficient_cadence_hz": 60,
    "source_timestamp_basis": "source_audio_sample_index",
    "delivery_timestamp_basis": "query_performance_counter",
    "sequence_policy": "strictly_monotonic",
    "non_finite_policy": "reject_frame_and_fail_open",
}

_EXPECTED_BINDINGS = frozenset(
    {
        "frame_lease_id",
        "source_frame_sequence",
        "selected_encounter_id",
        "selected_track_id",
        "track_epoch",
        "landmark_bounds_normalized",
        "mouth_mask_bounds_normalized",
        "cancellation_generation",
    }
)

_EXPECTED_ENVIRONMENT = {
    "platforms": ["windows_10_22h2_x64", "windows_11_x64"],
    "architecture": "x86_64",
    "build_configuration": "release",
    "compiler": "msvc_visual_studio_2022",
    "cuda": {"minimum": "12.8.0", "maximum_exclusive": "13.0.0", "reference": "12.9.0"},
    "tensorrt": {"minimum": "10.13.0", "maximum_exclusive": "11.0.0", "reference": "10.13.0"},
    "gpu": {
        "vendor": "nvidia",
        "minimum_compute_capability": "7.5",
        "reference_target": "rtx_4080_laptop_12_gib",
        "lower_memory_gate": "separate_6_gib_and_8_gib_measurement_required",
    },
    "minimum_system_ram_mib": 8192,
    "minimum_free_storage_mib": 10240,
    "runtime_network_access": False,
    "qualification_power_profiles": ["silent_cpu_boost_disabled", "balanced"],
}

_EXPECTED_ADMISSION = {
    "budget_source": "dxgi_query_video_memory_info",
    "local_visual_gpu_lease": "exclusive",
    "co_resident_local_models": "not_permitted_during_initial_qualification",
    "safety_margin_mib": 512,
    "required_peak_statistic": "measured_vram_p99_mib",
    "admission_formula": (
        "budget_mib-current_usage_mib-configured_game_reserve_mib-"
        "safety_margin_mib>=measured_vram_p99_mib"
    ),
    "on_rejection": "remain_unselectable_without_cloud_fallback",
}

_EXPECTED_THRESHOLDS = {
    "coefficient_compute_ms_p95_max": 16.6,
    "first_coefficient_ms_p95_max": 100,
    "audio_to_residual_ready_ms_p95_max": 50,
    "residual_composite_ms_p95_max": 0.5,
    "audio_visual_onset_skew_abs_ms_p95_max": 80,
    "average_game_fps_loss_percent_max": 3,
    "one_percent_low_fps_loss_percent_max": 5,
    "deadline_miss_percent_max": 0.1,
    "outside_mask_changed_pixel_percent_max": 0.1,
    "stale_or_wrong_track_presentations_max": 0,
    "restore_unmodified_within_displayed_frames_max": 1,
    "crashes_or_orphan_workers_max": 0,
}


def _raise(message: str) -> None:
    raise ContractError("invalid_lipsync_qualification_plan", message)


def _assert_exact(actual: Any, expected: Any, label: str) -> None:
    if actual != expected:
        _raise(f"{label} is incompatible with the pinned qualification contract")


def _reject_locations_and_credentials(value: Any, path: str = "$") -> None:
    if isinstance(value, Mapping):
        for key, child in value.items():
            if not isinstance(key, str) or _FORBIDDEN_FIELD_FRAGMENT.search(key):
                _raise(f"{path} contains a download-location or credential field")
            _reject_locations_and_credentials(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _reject_locations_and_credentials(child, f"{path}[{index}]")
    elif isinstance(value, str) and _FORBIDDEN_VALUE_FRAGMENT.search(value):
        _raise(f"{path} contains a download location")


@dataclass(frozen=True, slots=True)
class LipSyncQualificationPlan:
    raw: dict[str, Any]
    path: Path

    @classmethod
    def load(cls, path: str | Path) -> "LipSyncQualificationPlan":
        plan_path = Path(path).resolve()
        try:
            raw = json.loads(
                plan_path.read_text(encoding="utf-8"),
                parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"non-finite number {value}")),
            )
        except (OSError, json.JSONDecodeError, RecursionError, ValueError) as exc:
            raise ContractError(
                "invalid_lipsync_qualification_plan", "lip-sync qualification plan cannot be read"
            ) from exc
        cls.validate(raw)
        return cls(raw, plan_path)

    @staticmethod
    def validate(raw: Any) -> None:
        required = {
            "schema_version",
            "plan_id",
            "candidate_id",
            "display_name",
            "descriptor_kind",
            "selection",
            "source_identity",
            "io_contract",
            "residual_mapper_contract",
            "expected_environment",
            "resource_admission",
            "measurement_plan",
            "activation_gates",
        }
        if not isinstance(raw, dict) or set(raw) != required:
            _raise("qualification plan fields are incompatible")
        _reject_locations_and_credentials(raw)
        _assert_exact(raw["schema_version"], SCHEMA_VERSION, "schema version")
        _assert_exact(raw["plan_id"], PLAN_ID, "plan id")
        _assert_exact(raw["candidate_id"], CANDIDATE_ID, "candidate id")
        _assert_exact(
            raw["display_name"],
            "Audio2Face-3D regression v2.3 Mark Windows qualification",
            "display name",
        )
        _assert_exact(raw["descriptor_kind"], "non_downloadable_qualification_plan", "descriptor kind")
        _assert_exact(raw["selection"], _EXPECTED_SELECTION, "selection policy")
        _assert_exact(raw["source_identity"], _EXPECTED_SOURCE, "source identity")
        if not _IMMUTABLE_REVISION.fullmatch(raw["source_identity"]["model_revision"]):
            _raise("model revision is not an immutable 40-character commit")

        io_contract = raw["io_contract"]
        if not isinstance(io_contract, dict) or set(io_contract) != {"input", "output"}:
            _raise("I/O contract fields are incompatible")
        _assert_exact(io_contract["input"], _EXPECTED_INPUT, "audio input contract")
        _assert_exact(io_contract["output"], _EXPECTED_OUTPUT, "coefficient output contract")

        mapper = raw["residual_mapper_contract"]
        expected_mapper_fields = {
            "output_contract",
            "mouth_coefficient_allowlist",
            "non_mouth_coefficient_policy",
            "full_frame_replacement",
            "static_avatar_source",
            "base_frame_mutation",
            "alpha_outside_mask_zero",
            "mask_must_remain_inside_landmarks",
            "maximum_source_frame_advance",
            "maximum_displayed_frames",
            "fail_open_to_unmodified_frame",
            "required_bindings",
        }
        if not isinstance(mapper, dict) or set(mapper) != expected_mapper_fields:
            _raise("residual mapper fields are incompatible")
        if set(mapper["mouth_coefficient_allowlist"]) != _MOUTH_COEFFICIENTS or len(
            mapper["mouth_coefficient_allowlist"]
        ) != len(_MOUTH_COEFFICIENTS):
            _raise("mouth coefficient allowlist is not exact")
        if set(mapper["required_bindings"]) != _EXPECTED_BINDINGS or len(mapper["required_bindings"]) != len(
            _EXPECTED_BINDINGS
        ):
            _raise("current-frame residual bindings are not exact")
        mapper_safety = {key: mapper[key] for key in expected_mapper_fields - {"mouth_coefficient_allowlist", "required_bindings"}}
        _assert_exact(
            mapper_safety,
            {
                "output_contract": "npc.mouth-residual/v1",
                "non_mouth_coefficient_policy": "discard",
                "full_frame_replacement": False,
                "static_avatar_source": False,
                "base_frame_mutation": False,
                "alpha_outside_mask_zero": True,
                "mask_must_remain_inside_landmarks": True,
                "maximum_source_frame_advance": 0,
                "maximum_displayed_frames": 1,
                "fail_open_to_unmodified_frame": True,
            },
            "residual mapper safety policy",
        )

        _assert_exact(raw["expected_environment"], _EXPECTED_ENVIRONMENT, "expected environment")
        _assert_exact(raw["resource_admission"], _EXPECTED_ADMISSION, "resource admission policy")
        LipSyncQualificationPlan._validate_measurement(raw["measurement_plan"])
        _assert_exact(
            raw["activation_gates"],
            [
                "exact_sdk_commit_pinned",
                "artifact_hashes_and_sizes_recorded",
                "tensor_rt_engine_built_for_qualified_gpu",
                "cuda_tensor_rt_abi_match",
                "exact_license_text_reviewed",
                "worker_sandbox_and_no_egress_verified",
                "timestamped_arkit_coefficient_conformance",
                "strict_mouth_residual_conformance",
                "dxgi_resource_admission_passed",
                "latency_and_frame_impact_thresholds_passed",
                "occlusion_identity_freshness_and_cancellation_passed",
                "thirty_minute_soak_passed",
            ],
            "activation gates",
        )

    @staticmethod
    def _validate_measurement(measurement: Any) -> None:
        if not isinstance(measurement, dict) or set(measurement) != {
            "status",
            "observations",
            "workload",
            "required_record_fields",
            "acceptance_thresholds",
        }:
            _raise("measurement plan fields are incompatible")
        _assert_exact(measurement["status"], "not_measured", "measurement status")
        _assert_exact(measurement["observations"], {}, "measurement observations")
        _assert_exact(
            measurement["workload"],
            {
                "warmup_frames": 600,
                "minimum_measured_frames": 18000,
                "soak_minutes": 30,
                "coefficient_cadence_hz": 60,
                "capture_fixture": "original_or_licensed_moving_game_face_replay_with_pose_and_occlusion",
                "audio_fixture": "hash_attested_speech_pcm_with_phoneme_and_silence_coverage",
                "stress_events": [
                    "barge_in_cancellation",
                    "track_epoch_change",
                    "face_occlusion",
                    "source_frame_advance",
                    "device_loss",
                    "gpu_budget_pressure",
                ],
            },
            "measurement workload",
        )
        _assert_exact(
            measurement["required_record_fields"],
            [
                "sdk_commit",
                "model_revision",
                "worker_commit",
                "benchmark_revision",
                "measurement_utc",
                "windows_build",
                "gpu_name",
                "gpu_driver",
                "cuda_version",
                "tensorrt_version",
                "power_profile",
                "dxgi_budget_mib",
                "configured_game_reserve_mib",
                "fixture_hashes",
            ],
            "measurement record fields",
        )
        _assert_exact(measurement["acceptance_thresholds"], _EXPECTED_THRESHOLDS, "acceptance thresholds")

    def assert_catalog_binding(self, catalog: LipSyncPackCatalog) -> None:
        candidate = next(
            (entry for entry in catalog.raw["candidates"] if entry["id"] == self.raw["candidate_id"]),
            None,
        )
        if candidate is None:
            _raise("qualification plan candidate is absent from the lip-sync catalog")
        if candidate["status"] != "deferred" or candidate["source_access"] != "public_upstream":
            _raise("qualification plan candidate status or source access is incompatible")
        if candidate["requires_private_access"] or candidate["live_selectable"] or candidate["offline_selectable"]:
            _raise("unqualified Audio2Face candidate must remain unselectable and public")
