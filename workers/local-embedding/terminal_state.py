#!/usr/bin/env python3
"""Capture a content-free post-qualification terminal resource snapshot.

This probe is intentionally stdlib-only so it can run after the disposable
qualification virtual environment has been removed.  Every child process is
created without a Windows console window.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from datetime import datetime, timezone
from pathlib import Path


PROCESS_MARKERS = (
    "embedding-qualification\\venv",
    "workers\\local-embedding\\qualify.py",
    "workers/local-embedding/qualify.py",
    "workers\\local-embedding\\worker.py",
    "workers/local-embedding/worker.py",
)
GPU_MARKERS = ("python", "onnx", "embedding", "qualify.py", "worker.py")


def _run(command: list[str]) -> subprocess.CompletedProcess[str]:
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    if os.name != "nt" or flags == 0:
        raise RuntimeError("CREATE_NO_WINDOW is required for terminal-state probing")
    return subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
        creationflags=flags,
        close_fds=True,
    )


def _processes() -> tuple[list[dict[str, object]], str | None]:
    script = (
        "$ErrorActionPreference='Stop';"
        "Get-CimInstance Win32_Process | "
        "Select-Object ProcessId,Name,CommandLine | ConvertTo-Json -Compress"
    )
    completed = _run(
        ["powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script]
    )
    if completed.returncode != 0:
        return [], completed.stderr.strip() or f"powershell exit {completed.returncode}"
    payload = json.loads(completed.stdout or "[]")
    if isinstance(payload, dict):
        payload = [payload]
    matches: list[dict[str, object]] = []
    for process in payload:
        command_line = str(process.get("CommandLine") or "")
        lowered = command_line.lower()
        if any(marker.lower() in lowered for marker in PROCESS_MARKERS):
            matches.append(
                {
                    "pid": int(process["ProcessId"]),
                    "name": str(process.get("Name") or ""),
                }
            )
    return matches, None


def _gpu_processes() -> tuple[list[dict[str, object]], str | None]:
    completed = _run(
        [
            "nvidia-smi.exe",
            "--query-compute-apps=pid,process_name,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ]
    )
    if completed.returncode != 0:
        return [], completed.stderr.strip() or f"nvidia-smi exit {completed.returncode}"
    matches: list[dict[str, object]] = []
    for line in completed.stdout.splitlines():
        fields = [field.strip() for field in line.split(",")]
        if len(fields) < 3:
            continue
        name = fields[1]
        if any(marker in name.lower() for marker in GPU_MARKERS):
            memory = fields[2]
            matches.append(
                {
                    "pid": int(fields[0]),
                    "process_name": name,
                    "used_gpu_memory_mib": None if memory == "[N/A]" else int(memory),
                }
            )
    return matches, None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qualification-root", type=Path, required=True)
    parser.add_argument("--lock-file", type=Path, required=True)
    args = parser.parse_args()

    process_matches, process_error = _processes()
    gpu_matches, gpu_error = _gpu_processes()
    lock_state = args.lock_file.read_text(encoding="utf-8-sig").strip().lower()
    root_exists = args.qualification_root.exists()
    clean = (
        process_error is None
        and gpu_error is None
        and not process_matches
        and not gpu_matches
        and not root_exists
        and lock_state == "yes"
    )
    report = {
        "schema": "npc.embedding-terminal-state/v1",
        "captured_at_utc": datetime.now(timezone.utc).isoformat(),
        "headless_windows_probe": True,
        "qualification_root": str(args.qualification_root.resolve()),
        "qualification_root_exists": root_exists,
        "qualification_model_or_worker_processes": process_matches,
        "process_probe_error": process_error,
        "embedding_python_or_ort_compute_processes": gpu_matches,
        "gpu_probe_error": gpu_error,
        "cpu_backend_resident_vram_bytes": 0,
        "cpu_backend_workspace_vram_bytes": 0,
        "coordination_lock_observed": lock_state,
        "coordination_lock_modified_by_probe": False,
        "root_may_restore_lock_to_no": clean,
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if clean else 5


if __name__ == "__main__":
    raise SystemExit(main())
