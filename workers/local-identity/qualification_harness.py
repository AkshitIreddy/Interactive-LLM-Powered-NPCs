#!/usr/bin/env python3
"""Fail-closed Windows qualification supervisor for the YuNet/SFace pack.

The default action is a model-free preflight.  ``--execute`` is intentionally
hard to reach: it requires a one-time parent grant, the externally-owned GPU
coordination file to remain ``yes`` (CPU inference is still an AI-model run),
exact installed artifacts/runtime/driver pins, a rights-cleared fixture corpus,
and a real WGC qualification driver.  This file never downloads anything,
never changes the GPU coordination file, never signs evidence, and launches
children without a console window.

The native driver is a separate product boundary.  It owns command 16/17 WGC
leases, the hidden worker lifecycle, PresentMon/DXGI telemetry, and fixture
window rendering.  It emits a bounded length-framed evidence bundle described
below; this supervisor independently validates and persists that unsigned data.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import secrets
import io
import struct
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, BinaryIO

from model_spec import PACK_ID, PACK_MANIFEST_SHA256, PACK_REVISION, PackSpec, SpecError, sha256, uint
from qualification import RAW_SCHEMA, build_unsigned_candidate, load_plan, validate_raw_samples

CONFIG_SCHEMA = "npc.identity-qualification-run-config/v1"
GRANT_SCHEMA = "npc.identity-parent-model-run-grant/v1"
FIXTURE_SCHEMA = "npc.identity-rights-cleared-fixture-corpus/v1"
DRIVER_DESCRIPTOR_SCHEMA = "npc.identity-wgc-qualification-driver/v1"
BUNDLE_SCHEMA = "npc.identity-qualification-driver-bundle/v1"
QUALITY_SCHEMA = "npc.identity-open-set-quality-report/v1"
CLEANUP_SCHEMA = "npc.identity-qualification-cleanup-report/v1"
RUN_REPORT_SCHEMA = "npc.identity-unsigned-qualification-run/v1"
RUNTIME_INVENTORY_SCHEMA = "npc.identity-installed-runtime-inventory/v1"
MAX_JSON_BYTES = 64 * 1024 * 1024
MAX_FIXTURE_FILES = 50_000
MAX_RUN_SECONDS = 6 * 60 * 60
TOKEN_ENV = "NPC_IDENTITY_PARENT_GRANT_TOKEN"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
OPAQUE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,254}[A-Za-z0-9]$")
FORBIDDEN_TRAIT_KEYS = frozenset(
    {
        "age", "race", "ethnicity", "sex", "gender", "religion", "disability",
        "health", "sexual_orientation", "nationality", "political_affiliation",
        "emotion", "demographic", "protected_trait", "protected_traits",
    }
)


def _read_json(path: Path, label: str, *, maximum: int = 2 * 1024 * 1024) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size <= maximum:
        raise SpecError(f"{label} must be a bounded regular file")
    try:
        value = json.loads(path.read_bytes())
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SpecError(f"{label} must be UTF-8 JSON") from exc
    if not isinstance(value, dict):
        raise SpecError(f"{label} root must be an object")
    return value


def _exact_object(value: Any, label: str, fields: set[str]) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        raise SpecError(f"{label} fields are invalid")
    return value


def _opaque(value: Any, label: str) -> str:
    if not isinstance(value, str) or not OPAQUE_RE.fullmatch(value):
        raise SpecError(f"{label} must be a bounded opaque identifier")
    return value


def _digest_file(path: Path, label: str, *, maximum: int = 1024 * 1024 * 1024) -> str:
    if path.is_symlink() or not path.is_file() or not 0 < path.stat().st_size <= maximum:
        raise SpecError(f"{label} must be a bounded regular file")
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            size += len(chunk)
            if size > maximum:
                raise SpecError(f"{label} exceeds its size bound")
            digest.update(chunk)
    return digest.hexdigest()


def _resolve_file(raw: Any, base: Path, label: str, expected_digest: str | None = None) -> Path:
    if not isinstance(raw, str) or not raw or "\0" in raw:
        raise SpecError(f"{label} path is invalid")
    path = Path(raw)
    if not path.is_absolute():
        path = base / path
    path = path.resolve(strict=True)
    digest = _digest_file(path, label)
    if expected_digest is not None and digest != sha256(expected_digest, f"{label} digest"):
        raise SpecError(f"{label} digest differs from its reviewed pin")
    return path


def _resolve_directory(raw: Any, base: Path, label: str, *, must_be_empty: bool = False) -> Path:
    if not isinstance(raw, str) or not raw or "\0" in raw:
        raise SpecError(f"{label} path is invalid")
    path = Path(raw)
    if not path.is_absolute():
        path = base / path
    path = path.resolve(strict=True)
    if path.is_symlink() or not path.is_dir():
        raise SpecError(f"{label} must be a real directory")
    if must_be_empty and any(path.iterdir()):
        raise SpecError(f"{label} must be empty before qualification")
    return path


def _forbid_traits(value: Any, path: str = "fixture") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if not isinstance(key, str):
                raise SpecError(f"{path} contains a non-string key")
            normalized = key.casefold().replace("-", "_").replace(" ", "_")
            if normalized in FORBIDDEN_TRAIT_KEYS:
                raise SpecError(f"{path} contains forbidden protected-trait metadata")
            _forbid_traits(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _forbid_traits(child, f"{path}[{index}]")


@dataclass(frozen=True, slots=True)
class VerifiedRun:
    config_path: Path
    config: dict[str, Any]
    plan: dict[str, Any]
    manifest: Path
    artifact_root: Path
    runtime_python: Path
    runtime_requirements: Path
    runtime_inventory: Path
    driver: Path
    fixture_manifest: Path
    grant_file: Path
    gpu_lock_file: Path
    output_dir: Path


def validate_runtime_inventory(path: Path, runtime_python: Path) -> dict[str, Any]:
    value = _read_json(path, "installed runtime inventory", maximum=16 * 1024 * 1024)
    _exact_object(value, "installed runtime inventory", {"schema", "python", "python_sha256", "python_abi", "packages", "network_installed"})
    if value["schema"] != RUNTIME_INVENTORY_SCHEMA or value["python_abi"] != "cp312-win_amd64" or value["network_installed"] is not False:
        raise SpecError("installed runtime inventory is not the reviewed offline Windows ABI")
    recorded_python = Path(value["python"]).resolve(strict=True) if isinstance(value["python"], str) else None
    if recorded_python != runtime_python or _digest_file(runtime_python, "runtime Python") != sha256(value["python_sha256"], "runtime Python digest"):
        raise SpecError("runtime Python differs from its installed inventory")
    packages = value["packages"]
    expected = {"opencv-python-headless": "5.0.0.93", "numpy": "2.5.2"}
    if not isinstance(packages, list) or len(packages) != 2:
        raise SpecError("installed runtime must contain exactly the reviewed direct packages")
    found: dict[str, str] = {}
    for package in packages:
        _exact_object(package, "installed package", {"name", "version", "wheel_filename", "wheel_sha256", "record_sha256", "files"})
        found[package["name"]] = package["version"]
        sha256(package["wheel_sha256"], "wheel digest")
        sha256(package["record_sha256"], "installed RECORD digest")
        files = package["files"]
        if not isinstance(files, list) or not files or len(files) > 20_000:
            raise SpecError("installed package file inventory is invalid")
        site_root = runtime_python.parent.parent / "Lib" / "site-packages"
        seen_files: set[str] = set()
        for installed in files:
            _exact_object(installed, "installed runtime file", {"relative_path", "size_bytes", "sha256"})
            relative = installed["relative_path"]
            if not isinstance(relative, str) or not relative or "\\" in relative or relative.casefold() in seen_files:
                raise SpecError("installed runtime file path is invalid or duplicated")
            seen_files.add(relative.casefold())
            actual = (site_root / relative).resolve(strict=True)
            if site_root.resolve() not in actual.parents or actual.is_symlink() or not actual.is_file():
                raise SpecError("installed runtime file escapes site-packages")
            size = uint(installed["size_bytes"], "installed runtime file size", minimum=0, maximum=512 * 1024 * 1024)
            actual_digest = hashlib.sha256(b"").hexdigest() if size == 0 and actual.stat().st_size == 0 else _digest_file(actual, "installed runtime file", maximum=max(1, size))
            if actual.stat().st_size != size or actual_digest != sha256(installed["sha256"], "installed runtime file digest"):
                raise SpecError("installed runtime file differs from its offline inventory")
    if found != expected:
        raise SpecError("installed runtime package versions differ from the exact pin")
    return value


def validate_fixture_manifest(path: Path, plan: dict[str, Any]) -> dict[str, Any]:
    value = _read_json(path, "fixture corpus manifest", maximum=16 * 1024 * 1024)
    _forbid_traits(value)
    _exact_object(value, "fixture corpus manifest", {"schema", "suite_revision", "local_only", "partitions", "files", "rights_attestation"})
    if value["schema"] != FIXTURE_SCHEMA or value["suite_revision"] != plan["suite_revision"] or value["local_only"] is not True:
        raise SpecError("fixture corpus is not bound to this local qualification suite")
    attestation = _exact_object(value["rights_attestation"], "rights attestation", {"attested_by_local_user", "attested_unix_seconds", "no_web_scrape", "no_third_party_biometrics", "protected_trait_labels_absent"})
    if any(attestation[key] is not True for key in ("attested_by_local_user", "no_web_scrape", "no_third_party_biometrics", "protected_trait_labels_absent")):
        raise SpecError("fixture rights attestation is incomplete")
    uint(attestation["attested_unix_seconds"], "fixture attestation time", minimum=1)
    files = value["files"]
    if not isinstance(files, list) or not files or len(files) > MAX_FIXTURE_FILES:
        raise SpecError("fixture file inventory count is invalid")
    root = path.parent.resolve()
    seen: set[str] = set()
    subjects: dict[str, set[str]] = {"calibration_known": set(), "held_out_known": set(), "held_out_unknown": set()}
    counts: dict[tuple[str, str], int] = {}
    for item in files:
        _exact_object(item, "fixture file", {"id", "relative_path", "size_bytes", "sha256", "partition", "subject_id", "sequence_id", "frame_index", "provenance"})
        fixture_id = _opaque(item["id"], "fixture id")
        if fixture_id in seen:
            raise SpecError("fixture ids must be unique")
        seen.add(fixture_id)
        partition = item["partition"]
        if partition not in subjects:
            raise SpecError("fixture partition must separate calibration, known holdout, and unknown holdout")
        subject = _opaque(item["subject_id"], "fixture subject id")
        subjects[partition].add(subject)
        counts[(partition, subject)] = counts.get((partition, subject), 0) + 1
        provenance = _exact_object(item["provenance"], "fixture provenance", {"source_class", "explicit_user_consent", "local_only", "original_work_license"})
        source = provenance["source_class"]
        if source == "user_private":
            if provenance["explicit_user_consent"] is not True or provenance["original_work_license"] is not None:
                raise SpecError("user-private fixture provenance is incomplete")
        elif source == "original_synthetic":
            if provenance["explicit_user_consent"] is not True or not isinstance(provenance["original_work_license"], str) or not provenance["original_work_license"]:
                raise SpecError("original-synthetic fixture license is missing")
        else:
            raise SpecError("fixture source is not rights-cleared")
        if provenance["local_only"] is not True:
            raise SpecError("fixture must remain local-only")
        relative = item["relative_path"]
        if not isinstance(relative, str) or not relative or "\\" in relative:
            raise SpecError("fixture path must be portable and relative")
        candidate = (root / relative).resolve(strict=True)
        if root not in candidate.parents or candidate.is_symlink() or not candidate.is_file():
            raise SpecError("fixture path escapes its private corpus")
        size = uint(item["size_bytes"], "fixture size", minimum=1, maximum=64 * 1024 * 1024)
        if candidate.stat().st_size != size or _digest_file(candidate, "fixture", maximum=size) != sha256(item["sha256"], "fixture digest"):
            raise SpecError("fixture bytes differ from their private inventory")
        sequence = item["sequence_id"]
        if sequence is not None:
            _opaque(sequence, "fixture sequence id")
            uint(item["frame_index"], "fixture frame index", minimum=1)
        elif item["frame_index"] is not None:
            raise SpecError("fixture frame index requires a sequence id")
    quality = plan["open_set_quality"]
    if len(subjects["held_out_known"]) < quality["minimum_distinct_known_characters"] or len(subjects["held_out_unknown"]) < quality["minimum_distinct_unknown_characters"]:
        raise SpecError("fixture corpus does not meet held-out known/unknown subject minimums")
    if subjects["calibration_known"] & (subjects["held_out_known"] | subjects["held_out_unknown"]) or subjects["held_out_known"] & subjects["held_out_unknown"]:
        raise SpecError("fixture subjects overlap calibration or held-out partitions")
    if any(counts[("held_out_known", subject)] < quality["minimum_known_samples_per_character"] for subject in subjects["held_out_known"]):
        raise SpecError("known held-out subjects have too few samples")
    if any(counts[("held_out_unknown", subject)] < quality["minimum_unknown_samples_per_character"] for subject in subjects["held_out_unknown"]):
        raise SpecError("unknown held-out subjects have too few samples")
    return value


def validate_grant(path: Path, plan: dict[str, Any], now: int) -> dict[str, Any]:
    value = _read_json(path, "parent model-run grant", maximum=64 * 1024)
    _exact_object(value, "parent model-run grant", {"schema", "grant_id", "suite_revision", "pack_id", "pack_revision", "manifest_file_sha256", "backend", "allow_identity_model_execution", "cpu_still_requires_gpu_lane", "single_use", "issued_unix_seconds", "expires_unix_seconds", "maximum_run_seconds", "token_sha256", "authorized_by"})
    if (
        value["schema"] != GRANT_SCHEMA
        or value["suite_revision"] != plan["suite_revision"]
        or value["pack_id"] != PACK_ID
        or value["pack_revision"] != PACK_REVISION
        or value["manifest_file_sha256"] != PACK_MANIFEST_SHA256
        or value["backend"] != "opencv-dnn-cpu"
        or value["allow_identity_model_execution"] is not True
        or value["cpu_still_requires_gpu_lane"] is not True
        or value["single_use"] is not True
    ):
        raise SpecError("parent grant is not an exact one-time authorization for this CPU model run")
    _opaque(value["grant_id"], "grant id")
    issued = uint(value["issued_unix_seconds"], "grant issue time", minimum=1)
    expires = uint(value["expires_unix_seconds"], "grant expiry", minimum=1)
    duration = uint(value["maximum_run_seconds"], "maximum run duration", minimum=60, maximum=MAX_RUN_SECONDS)
    if not issued <= now < expires or expires - issued > MAX_RUN_SECONDS:
        raise SpecError("parent grant is expired, premature, or too long-lived")
    if not isinstance(value["authorized_by"], str) or value["authorized_by"] != "root_orchestrator":
        raise SpecError("parent grant authority is unsupported")
    sha256(value["token_sha256"], "parent grant token digest")
    return value


def validate_run_config(path: Path, *, require_empty_output: bool = True) -> VerifiedRun:
    config = _read_json(path, "qualification run config")
    _exact_object(config, "qualification run config", {"schema", "suite_revision", "paths", "pins", "target", "execution", "telemetry"})
    base = path.parent.resolve()
    paths = _exact_object(config["paths"], "run paths", {"plan", "pack_manifest", "artifact_root", "runtime_python", "runtime_requirements", "runtime_inventory", "driver_executable", "fixture_manifest", "parent_grant", "gpu_lock", "output_directory"})
    pins = _exact_object(config["pins"], "run pins", {"plan_sha256", "pack_manifest_sha256", "runtime_requirements_sha256", "driver_sha256", "fixture_manifest_sha256"})
    plan_path = _resolve_file(paths["plan"], base, "qualification plan", pins["plan_sha256"])
    plan = load_plan(plan_path)
    if config["schema"] != CONFIG_SCHEMA or config["suite_revision"] != plan["suite_revision"]:
        raise SpecError("run config is not bound to its qualification plan")
    manifest = _resolve_file(paths["pack_manifest"], base, "pack manifest", pins["pack_manifest_sha256"])
    PackSpec.load(manifest)
    artifact_root = _resolve_directory(paths["artifact_root"], base, "artifact root")
    for artifact in PackSpec.load(manifest).artifacts:
        installed = artifact_root.joinpath(*artifact.destination.parts)
        if installed.stat().st_size != artifact.size_bytes or _digest_file(installed, artifact.artifact_id, maximum=artifact.size_bytes) != artifact.sha256:
            raise SpecError(f"installed artifact {artifact.artifact_id} differs from the exact pin")
    runtime_python = _resolve_file(paths["runtime_python"], base, "runtime Python")
    runtime_requirements = _resolve_file(paths["runtime_requirements"], base, "runtime requirements", pins["runtime_requirements_sha256"])
    runtime_inventory = _resolve_file(paths["runtime_inventory"], base, "runtime inventory")
    validate_runtime_inventory(runtime_inventory, runtime_python)
    driver = _resolve_file(paths["driver_executable"], base, "qualification driver", pins["driver_sha256"])
    fixture_manifest = _resolve_file(paths["fixture_manifest"], base, "fixture manifest", pins["fixture_manifest_sha256"])
    validate_fixture_manifest(fixture_manifest, plan)
    grant_file = _resolve_file(paths["parent_grant"], base, "parent grant")
    gpu_lock_file = _resolve_file(paths["gpu_lock"], base, "GPU coordination file")
    output_dir = _resolve_directory(paths["output_directory"], base, "output directory", must_be_empty=require_empty_output)
    target = _exact_object(config["target"], "capture target", {"process_id", "window_handle", "executable_path", "executable_sha256", "online_or_anti_cheat", "overlay_capture_excluded"})
    uint(target["process_id"], "target process id", minimum=1, maximum=2**32 - 1)
    uint(target["window_handle"], "target window handle", minimum=1)
    _resolve_file(target["executable_path"], base, "target executable", target["executable_sha256"])
    if target["online_or_anti_cheat"] is not False or target["overlay_capture_excluded"] is not True:
        raise SpecError("online/anti-cheat targets are forbidden and overlay exclusion is mandatory")
    execution = _exact_object(config["execution"], "execution config", {"cpu_threads", "other_selected_local_models_loaded", "single_face_samples", "multi_face_samples", "lifecycle_samples", "driver_protocol", "driver_source", "no_download", "hidden_children", "emit_unsigned_only"})
    uint(execution["cpu_threads"], "CPU thread count", minimum=1, maximum=4)
    if (
        execution["other_selected_local_models_loaded"] is not True
        or execution["single_face_samples"] < 100
        or execution["multi_face_samples"] < 100
        or execution["lifecycle_samples"] < 20
        or execution["driver_protocol"] != DRIVER_DESCRIPTOR_SCHEMA
        or execution["driver_source"] != "windows_graphics_capture_command_16_17"
        or execution["no_download"] is not True
        or execution["hidden_children"] is not True
        or execution["emit_unsigned_only"] is not True
    ):
        raise SpecError("execution config weakens the qualification minimums")
    telemetry = _exact_object(config["telemetry"], "telemetry config", {"process_ram_cpu", "gpu_memory", "game_frame_time", "network_egress", "driver_cleanup"})
    expected_telemetry = {
        "process_ram_cpu": "windows_process_counters",
        "gpu_memory": "dxgi_per_process_dedicated_and_shared",
        "game_frame_time": "presentmon_etw_or_equivalent_current_process",
        "network_egress": "windows_tcp_udp_owner_pid_pre_post_and_during",
        "driver_cleanup": "job_object_kill_on_close_and_mapping_release_ack",
    }
    if telemetry != expected_telemetry:
        raise SpecError("telemetry sources are incomplete or unqualified")
    return VerifiedRun(path.resolve(), config, plan, manifest, artifact_root, runtime_python, runtime_requirements, runtime_inventory, driver, fixture_manifest, grant_file, gpu_lock_file, output_dir)


def _read_frame(stream: BinaryIO) -> dict[str, Any]:
    header = stream.read(4)
    if len(header) != 4:
        raise SpecError("qualification driver ended before its evidence frame")
    length = struct.unpack(">I", header)[0]
    if not 0 < length <= MAX_JSON_BYTES:
        raise SpecError("qualification driver evidence frame is oversized")
    payload = stream.read(length)
    if len(payload) != length:
        raise SpecError("qualification driver evidence frame was truncated")
    try:
        value = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SpecError("qualification driver emitted invalid UTF-8 JSON") from exc
    if not isinstance(value, dict):
        raise SpecError("qualification driver evidence must be an object")
    return value


def _read_frame_from_bytes(value: bytes) -> dict[str, Any]:
    stream = io.BytesIO(value)
    result = _read_frame(stream)
    if stream.read(1):
        raise SpecError("qualification driver emitted trailing unframed data")
    return result


def _write_frame(stream: BinaryIO, value: dict[str, Any]) -> None:
    payload = json.dumps(value, separators=(",", ":"), allow_nan=False).encode("utf-8")
    if not 0 < len(payload) <= MAX_JSON_BYTES:
        raise SpecError("qualification driver request is oversized")
    stream.write(struct.pack(">I", len(payload)) + payload)
    stream.flush()


def _encoded_frame(value: dict[str, Any]) -> bytes:
    payload = json.dumps(value, separators=(",", ":"), allow_nan=False).encode("utf-8")
    if not 0 < len(payload) <= MAX_JSON_BYTES:
        raise SpecError("qualification driver request is oversized")
    return struct.pack(">I", len(payload)) + payload


def validate_quality_report(plan: dict[str, Any], report: Any) -> dict[str, Any]:
    fields = {"schema", "suite_revision", "partition", "threshold_source", "counts", "metrics", "tracking", "calibration", "raw_decisions_sha256", "protected_trait_labels_used", "admission_ready"}
    report = _exact_object(report, "quality report", fields)
    if report["schema"] != QUALITY_SCHEMA or report["suite_revision"] != plan["suite_revision"] or report["partition"] != "held_out_only" or report["threshold_source"] != "calibration_only" or report["protected_trait_labels_used"] is not False or report["admission_ready"] is not False:
        raise SpecError("quality report is not a held-out unsigned report")
    counts = _exact_object(report["counts"], "quality counts", {"known_characters", "unknown_characters", "known_samples", "unknown_samples", "hard_negative_pairs", "multi_face_sequences", "tracking_frames"})
    quality = plan["open_set_quality"]
    if counts["known_characters"] < quality["minimum_distinct_known_characters"] or counts["unknown_characters"] < quality["minimum_distinct_unknown_characters"] or counts["hard_negative_pairs"] < quality["minimum_hard_negative_pairs"] or counts["multi_face_sequences"] < quality["minimum_multi_face_sequences"] or counts["tracking_frames"] < quality["minimum_multi_face_sequences"] * quality["minimum_tracking_frames_per_sequence"]:
        raise SpecError("quality report sample counts are below the plan")
    for name, value in report["metrics"].items():
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not 0.0 <= float(value) <= 1.0:
            raise SpecError(f"quality metric {name} is invalid")
    required_metrics = {"false_accept_rate", "false_reject_rate", "unknown_false_accept_rate", "ambiguity_rate", "manual_correction_rate", "association_accuracy", "reacquisition_accuracy"}
    if set(report["metrics"]) != required_metrics:
        raise SpecError("quality report metrics are incomplete")
    tracking = _exact_object(report["tracking"], "tracking quality", {"identity_switches", "track_fragmentation", "offscreen_continuity_errors", "reacquisition_failures", "false_matches"})
    for key, value in tracking.items():
        uint(value, key, minimum=0)
    calibration = _exact_object(report["calibration"], "quality calibration", {"match_threshold", "ambiguity_margin", "consensus_window", "minimum_votes", "top1_top2_margin_quantiles"})
    if not isinstance(calibration["match_threshold"], (int, float)) or not isinstance(calibration["ambiguity_margin"], (int, float)):
        raise SpecError("quality thresholds are invalid")
    uint(calibration["consensus_window"], "consensus window", minimum=1)
    uint(calibration["minimum_votes"], "minimum votes", minimum=1)
    quantiles = _exact_object(calibration["top1_top2_margin_quantiles"], "margin quantiles", {"p50", "p95", "p99", "min", "max"})
    if any(isinstance(value, bool) or not isinstance(value, (int, float)) for value in quantiles.values()):
        raise SpecError("margin distribution is invalid")
    sha256(report["raw_decisions_sha256"], "raw quality decisions digest")
    return report


def validate_cleanup_report(report: Any) -> dict[str, Any]:
    report = _exact_object(report, "cleanup report", {"schema", "worker_processes_remaining", "driver_children_remaining", "open_mapping_count", "unreleased_lease_count", "late_result_count", "remote_endpoint_count", "gpu_lock_unchanged", "job_object_closed", "pixels_persisted", "embeddings_persisted", "raw_network_inventory_sha256", "passed"})
    if report["schema"] != CLEANUP_SCHEMA or any(report[key] != 0 for key in ("worker_processes_remaining", "driver_children_remaining", "open_mapping_count", "unreleased_lease_count", "late_result_count", "remote_endpoint_count")) or report["gpu_lock_unchanged"] is not True or report["job_object_closed"] is not True or report["pixels_persisted"] is not False or report["embeddings_persisted"] is not False or report["passed"] is not True:
        raise SpecError("qualification cleanup/no-egress proof failed")
    sha256(report["raw_network_inventory_sha256"], "network inventory digest")
    return report


def validate_driver_bundle(plan: dict[str, Any], bundle: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    _exact_object(bundle, "driver bundle", {"schema", "descriptor", "raw_samples", "quality_report", "cleanup_report", "signatures", "admission_ready"})
    if bundle["schema"] != BUNDLE_SCHEMA or bundle["signatures"] != [] or bundle["admission_ready"] is not False:
        raise SpecError("driver attempted to emit signed or admissible evidence")
    descriptor = _exact_object(bundle["descriptor"], "driver descriptor", {"schema", "source", "command_ids", "advancing_wgc_frames", "fixture_backend", "hidden_processes", "no_download", "cpu_backend", "game_load_measured", "vram_measured", "network_egress_measured"})
    if descriptor != {"schema": DRIVER_DESCRIPTOR_SCHEMA, "source": "windows_graphics_capture", "command_ids": [16, 17], "advancing_wgc_frames": True, "fixture_backend": False, "hidden_processes": True, "no_download": True, "cpu_backend": "opencv-dnn-cpu", "game_load_measured": True, "vram_measured": True, "network_egress_measured": True}:
        raise SpecError("driver descriptor is not the real WGC qualification boundary")
    raw = bundle["raw_samples"]
    if not isinstance(raw, dict):
        raise SpecError("driver raw samples are invalid")
    validate_raw_samples(plan, raw)
    quality = validate_quality_report(plan, bundle["quality_report"])
    cleanup = validate_cleanup_report(bundle["cleanup_report"])
    return raw, quality, cleanup


def _consume_parent_grant(run: VerifiedRun, now: int) -> tuple[dict[str, Any], Path]:
    grant = validate_grant(run.grant_file, run.plan, now)
    token = os.environ.get(TOKEN_ENV)
    if not isinstance(token, str) or len(token) < 32 or len(token) > 1024 or hashlib.sha256(token.encode("utf-8")).hexdigest() != grant["token_sha256"]:
        raise SpecError(f"execute requires the exact one-time token in {TOKEN_ENV}")
    receipt = run.output_dir / f"grant-{grant['grant_id']}.consumed.json"
    payload = json.dumps({"schema": "npc.identity-parent-grant-consumption/v1", "grant_id": grant["grant_id"], "token_sha256": grant["token_sha256"], "consumed_unix_seconds": now}, separators=(",", ":")).encode("utf-8")
    try:
        with receipt.open("xb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    except FileExistsError as exc:
        raise SpecError("parent grant was already consumed") from exc
    return grant, receipt


def _assert_gpu_lock(path: Path, initial: bytes | None = None) -> bytes:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 16:
        raise SpecError("GPU coordination file is invalid")
    value = path.read_bytes()
    if value.strip().casefold() != b"yes":
        raise SpecError("externally-owned GPU/AI model lane is not granted")
    if initial is not None and not secrets.compare_digest(value, initial):
        raise SpecError("GPU coordination file changed during qualification")
    return value


def execute(run: VerifiedRun) -> dict[str, Any]:
    if os.name != "nt" or platform.machine().casefold() not in {"amd64", "x86_64"}:
        raise SpecError("real identity qualification executes only on 64-bit Windows")
    now = int(time.time())
    grant, receipt = _consume_parent_grant(run, now)
    initial_lock = _assert_gpu_lock(run.gpu_lock_file)
    request = {
        "schema": "npc.identity-qualification-driver-request/v1",
        "suite_revision": run.plan["suite_revision"],
        "grant_id": grant["grant_id"],
        "grant_token_sha256": grant["token_sha256"],
        "config": run.config,
        "resolved": {
            "plan": str(Path(run.config["paths"]["plan"]).resolve()),
            "pack_manifest": str(run.manifest),
            "artifact_root": str(run.artifact_root),
            "runtime_python": str(run.runtime_python),
            "runtime_inventory": str(run.runtime_inventory),
            "fixture_manifest": str(run.fixture_manifest),
            "output_directory": str(run.output_dir),
        },
        "requirements": {"download_allowed": False, "visible_console_allowed": False, "signed_output_allowed": False, "minimum_cold_reload_cancel_unload_restart": 20, "minimum_single_face_wgc_frames": 100, "minimum_multi_face_wgc_frames": 100},
    }
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0) | getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    env = {"SystemRoot": os.environ.get("SystemRoot", r"C:\Windows"), "TEMP": os.environ.get("TEMP", ""), "TMP": os.environ.get("TMP", "")}
    process = subprocess.Popen([str(run.driver), "--qualification-driver-v1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=creationflags, env=env)
    try:
        timeout = min(grant["maximum_run_seconds"], MAX_RUN_SECONDS)
        stdout, stderr = process.communicate(input=_encoded_frame(request), timeout=timeout)
        if len(stdout) > MAX_JSON_BYTES + 4 or len(stderr) > 64 * 1024:
            raise SpecError("qualification driver output exceeded its evidence/log bound")
        bundle = _read_frame_from_bytes(stdout)
        if process.returncode != 0:
            raise SpecError(f"qualification driver failed with exit {process.returncode}; bounded stderr sha256={hashlib.sha256(stderr).hexdigest()}")
        raw, quality, cleanup = validate_driver_bundle(run.plan, bundle)
        candidate = build_unsigned_candidate(run.plan, raw)
        report = {
            "schema": RUN_REPORT_SCHEMA,
            "suite_revision": run.plan["suite_revision"],
            "evidence_authority": "unsigned_current_device_candidate_not_admissible",
            "admission_ready": False,
            "signatures": [],
            "grant_consumption_sha256": _digest_file(receipt, "grant consumption receipt"),
            "raw_samples": raw,
            "quality_report": quality,
            "cleanup_report": cleanup,
            "resource_candidate": candidate,
        }
        _assert_gpu_lock(run.gpu_lock_file, initial_lock)
        output = run.output_dir / "unsigned-identity-qualification-run.v1.json"
        output.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8", newline="\n")
        return report
    except subprocess.TimeoutExpired as exc:
        process.kill()
        process.wait(timeout=10)
        raise SpecError("qualification driver exceeded the parent grant duration") from exc
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
        _assert_gpu_lock(run.gpu_lock_file, initial_lock)


def preflight(run: VerifiedRun) -> dict[str, Any]:
    grant = _read_json(run.grant_file, "parent grant template", maximum=64 * 1024)
    fixture = _read_json(run.fixture_manifest, "fixture corpus", maximum=16 * 1024 * 1024)
    return {
        "schema": "npc.identity-qualification-preflight/v1",
        "status": "prepared_not_executed",
        "model_loaded": False,
        "model_inference_performed": False,
        "gpu_lock_state_evaluated": False,
        "gpu_lock_changed": False,
        "network_used": False,
        "suite_revision": run.plan["suite_revision"],
        "pack_manifest_sha256": PACK_MANIFEST_SHA256,
        "driver_sha256": run.config["pins"]["driver_sha256"],
        "fixture_manifest_sha256": hashlib.sha256(json.dumps(fixture, sort_keys=True).encode()).hexdigest(),
        "grant_schema_present": grant.get("schema") == GRANT_SCHEMA,
        "execute_requires": [TOKEN_ENV, "fresh_one_time_parent_grant", "gpu_lock_exact_yes_owned_by_parent", "64_bit_windows", "hidden_hash_pinned_native_driver"],
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prepare or execute the real hidden Windows YuNet/SFace qualification run; default preflight never loads a model",
        epilog="No mode downloads artifacts, changes gpu use.txt, signs evidence, or grants production admission.",
    )
    parser.add_argument("--config", type=Path, required=True, help="Exact run config with installed paths and SHA-256 pins")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--preflight", action="store_true", help="Validate files/contracts only; never read or change the GPU lock")
    mode.add_argument("--execute", action="store_true", help=f"Run hidden on Windows; requires a fresh parent grant and {TOKEN_ENV}")
    parser.add_argument("--acknowledge-real-model-run", action="store_true", help="Required with --execute; confirms CPU inference still occupies the serialized AI-model lane")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        run = validate_run_config(args.config, require_empty_output=True)
        if args.execute:
            if not args.acknowledge_real_model_run:
                raise SpecError("--execute requires --acknowledge-real-model-run")
            result = execute(run)
        else:
            result = preflight(run)
        print(json.dumps(result, separators=(",", ":"), allow_nan=False))
        return 0
    except (OSError, SpecError, subprocess.SubprocessError) as exc:
        print(f"identity qualification harness rejected: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
