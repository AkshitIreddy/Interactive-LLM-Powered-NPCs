#!/usr/bin/env python3
"""Run the already-attested MuseTalk 1.5 pack on a realistic NPC fixture.

This is a bounded Windows qualification harness, not an application runtime or
pack installer. Large weights and every generated artifact remain on E:\temp.
The shared GPU marker is acquired only after hosted API tests have completed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import threading
import time
from pathlib import Path


CREATE_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def run_hidden(arguments: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        stdin=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
        errors="replace",
        creationflags=CREATE_NO_WINDOW,
        **kwargs,
    )


def monitor_gpu(stop: threading.Event, samples: list[dict[str, int]]) -> None:
    executable = Path(os.environ.get("WINDIR", r"C:\Windows")) / "System32/nvidia-smi.exe"
    while not stop.wait(0.25):
        try:
            result = run_hidden(
                [
                    str(executable),
                    "--query-gpu=memory.used,utilization.gpu",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                timeout=5,
                check=False,
            )
            values = [int(value.strip()) for value in result.stdout.split(",")]
            if result.returncode == 0 and len(values) == 2:
                samples.append({"memory_used_mib": values[0], "gpu_utilization_percent": values[1]})
        except (OSError, ValueError, subprocess.SubprocessError):
            pass


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--portrait", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--template-job", type=Path, required=True)
    parser.add_argument(
        "--gpu-marker",
        type=Path,
        default=Path(r"C:\Users\akshi\Desktop\Code Palace\gpu use.txt"),
    )
    args = parser.parse_args()

    for value, label in ((args.portrait, "portrait"), (args.audio, "audio"), (args.template_job, "template job")):
        if not value.is_file() or value.is_symlink():
            raise RuntimeError(f"{label} must be a regular non-symlink file")
    if args.output_root.exists():
        raise RuntimeError("output root already exists; use a fresh qualification directory")

    marker_before = args.gpu_marker.read_text(encoding="utf-8").strip().lower()
    if marker_before != "no":
        raise RuntimeError(f"GPU lease unavailable: marker is {marker_before!r}")

    inputs = args.output_root / "inputs"
    output = args.output_root / "output"
    workspace = args.output_root / "workspace"
    for directory in (inputs, output, workspace):
        directory.mkdir(parents=True)
    # The attested adapter intentionally allowlists the exact upstream command
    # shape derived from these neutral basenames.
    portrait = inputs / "portrait.png"
    audio = inputs / "narration.wav"
    shutil.copyfile(args.portrait, portrait)
    shutil.copyfile(args.audio, audio)

    job = json.loads(args.template_job.read_text(encoding="utf-8"))
    job["sceneId"] = "npc2-mara-venn-realistic-qualification"
    job["profileId"] = "presenter-portrait.mara-venn-v1"
    job["seed"] = 20260901
    job["gpuLease"] = {
        "deviceId": "cuda:0",
        "leaseId": "npc2-musetalk-mara-venn-v1",
        "mutexName": r"global\interactive-npcs-musetalk-qualification",
        "owner": "Interactive NPCs 2.0 realistic fixture qualification",
        "vramBytes": 10 * 1024 * 1024 * 1024,
    }
    job["inputs"]["portrait"] = {
        "mediaType": "image/png",
        "path": str(portrait),
        "sha256": sha256(portrait),
    }
    job["inputs"]["audio"] = {
        "mediaType": "audio/wav",
        "path": str(audio),
        "sha256": sha256(audio),
    }
    video = output / "musetalk-mara-venn-v1.mp4"
    progress = args.output_root / "progress.ndjson"
    job["output"] = {"mediaType": "video/mp4", "path": str(video)}
    job["progress"] = {
        "mediaType": "application/x-ndjson",
        "path": str(progress),
        "schemaVersion": 1,
    }
    job_path = args.output_root / "job.json"
    job_path.write_text(json.dumps(job, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    python = args.runtime_root / "venv/Scripts/python.exe"
    worker = args.runtime_root / "worker/local_presenter_worker.py"
    ffprobe = args.runtime_root / "bin/ffprobe.exe"
    for value in (python, worker, ffprobe):
        if not value.is_file():
            raise RuntimeError(f"attested runtime input is missing: {value}")

    environment = os.environ.copy()
    environment.update(
        {
            "PYTHONUTF8": "1",
            "PYTHONUNBUFFERED": "1",
            "TEMP": str(args.output_root / "temp"),
            "TMP": str(args.output_root / "temp"),
            "TORCH_HOME": str(args.output_root / "cache/torch"),
            "HF_HOME": str(args.output_root / "cache/huggingface"),
            "HUGGINGFACE_HUB_CACHE": str(args.output_root / "cache/huggingface/hub"),
            "XDG_CACHE_HOME": str(args.output_root / "cache"),
        }
    )
    Path(environment["TEMP"]).mkdir(parents=True)
    samples: list[dict[str, int]] = []
    stop = threading.Event()
    monitor = threading.Thread(target=monitor_gpu, args=(stop, samples), daemon=True)
    stdout_path = args.output_root / "worker.stdout.log"
    stderr_path = args.output_root / "worker.stderr.log"
    started = time.perf_counter()
    args.gpu_marker.write_text("yes\n", encoding="utf-8")
    try:
        monitor.start()
        with stdout_path.open("w", encoding="utf-8") as stdout, stderr_path.open("w", encoding="utf-8") as stderr:
            completed = subprocess.run(
                [
                    str(python),
                    str(worker),
                    "--portrait",
                    str(portrait),
                    "--audio",
                    str(audio),
                    "--output",
                    str(video),
                    "--workspace",
                    str(workspace),
                    "--job",
                    str(job_path),
                    "--seed",
                    "20260901",
                ],
                cwd=args.output_root,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                creationflags=CREATE_NO_WINDOW,
                timeout=900,
                check=False,
            )
    finally:
        stop.set()
        monitor.join(timeout=5)
        args.gpu_marker.write_text("no\n", encoding="utf-8")
    wall_seconds = round(time.perf_counter() - started, 3)

    probe: dict[str, object] = {}
    if completed.returncode == 0 and video.is_file():
        probed = run_hidden(
            [str(ffprobe), "-v", "error", "-show_entries", "format=duration,size:stream=codec_name,width,height,avg_frame_rate", "-of", "json", str(video)],
            capture_output=True,
            timeout=30,
            check=False,
        )
        if probed.returncode == 0:
            probe = json.loads(probed.stdout)

    report = {
        "schema_version": 1,
        "status": "passed" if completed.returncode == 0 and video.is_file() else "failed",
        "scope": "standalone-realistic-fixture-local-model-qualification-not-app-e2e",
        "model": "MuseTalk 1.5",
        "model_revision": job["modelRevision"],
        "portrait_sha256": sha256(portrait),
        "audio_sha256": sha256(audio),
        "job_sha256": sha256(job_path),
        "worker_exit_code": completed.returncode,
        "wall_seconds": wall_seconds,
        "gpu_samples": len(samples),
        "peak_gpu_memory_used_mib": max((item["memory_used_mib"] for item in samples), default=None),
        "peak_gpu_utilization_percent": max((item["gpu_utilization_percent"] for item in samples), default=None),
        "gpu_marker_before": marker_before,
        "gpu_marker_restored": args.gpu_marker.read_text(encoding="utf-8").strip().lower() == "no",
        "output_path": str(video) if video.is_file() else None,
        "output_sha256": sha256(video) if video.is_file() else None,
        "output_probe": probe,
        "contains_credentials": False,
    }
    report_path = args.output_root / "qualification.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
