#!/usr/bin/env python3
"""Build a tightly masked, source-preserving native review mouth atlas.

This converter consumes the same private multi-observation proof as
``prepare-multi-observation-review-atlas.py``.  It differs in three important
ways:

* generated teacher pixels are retained only around the visible lip and oral
  anatomy instead of replacing a large lower-face ellipse;
* the teacher is sharpened locally while its surrounding high-frequency source
  texture is restored at the feather boundary; and
* acoustically labelled silence observations use the untouched source mouth,
  because a teacher frame with visible teeth is not a valid closure state.

The output remains a private, identity-bound review pack.  It contains no model
weights and is not a generic-game enrollment implementation.
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


ANALYSIS_FREQUENCIES = np.asarray(
    [260.0, 520.0, 780.0, 1100.0, 1600.0, 2300.0, 3400.0]
)


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def require_e_temp(path: Path, *, must_exist: bool = False) -> Path:
    resolved = path.resolve(strict=must_exist)
    if resolved.drive.lower() != "e:" or "temp" not in {
        part.lower() for part in resolved.parts
    }:
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
        raise ValueError(f"could not decode video: {path}")
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
    crossings = (
        float(np.mean((finite[1:] >= 0) != (finite[:-1] >= 0)))
        if finite.size > 1
        else 0.0
    )

    analysis = finite[-2048:]
    dc = float(np.mean(finite))
    previous = 0.0
    q1 = np.zeros(7, dtype=np.float64)
    q2 = np.zeros(7, dtype=np.float64)
    recurrence = 2.0 * np.cos(2.0 * math.pi * ANALYSIS_FREQUENCIES / rate)
    for index, value in enumerate(analysis):
        mono = float(value) - dc
        emphasized = mono - previous * 0.86
        previous = mono
        phase = index / max(1, analysis.size - 1)
        windowed = emphasized * (0.5 - 0.5 * math.cos(2.0 * math.pi * phase))
        q0 = windowed + recurrence * q1 - q2
        q2, q1 = q1, q0
    energy = np.maximum(0.0, q1 * q1 + q2 * q2 - recurrence * q1 * q2)
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
    jaw = opening * min(
        1.0, max(0.0, 0.84 + open_vowel * 0.16 + spread * 0.03 - frication * 0.06)
    )
    close = min(1.0, max(0.0, 1.0 - opening * 1.8))
    funnel = opening * min(1.0, max(0.0, rounded * 0.82 + frication * 0.24))
    pucker = opening * rounded * (1.0 - frication * 0.55) * 0.58
    smile = opening * spread * (1.0 - frication * 0.4) * 0.72
    upper = opening * min(1.0, max(0.0, spread * 0.28 + frication * 0.34))
    lower = opening * min(1.0, max(0.0, 0.46 + open_vowel * 0.42 + spread * 0.08))
    return [
        round(value, 8)
        for value in (jaw, close, funnel, pucker, smile, smile, upper, lower)
    ]


def canonical_image(
    frame: np.ndarray,
    geometry: dict[str, float],
    width: int,
    height: int,
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    frame_height, frame_width = frame.shape[:2]
    mouth_width = float(geometry["width"]) * frame_width
    if mouth_width < 4.0:
        raise ValueError("mouth geometry is too small")
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
        frame,
        map_x,
        map_y,
        cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    return sampled, grid_x, grid_y


def _central_component(mask: np.ndarray) -> np.ndarray:
    count, labels, stats, centroids = cv2.connectedComponentsWithStats(mask, 8)
    if count <= 1:
        return mask
    height, width = mask.shape
    best = 0
    best_score = -1.0
    for label in range(1, count):
        area = float(stats[label, cv2.CC_STAT_AREA])
        cx, cy = centroids[label]
        distance = ((cx - width * 0.5) / width) ** 2 + ((cy - height * 0.5) / height) ** 2
        score = area / (1.0 + distance * 32.0)
        if score > best_score:
            best = label
            best_score = score
    return np.where(labels == best, 255, 0).astype(np.uint8)


def adaptive_anatomy_mask(
    source: np.ndarray,
    teacher: np.ndarray,
    grid_x: np.ndarray,
    grid_y: np.ndarray,
) -> tuple[np.ndarray, dict[str, float]]:
    """Return a tight alpha around changed lip/oral anatomy.

    The previous exporter used almost the complete canonical crop.  That means
    a low-resolution generated philtrum and chin replaced sharper live pixels.
    Difference and colour evidence are used only to shape a conservative,
    centrally bounded support; no semantic model is required at runtime.
    """

    source_f = source.astype(np.float32)
    teacher_f = teacher.astype(np.float32)
    difference = np.max(np.abs(teacher_f - source_f), axis=2)
    difference = cv2.GaussianBlur(difference, (0, 0), 1.1)
    prior = ((grid_x / 0.78) ** 2 + (grid_y / 0.48) ** 2) <= 1.0
    core = ((grid_x / 0.68) ** 2 + (grid_y / 0.38) ** 2) <= 1.0
    values = difference[prior]
    threshold = max(6.0, float(np.percentile(values, 52.0)) * 0.72)

    lab = cv2.cvtColor(teacher, cv2.COLOR_BGR2LAB)
    gray = cv2.cvtColor(teacher, cv2.COLOR_BGR2GRAY).astype(np.float32)
    outer_skin = ((np.abs(grid_x) < 0.84) & (np.abs(grid_y) > 0.48))
    skin_luma = float(np.median(gray[outer_skin])) if np.any(outer_skin) else float(np.median(gray))
    skin_a = float(np.median(lab[..., 1][outer_skin])) if np.any(outer_skin) else float(np.median(lab[..., 1]))
    lip_colour = lab[..., 1].astype(np.float32) > skin_a + 2.0
    cavity = (gray < skin_luma * 0.63) & (np.abs(grid_y) < 0.30)
    teeth = (
        (gray > min(245.0, skin_luma * 1.12))
        & (np.abs(grid_x) < 0.63)
        & (np.abs(grid_y) < 0.27)
    )
    evidence = prior & (
        (difference >= threshold)
        | (core & lip_colour)
        | cavity
        | teeth
    )
    evidence_u8 = np.where(evidence, 255, 0).astype(np.uint8)
    evidence_u8 = cv2.morphologyEx(
        evidence_u8, cv2.MORPH_CLOSE, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (9, 7))
    )
    evidence_u8 = _central_component(evidence_u8)
    evidence_u8 = cv2.dilate(
        evidence_u8, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (7, 5)), iterations=1
    )
    evidence_u8[~prior] = 0
    alpha = cv2.GaussianBlur(evidence_u8.astype(np.float32) / 255.0, (0, 0), 1.15)
    alpha *= prior.astype(np.float32)
    alpha[core & (evidence_u8 > 0)] = np.maximum(alpha[core & (evidence_u8 > 0)], 0.94)
    alpha = np.clip(alpha, 0.0, 1.0)

    tooth_pixels = teeth & (alpha > 0.5)
    tooth_width = 0
    tooth_height = 0
    if np.any(tooth_pixels):
        ys, xs = np.nonzero(tooth_pixels)
        tooth_width = int(xs.max() - xs.min() + 1)
        tooth_height = int(ys.max() - ys.min() + 1)
    metrics = {
        "differenceThreshold": round(threshold, 4),
        "alphaPixelShare": round(float(np.mean(alpha > 1.0 / 255.0)), 6),
        "opaquePixelShare": round(float(np.mean(alpha >= 0.94)), 6),
        "toothBandAspect": round(tooth_width / max(1, tooth_height), 4),
    }
    return alpha, metrics


def normalized_oral_texture(
    source: np.ndarray,
    teacher: np.ndarray,
    grid_x: np.ndarray,
    grid_y: np.ndarray,
) -> tuple[np.ndarray, np.ndarray, dict[str, float]]:
    """Normalize only observed cavity/teeth pixels into the runtime oral slot.

    The native compositor owns the current-frame lip surface and deforms it
    with the tracked contour.  The atlas contributes only pixels that a closed
    source frame cannot provide: cavity, teeth and tongue.  Keeping those
    responsibilities separate avoids a second, blurry outer-lip contour.
    """

    sharpened, detail_metrics = restore_detail(
        source, teacher, np.ones(source.shape[:2], np.float32)
    )
    gray = cv2.cvtColor(teacher, cv2.COLOR_BGR2GRAY).astype(np.float32)
    hsv = cv2.cvtColor(teacher, cv2.COLOR_BGR2HSV)
    central = (np.abs(grid_x) < 0.68) & (np.abs(grid_y) < 0.31)
    skin_support = (np.abs(grid_x) < 0.80) & (grid_y < -0.42)
    skin_luma = (
        float(np.median(gray[skin_support]))
        if np.any(skin_support)
        else float(np.median(gray))
    )
    central_values = gray[central]
    dark_threshold = min(skin_luma * 0.72, float(np.percentile(central_values, 34.0)))
    dark = central & (gray <= dark_threshold)
    dark_u8 = _central_component(np.where(dark, 255, 0).astype(np.uint8))
    if np.count_nonzero(dark_u8) < 8:
        dark_u8 = _central_component(
            np.where(central & (gray <= skin_luma * 0.82), 255, 0).astype(np.uint8)
        )
    neighbourhood = cv2.dilate(
        dark_u8, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (13, 9)), iterations=1
    ) > 0
    teeth = (
        central
        & neighbourhood
        & (gray >= min(242.0, skin_luma * 1.08))
        & (hsv[..., 1] <= 105)
    )
    oral_u8 = np.where((dark_u8 > 0) | teeth, 255, 0).astype(np.uint8)
    oral_u8 = cv2.morphologyEx(
        oral_u8,
        cv2.MORPH_CLOSE,
        cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (9, 5)),
    )
    oral_u8 = cv2.dilate(
        oral_u8, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 3)), iterations=1
    )
    ys, xs = np.nonzero(oral_u8)
    if xs.size < 8:
        raise ValueError("teacher state has no usable observed oral interior")
    x0 = max(0, int(xs.min()) - 2)
    x1 = min(teacher.shape[1], int(xs.max()) + 3)
    y0 = max(0, int(ys.min()) - 2)
    y1 = min(teacher.shape[0], int(ys.max()) + 3)

    # These coordinates match compose_atlas_residual's canonical oral slot.
    # The destination contour, rather than this rectangle, remains the hard
    # modification boundary at runtime.
    target = np.zeros_like(teacher)
    target_alpha = np.zeros(teacher.shape[:2], np.float32)
    target_support = (
        (grid_x >= -0.67)
        & (grid_x <= 0.67)
        & (grid_y >= -0.24)
        & (grid_y <= 0.20)
    )
    target_ys, target_xs = np.nonzero(target_support)
    tx0, tx1 = int(target_xs.min()), int(target_xs.max()) + 1
    ty0, ty1 = int(target_ys.min()), int(target_ys.max()) + 1
    target_width = tx1 - tx0
    target_height = ty1 - ty0
    oral_crop = sharpened[y0:y1, x0:x1]
    mask_crop = oral_u8[y0:y1, x0:x1]
    resized_oral = cv2.resize(
        oral_crop, (target_width, target_height), interpolation=cv2.INTER_LANCZOS4
    )
    resized_mask = cv2.resize(
        mask_crop, (target_width, target_height), interpolation=cv2.INTER_LINEAR
    ).astype(np.float32) / 255.0
    resized_mask = cv2.GaussianBlur(resized_mask, (0, 0), 0.72)
    resized_mask = np.clip(resized_mask, 0.0, 1.0)
    target[ty0:ty1, tx0:tx1] = resized_oral
    target_alpha[ty0:ty1, tx0:tx1] = resized_mask

    tooth_pixels = teeth[y0:y1, x0:x1]
    tooth_width = 0
    tooth_height = 0
    if np.any(tooth_pixels):
        tooth_ys, tooth_xs = np.nonzero(tooth_pixels)
        tooth_width = int(tooth_xs.max() - tooth_xs.min() + 1)
        tooth_height = int(tooth_ys.max() - tooth_ys.min() + 1)
    return target, target_alpha, {
        **detail_metrics,
        "oralSourceBounds": [x0, y0, x1, y1],
        "oralAlphaPixelShare": round(float(np.mean(target_alpha > 1.0 / 255.0)), 6),
        "toothBandAspect": round(tooth_width / max(1, tooth_height), 4),
        "darkThreshold": round(dark_threshold, 4),
    }


def restore_detail(
    source: np.ndarray,
    teacher: np.ndarray,
    alpha: np.ndarray,
) -> tuple[np.ndarray, dict[str, float]]:
    source_f = source.astype(np.float32)
    teacher_f = teacher.astype(np.float32)
    teacher_low = cv2.GaussianBlur(teacher_f, (0, 0), 0.72)
    teacher_mid = cv2.GaussianBlur(teacher_f, (0, 0), 1.65)
    enhanced = teacher_f + (teacher_f - teacher_low) * 0.78 + (teacher_f - teacher_mid) * 0.18

    # Only the soft boundary receives source high-frequency texture.  Its small
    # amplitude restores skin grain without reintroducing the source's closed
    # lip contour into the teacher's open oral geometry.
    source_high = source_f - cv2.GaussianBlur(source_f, (0, 0), 0.9)
    boundary = np.clip(1.0 - np.abs(alpha * 2.0 - 1.0), 0.0, 1.0)
    enhanced += source_high * (boundary[..., None] * 0.34)
    enhanced = np.clip(enhanced, 0.0, 255.0).astype(np.uint8)

    def laplacian_variance(image: np.ndarray, support: np.ndarray) -> float:
        gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
        lap = cv2.Laplacian(gray, cv2.CV_32F)
        values = lap[support]
        return float(np.var(values)) if values.size else 0.0

    support = alpha >= 0.5
    return enhanced, {
        "teacherLaplacianVariance": round(laplacian_variance(teacher, support), 4),
        "enhancedLaplacianVariance": round(laplacian_variance(enhanced, support), 4),
    }


def premultiply(image: np.ndarray, alpha: np.ndarray) -> bytes:
    alpha = np.clip(alpha, 0.0, 1.0)
    alpha_byte = np.rint(alpha * 255.0).astype(np.uint8)
    premultiplied = np.rint(image.astype(np.float32) * alpha[..., None]).astype(np.uint8)
    return np.dstack((premultiplied, alpha_byte)).tobytes()


def make_preview(
    rows: list[tuple[str, np.ndarray, np.ndarray, np.ndarray]],
    output: Path,
) -> None:
    if not rows:
        return
    cell_h, cell_w = rows[0][1].shape[:2]
    scale = 2
    board = np.full((len(rows) * cell_h * scale, cell_w * scale * 3, 3), 18, np.uint8)
    for row, (label, source, teacher, result) in enumerate(rows):
        y0 = row * cell_h * scale
        for column, image in enumerate((source, teacher, result)):
            enlarged = cv2.resize(image, (cell_w * scale, cell_h * scale), interpolation=cv2.INTER_NEAREST)
            x0 = column * cell_w * scale
            board[y0 : y0 + cell_h * scale, x0 : x0 + cell_w * scale] = enlarged
        cv2.putText(
            board,
            label,
            (6, y0 + 20),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.55,
            (245, 245, 245),
            1,
            cv2.LINE_AA,
        )
    cv2.imwrite(str(output), board)


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

    source_path = require_e_temp(Path(proof["source"]), must_exist=True)
    teacher_path = require_e_temp(Path(proof["teacher"]), must_exist=True)
    audio_path = require_e_temp(Path(proof["enrollment_audio"]), must_exist=True)
    if sha256(audio_path) != proof["enrollment_audio_sha256"]:
        raise ValueError("enrollment audio hash changed")
    source_frames, source_fps = read_video(source_path)
    teacher_frames, teacher_fps = read_video(teacher_path)
    if abs(source_fps - teacher_fps) > 0.01:
        raise ValueError("source and teacher frame rates differ")
    samples, rate, channels = read_wav(audio_path)

    texture = bytearray()
    review_states: list[dict[str, object]] = []
    quality_states: list[dict[str, object]] = []
    preview_rows: list[tuple[str, np.ndarray, np.ndarray, np.ndarray]] = []
    for output_index, state in enumerate(states):
        frame_index = int(state["state_index"])
        if frame_index < 0 or frame_index >= min(len(source_frames), len(teacher_frames)):
            raise ValueError("atlas state index is outside the source or teacher video")
        source_frame = source_frames[frame_index]
        teacher_frame = teacher_frames[frame_index]
        if sha256_bytes(source_frame.tobytes()) != state["source_frame_sha256"]:
            raise ValueError(f"decoded source frame {frame_index} hash changed")
        if sha256_bytes(teacher_frame.tobytes()) != state["teacher_frame_sha256"]:
            raise ValueError(f"decoded teacher frame {frame_index} hash changed")

        binding = state["audio_binding"]
        first = int(binding["first_sample_index"])
        count = int(binding["sample_count"])
        if count <= 0 or first < 0 or first + count > samples.size:
            raise ValueError("enrollment audio interval is invalid")

        source_canonical, grid_x, grid_y = canonical_image(
            source_frame, state["geometry"], args.width, args.height
        )
        teacher_canonical, _, _ = canonical_image(
            teacher_frame, state["geometry"], args.width, args.height
        )
        viseme_class = str(state.get("viseme_class", "unknown"))
        if viseme_class == "silence":
            # Silence is a zero oral residual.  The native contour path keeps
            # or closes the current source lips instead of pasting a generated
            # teacher frame whose audio label may not match its appearance.
            selected = np.zeros_like(source_canonical)
            alpha = np.zeros(source_canonical.shape[:2], np.float32)
            mask_metrics = {
                "differenceThreshold": 0.0,
                "alphaPixelShare": 0.0,
                "opaquePixelShare": 0.0,
                "toothBandAspect": 0.0,
            }
            detail_metrics = {
                "teacherLaplacianVariance": 0.0,
                "enhancedLaplacianVariance": 0.0,
            }
            source_override = True
        else:
            selected, alpha, oral_metrics = normalized_oral_texture(
                source_canonical, teacher_canonical, grid_x, grid_y
            )
            mask_metrics = {
                "differenceThreshold": 0.0,
                "alphaPixelShare": oral_metrics["oralAlphaPixelShare"],
                "opaquePixelShare": round(float(np.mean(alpha >= 0.94)), 6),
                "toothBandAspect": oral_metrics["toothBandAspect"],
                "oralSourceBounds": oral_metrics["oralSourceBounds"],
                "darkThreshold": oral_metrics["darkThreshold"],
            }
            detail_metrics = {
                "teacherLaplacianVariance": oral_metrics["teacherLaplacianVariance"],
                "enhancedLaplacianVariance": oral_metrics["enhancedLaplacianVariance"],
            }
            source_override = False

        texture.extend(premultiply(selected, alpha))
        coefficients = native_coefficients_from_pcm(samples[first : first + count], rate)
        review_states.append(
            {
                "index": output_index,
                "coefficients": coefficients,
                "enrolledPose": [0.0, 0.0, math.degrees(float(state["geometry"]["roll"]))],
            }
        )
        quality_states.append(
            {
                "index": output_index,
                "sourceStateIndex": frame_index,
                "visemeClass": viseme_class,
                "sourceClosureOverride": source_override,
                **mask_metrics,
                **detail_metrics,
            }
        )
        composite = np.rint(
            selected.astype(np.float32) * alpha[..., None]
            + source_canonical.astype(np.float32) * (1.0 - alpha[..., None])
        ).astype(np.uint8)
        preview_rows.append((f"state {output_index} / {viseme_class}", source_canonical, teacher_canonical, composite))

    output.mkdir(parents=True)
    texture_path = output / "atlas-bgra8-premultiplied.bin"
    texture_path.write_bytes(texture)
    state_bytes = args.width * args.height * 4
    lineage = sha256_bytes(
        (
            sha256(proof_root / "proof.json")
            + "\n"
            + sha256(proof_root / "atlas-manifest.json")
            + "\nsource-preserving-v2"
        ).encode()
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
    quality_path = output / "atlas-quality.json"
    quality_path.write_text(
        json.dumps(
            {
                "schema": "interactive-npcs-source-preserving-atlas-quality/v1",
                "scope": "private-synthetic-mara-review-only",
                "source": str(source_path),
                "teacher": str(teacher_path),
                "sourceFps": source_fps,
                "teacherFps": teacher_fps,
                "states": quality_states,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    preview_path = output / "atlas-source-teacher-result-board.png"
    make_preview(preview_rows, preview_path)
    print(
        json.dumps(
            {
                "status": "prepared",
                "scope": "private-synthetic-mara-only",
                "output": str(output),
                "states": len(review_states),
                "textureBytes": len(texture),
                "textureSha256": sha256(texture_path),
                "manifestSha256": sha256(manifest_path),
                "quality": str(quality_path),
                "preview": str(preview_path),
            }
        )
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"source-preserving review atlas error: {error}", file=sys.stderr)
        sys.exit(2)
