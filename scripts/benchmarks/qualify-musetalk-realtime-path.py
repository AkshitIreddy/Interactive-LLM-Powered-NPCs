#!/usr/bin/env python3
"""Measure MuseTalk's prepared-avatar path without file-video overhead.

This is a qualification probe, not an application worker.  It deliberately
keeps every large/generated artifact under an explicit E:\\temp output root,
loads the model once, caches the source-face latent/mask once, and then measures
several batch sizes over the same decoded audio features.  The report separates
the expensive cold path from the hot neural and CPU-composite paths.

The probe must be run with the pinned Windows MuseTalk Python environment.  It
does not download anything and it refuses to take the shared GPU marker unless
the marker contains exactly ``no``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import sys
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable


SCHEMA = "interactive-npcs-musetalk-realtime-qualification/v1"
MARKER_FREE = "no"
MARKER_BUSY = "yes"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def percentile(values: Iterable[float], quantile: float) -> float:
    ordered = sorted(float(value) for value in values)
    if not ordered:
        raise ValueError("percentile requires at least one value")
    position = (len(ordered) - 1) * quantile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def parse_batch_sizes(value: str) -> list[int]:
    try:
        sizes = [int(item.strip()) for item in value.split(",") if item.strip()]
    except ValueError as error:
        raise argparse.ArgumentTypeError("batch sizes must be comma-separated integers") from error
    if not sizes or any(size < 1 or size > 32 for size in sizes):
        raise argparse.ArgumentTypeError("batch sizes must contain values from 1 through 32")
    if len(set(sizes)) != len(sizes):
        raise argparse.ArgumentTypeError("batch sizes must not contain duplicates")
    return sizes


def parse_face_box(value: str) -> tuple[int, int, int, int]:
    try:
        coordinates = tuple(int(round(float(item.strip()))) for item in value.split(","))
    except ValueError as error:
        raise argparse.ArgumentTypeError("face box must be x1,y1,x2,y2") from error
    if len(coordinates) != 4:
        raise argparse.ArgumentTypeError("face box must contain exactly four coordinates")
    x1, y1, x2, y2 = coordinates
    if min(coordinates) < 0 or x2 <= x1 or y2 <= y1:
        raise argparse.ArgumentTypeError("face box must have non-negative ordered coordinates")
    return x1, y1, x2, y2


def require_file(path: Path, label: str) -> Path:
    resolved = path.resolve(strict=True)
    if not resolved.is_file() or resolved.is_symlink():
        raise RuntimeError(f"{label} must be a regular non-symlink file")
    return resolved


def require_directory(path: Path, label: str) -> Path:
    resolved = path.resolve(strict=True)
    if not resolved.is_dir() or resolved.is_symlink():
        raise RuntimeError(f"{label} must be a regular non-symlink directory")
    return resolved


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    drive = resolved.drive.lower()
    normalized = str(resolved).replace("/", "\\").lower()
    if drive != "e:" or not normalized.startswith("e:\\temp\\"):
        raise RuntimeError("output root must be a new directory below E:\\temp")
    if resolved.exists():
        raise RuntimeError("output root already exists; use a fresh qualification directory")
    return resolved


@dataclass(frozen=True)
class BatchResult:
    batch_size: int
    frame_count: int
    batch_count: int
    first_batch_ms: float
    neural_and_decode_ms: float
    neural_and_decode_fps: float
    batch_p50_ms: float
    batch_p95_ms: float
    batch_p99_ms: float
    composite_total_ms: float
    composite_fps: float
    composite_p95_ms: float
    hot_path_fps: float


def monitor_gpu(stop: threading.Event, samples: list[dict[str, int]]) -> None:
    """Collect aggregate GPU telemetry without ever writing command output."""
    import subprocess

    executable = Path(os.environ.get("WINDIR", r"C:\Windows")) / "System32/nvidia-smi.exe"
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    while not stop.wait(0.1):
        try:
            completed = subprocess.run(
                [
                    str(executable),
                    "--query-gpu=memory.used,utilization.gpu",
                    "--format=csv,noheader,nounits",
                ],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                creationflags=creationflags,
                timeout=3,
                check=False,
            )
            values = [int(value.strip()) for value in completed.stdout.split(",")]
            if completed.returncode == 0 and len(values) == 2:
                samples.append({"memory_used_mib": values[0], "gpu_utilization_percent": values[1]})
        except (OSError, ValueError, subprocess.SubprocessError):
            continue


def synchronize(torch: Any) -> None:
    if torch.cuda.is_available():
        torch.cuda.synchronize()


def validate_face_box(box: tuple[int, int, int, int], width: int, height: int) -> None:
    x1, y1, x2, y2 = box
    if x2 > width or y2 > height:
        raise RuntimeError("face box extends beyond the portrait")
    if x2 - x1 < 32 or y2 - y1 < 32:
        raise RuntimeError("face box must be at least 32 by 32 pixels")


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--portrait", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--face-box", type=parse_face_box)
    parser.add_argument("--batch-sizes", type=parse_batch_sizes, default=parse_batch_sizes("1,4,8,20"))
    parser.add_argument("--warmup-batches", type=int, default=2)
    parser.add_argument("--fps", type=int, default=25)
    parser.add_argument(
        "--gpu-marker",
        type=Path,
        default=Path(r"C:\Users\akshi\Desktop\Code Palace\gpu use.txt"),
    )
    args = parser.parse_args()

    if args.warmup_batches < 1 or args.warmup_batches > 10:
        raise RuntimeError("warmup batches must be from 1 through 10")
    if args.fps != 25:
        raise RuntimeError("MuseTalk 1.5 qualification is pinned to its trained 25 fps cadence")

    portrait = require_file(args.portrait, "portrait")
    audio = require_file(args.audio, "audio")
    runtime_root = require_directory(args.runtime_root, "runtime root")
    source_root = require_directory(args.source_root, "source root")
    output_root = require_e_temp(args.output_root)
    marker = require_file(args.gpu_marker, "GPU marker")

    model_files = {
        "unet": require_file(runtime_root / "models/musetalkV15/unet.pth", "MuseTalk UNet"),
        "unet_config": require_file(runtime_root / "models/musetalkV15/musetalk.json", "MuseTalk config"),
        "vae": require_file(
            runtime_root / "models/sd-vae-ft-mse/diffusion_pytorch_model.safetensors",
            "VAE weights",
        ),
        "whisper": require_file(runtime_root / "models/whisper/model.safetensors", "Whisper weights"),
        "realtime_source": require_file(
            source_root / "scripts/realtime_inference.py", "upstream realtime entrypoint"
        ),
    }

    if marker.read_text(encoding="utf-8").strip().lower() != MARKER_FREE:
        raise RuntimeError("GPU lease unavailable; shared marker is not 'no'")

    output_root.mkdir(parents=True)
    report_path = output_root / "qualification.json"
    samples: list[dict[str, int]] = []
    stop = threading.Event()
    monitor = threading.Thread(target=monitor_gpu, args=(stop, samples), daemon=True)
    marker_before = marker.read_text(encoding="utf-8")
    started = time.perf_counter()
    acquired = False

    try:
        # Check again immediately before acquisition.  The shared marker is the
        # user's coordination contract; this probe never steals an occupied GPU.
        if marker.read_text(encoding="utf-8").strip().lower() != MARKER_FREE:
            raise RuntimeError("GPU lease changed before acquisition")
        marker.write_text(f"{MARKER_BUSY}\n", encoding="utf-8")
        acquired = True
        monitor.start()

        os.chdir(runtime_root)
        sys.path.insert(0, str(source_root))

        import cv2  # type: ignore
        import numpy as np  # type: ignore
        import torch  # type: ignore
        from transformers import WhisperModel  # type: ignore

        from musetalk.utils.audio_processor import AudioProcessor  # type: ignore
        from musetalk.utils.blending import get_image_blending, get_image_prepare_material  # type: ignore
        from musetalk.utils.face_parsing import FaceParsing  # type: ignore
        from musetalk.utils.utils import datagen, load_all_model  # type: ignore

        if not torch.cuda.is_available():
            raise RuntimeError("CUDA is unavailable in the pinned MuseTalk environment")
        device = torch.device("cuda:0")
        torch.manual_seed(20260901)
        torch.cuda.manual_seed_all(20260901)
        torch.backends.cudnn.benchmark = True

        cold_load_started = time.perf_counter()
        vae, unet, positional_encoding = load_all_model(
            unet_model_path=str(model_files["unet"]),
            vae_type="sd-vae-ft-mse",
            unet_config=str(model_files["unet_config"]),
            device=device,
        )
        positional_encoding = positional_encoding.half().to(device).eval()
        vae.vae = vae.vae.half().to(device).eval()
        unet.model = unet.model.half().to(device).eval()
        whisper = WhisperModel.from_pretrained(runtime_root / "models/whisper")
        whisper = whisper.to(device=device, dtype=unet.model.dtype).eval()
        whisper.requires_grad_(False)
        audio_processor = AudioProcessor(feature_extractor_path=str(runtime_root / "models/whisper"))
        face_parser = FaceParsing()
        synchronize(torch)
        cold_model_load_ms = (time.perf_counter() - cold_load_started) * 1000.0

        frame = cv2.imread(str(portrait), cv2.IMREAD_COLOR)
        if frame is None:
            raise RuntimeError("portrait could not be decoded")
        height, width = frame.shape[:2]

        prepare_started = time.perf_counter()
        if args.face_box is None:
            # DWPose is a cold enrollment dependency, not part of the prepared
            # avatar hot path. Import it only when the caller has not supplied
            # a previously qualified ROI; importing it eagerly also makes its
            # source-relative config lookup run for an otherwise cached probe.
            from musetalk.utils.preprocessing import get_landmark_and_bbox  # type: ignore

            coordinates, frames = get_landmark_and_bbox([str(portrait)], 0)
            if not coordinates or tuple(coordinates[0]) == (0.0, 0.0, 0.0, 0.0):
                raise RuntimeError("upstream landmark detector did not find a usable face")
            x1, y1, x2, y2 = (int(value) for value in coordinates[0])
            frame = frames[0]
            face_box_source = "upstream-dwpose"
        else:
            x1, y1, x2, y2 = args.face_box
            face_box_source = "explicit-qualified-roi"
        y2 = min(height, y2 + 10)
        box = (x1, y1, x2, y2)
        validate_face_box(box, width, height)
        crop = frame[y1:y2, x1:x2]
        crop = cv2.resize(crop, (256, 256), interpolation=cv2.INTER_LANCZOS4)
        source_latent = vae.get_latents_for_unet(crop)
        mask, mask_crop_box = get_image_prepare_material(frame, list(box), fp=face_parser, mode="jaw")
        synchronize(torch)
        avatar_prepare_ms = (time.perf_counter() - prepare_started) * 1000.0

        audio_started = time.perf_counter()
        input_features, librosa_length = audio_processor.get_audio_feature(
            str(audio), weight_dtype=unet.model.dtype
        )
        whisper_chunks = audio_processor.get_whisper_chunk(
            input_features,
            device,
            unet.model.dtype,
            whisper,
            librosa_length,
            fps=args.fps,
            audio_padding_length_left=2,
            audio_padding_length_right=2,
        )
        synchronize(torch)
        audio_feature_ms = (time.perf_counter() - audio_started) * 1000.0
        frame_count = len(whisper_chunks)
        if frame_count < 1:
            raise RuntimeError("audio produced no MuseTalk frame features")

        timestep = torch.tensor([0], device=device)

        @torch.no_grad()
        def infer_batch(whisper_batch: Any, latent_batch: Any) -> Any:
            audio_features = positional_encoding(whisper_batch.to(device))
            latents = latent_batch.to(device=device, dtype=unet.model.dtype)
            predicted = unet.model(
                latents,
                timestep,
                encoder_hidden_states=audio_features,
            ).sample
            return vae.decode_latents(predicted.to(device=device, dtype=vae.vae.dtype))

        results: list[BatchResult] = []
        sample_frame_path: Path | None = None
        for batch_size in args.batch_sizes:
            first_chunk = whisper_chunks[:batch_size]
            first_latents = [source_latent] * max(1, len(first_chunk))
            for _ in range(args.warmup_batches):
                warmup = next(datagen(first_chunk, first_latents, batch_size=batch_size, device=device))
                infer_batch(*warmup)
            synchronize(torch)

            batch_times: list[float] = []
            composite_times: list[float] = []
            generated_frames = 0
            first_batch_ms = 0.0
            neural_started = time.perf_counter()
            for batch_index, (whisper_batch, latent_batch) in enumerate(
                datagen(whisper_chunks, [source_latent], batch_size=batch_size, device=device)
            ):
                synchronize(torch)
                batch_started = time.perf_counter()
                decoded = infer_batch(whisper_batch, latent_batch)
                synchronize(torch)
                batch_ms = (time.perf_counter() - batch_started) * 1000.0
                batch_times.append(batch_ms)
                if batch_index == 0:
                    first_batch_ms = batch_ms

                for decoded_frame in decoded:
                    composite_started = time.perf_counter()
                    resized = cv2.resize(decoded_frame.astype(np.uint8), (x2 - x1, y2 - y1))
                    composited = get_image_blending(frame, resized, box, mask, mask_crop_box)
                    composite_times.append((time.perf_counter() - composite_started) * 1000.0)
                    generated_frames += 1
                    if sample_frame_path is None and batch_size == args.batch_sizes[0]:
                        sample_frame_path = output_root / "sample-composited-frame.png"
                        if not cv2.imwrite(str(sample_frame_path), composited):
                            raise RuntimeError("sample frame could not be written")
            neural_ms = (time.perf_counter() - neural_started) * 1000.0 - sum(composite_times)
            composite_ms = sum(composite_times)
            if generated_frames != frame_count:
                raise RuntimeError("generated frame count does not match audio feature count")
            hot_ms = neural_ms + composite_ms
            results.append(
                BatchResult(
                    batch_size=batch_size,
                    frame_count=generated_frames,
                    batch_count=len(batch_times),
                    first_batch_ms=round(first_batch_ms, 3),
                    neural_and_decode_ms=round(neural_ms, 3),
                    neural_and_decode_fps=round(generated_frames * 1000.0 / neural_ms, 3),
                    batch_p50_ms=round(percentile(batch_times, 0.50), 3),
                    batch_p95_ms=round(percentile(batch_times, 0.95), 3),
                    batch_p99_ms=round(percentile(batch_times, 0.99), 3),
                    composite_total_ms=round(composite_ms, 3),
                    composite_fps=round(generated_frames * 1000.0 / composite_ms, 3),
                    composite_p95_ms=round(percentile(composite_times, 0.95), 3),
                    hot_path_fps=round(generated_frames * 1000.0 / hot_ms, 3),
                )
            )

        runtime_seconds = time.perf_counter() - started
        report = {
            "schema": SCHEMA,
            "status": "measured",
            "scope": "standalone-prepared-avatar-hot-path-not-app-e2e-not-pack-admission",
            "execution": {
                "platform": sys.platform,
                "torch": torch.__version__,
                "torch_cuda": torch.version.cuda,
                "gpu_name": torch.cuda.get_device_name(0),
                "fps": args.fps,
                "precision": "fp16",
                "warmup_batches": args.warmup_batches,
            },
            "inputs": {
                "portrait_sha256": sha256(portrait),
                "audio_sha256": sha256(audio),
                "portrait_width": width,
                "portrait_height": height,
                "audio_frame_count": frame_count,
                "face_box": list(box),
                "face_box_source": face_box_source,
            },
            "artifacts": {
                name: {"sha256": sha256(path), "bytes": path.stat().st_size}
                for name, path in model_files.items()
            },
            "cold_path": {
                "model_load_ms": round(cold_model_load_ms, 3),
                "avatar_prepare_ms": round(avatar_prepare_ms, 3),
                "audio_feature_ms": round(audio_feature_ms, 3),
            },
            "hot_path": [asdict(result) for result in results],
            "telemetry": {
                "samples": len(samples),
                "peak_total_gpu_memory_used_mib": max(
                    (sample["memory_used_mib"] for sample in samples), default=None
                ),
                "peak_gpu_utilization_percent": max(
                    (sample["gpu_utilization_percent"] for sample in samples), default=None
                ),
            },
            "sample_frame": None
            if sample_frame_path is None
            else {
                "sha256": sha256(sample_frame_path),
                "bytes": sample_frame_path.stat().st_size,
            },
            "wall_seconds": round(runtime_seconds, 3),
            "claims": {
                "streaming_audio": False,
                "persistent_model": True,
                "cached_avatar": True,
                "current_game_frame_pipeline": False,
                "installed_app_e2e": False,
                "qualified_pack": False,
            },
        }
        report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(json.dumps({"status": "measured", "report": str(report_path)}, sort_keys=True))
        return 0
    finally:
        stop.set()
        if monitor.is_alive():
            monitor.join(timeout=5)
        if acquired:
            marker.write_text(marker_before, encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
