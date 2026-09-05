#!/usr/bin/env python3
"""Run the pinned MuseTalk 1.5 worker on a moving source, fully headless.

This is a qualification harness, not a product runtime. It preserves the
worker's exact-hash, offline, fixed-argv boundary and records enough provenance
to distinguish a real moving-video inference from a static-portrait demo.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def pin(path: Path, role: str) -> dict[str, object]:
    return {"path": str(path.resolve(strict=True)), "role": role, "sha256": sha256(path)}


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--video", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    runtime = args.runtime_root.resolve(strict=True)
    source = args.source_root.resolve(strict=True)
    video = args.video.resolve(strict=True)
    audio = args.audio.resolve(strict=True)
    output_dir = args.output_dir.resolve()
    if output_dir.exists():
        raise RuntimeError("output directory must not already exist")

    inputs = output_dir / "inputs"
    delivery = output_dir / "output"
    workspace = output_dir / "workspace"
    inputs.mkdir(parents=True)
    delivery.mkdir()
    workspace.mkdir()
    copied_video = inputs / "moving-source.mp4"
    copied_audio = inputs / "speech.wav"
    ffmpeg = runtime / "bin/ffmpeg.exe"
    preprocessing_ffmpeg = Path(shutil.which("ffmpeg") or ffmpeg)
    subprocess.run(
        [
            str(preprocessing_ffmpeg), "-hide_banner", "-nostdin", "-loglevel", "error", "-y",
            "-i", str(video), "-an", "-vf", "fps=25", "-c:v", "libx264",
            "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", str(copied_video),
        ],
        stdin=subprocess.DEVNULL, capture_output=True, check=True,
    )
    shutil.copyfile(audio, copied_audio)

    worker = runtime / "worker/local_presenter_worker.py"
    adapter = runtime / "worker/musetalk_v15_adapter.py"
    ffprobe = runtime / "bin/ffprobe.exe"
    manifest = runtime / "source-manifest.json"
    inference = source / "scripts/inference.py"
    models = runtime / "models"
    files = [
        pin(adapter, "adapter-entrypoint"),
        pin(models / "whisper/config.json", "audio-feature-config"),
        pin(models / "whisper/preprocessor_config.json", "audio-feature-preprocessor"),
        pin(models / "whisper/model.safetensors", "audio-feature-weights"),
        pin(models / "face-detection/s3fd-619a316812.pth", "face-detection-weights"),
        pin(models / "dwpose/dw-ll_ucoco_384.pth", "face-landmark-weights"),
        pin(models / "face-parse-bisent/79999_iter.pth", "face-parse-weights"),
        pin(models / "face-parse-bisent/resnet18-5c106cde.pth", "face-resnet-weights"),
        pin(models / "musetalkV15/musetalk.json", "musetalk-config"),
        pin(inference, "musetalk-inference-entrypoint"),
        pin(models / "musetalkV15/unet.pth", "musetalk-weights"),
        pin(manifest, "runtime-source-manifest"),
        pin(models / "sd-vae-ft-mse/config.json", "vae-config"),
        pin(models / "sd-vae-ft-mse/diffusion_pytorch_model.safetensors", "vae-weights"),
    ]
    output = delivery / "musetalk-moving-source.mp4"
    progress = output_dir / "progress.ndjson"
    job = {
        "schemaVersion": 2,
        "model": "musetalk-1.5",
        "modelRevision": "code-" + subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
        ).strip(),
        "seed": 20260905,
        "profileId": "moving-mara-independent-comparator-v1",
        "sceneId": "moving-mara-independent-comparator-v1",
        "encoding": {
            "policy": "independent-headless-comparator-v1",
            "encoder": "h264_qsv",
            "codecArguments": ["-c:v", "h264_qsv"],
            "audioEncoder": "aac",
            "pixelFormat": "yuv420p",
            "ffmpegPath": str(ffmpeg),
            "ffmpegSha256": sha256(ffmpeg),
        },
        "gpuLease": {
            "deviceId": "cuda:0",
            "leaseId": "interactive-npcs-moving-musetalk-20260905",
            "mutexName": "global\\interactive-npcs-musetalk-qualification",
            "owner": "Interactive NPCs independent moving-source comparator",
            "vramBytes": 10 * 1024**3,
        },
        "inputs": {
            "portrait": {
                "path": str(copied_video),
                "mediaType": "video/mp4",
                "sha256": sha256(copied_video),
            },
            "audio": {
                "path": str(copied_audio),
                "mediaType": "audio/wav",
                "sha256": sha256(copied_audio),
            },
        },
        "output": {"path": str(output), "mediaType": "video/mp4"},
        "progress": {"path": str(progress), "mediaType": "application/x-ndjson", "schemaVersion": 1},
        "workerContract": {
            "contractId": "alystria.musetalk.worker.v1",
            "entrypoint": {"path": str(worker), "sha256": sha256(worker)},
            "files": files,
        },
    }
    job_path = output_dir / "job.json"
    job_path.write_text(json.dumps(job, indent=2) + "\n", encoding="utf-8")

    python = runtime / "venv/Scripts/python.exe"
    environment = os.environ.copy()
    environment["PYTHONUTF8"] = "1"
    environment["PYTHONIOENCODING"] = "utf-8"
    for key, value in {
        "HF_HOME": output_dir / "cache/huggingface",
        "HUGGINGFACE_HUB_CACHE": output_dir / "cache/huggingface/hub",
        "TORCH_HOME": output_dir / "cache/torch",
        "TEMP": output_dir / "temp",
        "TMP": output_dir / "temp",
    }.items():
        Path(value).mkdir(parents=True, exist_ok=True)
        environment[key] = str(value)
    command = [
        str(python), str(worker), "--job", str(job_path), "--portrait", str(copied_video),
        "--audio", str(copied_audio), "--output", str(output), "--workspace", str(workspace),
        "--seed", str(job["seed"]),
    ]
    started = time.perf_counter()
    result = subprocess.run(
        command, cwd=output_dir, env=environment, stdin=subprocess.DEVNULL,
        capture_output=True, timeout=3600, check=False,
    )
    elapsed = time.perf_counter() - started
    (output_dir / "worker.stdout.log").write_text(
        result.stdout.decode("utf-8", errors="replace"), encoding="utf-8"
    )
    (output_dir / "worker.stderr.log").write_text(
        result.stderr.decode("utf-8", errors="replace"), encoding="utf-8"
    )
    evidence: dict[str, object] = {
        "schema": "interactive-npcs-moving-musetalk-comparator/v1",
        "scope": "offline-whole-clip-moving-source-comparator-not-product-runtime",
        "exit_code": result.returncode,
        "wall_seconds": round(elapsed, 3),
        "source_video_sha256": sha256(copied_video),
        "audio_sha256": sha256(copied_audio),
        "job_sha256": sha256(job_path),
        "model_revision": job["modelRevision"],
    }
    if output.is_file():
        probe = subprocess.run(
            [str(ffprobe), "-v", "error", "-show_streams", "-show_format", "-of", "json", str(output)],
            capture_output=True, text=True, check=True,
        )
        evidence.update(output_sha256=sha256(output), output_probe=json.loads(probe.stdout))
    (output_dir / "qualification.json").write_text(
        json.dumps(evidence, indent=2) + "\n", encoding="utf-8"
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
