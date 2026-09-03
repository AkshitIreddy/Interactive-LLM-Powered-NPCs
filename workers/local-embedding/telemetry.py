"""Content-free resource and latency telemetry for the embedding worker."""

from __future__ import annotations

import ctypes
import os
import platform
import statistics
import time
from collections import deque
from dataclasses import dataclass
from typing import Iterable


def _rss_bytes() -> int | None:
    if os.name == "nt":
        try:
            from ctypes import wintypes

            class PROCESS_MEMORY_COUNTERS_EX(ctypes.Structure):
                _fields_ = [
                    ("cb", wintypes.DWORD),
                    ("PageFaultCount", wintypes.DWORD),
                    ("PeakWorkingSetSize", ctypes.c_size_t),
                    ("WorkingSetSize", ctypes.c_size_t),
                    ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                    ("PagefileUsage", ctypes.c_size_t),
                    ("PeakPagefileUsage", ctypes.c_size_t),
                    ("PrivateUsage", ctypes.c_size_t),
                ]

            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            psapi = ctypes.WinDLL("psapi", use_last_error=True)
            kernel32.GetCurrentProcess.argtypes = []
            kernel32.GetCurrentProcess.restype = wintypes.HANDLE
            psapi.GetProcessMemoryInfo.argtypes = [
                wintypes.HANDLE,
                ctypes.POINTER(PROCESS_MEMORY_COUNTERS_EX),
                wintypes.DWORD,
            ]
            psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
            counters = PROCESS_MEMORY_COUNTERS_EX()
            counters.cb = ctypes.sizeof(counters)
            process = kernel32.GetCurrentProcess()
            ok = psapi.GetProcessMemoryInfo(process, ctypes.byref(counters), counters.cb)
            return int(counters.WorkingSetSize) if ok else None
        except (AttributeError, OSError, ValueError):
            return None
    try:
        with open("/proc/self/statm", "r", encoding="ascii") as handle:
            resident_pages = int(handle.read().split()[1])
        return resident_pages * os.sysconf("SC_PAGE_SIZE")
    except (OSError, ValueError, IndexError):
        return None


def _cpu_time_ns() -> int:
    return time.process_time_ns()


def percentile(samples: Iterable[float], quantile: float) -> float | None:
    values = sorted(float(sample) for sample in samples)
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    position = (len(values) - 1) * quantile
    lower = int(position)
    upper = min(lower + 1, len(values) - 1)
    fraction = position - lower
    return values[lower] + (values[upper] - values[lower]) * fraction


@dataclass(frozen=True, slots=True)
class ResourceSnapshot:
    monotonic_ns: int
    cpu_time_ns: int
    rss_bytes: int | None


class Telemetry:
    """Bounded measurements with no prompt text, vectors, paths, or identities."""

    def __init__(self, sample_capacity: int = 512) -> None:
        self._load_millis: float | None = None
        self._latencies_ms: deque[float] = deque(maxlen=sample_capacity)
        self._queue_wait_ms: deque[float] = deque(maxlen=sample_capacity)
        self._batch_sizes: deque[int] = deque(maxlen=sample_capacity)
        self._peak_rss_bytes: int | None = _rss_bytes()
        self._inference_cpu_ms: deque[float] = deque(maxlen=sample_capacity)
        self._completed_items = 0
        self._cancelled_items = 0
        self._failed_items = 0

    @staticmethod
    def snapshot() -> ResourceSnapshot:
        return ResourceSnapshot(time.monotonic_ns(), _cpu_time_ns(), _rss_bytes())

    def record_load(self, started_ns: int) -> None:
        self._load_millis = (time.monotonic_ns() - started_ns) / 1_000_000.0
        self._sample_rss()

    def record_batch(
        self,
        started: ResourceSnapshot,
        *,
        queue_wait_ms: float,
        batch_size: int,
        completed_items: int,
    ) -> None:
        ended = self.snapshot()
        self._latencies_ms.append((ended.monotonic_ns - started.monotonic_ns) / 1_000_000.0)
        self._inference_cpu_ms.append((ended.cpu_time_ns - started.cpu_time_ns) / 1_000_000.0)
        self._queue_wait_ms.append(queue_wait_ms)
        self._batch_sizes.append(batch_size)
        self._completed_items += completed_items
        self._sample_rss(ended.rss_bytes)

    def record_cancelled(self, items: int) -> None:
        self._cancelled_items += items

    def record_failed(self, items: int) -> None:
        self._failed_items += items

    def _sample_rss(self, value: int | None = None) -> None:
        current = _rss_bytes() if value is None else value
        if current is not None:
            self._peak_rss_bytes = max(self._peak_rss_bytes or 0, current)

    def report(self, *, backend: str | None, loaded: bool, current_rss_bytes: int | None = None) -> dict[str, object]:
        self._sample_rss(current_rss_bytes)
        latency = list(self._latencies_ms)
        queue = list(self._queue_wait_ms)
        batch = list(self._batch_sizes)
        cpu = list(self._inference_cpu_ms)
        return {
            "schema": "npc.embedding-resource-report/v1",
            "provenance": "measured_worker_process",
            "platform": platform.system().lower(),
            "backend": backend,
            "loaded": loaded,
            "ram": {
                "rss_bytes": _rss_bytes(),
                "peak_observed_rss_bytes": self._peak_rss_bytes,
                "measurement": "process_working_set",
            },
            "vram": {
                "resident_bytes": 0 if backend == "cpu" else None,
                "peak_transient_bytes": 0 if backend == "cpu" else None,
                "measurement": "backend_invariant" if backend == "cpu" else "supervisor_telemetry_required",
            },
            "load_millis": self._load_millis,
            "latency_millis": {
                "sample_count": len(latency),
                "p50": percentile(latency, 0.50),
                "p95": percentile(latency, 0.95),
                "p99": percentile(latency, 0.99),
            },
            "queue_wait_millis": {
                "p50": percentile(queue, 0.50),
                "p95": percentile(queue, 0.95),
                "p99": percentile(queue, 0.99),
            },
            "cpu_millis": {
                "p50": percentile(cpu, 0.50),
                "p95": percentile(cpu, 0.95),
                "p99": percentile(cpu, 0.99),
            },
            "batch": {
                "sample_count": len(batch),
                "mean_items": statistics.fmean(batch) if batch else None,
                "maximum_items": max(batch) if batch else None,
            },
            "counters": {
                "completed_items": self._completed_items,
                "cancelled_items": self._cancelled_items,
                "failed_items": self._failed_items,
            },
        }
