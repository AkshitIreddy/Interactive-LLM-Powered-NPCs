#!/usr/bin/env python3
"""Bounded, dependency-free benchmark evidence aggregation.

The harness deliberately consumes timing/counter observations instead of provider
payloads.  Its strict row allowlist keeps prompts, credentials, audio, and model
responses out of the generated evidence artifact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import random
import re
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Sequence


REPORT_SCHEMA_VERSION = "interactive-npcs-benchmark-report/v1"
SAMPLE_SCHEMA_VERSION = "interactive-npcs-benchmark-sample/v1"
DEFAULT_MAX_RECORDS = 100_000
HARD_MAX_RECORDS = 1_000_000
DEFAULT_MAX_INPUT_BYTES = 16 * 1024 * 1024
HARD_MAX_INPUT_BYTES = 256 * 1024 * 1024
DEFAULT_TIMEOUT_SECONDS = 60.0
HARD_TIMEOUT_SECONDS = 900.0


@dataclass(frozen=True)
class MetricDefinition:
    name: str
    unit: str
    description: str


METRICS: tuple[MetricDefinition, ...] = (
    MetricDefinition("capture.latency_ms", "ms", "Capture request to owned frame availability."),
    MetricDefinition("identity.latency_ms", "ms", "Owned frame availability to identity-lock decision."),
    MetricDefinition("llm.ttft_ms", "ms", "LLM request submission to first response token."),
    MetricDefinition("llm.output_tokens", "tokens", "Delivered LLM output token count."),
    MetricDefinition("llm.tokens_per_second", "tokens_per_second", "LLM output throughput after first token."),
    MetricDefinition("tts.ttfa_ms", "ms", "TTS request submission to first decoded audio ready for playback."),
    MetricDefinition("tts.audio_completion_ms", "ms", "TTS request submission to final decoded audio chunk."),
    MetricDefinition("audio.submission_ms", "ms", "Decoded audio availability to audio endpoint submission."),
    MetricDefinition("compositor.latency_ms", "ms", "Compatible source frame to composed frame availability."),
    MetricDefinition("compositor.fps", "frames_per_second", "Compositor output frame rate over the sample window."),
    MetricDefinition("compositor.stale_drops", "count", "Stale compositor outputs discarded in the sample window."),
    MetricDefinition("end_to_end.turn_ms", "ms", "Final user-input boundary to first submitted response audio."),
)
METRIC_BY_NAME = {metric.name: metric for metric in METRICS}
EXECUTION_MODES = {"simulated", "mocked", "live"}
MEASUREMENT_KINDS = {"measured", "planning_estimate"}
ALLOWED_SAMPLE_KEYS = {
    "schema_version",
    "sample_id",
    "turn_id",
    "metric",
    "value",
    "unit",
    "observed_at_utc",
    "execution_mode",
    "measurement_kind",
}
SAFE_ID_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,95}$")
ISO_UTC_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?Z$")
SECRET_KEY_PATTERN = re.compile(
    r"(?:api[_-]?key|authorization|bearer|password|passwd|secret|credential|cookie|private[_-]?key)",
    re.IGNORECASE,
)


class BenchmarkError(ValueError):
    """A safe, user-facing validation error that never includes row contents."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def percentile(values: Sequence[float], probability: float) -> float:
    """Return the deterministic R-7 / NumPy-default linear percentile."""
    if not values:
        raise BenchmarkError("Cannot calculate a percentile for an empty sample set.")
    if not 0.0 <= probability <= 1.0:
        raise BenchmarkError("Percentile probability must be between zero and one.")
    ordered = sorted(float(value) for value in values)
    position = (len(ordered) - 1) * probability
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] + (ordered[upper] - ordered[lower]) * fraction


def rounded(value: float) -> float:
    return round(float(value), 6)


def _validate_bounds(max_records: int, max_input_bytes: int, timeout_seconds: float) -> None:
    if not 1 <= max_records <= HARD_MAX_RECORDS:
        raise BenchmarkError(f"max_records must be between 1 and {HARD_MAX_RECORDS}.")
    if not 1 <= max_input_bytes <= HARD_MAX_INPUT_BYTES:
        raise BenchmarkError(f"max_input_bytes must be between 1 and {HARD_MAX_INPUT_BYTES}.")
    if not 0.1 <= timeout_seconds <= HARD_TIMEOUT_SECONDS:
        raise BenchmarkError(f"timeout_seconds must be between 0.1 and {HARD_TIMEOUT_SECONDS}.")


def validate_sample(row: Any, line_number: int) -> dict[str, Any]:
    if not isinstance(row, dict):
        raise BenchmarkError(f"Line {line_number}: sample must be a JSON object.")
    unknown = sorted(set(row) - ALLOWED_SAMPLE_KEYS)
    if unknown:
        redacted_names = ["<secret-like-field>" if SECRET_KEY_PATTERN.search(key) else key for key in unknown]
        raise BenchmarkError(f"Line {line_number}: unsupported field(s): {', '.join(redacted_names)}.")
    missing = sorted(ALLOWED_SAMPLE_KEYS - set(row))
    if missing:
        raise BenchmarkError(f"Line {line_number}: missing required field(s): {', '.join(missing)}.")
    if row["schema_version"] != SAMPLE_SCHEMA_VERSION:
        raise BenchmarkError(f"Line {line_number}: unsupported sample schema_version.")
    if row["metric"] not in METRIC_BY_NAME:
        raise BenchmarkError(f"Line {line_number}: unsupported metric name.")
    definition = METRIC_BY_NAME[row["metric"]]
    if row["unit"] != definition.unit:
        raise BenchmarkError(f"Line {line_number}: unit does not match metric {definition.name}.")
    if row["execution_mode"] not in EXECUTION_MODES:
        raise BenchmarkError(f"Line {line_number}: unsupported execution_mode.")
    if row["measurement_kind"] not in MEASUREMENT_KINDS:
        raise BenchmarkError(f"Line {line_number}: unsupported measurement_kind.")
    for field in ("sample_id", "turn_id"):
        value = row[field]
        if not isinstance(value, str) or not SAFE_ID_PATTERN.fullmatch(value):
            raise BenchmarkError(f"Line {line_number}: {field} is not a safe bounded identifier.")
    timestamp = row["observed_at_utc"]
    if not isinstance(timestamp, str) or not ISO_UTC_PATTERN.fullmatch(timestamp):
        raise BenchmarkError(f"Line {line_number}: observed_at_utc must be an ISO-8601 UTC timestamp ending in Z.")
    value = row["value"]
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        raise BenchmarkError(f"Line {line_number}: value must be a finite number.")
    if float(value) < 0:
        raise BenchmarkError(f"Line {line_number}: value must not be negative.")
    if definition.unit in {"count", "tokens"} and not float(value).is_integer():
        raise BenchmarkError(f"Line {line_number}: {definition.unit} values must be whole numbers.")
    return row


def strict_json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            safe_key = "<secret-like-field>" if SECRET_KEY_PATTERN.search(key) else key
            raise BenchmarkError(f"Duplicate JSON field: {safe_key}.")
        result[key] = value
    return result


def read_jsonl(
    path: Path,
    *,
    max_records: int,
    max_input_bytes: int,
    timeout_seconds: float,
) -> tuple[list[dict[str, Any]], str, int, float]:
    _validate_bounds(max_records, max_input_bytes, timeout_seconds)
    try:
        size = path.stat().st_size
    except OSError as exc:
        raise BenchmarkError("Input file could not be inspected.") from exc
    if size > max_input_bytes:
        raise BenchmarkError(f"Input exceeds the {max_input_bytes}-byte limit.")

    started = time.monotonic()
    digest = hashlib.sha256()
    rows: list[dict[str, Any]] = []
    try:
        with path.open("rb") as stream:
            for line_number, raw_line in enumerate(stream, start=1):
                if time.monotonic() - started > timeout_seconds:
                    raise BenchmarkError(f"Input processing exceeded the {timeout_seconds:g}-second limit.")
                digest.update(raw_line)
                if not raw_line.strip():
                    continue
                if len(rows) >= max_records:
                    raise BenchmarkError(f"Input exceeds the {max_records}-record limit.")
                try:
                    decoded = raw_line.decode("utf-8")
                except UnicodeDecodeError as exc:
                    raise BenchmarkError(f"Line {line_number}: input must be UTF-8.") from exc
                try:
                    parsed = json.loads(decoded, object_pairs_hook=strict_json_object)
                except json.JSONDecodeError as exc:
                    raise BenchmarkError(f"Line {line_number}: invalid JSON.") from exc
                rows.append(validate_sample(parsed, line_number))
    except OSError as exc:
        raise BenchmarkError("Input file could not be read.") from exc
    elapsed_ms = (time.monotonic() - started) * 1000.0
    if not rows:
        raise BenchmarkError("Input contains no benchmark samples.")
    return rows, digest.hexdigest(), size, elapsed_ms


def _uniform_label(rows: Sequence[dict[str, Any]], field: str) -> str:
    values = {str(row[field]) for row in rows}
    if len(values) != 1:
        raise BenchmarkError(f"All samples in one report must use the same {field}.")
    return next(iter(values))


def summarize_rows(
    rows: Sequence[dict[str, Any]],
    *,
    report_id: str,
    source_sha256: str,
    source_bytes: int,
    timeout_seconds: float,
    max_records: int,
    elapsed_ms: float,
    generated_at_utc: str | None = None,
) -> dict[str, Any]:
    if not SAFE_ID_PATTERN.fullmatch(report_id):
        raise BenchmarkError("report_id is not a safe bounded identifier.")
    execution_mode = _uniform_label(rows, "execution_mode")
    measurement_kind = _uniform_label(rows, "measurement_kind")
    grouped: dict[str, list[float]] = {definition.name: [] for definition in METRICS}
    for row in rows:
        grouped[str(row["metric"])].append(float(row["value"]))
    missing = [name for name, values in grouped.items() if not values]
    if missing:
        raise BenchmarkError("Missing required metric(s): " + ", ".join(missing) + ".")

    system_name = platform.system() or "Unknown"
    acceptance_eligible = execution_mode == "live" and measurement_kind == "measured" and system_name == "Windows"
    if measurement_kind == "planning_estimate":
        eligibility_reason = "Planning estimates are never benchmark evidence."
    elif execution_mode != "live":
        eligibility_reason = f"{execution_mode.capitalize()} measurements are test evidence, not live acceptance evidence."
    elif system_name != "Windows":
        eligibility_reason = "Live acceptance evidence must be collected by the harness on Windows."
    else:
        eligibility_reason = "Live measured Windows evidence; acceptance still depends on scenario and artifact review."

    metrics: list[dict[str, Any]] = []
    for definition in METRICS:
        values = grouped[definition.name]
        metrics.append(
            {
                "name": definition.name,
                "unit": definition.unit,
                "description": definition.description,
                "sample_count": len(values),
                "min": rounded(min(values)),
                "max": rounded(max(values)),
                "mean": rounded(sum(values) / len(values)),
                "p50": rounded(percentile(values, 0.50)),
                "p95": rounded(percentile(values, 0.95)),
                "p99": rounded(percentile(values, 0.99)),
                "execution_mode": execution_mode,
                "measurement_kind": measurement_kind,
            }
        )

    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "report_id": report_id,
        "generated_at_utc": generated_at_utc or utc_now(),
        "classification": {
            "execution_mode": execution_mode,
            "measurement_kind": measurement_kind,
            "acceptance_eligible": acceptance_eligible,
            "reason": eligibility_reason,
        },
        "source": {
            "format": "jsonl",
            "sha256": source_sha256,
            "bytes": source_bytes,
            "records": len(rows),
            "path_recorded": False,
            "payloads_recorded": False,
        },
        "bounds": {
            "timeout_seconds": timeout_seconds,
            "max_records": max_records,
            "records_processed": len(rows),
            "elapsed_ms": rounded(elapsed_ms),
            "completed_within_bounds": elapsed_ms <= timeout_seconds * 1000.0 and len(rows) <= max_records,
        },
        "machine": {
            "operating_system": system_name,
            "architecture": platform.machine() or "unknown",
            "python_version": platform.python_version(),
            "logical_processor_count": os.cpu_count() or 1,
            "hostname_recorded": False,
            "environment_variables_recorded": False,
        },
        "percentile_method": "R-7 linear interpolation; position=(n-1)*q",
        "metrics": metrics,
    }


def write_json_atomic(path: Path, document: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(document, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    temporary: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", newline="\n", dir=path.parent, prefix=f".{path.name}.", suffix=".tmp", delete=False
        ) as stream:
            temporary = stream.name
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def deterministic_simulation(iterations: int, seed: int, observed_at_utc: str) -> list[dict[str, Any]]:
    if not 1 <= iterations <= 100_000:
        raise BenchmarkError("iterations must be between 1 and 100000.")
    if not ISO_UTC_PATTERN.fullmatch(observed_at_utc):
        raise BenchmarkError("observed_at_utc must be an ISO-8601 UTC timestamp ending in Z.")
    randomizer = random.Random(seed)
    baselines = {
        "capture.latency_ms": 8.0,
        "identity.latency_ms": 11.0,
        "llm.ttft_ms": 260.0,
        "llm.output_tokens": 32.0,
        "llm.tokens_per_second": 48.0,
        "tts.ttfa_ms": 145.0,
        "tts.audio_completion_ms": 790.0,
        "audio.submission_ms": 5.0,
        "compositor.latency_ms": 13.0,
        "compositor.fps": 59.0,
        "compositor.stale_drops": 1.0,
        "end_to_end.turn_ms": 680.0,
    }
    scales = {
        "capture.latency_ms": 2.0,
        "identity.latency_ms": 3.0,
        "llm.ttft_ms": 65.0,
        "llm.output_tokens": 9.0,
        "llm.tokens_per_second": 8.0,
        "tts.ttfa_ms": 35.0,
        "tts.audio_completion_ms": 140.0,
        "audio.submission_ms": 1.5,
        "compositor.latency_ms": 4.0,
        "compositor.fps": 4.0,
        "compositor.stale_drops": 1.0,
        "end_to_end.turn_ms": 120.0,
    }
    rows: list[dict[str, Any]] = []
    for iteration in range(iterations):
        turn_id = f"sim-turn-{iteration + 1:06d}"
        for definition in METRICS:
            jitter = randomizer.uniform(-1.0, 1.0) * scales[definition.name]
            value = max(0.0, baselines[definition.name] + jitter)
            if definition.unit in {"count", "tokens"}:
                value = float(round(value))
            rows.append(
                {
                    "schema_version": SAMPLE_SCHEMA_VERSION,
                    "sample_id": f"{turn_id}:{definition.name}",
                    "turn_id": turn_id,
                    "metric": definition.name,
                    "value": rounded(value),
                    "unit": definition.unit,
                    "observed_at_utc": observed_at_utc,
                    "execution_mode": "simulated",
                    "measurement_kind": "measured",
                }
            )
    return rows


def _rows_digest(rows: Iterable[dict[str, Any]]) -> tuple[str, int]:
    payload = "".join(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n" for row in rows).encode("utf-8")
    return hashlib.sha256(payload).hexdigest(), len(payload)


def validate_report_shape(report: Any) -> None:
    if not isinstance(report, dict) or report.get("schema_version") != REPORT_SCHEMA_VERSION:
        raise BenchmarkError("Unsupported report schema_version.")
    expected_top = {
        "schema_version", "report_id", "generated_at_utc", "classification", "source", "bounds", "machine", "percentile_method", "metrics"
    }
    if set(report) != expected_top:
        raise BenchmarkError("Report does not have the exact v1 top-level fields.")
    if not isinstance(report.get("report_id"), str) or not SAFE_ID_PATTERN.fullmatch(report["report_id"]):
        raise BenchmarkError("Report has an invalid report_id.")
    if not isinstance(report.get("generated_at_utc"), str) or not ISO_UTC_PATTERN.fullmatch(report["generated_at_utc"]):
        raise BenchmarkError("Report has an invalid generated_at_utc.")
    if report.get("percentile_method") != "R-7 linear interpolation; position=(n-1)*q":
        raise BenchmarkError("Report has an unsupported percentile method.")

    classification = report.get("classification")
    if not isinstance(classification, dict) or set(classification) != {
        "execution_mode", "measurement_kind", "acceptance_eligible", "reason"
    }:
        raise BenchmarkError("Report has an invalid classification object.")
    if classification["execution_mode"] not in EXECUTION_MODES:
        raise BenchmarkError("Report has an invalid execution mode.")
    if classification["measurement_kind"] not in MEASUREMENT_KINDS:
        raise BenchmarkError("Report has an invalid measurement kind.")
    if not isinstance(classification["acceptance_eligible"], bool):
        raise BenchmarkError("Report has an invalid acceptance eligibility value.")
    if not isinstance(classification["reason"], str) or not 1 <= len(classification["reason"]) <= 240:
        raise BenchmarkError("Report has an invalid classification reason.")

    source = report.get("source")
    if not isinstance(source, dict) or set(source) != {
        "format", "sha256", "bytes", "records", "path_recorded", "payloads_recorded"
    }:
        raise BenchmarkError("Report has an invalid source object.")
    if source["format"] != "jsonl" or not isinstance(source["sha256"], str) or not re.fullmatch(r"[a-f0-9]{64}", source["sha256"]):
        raise BenchmarkError("Report has invalid source identity.")
    if isinstance(source["bytes"], bool) or not isinstance(source["bytes"], int) or not 1 <= source["bytes"] <= HARD_MAX_INPUT_BYTES:
        raise BenchmarkError("Report has an invalid source byte count.")
    if isinstance(source["records"], bool) or not isinstance(source["records"], int) or not len(METRICS) <= source["records"] <= HARD_MAX_RECORDS:
        raise BenchmarkError("Report has an invalid source record count.")
    if source["path_recorded"] is not False or source["payloads_recorded"] is not False:
        raise BenchmarkError("Report violates source privacy invariants.")

    bounds = report.get("bounds")
    if not isinstance(bounds, dict) or set(bounds) != {
        "timeout_seconds", "max_records", "records_processed", "elapsed_ms", "completed_within_bounds"
    }:
        raise BenchmarkError("Report has an invalid bounds object.")
    for key in ("timeout_seconds", "elapsed_ms"):
        if isinstance(bounds[key], bool) or not isinstance(bounds[key], (int, float)) or not math.isfinite(float(bounds[key])):
            raise BenchmarkError(f"Report has an invalid {key} bound.")
    if not 0.1 <= float(bounds["timeout_seconds"]) <= HARD_TIMEOUT_SECONDS or float(bounds["elapsed_ms"]) < 0:
        raise BenchmarkError("Report bounds are out of range.")
    for key in ("max_records", "records_processed"):
        if isinstance(bounds[key], bool) or not isinstance(bounds[key], int):
            raise BenchmarkError(f"Report has an invalid {key} bound.")
    if not 1 <= bounds["max_records"] <= HARD_MAX_RECORDS:
        raise BenchmarkError("Report max_records is out of range.")
    if not len(METRICS) <= bounds["records_processed"] <= bounds["max_records"]:
        raise BenchmarkError("Report records_processed is out of range.")
    if bounds["completed_within_bounds"] is not True or bounds["elapsed_ms"] > bounds["timeout_seconds"] * 1000.0:
        raise BenchmarkError("Report did not complete within its declared bounds.")
    if source["records"] != bounds["records_processed"]:
        raise BenchmarkError("Report source and bound record counts disagree.")

    machine = report.get("machine")
    if not isinstance(machine, dict) or set(machine) != {
        "operating_system", "architecture", "python_version", "logical_processor_count", "hostname_recorded", "environment_variables_recorded"
    }:
        raise BenchmarkError("Report has an invalid machine object.")
    for key in ("operating_system", "architecture", "python_version"):
        if not isinstance(machine[key], str) or not 1 <= len(machine[key]) <= 64:
            raise BenchmarkError(f"Report has an invalid machine {key}.")
    if not re.match(r"^\d+\.\d+\.\d+", machine["python_version"]):
        raise BenchmarkError("Report has an invalid Python version.")
    if isinstance(machine["logical_processor_count"], bool) or not isinstance(machine["logical_processor_count"], int) or machine["logical_processor_count"] < 1:
        raise BenchmarkError("Report has an invalid logical processor count.")
    if machine["hostname_recorded"] is not False or machine["environment_variables_recorded"] is not False:
        raise BenchmarkError("Report violates machine privacy invariants.")

    expected_eligible = (
        classification["execution_mode"] == "live"
        and classification["measurement_kind"] == "measured"
        and machine["operating_system"] == "Windows"
    )
    if classification["acceptance_eligible"] is not expected_eligible:
        raise BenchmarkError("Report acceptance eligibility conflicts with its classification or machine.")

    metrics = report.get("metrics")
    if not isinstance(metrics, list) or len(metrics) != len(METRICS):
        raise BenchmarkError("Report does not contain exactly one result for every required metric.")
    names = {metric.get("name") for metric in metrics if isinstance(metric, dict)}
    if names != set(METRIC_BY_NAME):
        raise BenchmarkError("Report metric names do not match the required metric set.")
    for metric in metrics:
        expected_metric_keys = {
            "name", "unit", "description", "sample_count", "min", "max", "mean", "p50", "p95", "p99", "execution_mode", "measurement_kind"
        }
        if not isinstance(metric, dict) or set(metric) != expected_metric_keys:
            raise BenchmarkError("Report has an invalid metric object.")
        definition = METRIC_BY_NAME[metric["name"]]
        if metric["unit"] != definition.unit or metric["description"] != definition.description:
            raise BenchmarkError(f"Metric {definition.name} has an invalid unit or definition.")
        if metric["execution_mode"] != classification["execution_mode"] or metric["measurement_kind"] != classification["measurement_kind"]:
            raise BenchmarkError(f"Metric {definition.name} classification conflicts with the report.")
        if isinstance(metric["sample_count"], bool) or not isinstance(metric["sample_count"], int) or not 1 <= metric["sample_count"] <= HARD_MAX_RECORDS:
            raise BenchmarkError(f"Metric {definition.name} has an invalid sample_count.")
        for key in ("min", "max", "mean", "p50", "p95", "p99"):
            value = metric.get(key)
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(float(value)) or float(value) < 0:
                raise BenchmarkError(f"Metric {metric.get('name', '<unknown>')} has an invalid {key}.")
        if not metric["min"] <= metric["p50"] <= metric["p95"] <= metric["p99"] <= metric["max"]:
            raise BenchmarkError(f"Metric {definition.name} percentile ordering is invalid.")
        if not metric["min"] <= metric["mean"] <= metric["max"]:
            raise BenchmarkError(f"Metric {definition.name} mean is outside its range.")


def command_summarize(args: argparse.Namespace) -> int:
    rows, digest, size, elapsed_ms = read_jsonl(
        Path(args.input),
        max_records=args.max_records,
        max_input_bytes=args.max_input_bytes,
        timeout_seconds=args.timeout_seconds,
    )
    report = summarize_rows(
        rows,
        report_id=args.report_id,
        source_sha256=digest,
        source_bytes=size,
        timeout_seconds=args.timeout_seconds,
        max_records=args.max_records,
        elapsed_ms=elapsed_ms,
    )
    validate_report_shape(report)
    write_json_atomic(Path(args.output), report)
    print(f"Wrote benchmark report: {args.output}")
    return 0


def command_simulate(args: argparse.Namespace) -> int:
    started = time.monotonic()
    _validate_bounds(args.max_records, DEFAULT_MAX_INPUT_BYTES, args.timeout_seconds)
    required_records = args.iterations * len(METRICS)
    if required_records > args.max_records:
        raise BenchmarkError(f"Simulation would exceed the {args.max_records}-record limit.")
    rows = deterministic_simulation(args.iterations, args.seed, args.observed_at_utc)
    elapsed_ms = (time.monotonic() - started) * 1000.0
    if elapsed_ms > args.timeout_seconds * 1000.0:
        raise BenchmarkError(f"Simulation exceeded the {args.timeout_seconds:g}-second limit.")
    digest, size = _rows_digest(rows)
    report = summarize_rows(
        rows,
        report_id=args.report_id,
        source_sha256=digest,
        source_bytes=size,
        timeout_seconds=args.timeout_seconds,
        max_records=args.max_records,
        elapsed_ms=elapsed_ms,
    )
    validate_report_shape(report)
    write_json_atomic(Path(args.output), report)
    print(f"Wrote simulated benchmark report: {args.output}")
    return 0


def command_validate(args: argparse.Namespace) -> int:
    try:
        with Path(args.input).open("r", encoding="utf-8") as stream:
            report = json.load(stream, object_pairs_hook=strict_json_object)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise BenchmarkError("Report could not be read as UTF-8 JSON.") from exc
    validate_report_shape(report)
    print("Benchmark report shape is valid.")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Create bounded, aggregate-only Interactive NPC benchmark evidence.")
    subcommands = parser.add_subparsers(dest="command", required=True)

    summarize = subcommands.add_parser("summarize", help="Aggregate strict JSONL observations into a v1 report.")
    summarize.add_argument("--input", required=True, help="UTF-8 JSONL sample file. Provider payloads are forbidden.")
    summarize.add_argument("--output", required=True, help="Destination report JSON path.")
    summarize.add_argument("--report-id", required=True, help="Non-secret identifier (letters, digits, dot, colon, underscore, hyphen).")
    summarize.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    summarize.add_argument("--max-records", type=int, default=DEFAULT_MAX_RECORDS)
    summarize.add_argument("--max-input-bytes", type=int, default=DEFAULT_MAX_INPUT_BYTES)
    summarize.set_defaults(handler=command_summarize)

    simulate = subcommands.add_parser("simulate", help="Generate deterministic simulated measurements for harness checks only.")
    simulate.add_argument("--output", required=True, help="Destination report JSON path.")
    simulate.add_argument("--report-id", default="deterministic-simulation")
    simulate.add_argument("--iterations", type=int, default=100)
    simulate.add_argument("--seed", type=int, default=20260830)
    simulate.add_argument("--observed-at-utc", default="2026-08-30T00:00:00Z")
    simulate.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    simulate.add_argument("--max-records", type=int, default=DEFAULT_MAX_RECORDS)
    simulate.set_defaults(handler=command_simulate)

    validate = subcommands.add_parser("validate", help="Validate the strict shape of an existing v1 report.")
    validate.add_argument("--input", required=True)
    validate.set_defaults(handler=command_validate)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.handler(args))
    except BenchmarkError as exc:
        print(f"benchmark error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
