#!/usr/bin/env python3
"""Validate real identity benchmark samples and build an unsigned candidate.

This module never downloads, loads, or invokes a model. It is the fail-closed
aggregation half of the future Windows qualification harness. A trusted native
runner must collect the raw samples and the Model Manager signing path must
verify/sign the resulting resource envelope before it can become admissible.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any, Iterable

from model_spec import PACK_ID, PACK_MANIFEST_SHA256, PACK_REVISION, SpecError, sha256, uint

PLAN_SCHEMA = "npc.identity-current-device-qualification-plan/v1"
RAW_SCHEMA = "npc.identity-qualification-raw-samples/v1"
CANDIDATE_SCHEMA = "npc.identity-unsigned-qualification-candidate/v1"
RESOURCE_SCHEMA = "npc.measured-resource-envelope/v1"
MAX_RAW_SAMPLES = 100_000
MAX_RAW_BYTES = 64 * 1024 * 1024


def _object(value: Any, name: str, fields: set[str]) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        raise SpecError(f"{name} fields are invalid")
    return value


def load_plan(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 2 * 1024 * 1024:
        raise SpecError("qualification plan must be a bounded regular file")
    try:
        plan = json.loads(path.read_bytes())
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SpecError("qualification plan must be UTF-8 JSON") from exc
    if not isinstance(plan, dict) or plan.get("schema") != PLAN_SCHEMA:
        raise SpecError("qualification plan schema is unsupported")
    bindings = plan.get("bindings")
    if not isinstance(bindings, dict) or (
        bindings.get("pack_id") != PACK_ID
        or bindings.get("pack_revision") != PACK_REVISION
        or bindings.get("manifest_file_sha256") != PACK_MANIFEST_SHA256
    ):
        raise SpecError("qualification plan is not bound to the reviewed pack")
    gate = plan.get("execution_gate")
    if not isinstance(gate, dict) or (
        gate.get("model_download_performed") is not False
        or gate.get("model_inference_performed") is not False
        or gate.get("allow_synthetic_measurements") is not False
        or gate.get("allow_unsigned_admission") is not False
    ):
        raise SpecError("qualification plan execution gate was weakened")
    phases = plan.get("performance_phases")
    if not isinstance(phases, list) or not phases:
        raise SpecError("qualification plan has no performance phases")
    phase_ids: set[str] = set()
    for phase in phases:
        if not isinstance(phase, dict):
            raise SpecError("qualification phase is invalid")
        phase_id = phase.get("id")
        minimum = phase.get("minimum_successful_samples")
        metrics = phase.get("required_metrics")
        if (
            not isinstance(phase_id, str)
            or phase_id in phase_ids
            or isinstance(minimum, bool)
            or not isinstance(minimum, int)
            or minimum < 20
            or not isinstance(metrics, list)
            or not metrics
            or len(metrics) != len(set(metrics))
            or any(not isinstance(metric, str) or not metric for metric in metrics)
        ):
            raise SpecError("qualification phase is incomplete")
        phase_ids.add(phase_id)
    outputs = plan.get("required_outputs")
    if not isinstance(outputs, dict) or (
        outputs.get("resource_envelope_schema") != RESOURCE_SCHEMA
        or outputs.get("resource_envelope_minimum_sample_count") != 20
        or outputs.get("resource_envelope_signature_required") is not True
        or outputs.get("whole_loadout_admission_required") is not True
    ):
        raise SpecError("qualification output gate is incomplete")
    return plan


def _read_raw(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_RAW_BYTES:
        raise SpecError("raw sample file must be a bounded regular file")
    try:
        value = json.loads(path.read_bytes())
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SpecError("raw samples must be UTF-8 JSON") from exc
    return _object(
        value,
        "raw sample document",
        {
            "schema",
            "suite_revision",
            "pack_id",
            "pack_revision",
            "manifest_file_sha256",
            "device_fingerprint_sha256",
            "measured_unix_seconds",
            "expires_unix_seconds",
            "sequence",
            "samples",
        },
    )


def validate_raw_samples(plan: dict[str, Any], raw: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    bindings = plan["bindings"]
    if (
        raw.get("schema") != RAW_SCHEMA
        or raw.get("suite_revision") != plan.get("suite_revision")
        or raw.get("pack_id") != bindings["pack_id"]
        or raw.get("pack_revision") != bindings["pack_revision"]
        or raw.get("manifest_file_sha256") != bindings["manifest_file_sha256"]
    ):
        raise SpecError("raw samples are not bound to the qualification plan")
    device = sha256(raw.get("device_fingerprint_sha256"), "device fingerprint")
    measured = uint(raw.get("measured_unix_seconds"), "measurement time", minimum=1)
    expires = uint(raw.get("expires_unix_seconds"), "measurement expiry", minimum=1)
    uint(raw.get("sequence"), "measurement sequence", minimum=1)
    if expires <= measured or expires - measured > 30 * 24 * 60 * 60:
        raise SpecError("raw sample lifetime is invalid")
    samples = raw.get("samples")
    if not isinstance(samples, list) or not samples or len(samples) > MAX_RAW_SAMPLES:
        raise SpecError("raw sample count is invalid")

    phases = {phase["id"]: phase for phase in plan["performance_phases"]}
    grouped: dict[str, list[dict[str, Any]]] = {phase_id: [] for phase_id in phases}
    seen: set[tuple[str, int]] = set()
    for sample in samples:
        sample = _object(sample, "raw sample", {"phase_id", "sample_index", "success", "metrics"})
        phase_id = sample.get("phase_id")
        phase = phases.get(phase_id)
        index = uint(sample.get("sample_index"), "sample index", minimum=1)
        if phase is None or (phase_id, index) in seen or not isinstance(sample.get("success"), bool):
            raise SpecError("raw sample identity is invalid")
        seen.add((phase_id, index))
        metrics = sample.get("metrics")
        required_metrics = set(phase["required_metrics"])
        if not isinstance(metrics, dict) or set(metrics) != required_metrics:
            raise SpecError(f"raw sample metrics differ for phase {phase_id}")
        for name, value in metrics.items():
            if (
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(float(value))
                or float(value) < 0.0
            ):
                raise SpecError(f"raw metric {name} must be finite and non-negative")
        if sample["success"]:
            grouped[phase_id].append(sample)

    for phase_id, phase in phases.items():
        if len(grouped[phase_id]) < phase["minimum_successful_samples"]:
            raise SpecError(f"phase {phase_id} does not meet its real successful-sample minimum")
    if not device:
        raise SpecError("device fingerprint is missing")
    return grouped


def _nearest_rank(values: Iterable[float], percentile: float) -> int:
    ordered = sorted(float(value) for value in values)
    if not ordered:
        raise SpecError("cannot aggregate an empty measurement")
    rank = max(1, math.ceil(percentile * len(ordered))) - 1
    return math.ceil(ordered[rank])


def _metric(group: list[dict[str, Any]], name: str) -> list[float]:
    return [float(sample["metrics"][name]) for sample in group]


def build_unsigned_candidate(plan: dict[str, Any], raw: dict[str, Any]) -> dict[str, Any]:
    grouped = validate_raw_samples(plan, raw)
    single = grouped["single_face_inference"]
    multi = grouped["multi_face_inference"]
    inference = single + multi
    cpu_time = _metric(inference, "process_cpu_millis_delta")
    process_ram = _metric(single, "process_private_bytes_peak") + _metric(
        multi, "process_private_bytes_peak"
    )
    dedicated_vram = _metric(single, "dedicated_vram_bytes_peak") + _metric(
        multi, "dedicated_vram_bytes_peak"
    )
    if any(value != 0.0 for value in dedicated_vram):
        raise SpecError("CPU-resident identity samples reported dedicated model VRAM")

    # p99_total_ram_bytes is deliberately conservative until the trusted runner
    # adds a separately attested whole-loadout total. It may not be smaller than
    # the process p99, and this unsigned candidate can never be admitted.
    resident_ram = _nearest_rank(process_ram, 0.99)
    payload = {
        "schema": RESOURCE_SCHEMA,
        "report_id": f"identity-{raw['device_fingerprint_sha256'][:16]}-{raw['sequence']}",
        "sequence": raw["sequence"],
        "measured_unix_seconds": raw["measured_unix_seconds"],
        "expires_unix_seconds": raw["expires_unix_seconds"],
        "device_fingerprint_sha256": raw["device_fingerprint_sha256"],
        "identity": {"pack_id": PACK_ID, "revision": PACK_REVISION},
        "manifest_sha256": PACK_MANIFEST_SHA256,
        "capability": "vision",
        "benchmark_suite_revision": plan["suite_revision"],
        "runtime": plan["bindings"]["runtime"],
        "runtime_revision": plan["bindings"]["runtime_revision"],
        "backend": plan["bindings"]["backend"],
        "sample_count": min(len(group) for group in grouped.values()),
        "placements": {
            "cpu_resident": {
                "resident_ram_bytes": resident_ram,
                "p99_total_ram_bytes": resident_ram,
                "resident_vram_bytes": 0,
                "p99_workspace_vram_bytes": 0,
                "p99_load_millis": _nearest_rank(
                    _metric(grouped["cold_load"], "duration_millis"), 0.99
                ),
                "p99_reload_millis": _nearest_rank(
                    _metric(grouped["warm_reload"], "duration_millis"), 0.99
                ),
                "p99_operation_millis": _nearest_rank(
                    _metric(inference, "duration_millis"), 0.99
                ),
            }
        },
    }
    return {
        "schema": CANDIDATE_SCHEMA,
        "evidence_authority": "unsigned_candidate_not_admissible",
        "admission_ready": False,
        "resource_envelope_candidate": {"signed": payload, "signatures": []},
        "supplemental_cpu": {
            "p50_operation_cpu_millis": _nearest_rank(cpu_time, 0.50),
            "p95_operation_cpu_millis": _nearest_rank(cpu_time, 0.95),
            "p99_operation_cpu_millis": _nearest_rank(cpu_time, 0.99),
        },
        "required_next_gates": [
            "trusted_runner_attests_whole_loadout_total_ram",
            "rights_cleared_open_set_quality_report",
            "self_test_attestation",
            "trusted_measurement_signature",
            "whole_loadout_resource_governor_admission",
            "sface_rights_resolution",
        ],
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate local identity qualification evidence")
    parser.add_argument("--plan", type=Path, default=Path(__file__).with_name("qualification-plan.v1.json"))
    parser.add_argument("--raw-samples", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        plan = load_plan(args.plan)
        if args.raw_samples is None:
            print(json.dumps({"schema": PLAN_SCHEMA, "status": "valid_prepared_not_executed"}, separators=(",", ":")))
            return 0
        raw = _read_raw(args.raw_samples)
        print(json.dumps(build_unsigned_candidate(plan, raw), separators=(",", ":"), allow_nan=False))
        return 0
    except (OSError, SpecError) as exc:
        print(f"identity qualification rejected: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
