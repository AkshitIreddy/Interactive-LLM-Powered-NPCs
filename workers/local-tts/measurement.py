"""Content-free qualification hooks for the optional local TTS pack.

This module summarizes observations supplied by an external benchmark
supervisor.  It does not launch the worker, inspect hardware, sign evidence, or
create a QualifiedResourceEnvelopeV1.  The Model Manager remains the sole trust
boundary for device binding, expiry, signature verification, and admission.
"""

from __future__ import annotations

import hashlib
import math
from dataclasses import dataclass
from typing import Iterable, Literal

REPORT_SCHEMA = "npc.local-tts.measurement/v1"
SUITE_REVISION = "kokoro-cpu-qualification/1"
MINIMUM_SAMPLES = 20
Phase = Literal["load", "reload", "synthesis", "cancel"]


class MeasurementError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class Observation:
    phase: Phase
    duration_millis: float
    process_rss_bytes: int
    resident_vram_bytes: int = 0
    workspace_vram_bytes: int = 0
    voice_id: str | None = None
    text_profile: str | None = None
    first_pcm_millis: float | None = None
    audio_duration_millis: float | None = None

    def validate(self) -> None:
        if self.phase not in {"load", "reload", "synthesis", "cancel"}:
            raise MeasurementError("unknown measurement phase")
        if not math.isfinite(self.duration_millis) or self.duration_millis <= 0:
            raise MeasurementError("duration must be positive and finite")
        if self.process_rss_bytes <= 0:
            raise MeasurementError("process RSS telemetry is required")
        if self.resident_vram_bytes != 0 or self.workspace_vram_bytes != 0:
            raise MeasurementError("the pinned local TTS placement is CPU-only")
        if self.phase == "synthesis":
            if not self.voice_id or not self.text_profile:
                raise MeasurementError("synthesis observations need voice/profile IDs")
            if (
                self.first_pcm_millis is None
                or not math.isfinite(self.first_pcm_millis)
                or self.first_pcm_millis <= 0
                or self.first_pcm_millis > self.duration_millis
            ):
                raise MeasurementError("first PCM latency is missing or invalid")
            if (
                self.audio_duration_millis is None
                or not math.isfinite(self.audio_duration_millis)
                or self.audio_duration_millis <= 0
            ):
                raise MeasurementError("audio duration is missing or invalid")
        elif (
            self.voice_id is not None
            or self.text_profile is not None
            or self.first_pcm_millis is not None
            or self.audio_duration_millis is not None
        ):
            raise MeasurementError("non-synthesis observations cannot carry text/audio data")


def _percentile(values: Iterable[float], fraction: float) -> float:
    ordered = sorted(values)
    if not ordered:
        raise MeasurementError("cannot summarize an empty distribution")
    if not 0 <= fraction <= 1:
        raise MeasurementError("percentile must be within zero and one")
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return float(ordered[lower])
    weight = position - lower
    return float(ordered[lower] * (1.0 - weight) + ordered[upper] * weight)


def _p99_millis(values: Iterable[float]) -> int:
    return max(1, math.ceil(_percentile(values, 0.99)))


def _distribution(values: list[float]) -> dict[str, float]:
    return {
        "p50": _percentile(values, 0.50),
        "p95": _percentile(values, 0.95),
        "p99": _percentile(values, 0.99),
    }


def build_unsigned_report(
    *,
    pack_id: str,
    revision: str,
    manifest_sha256: str,
    runtime_revision: str,
    observations: Iterable[Observation],
) -> dict[str, object]:
    """Build a review artifact only after a complete real qualification run.

    At least 20 independent load, reload, and synthesis observations are
    required.  Reload is deliberately separate from first load because the
    residency governor uses measured reload cost for eviction decisions.
    """

    if not pack_id or not revision or not runtime_revision:
        raise MeasurementError("immutable pack/runtime identity is required")
    if (
        len(manifest_sha256) != 64
        or any(character not in "0123456789abcdef" for character in manifest_sha256)
    ):
        raise MeasurementError("manifest SHA-256 is invalid")
    rows = list(observations)
    for row in rows:
        row.validate()
    grouped = {
        phase: [row for row in rows if row.phase == phase]
        for phase in ("load", "reload", "synthesis", "cancel")
    }
    for phase in ("load", "reload", "synthesis"):
        if len(grouped[phase]) < MINIMUM_SAMPLES:
            raise MeasurementError(
                f"{phase} requires at least {MINIMUM_SAMPLES} observations"
            )
    if not grouped["cancel"]:
        raise MeasurementError("at least one cancellation observation is required")
    synthesis = grouped["synthesis"]
    assert synthesis
    voices = {row.voice_id for row in synthesis}
    profiles = {row.text_profile for row in synthesis}
    if len(voices) < 3 or not {"short", "medium", "long"}.issubset(profiles):
        raise MeasurementError(
            "qualification must cover three voices and short/medium/long text"
        )

    load_millis = [row.duration_millis for row in grouped["load"]]
    reload_millis = [row.duration_millis for row in grouped["reload"]]
    operation_millis = [row.duration_millis for row in synthesis]
    first_pcm_millis = [
        row.first_pcm_millis for row in synthesis if row.first_pcm_millis is not None
    ]
    realtime_factors = [
        row.duration_millis / row.audio_duration_millis
        for row in synthesis
        if row.audio_duration_millis is not None
    ]
    rss = [row.process_rss_bytes for row in rows]
    report_identity = (
        f"{pack_id}\n{revision}\n{manifest_sha256}\n{runtime_revision}\n"
        f"{len(rows)}\n{max(rss)}"
    ).encode("utf-8")
    return {
        "schema": REPORT_SCHEMA,
        "suite_revision": SUITE_REVISION,
        "report_content_id": hashlib.sha256(report_identity).hexdigest(),
        "identity": {"pack_id": pack_id, "revision": revision},
        "manifest_sha256": manifest_sha256,
        "capability": "tts",
        "runtime": "sherpa-onnx-c-api",
        "runtime_revision": runtime_revision,
        "backend": "cpu",
        "sample_count": min(
            len(grouped["load"]),
            len(grouped["reload"]),
            len(synthesis),
        ),
        "placement_projection": {
            "residency_mode": "cpu_resident",
            "resident_ram_bytes": min(
                row.process_rss_bytes for row in synthesis
            ),
            "p99_total_ram_bytes": math.ceil(_percentile(rss, 0.99)),
            "resident_vram_bytes": 0,
            "p99_workspace_vram_bytes": 0,
            "p99_load_millis": _p99_millis(load_millis),
            "p99_reload_millis": _p99_millis(reload_millis),
            "p99_operation_millis": _p99_millis(operation_millis),
        },
        "latency": {
            "load_millis": _distribution(load_millis),
            "reload_millis": _distribution(reload_millis),
            "operation_millis": _distribution(operation_millis),
            "first_pcm_millis": _distribution(first_pcm_millis),
            "realtime_factor": _distribution(realtime_factors),
            "cancel_ack_millis": _distribution(
                [row.duration_millis for row in grouped["cancel"]]
            ),
        },
        "coverage": {
            "voice_ids": sorted(voice for voice in voices if voice is not None),
            "text_profiles": sorted(
                profile for profile in profiles if profile is not None
            ),
        },
        "admission": {
            "qualified_resource_envelope_created": False,
            "admission_allowed": False,
            "reason": (
                "Unsigned worker observations are not trusted admission evidence. "
                "Model Manager must bind the current device, game reserve, "
                "validity window, monotonic sequence, and trusted signature."
            ),
            "outer_probe_required": [
                "installed_bytes",
                "device_fingerprint_sha256",
                "game_and_desktop_memory_pressure",
                "game_frame_time_baseline_and_active",
                "measurement_validity_window",
                "measurement_signature",
            ],
        },
    }

