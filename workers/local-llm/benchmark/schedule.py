"""Deterministic balanced schedules and measured-distribution helpers."""

from __future__ import annotations

from collections import Counter
from typing import Iterable

from npc_local_llm.errors import LocalLlmError
from npc_local_llm.measurements import distribution_millis


BACKENDS = ("vulkan", "cuda")


def abba_schedule(samples_per_backend: int) -> tuple[str, ...]:
    """Return alternating ABBA/BAAB blocks with exactly N runs per backend."""
    if samples_per_backend < 4 or samples_per_backend % 4:
        raise LocalLlmError(
            "invalid_benchmark_schedule",
            "samples per backend must be a positive multiple of four and at least four",
        )
    schedule: list[str] = []
    pairs = samples_per_backend // 4
    for _ in range(pairs):
        schedule.extend(("vulkan", "cuda", "cuda", "vulkan"))
        schedule.extend(("cuda", "vulkan", "vulkan", "cuda"))
    counts = Counter(schedule)
    if counts != Counter({"vulkan": samples_per_backend, "cuda": samples_per_backend}):
        raise AssertionError("balanced schedule construction failed")
    return tuple(schedule)


def warmup_schedule(warmups_per_backend: int) -> tuple[str, ...]:
    if warmups_per_backend != 2:
        raise LocalLlmError("invalid_benchmark_schedule", "the frozen warmup contract requires two runs per backend")
    return ("vulkan", "cuda", "cuda", "vulkan")


def measured_distribution(values: Iterable[float], *, minimum_samples: int = 20) -> dict[str, float | int | None]:
    samples = list(values)
    if len(samples) < minimum_samples:
        raise LocalLlmError(
            "insufficient_benchmark_samples",
            f"p99 reporting requires at least {minimum_samples} measured observations",
        )
    return distribution_millis(samples)

