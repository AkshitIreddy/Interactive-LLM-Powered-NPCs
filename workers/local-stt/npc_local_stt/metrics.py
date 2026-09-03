from __future__ import annotations

from dataclasses import dataclass, field
import ctypes
import os
from pathlib import Path
import statistics
import time
from typing import Any


def process_rss_bytes() -> int | None:
    if os.name == "nt":
        try:
            class PROCESS_MEMORY_COUNTERS_EX(ctypes.Structure):
                _fields_ = [
                    ("cb", ctypes.c_ulong),
                    ("PageFaultCount", ctypes.c_ulong),
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

            counters = PROCESS_MEMORY_COUNTERS_EX()
            counters.cb = ctypes.sizeof(counters)
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            psapi = ctypes.WinDLL("psapi", use_last_error=True)
            kernel32.GetCurrentProcess.argtypes = []
            kernel32.GetCurrentProcess.restype = ctypes.c_void_p
            psapi.GetProcessMemoryInfo.argtypes = [
                ctypes.c_void_p,
                ctypes.POINTER(PROCESS_MEMORY_COUNTERS_EX),
                ctypes.c_ulong,
            ]
            psapi.GetProcessMemoryInfo.restype = ctypes.c_int
            if psapi.GetProcessMemoryInfo(
                kernel32.GetCurrentProcess(), ctypes.byref(counters), counters.cb
            ):
                return int(counters.WorkingSetSize)
        except (AttributeError, OSError):
            return None
        return None
    statm = Path("/proc/self/statm")
    try:
        resident_pages = int(statm.read_text(encoding="ascii").split()[1])
        return resident_pages * os.sysconf("SC_PAGE_SIZE")
    except (OSError, ValueError, IndexError, AttributeError):
        return None


@dataclass
class MetricsTracker:
    evidence_class: str = "runtime_hook"
    started_monotonic: float = field(default_factory=time.monotonic)
    started_cpu: float = field(default_factory=time.process_time)
    initial_rss_bytes: int | None = field(default_factory=process_rss_bytes)
    peak_rss_bytes: int | None = None
    load_durations_ms: list[float] = field(default_factory=list)
    inference_durations_ms: list[float] = field(default_factory=list)
    analyzed_audio_ms: int = 0
    first_pcm_at: float | None = None
    first_partial_at: float | None = None
    session_end_at: float | None = None
    final_at: float | None = None
    pcm_frames: int = 0
    pcm_samples: int = 0
    dropped_pcm_frames: int = 0
    completed_utterances: int = 0

    def sample_memory(self) -> None:
        current = process_rss_bytes()
        if current is not None:
            self.peak_rss_bytes = max(self.peak_rss_bytes or 0, current)

    def record_load(self, duration_ms: float) -> None:
        self.load_durations_ms.append(duration_ms)
        self.sample_memory()

    def record_pcm(self, samples: int) -> None:
        if self.first_pcm_at is None:
            self.first_pcm_at = time.monotonic()
        self.pcm_frames += 1
        self.pcm_samples += samples
        self.sample_memory()

    def record_delta(self, inference_ms: float, analyzed_audio_ms: int, *, partial: bool, final: bool) -> None:
        if inference_ms >= 0:
            self.inference_durations_ms.append(inference_ms)
        self.analyzed_audio_ms += max(0, analyzed_audio_ms)
        now = time.monotonic()
        if partial and self.first_partial_at is None:
            self.first_partial_at = now
        if final:
            self.final_at = now
            self.completed_utterances += 1
        self.sample_memory()

    def mark_session_end(self) -> None:
        self.session_end_at = time.monotonic()

    @staticmethod
    def _percentile(values: list[float], percentile: float) -> float | None:
        if not values:
            return None
        ordered = sorted(values)
        rank = (len(ordered) - 1) * percentile
        lower = int(rank)
        upper = min(len(ordered) - 1, lower + 1)
        fraction = rank - lower
        return ordered[lower] + (ordered[upper] - ordered[lower]) * fraction

    def snapshot(self) -> dict[str, Any]:
        now = time.monotonic()
        rss = process_rss_bytes()
        cpu_seconds = time.process_time() - self.started_cpu
        wall_seconds = max(0.000001, now - self.started_monotonic)
        first_partial_ms = None
        if self.first_pcm_at is not None and self.first_partial_at is not None:
            first_partial_ms = (self.first_partial_at - self.first_pcm_at) * 1000
        final_after_end_ms = None
        if self.session_end_at is not None and self.final_at is not None:
            final_after_end_ms = max(0.0, (self.final_at - self.session_end_at) * 1000)
        inference_total = sum(self.inference_durations_ms)
        rtf = inference_total / self.analyzed_audio_ms if self.analyzed_audio_ms else None
        return {
            "evidenceClass": self.evidence_class,
            "process": {
                "rssBytes": rss,
                "initialRssBytes": self.initial_rss_bytes,
                "peakRssBytes": self.peak_rss_bytes,
                "cpuSeconds": cpu_seconds,
                "wallSeconds": wall_seconds,
                "averageCpuPercentOneCore": 100.0 * cpu_seconds / wall_seconds,
            },
            "gpu": {
                "backendPolicy": "cpu_only",
                "loadedVramMiB": None,
                "peakVramMiB": None,
                "telemetrySource": "policy_requires_external_dxgi_or_nvml_confirmation",
            },
            "loadMs": {
                "last": self.load_durations_ms[-1] if self.load_durations_ms else None,
                "p50": self._percentile(self.load_durations_ms, 0.50),
                "p95": self._percentile(self.load_durations_ms, 0.95),
                "p99": self._percentile(self.load_durations_ms, 0.99),
            },
            "inferenceMs": {
                "count": len(self.inference_durations_ms),
                "p50": self._percentile(self.inference_durations_ms, 0.50),
                "p95": self._percentile(self.inference_durations_ms, 0.95),
                "p99": self._percentile(self.inference_durations_ms, 0.99),
                "maximum": max(self.inference_durations_ms) if self.inference_durations_ms else None,
            },
            "latency": {
                "firstPartialAfterFirstPcmMs": first_partial_ms,
                "finalAfterSessionEndMs": final_after_end_ms,
                "realTimeFactor": rtf,
            },
            "counters": {
                "pcmFrames": self.pcm_frames,
                "pcmSamples": self.pcm_samples,
                "droppedPcmFrames": self.dropped_pcm_frames,
                "completedUtterances": self.completed_utterances,
            },
        }
