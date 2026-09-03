"""Unsigned local-review measurements; never self-promote to admission evidence."""

from __future__ import annotations

import ctypes
import json
import math
import os
import statistics
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from .constants import MEASUREMENT_SCHEMA, MODEL_ID, RUNTIME_ABI, RUNTIME_COMMIT, RUNTIME_RELEASE
from .errors import LocalLlmError
from .windows_job import CREATE_NO_WINDOW


def percentile(values: Iterable[float], percentile_value: float) -> float | None:
    ordered = sorted(values)
    if not ordered:
        return None
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * percentile_value
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def distribution_millis(values: Iterable[float]) -> dict[str, float | int | None]:
    samples = list(values)
    return {
        "sample_count": len(samples),
        "p50": percentile(samples, 0.50),
        "p95": percentile(samples, 0.95),
        "p99": percentile(samples, 0.99),
    }


def nvidia_smi_snapshot(label: str) -> dict[str, Any]:
    command = [
        "nvidia-smi",
        "--query-gpu=index,name,uuid,memory.total,memory.used,memory.free,utilization.gpu,pstate",
        "--format=csv,noheader,nounits",
    ]
    try:
        completed = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
            creationflags=CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise LocalLlmError("nvidia_smi_failed", "NVIDIA telemetry command failed") from error
    devices = []
    for line in completed.stdout.splitlines():
        fields = [field.strip() for field in line.split(",")]
        if len(fields) != 8:
            raise LocalLlmError("nvidia_smi_failed", "NVIDIA telemetry output shape is invalid")
        devices.append(
            {
                "index": int(fields[0]),
                "name": fields[1],
                "uuid": fields[2],
                "memory_total_mib": int(fields[3]),
                "memory_used_mib": int(fields[4]),
                "memory_free_mib": int(fields[5]),
                "utilization_gpu_percent": int(fields[6]),
                "pstate": fields[7],
            }
        )
    return {"label": label, "captured_unix_ms": int(time.time() * 1000), "devices": devices}


def windows_process_memory(pid: int) -> dict[str, int]:
    if os.name != "nt":
        raise LocalLlmError("unsupported_platform", "Windows process telemetry requires Windows")
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    PROCESS_VM_READ = 0x0010

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

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    kernel32.OpenProcess.restype = ctypes.c_void_p
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, False, pid)
    if not handle:
        raise LocalLlmError("process_telemetry_failed", "runtime process memory could not be read")
    try:
        counters = PROCESS_MEMORY_COUNTERS_EX()
        counters.cb = ctypes.sizeof(counters)
        if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
            raise LocalLlmError("process_telemetry_failed", "runtime process memory could not be read")
        return {
            "working_set_bytes": int(counters.WorkingSetSize),
            "peak_working_set_bytes": int(counters.PeakWorkingSetSize),
            "private_bytes": int(counters.PrivateUsage),
            "peak_pagefile_bytes": int(counters.PeakPagefileUsage),
        }
    finally:
        kernel32.CloseHandle(handle)


def local_review_report(
    *,
    backend: str,
    model_manifest_sha256: str,
    runtime_archive_sha256: str,
    load_millis: float,
    reload_millis: float,
    ttft_millis: float,
    total_millis: float,
    output_tokens: int | None,
    inter_token_millis: list[float],
    process_memory: dict[str, int],
    gpu_snapshots: list[dict[str, Any]],
    cancellation: dict[str, Any],
    checksum_and_license_verified: bool,
) -> dict[str, Any]:
    tokens_per_second = None
    if output_tokens is not None and output_tokens > 0 and total_millis > ttft_millis:
        tokens_per_second = output_tokens / ((total_millis - ttft_millis) / 1000.0)
    return {
        "schema": MEASUREMENT_SCHEMA,
        "provenance": "measured_local_review",
        "admissible_for_resource_governor": False,
        "non_admission_reasons": [
            "unsigned",
            "single-run qualification",
            "minimum 20-sample signed envelope not yet produced",
        ],
        "measured_unix_ms": int(time.time() * 1000),
        "pack_id": MODEL_ID,
        "manifest_sha256": model_manifest_sha256,
        "runtime": "llama.cpp",
        "runtime_abi": RUNTIME_ABI,
        "runtime_revision": f"{RUNTIME_RELEASE}@{RUNTIME_COMMIT}",
        "runtime_archive_sha256": runtime_archive_sha256,
        "backend": backend,
        "checksum_and_license_verified": checksum_and_license_verified,
        "latency_millis": {
            "load": load_millis,
            "reload": reload_millis,
            "time_to_first_token": ttft_millis,
            "total": total_millis,
            "inter_token": distribution_millis(inter_token_millis),
        },
        "throughput": {
            "output_tokens": output_tokens,
            "tokens_per_second": tokens_per_second,
        },
        "process_memory": process_memory,
        "gpu_snapshots": gpu_snapshots,
        "cancellation": cancellation,
    }
