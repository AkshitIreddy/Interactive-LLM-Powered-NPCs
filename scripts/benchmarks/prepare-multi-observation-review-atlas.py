#!/usr/bin/env python3
"""Export a private multi-observation teacher atlas to the native review format.

This is intentionally a review-data converter, not a model downloader. It
verifies every selected decoded teacher frame against the proof manifest,
normalizes each tight lip texture into the native canonical coordinate system,
and derives the native PCM coefficient address from the exact enrollment audio
interval that produced that observation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
import wave

import cv2
import numpy as np


ANALYSIS_FREQUENCIES = np.asarray([260.0, 520.0, 780.0, 1100.0, 1600.0, 2300.0, 3400.0])


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def require_e_temp(path: Path, *, must_exist: bool = False) -> Path:
    resolved = path.resolve(strict=must_exist)
    if resolved.drive.lower() != "e:" or "temp" not in [part.lower() for part in resolved.parts]:
        raise ValueError("private review atlas inputs and outputs must stay under E:\\temp")
    return resolved


def read_video(path: Path) -> tuple[list[np.ndarray], float]:
    capture = cv2.VideoCapture(str(path))
    fps = float(capture.get(cv2.CAP_PROP_FPS))
    frames: list[np.ndarray] = []
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        frames.append(frame)
    capture.release()
    if not frames or fps <= 0:
        raise ValueError(f"could not decode teacher video: {path}")
    return frames, fps


def read_wav(path: Path) -> tuple[np.ndarray, int, int]:
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        rate = stream.getframerate()
        width = stream.getsampwidth()
        raw = stream.readframes(stream.getnframes())
    if width != 2 or channels < 1 or channels > 2 or rate < 8000:
        raise ValueError("enrollment audio must be mono/stereo PCM16 WAV")
    samples = np.frombuffer(raw, dtype="<i2").astype(np.float64).reshape(-1, channels)
    return samples.mean(axis=1) / 32768.0, rate, channels


def smooth_unit(value: float) -> float:
    value = min(1.0, max(0.0, value))
    return value * value * (3.0 - 2.0 * value)


def native_coefficients_from_pcm(samples: np.ndarray, rate: int) -> list[float]:
    if samples.size == 0:
        return [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    finite = np.clip(samples[np.isfinite(samples)], -1.0, 1.0)
    if finite.size == 0:
        return [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    rms = float(np.sqrt(np.mean(finite * finite)))
    rms_db = 20.0 * math.log10(rms) if rms > 1e-9 else -120.0
    opening = min(1.0, max(0.0, (rms_db + 48.0) / 30.0))
    crossings = float(np.mean((finite[1:] >= 0) != (finite[:-1] >= 0))) if finite.size > 1 else 0.0

    analysis = finite[-2048:]
    dc = float(np.mean(finite))
    previous = 0.0
    q1 = np.zeros(7, dtype=np.float64)
    q2 = np.zeros(7, dtype=np.float64)
    coefficients = 2.0 * np.cos(2.0 * math.pi * ANALYSIS_FREQUENCIES / rate)
    for index, value in enumerate(analysis):
        mono = float(value) - dc
        emphasized = mono - previous * 0.86
        previous = mono
        phase = index / max(1, analysis.size - 1)
        windowed = emphasized * (0.5 - 0.5 * math.cos(2.0 * math.pi * phase))
        q0 = windowed + coefficients * q1 - q2
        q2, q1 = q1, q0
    energy = np.maximum(0.0, q1 * q1 + q2 * q2 - coefficients * q1 * q2)
    total = float(energy.sum()) + 1e-12
    low = float(energy[0:2].sum()) / total
    open_mid = float(energy[2:4].sum()) / total
    bright = float(energy[4:6].sum()) / total
    fricative = float(energy[6]) / total
    rounded = smooth_unit((low - 0.18) / 0.62)
    open_vowel = smooth_unit((open_mid - 0.12) / 0.68)
    spread = smooth_unit((bright - 0.10) / 0.70) * (1.0 - 0.35 * fricative)
    frication = max(
        smooth_unit((fricative - 0.06) / 0.62),
        smooth_unit((crossings - 0.08) * 3.2),
    )
    jaw = opening * min(1.0, max(0.0, 0.84 + open_vowel * 0.16 + spread * 0.03 - frication * 0.06))
    close = min(1.0, max(0.0, 1.0 - opening * 1.8))
    funnel = opening * min(1.0, max(0.0, rounded * 0.82 + frication * 0.24))
    pucker = opening * rounded * (1.0 - frication * 0.55) * 0.58
    smile = opening * spread * (1.0 - frication * 0.4) * 0.72
    upper = opening * min(1.0, max(0.0, spread * 0.28 + frication * 0.34))
    lower = opening * min(1.0, max(0.0, 0.46 + open_vowel * 0.42 + spread * 0.08))
    return [round(value, 8) for value in (jaw, close, funnel, pucker, smile, smile, upper, lower)]


def canonical_patch(
    frame: np.ndarray,
    geometry: dict[str, float],
    width: int,
    height: int,
) -> bytes:
    frame_height, frame_width = frame.shape[:2]
    mouth_width = float(geometry["width"]) * frame_width
    if mouth_width < 4.0:
        raise ValueError("teacher mouth geometry is too small")
    center_x = float(geometry["center_x"]) * frame_width
    center_y = float(geometry["center_y"]) * frame_height
    roll = float(geometry["roll"])
    canonical_width = mouth_width * 1.34
    canonical_height = canonical_width * 0.625
    unit_x = (np.arange(width, dtype=np.float32) + 0.5) / width * 2.0 - 1.0
    unit_y = (np.arange(height, dtype=np.float32) + 0.5) / height * 2.0 - 1.0
    grid_x, grid_y = np.meshgrid(unit_x, unit_y)
    local_x = grid_x * canonical_width * 0.5
    local_y = grid_y * canonical_height * 0.5
    cosine, sine = math.cos(roll), math.sin(roll)
    map_x = (center_x + cosine * local_x - sine * local_y).astype(np.float32)
    map_y = (center_y + sine * local_x + cosine * local_y).astype(np.float32)
    sampled = cv2.remap(
        frame, map_x, map_y, cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )

    # Matches the successful prototype's tight target ROI after conversion to
    # canonical coordinates: 0.66 mouth widths horizontally and 0.34 vertically.
    radius = np.sqrt((grid_x / 0.985) ** 2 + (grid_y / 0.812) ** 2)
    alpha = np.clip((1.0 - radius) / 0.11, 0.0, 1.0)
    alpha = alpha * alpha * (3.0 - 2.0 * alpha)
    alpha_byte = np.rint(alpha * 255.0).astype(np.uint8)
    premultiplied = np.rint(sampled.astype(np.float32) * alpha[..., None]).astype(np.uint8)
    bgra = np.dstack((premultiplied, alpha_byte))
    return bgra.tobytes()


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--width", type=int, default=206)
    parser.add_argument("--height", type=int, default=143)
    args = parser.parse_args()
    proof_root = require_e_temp(args.proof, must_exist=True)
    output = require_e_temp(args.output)
    if output.exists():
        raise ValueError(f"refusing to overwrite existing atlas: {output}")
    if not 32 <= args.width <= 512 or not 32 <= args.height <= 512:
        raise ValueError("canonical dimensions must be in 32..512")

    proof = json.loads((proof_root / "proof.json").read_text(encoding="utf-8"))
    atlas = json.loads((proof_root / "atlas-manifest.json").read_text(encoding="utf-8"))
    if proof.get("schema") != "interactive-npcs-multi-observation-residual-proof/v1":
        raise ValueError("unsupported proof schema")
    if atlas.get("schema") != "interactive-npcs-private-generated-teacher-mouth-atlas/v1":
        raise ValueError("unsupported source atlas schema")
    states = atlas.get("states", [])
    if not 4 <= len(states) <= 16:
        raise ValueError("review atlas must contain 4..16 states")

    teacher_path = require_e_temp(Path(proof["teacher"]), must_exist=True)
    audio_path = require_e_temp(Path(proof["enrollment_audio"]), must_exist=True)
    if sha256(audio_path) != proof["enrollment_audio_sha256"]:
        raise ValueError("enrollment audio hash changed")
    frames, teacher_fps = read_video(teacher_path)
    samples, rate, channels = read_wav(audio_path)
    texture = bytearray()
    review_states = []
    for output_index, state in enumerate(states):
        frame_index = int(state["state_index"])
        if frame_index < 0 or frame_index >= len(frames):
            raise ValueError("teacher state index is outside the video")
        frame = frames[frame_index]
        if sha256_bytes(frame.tobytes()) != state["teacher_frame_sha256"]:
            raise ValueError(f"decoded teacher frame {frame_index} hash changed")
        binding = state["audio_binding"]
        first = int(binding["first_sample_index"])
        count = int(binding["sample_count"])
        if count <= 0 or first < 0 or first + count > samples.size:
            raise ValueError("enrollment audio interval is invalid")
        texture.extend(canonical_patch(frame, state["geometry"], args.width, args.height))
        review_states.append({
            "index": output_index,
            "coefficients": native_coefficients_from_pcm(samples[first : first + count], rate),
            "enrolledPose": [0.0, 0.0, math.degrees(float(state["geometry"]["roll"]))],
        })

    output.mkdir(parents=True)
    texture_path = output / "atlas-bgra8-premultiplied.bin"
    texture_path.write_bytes(texture)
    state_bytes = args.width * args.height * 4
    lineage = sha256_bytes(
        (sha256(proof_root / "proof.json") + "\n" + sha256(proof_root / "atlas-manifest.json")).encode()
    )
    manifest = {
        "schemaVersion": 1,
        "identityRevision": int(lineage[:16], 16) or 1,
        "texture": {
            "file": texture_path.name,
            "sha256": sha256(texture_path),
            "width": args.width,
            "height": args.height,
            "strideBytes": args.width * 4,
            "stateCount": len(review_states),
            "stateBytes": state_bytes,
        },
        "states": review_states,
    }
    manifest_path = output / "atlas.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "status": "prepared",
        "scope": "private-synthetic-mara-only",
        "output": str(output),
        "states": len(review_states),
        "textureBytes": len(texture),
        "textureSha256": sha256(texture_path),
        "manifestSha256": sha256(manifest_path),
    }))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"multi-observation review atlas error: {error}", file=sys.stderr)
        sys.exit(2)
