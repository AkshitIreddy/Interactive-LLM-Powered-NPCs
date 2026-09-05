#!/usr/bin/env python3
"""Measure MuseTalk 1.5 batch-one inference on fresh moving source frames.

The model, Whisper encoder, and face parser stay resident. Every measured item
encodes the current source frame, runs one audio-conditioned UNet item, decodes
it, and composites it. Video decoding and whole-clip Whisper extraction are
reported separately because the published model does not expose a documented
causal audio frontend.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
from typing import Any


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def percentile(values: list[float], quantile: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * quantile
    lower, upper = math.floor(position), math.ceil(position)
    return ordered[lower] if lower == upper else ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


def stats(values: list[float]) -> dict[str, float]:
    return {
        "p50_ms": round(percentile(values, 0.50), 3),
        "p95_ms": round(percentile(values, 0.95), 3),
        "max_ms": round(max(values), 3),
        "mean_ms": round(sum(values) / len(values), 3),
    }


def monitor_gpu(stop: threading.Event, samples: list[dict[str, int]]) -> None:
    executable = Path(os.environ.get("WINDIR", r"C:\Windows")) / "System32/nvidia-smi.exe"
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    while not stop.wait(0.05):
        result = subprocess.run(
            [str(executable), "--query-gpu=memory.used,utilization.gpu", "--format=csv,noheader,nounits"],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", errors="replace",
            creationflags=flags, timeout=3, check=False,
        )
        try:
            memory, utilization = (int(value.strip()) for value in result.stdout.split(","))
        except (ValueError, TypeError):
            continue
        samples.append({"memory_used_mib": memory, "gpu_utilization_percent": utilization})


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--video", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--face-box", nargs=4, type=int, required=True)
    parser.add_argument("--gpu-marker", type=Path, default=Path(r"C:\Users\akshi\Desktop\Code Palace\gpu use.txt"))
    args = parser.parse_args()

    video = args.video.resolve(strict=True)
    audio = args.audio.resolve(strict=True)
    runtime = args.runtime_root.resolve(strict=True)
    source = args.source_root.resolve(strict=True)
    output = args.output_root.resolve()
    marker = args.gpu_marker.resolve(strict=True)
    if output.exists() or output.drive.lower() != "e:" or not str(output).lower().startswith("e:\\temp\\"):
        raise RuntimeError("output must be a fresh directory below E:\\temp")
    if marker.read_text(encoding="utf-8").strip().lower() != "no":
        raise RuntimeError("GPU lock is unavailable")
    output.mkdir(parents=True)

    marker_before = marker.read_text(encoding="utf-8")
    samples: list[dict[str, int]] = []
    stop = threading.Event()
    monitor = threading.Thread(target=monitor_gpu, args=(stop, samples), daemon=True)
    acquired = False
    try:
        if marker.read_text(encoding="utf-8").strip().lower() != "no":
            raise RuntimeError("GPU lock changed before acquisition")
        marker.write_text("yes\n", encoding="utf-8")
        acquired = True
        monitor.start()
        os.environ.update(PYTHONUTF8="1", PYTHONIOENCODING="utf-8", HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1")
        os.chdir(runtime)
        sys.path.insert(0, str(source))

        import cv2  # type: ignore
        import numpy as np  # type: ignore
        import torch  # type: ignore
        from transformers import WhisperModel  # type: ignore
        from musetalk.utils.audio_processor import AudioProcessor  # type: ignore
        from musetalk.utils.blending import get_image_blending, get_image_prepare_material  # type: ignore
        from musetalk.utils.face_parsing import FaceParsing  # type: ignore
        from musetalk.utils.utils import datagen, load_all_model  # type: ignore

        device = torch.device("cuda:0")
        torch.manual_seed(20260905)
        torch.backends.cudnn.benchmark = True
        sync = torch.cuda.synchronize

        load_started = time.perf_counter()
        vae, unet, positional = load_all_model(
            unet_model_path=str(runtime / "models/musetalkV15/unet.pth"),
            vae_type="sd-vae-ft-mse",
            unet_config=str(runtime / "models/musetalkV15/musetalk.json"),
            device=device,
        )
        positional = positional.half().to(device).eval()
        vae.vae = vae.vae.half().to(device).eval()
        unet.model = unet.model.half().to(device).eval()
        whisper = WhisperModel.from_pretrained(runtime / "models/whisper").to(device=device, dtype=unet.model.dtype).eval()
        whisper.requires_grad_(False)
        audio_processor = AudioProcessor(feature_extractor_path=str(runtime / "models/whisper"))
        face_parser = FaceParsing()
        sync()
        model_load_ms = (time.perf_counter() - load_started) * 1000

        decode_started = time.perf_counter()
        capture = cv2.VideoCapture(str(video))
        frames: list[Any] = []
        while True:
            ok, frame = capture.read()
            if not ok:
                break
            frames.append(frame)
        source_fps = float(capture.get(cv2.CAP_PROP_FPS))
        capture.release()
        video_decode_ms = (time.perf_counter() - decode_started) * 1000
        if not frames:
            raise RuntimeError("moving source decoded no frames")

        audio_started = time.perf_counter()
        features, librosa_length = audio_processor.get_audio_feature(str(audio), weight_dtype=unet.model.dtype)
        chunks = audio_processor.get_whisper_chunk(
            features, device, unet.model.dtype, whisper, librosa_length, fps=25,
            audio_padding_length_left=2, audio_padding_length_right=2,
        )
        sync()
        audio_feature_ms = (time.perf_counter() - audio_started) * 1000
        count = min(len(frames), len(chunks))
        frames, chunks = frames[:count], chunks[:count]
        x1, y1, x2, y2 = args.face_box
        height, width = frames[0].shape[:2]
        if not (0 <= x1 < x2 <= width and 0 <= y1 < y2 <= height):
            raise RuntimeError("face box is outside the source frame")
        timestep = torch.tensor([0], device=device)

        def keep_only_lip_region(original: Any, composite: Any) -> Any:
            face_width, face_height = x2 - x1, y2 - y1
            lip_mask = np.zeros(original.shape[:2], dtype=np.uint8)
            cv2.ellipse(
                lip_mask,
                (x1 + face_width // 2, y1 + round(face_height * 0.715)),
                (max(2, round(face_width * 0.26)), max(2, round(face_height * 0.075))),
                0, 0, 360, 255, -1, lineType=cv2.LINE_AA,
            )
            feather = max(3, round(min(face_width, face_height) * 0.018))
            if feather % 2 == 0:
                feather += 1
            alpha = cv2.GaussianBlur(lip_mask, (feather, feather), 0).astype(np.float32)[..., None] / 255.0
            return np.clip(composite.astype(np.float32) * alpha + original.astype(np.float32) * (1.0 - alpha), 0, 255).astype(np.uint8)

        def current_latent(frame: Any) -> Any:
            crop = cv2.resize(frame[y1:y2, x1:x2], (256, 256), interpolation=cv2.INTER_LANCZOS4)
            return vae.get_latents_for_unet(crop)

        # Warm all CUDA kernels and the CPU mask path without including first-use costs.
        latent = current_latent(frames[0])
        mask, mask_box = get_image_prepare_material(frames[0], [x1, y1, x2, y2], fp=face_parser, mode="raw")
        whisper_batch, latent_batch = next(datagen([chunks[0]], [latent], batch_size=1, device=device))
        with torch.no_grad():
            conditioned = positional(whisper_batch.to(device))
            predicted = unet.model(latent_batch.to(device=device, dtype=unet.model.dtype), timestep, encoder_hidden_states=conditioned).sample
            decoded = vae.decode_latents(predicted.to(device=device, dtype=vae.vae.dtype))[0]
        sync()
        warm_composite = get_image_blending(frames[0], cv2.resize(decoded.astype(np.uint8), (x2 - x1, y2 - y1)), [x1, y1, x2, y2], mask, mask_box)
        _ = keep_only_lip_region(frames[0], warm_composite)

        timings = {key: [] for key in ("mask_prepare", "current_frame_encode", "audio_position", "unet", "decode", "composite", "total")}
        sample_images: list[Any] = []
        with torch.no_grad():
            for index, (frame, chunk) in enumerate(zip(frames, chunks)):
                total_started = time.perf_counter()
                started = time.perf_counter()
                mask, mask_box = get_image_prepare_material(frame, [x1, y1, x2, y2], fp=face_parser, mode="raw")
                timings["mask_prepare"].append((time.perf_counter() - started) * 1000)

                sync(); started = time.perf_counter()
                latent = current_latent(frame)
                sync(); timings["current_frame_encode"].append((time.perf_counter() - started) * 1000)
                whisper_batch, latent_batch = next(datagen([chunk], [latent], batch_size=1, device=device))

                sync(); started = time.perf_counter()
                conditioned = positional(whisper_batch.to(device))
                sync(); timings["audio_position"].append((time.perf_counter() - started) * 1000)

                sync(); started = time.perf_counter()
                predicted = unet.model(latent_batch.to(device=device, dtype=unet.model.dtype), timestep, encoder_hidden_states=conditioned).sample
                sync(); timings["unet"].append((time.perf_counter() - started) * 1000)

                sync(); started = time.perf_counter()
                decoded = vae.decode_latents(predicted.to(device=device, dtype=vae.vae.dtype))[0]
                sync(); timings["decode"].append((time.perf_counter() - started) * 1000)

                started = time.perf_counter()
                resized = cv2.resize(decoded.astype(np.uint8), (x2 - x1, y2 - y1))
                full_composite = get_image_blending(frame, resized, [x1, y1, x2, y2], mask, mask_box)
                composited = keep_only_lip_region(frame, full_composite)
                timings["composite"].append((time.perf_counter() - started) * 1000)
                timings["total"].append((time.perf_counter() - total_started) * 1000)
                if index in {3, 10, 18, 26}:
                    sample_images.append((index, composited.copy()))

        for index, image in sample_images:
            cv2.imwrite(str(output / f"fresh-frame-{index:02d}.png"), image)
        report = {
            "schema": "interactive-npcs-musetalk-fresh-frame-qualification/v1",
            "status": "measured-not-qualified",
            "scope": "persistent-model-batch-one-fresh-moving-frame-not-streaming-audio-not-app-e2e",
            "model_revision": subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip(),
            "inputs": {
                "video": str(video), "video_sha256": sha256(video), "audio": str(audio), "audio_sha256": sha256(audio),
                "decoded_source_frames": len(frames), "audio_conditioning_frames": len(chunks), "measured_frames": count,
                "source_fps": source_fps, "face_box": [x1, y1, x2, y2],
            },
            "cold_or_outside_frame_loop": {
                "model_load_ms": round(model_load_ms, 3), "video_decode_ms": round(video_decode_ms, 3),
                "whole_clip_whisper_feature_ms": round(audio_feature_ms, 3),
            },
            "fresh_frame_stages": {name: stats(values) for name, values in timings.items()},
            "fresh_frame_budget": {
                "p95_ms": stats(timings["total"])["p95_ms"],
                "p95_fps_equivalent": round(1000 / stats(timings["total"])["p95_ms"], 3),
                "meets_50_ms": stats(timings["total"])["p95_ms"] <= 50,
            },
            "telemetry": {
                "samples": len(samples),
                "peak_total_gpu_memory_used_mib": max((sample["memory_used_mib"] for sample in samples), default=None),
                "peak_gpu_utilization_percent": max((sample["gpu_utilization_percent"] for sample in samples), default=None),
            },
            "limitations": [
                "Whisper features were computed for the whole clip and are not proven causal.",
                "A fixed trusted face ROI isolates model cost from detector cost; arbitrary-game tracking is outside this probe.",
                "GPU telemetry is device-total usage and may include unrelated baseline allocations.",
            ],
        }
        (output / "qualification.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"report": str(output / "qualification.json"), "p95_ms": report["fresh_frame_budget"]["p95_ms"]}))
        return 0
    finally:
        stop.set()
        if monitor.is_alive():
            monitor.join(timeout=5)
        if acquired:
            marker.write_text(marker_before, encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
