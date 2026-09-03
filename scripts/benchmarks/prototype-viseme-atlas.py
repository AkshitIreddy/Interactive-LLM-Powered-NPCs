#!/usr/bin/env python3
"""Build and measure a tiny per-character mouth atlas from a teacher video.

This is an architecture proof, not a distributable visual pack.  A heavyweight
teacher such as MuseTalk runs only during character enrollment.  The resulting
mouth patches, audio centroids and feather mask are small enough to use without
keeping the teacher model resident beside a game.

The proof deliberately separates two questions:

* ``teacher-atlas-preview.mp4`` uses visual labels from the teacher frames and
  measures how much quality is lost by quantizing the mouth to a tiny atlas.
* ``audio-atlas-preview.mp4`` uses a dependency-free MFCC-like nearest-centroid
  driver.  It demonstrates the runtime shape, but does not qualify phoneme or
  viseme accuracy.  Production should prefer provider viseme timings whenever
  the selected TTS service exposes them.

All generated data must be written below E:\\temp.  The script does not access
the network, credentials, or the shared GPU marker.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import statistics
import time
import wave
from pathlib import Path
from typing import Any, Iterable, Sequence


SCHEMA = "interactive-npcs-character-mouth-atlas-proof/v1"


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


def parse_box(value: str) -> tuple[int, int, int, int]:
    try:
        coordinates = tuple(int(round(float(item.strip()))) for item in value.split(","))
    except ValueError as error:
        raise argparse.ArgumentTypeError("box must be x1,y1,x2,y2") from error
    if len(coordinates) != 4:
        raise argparse.ArgumentTypeError("box must contain exactly four coordinates")
    x1, y1, x2, y2 = coordinates
    if min(coordinates) < 0 or x2 <= x1 or y2 <= y1:
        raise argparse.ArgumentTypeError("box must have non-negative ordered coordinates")
    return x1, y1, x2, y2


def require_file(path: Path, label: str) -> Path:
    resolved = path.resolve(strict=True)
    if not resolved.is_file() or resolved.is_symlink():
        raise RuntimeError(f"{label} must be a regular non-symlink file")
    return resolved


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    normalized = str(resolved).replace("/", "\\").lower()
    if resolved.drive.lower() != "e:" or not normalized.startswith("e:\\temp\\"):
        raise RuntimeError("output root must be a new directory below E:\\temp")
    if resolved.exists():
        raise RuntimeError("output root already exists; use a fresh proof directory")
    return resolved


def validate_box(box: tuple[int, int, int, int], width: int, height: int, label: str) -> None:
    x1, y1, x2, y2 = box
    if x2 > width or y2 > height or x2 - x1 < 16 or y2 - y1 < 16:
        raise RuntimeError(f"{label} is outside the image or too small")


def derive_mouth_box(
    face_box: tuple[int, int, int, int], width: int, height: int
) -> tuple[int, int, int, int]:
    """Return a conservative lower-face box for frontal enrollment portraits.

    The runtime must replace this enrollment geometry with live semantic mouth
    landmarks.  These ratios merely prevent the proof from learning MuseTalk's
    broad lower-face blending boundary as if it were a mouth texture.
    """

    x1, y1, x2, y2 = face_box
    validate_box(face_box, width, height, "face box")
    face_width = x2 - x1
    face_height = y2 - y1
    mouth = (
        max(0, int(round(x1 + face_width * 0.18))),
        max(0, int(round(y1 + face_height * 0.58))),
        min(width, int(round(x1 + face_width * 0.84))),
        min(height, int(round(y1 + face_height * 0.93))),
    )
    validate_box(mouth, width, height, "derived mouth box")
    return mouth


def feather_mask(width: int, height: int) -> Any:
    import numpy as np

    if width < 2 or height < 2:
        raise ValueError("feather mask requires at least 2 by 2 pixels")
    x = (np.arange(width, dtype=np.float32) + 0.5 - width / 2.0) / (width / 2.0)
    y = (np.arange(height, dtype=np.float32) + 0.5 - height / 2.0) / (height / 2.0)
    radius = np.sqrt((x[None, :] / 1.0) ** 2 + (y[:, None] / 0.93) ** 2)
    # Opaque across most of the mouth, then a wide cosine-like falloff so a
    # teacher patch never introduces a hard rectangular edge.
    alpha = np.clip((1.0 - radius) / 0.22, 0.0, 1.0)
    alpha = alpha * alpha * (3.0 - 2.0 * alpha)
    return np.rint(alpha * 255.0).astype(np.uint8)


def pairwise_squared(features: Any) -> Any:
    import numpy as np

    values = np.asarray(features, dtype=np.float32)
    squares = np.sum(values * values, axis=1, keepdims=True)
    return np.maximum(squares + squares.T - 2.0 * values @ values.T, 0.0)


def k_medoids(features: Any, state_count: int, neutral_feature: Any) -> tuple[list[int], Any]:
    """Deterministic farthest-first PAM for the small enrollment sequence."""

    import numpy as np

    values = np.asarray(features, dtype=np.float32)
    if values.ndim != 2 or values.shape[0] < state_count or state_count < 2:
        raise ValueError("state count must be from 2 through the number of frames")
    neutral = np.asarray(neutral_feature, dtype=np.float32).reshape(1, -1)
    if neutral.shape[1] != values.shape[1]:
        raise ValueError("neutral feature width does not match frame features")

    distances = pairwise_squared(values)
    first = int(np.argmin(np.sum((values - neutral) ** 2, axis=1)))
    medoids = [first]
    while len(medoids) < state_count:
        nearest = np.min(distances[:, medoids], axis=1)
        nearest[medoids] = -1.0
        medoids.append(int(np.argmax(nearest)))

    assignments = np.argmin(distances[:, medoids], axis=1)
    for _ in range(64):
        updated: list[int] = []
        for state in range(state_count):
            members = np.flatnonzero(assignments == state)
            if members.size == 0:
                nearest = np.min(distances[:, medoids], axis=1)
                nearest[medoids + updated] = -1.0
                updated.append(int(np.argmax(nearest)))
                continue
            local = distances[np.ix_(members, members)]
            updated.append(int(members[int(np.argmin(np.sum(local, axis=1)))]))
        if updated == medoids:
            break
        medoids = updated
        assignments = np.argmin(distances[:, medoids], axis=1)

    # State zero is always the patch nearest the enrollment portrait.  This
    # gives silence a stable, closed-looking fallback even without transcripts.
    neutral_state = medoids.index(min(medoids, key=lambda index: float(
        np.sum((values[index] - neutral) ** 2)
    )))
    if neutral_state != 0:
        medoids[0], medoids[neutral_state] = medoids[neutral_state], medoids[0]
    assignments = np.argmin(distances[:, medoids], axis=1)
    return medoids, assignments.astype(np.int32)


def smooth_labels(labels: Sequence[int]) -> list[int]:
    result = [int(value) for value in labels]
    if len(result) < 3:
        return result
    # Remove one-frame A-B-A spikes. Longer transitions are preserved because
    # they may be real consonants, and providers can supply their exact timing.
    for index in range(1, len(result) - 1):
        if result[index - 1] == result[index + 1] != result[index]:
            result[index] = result[index - 1]
    return result


def read_wav_mono(path: Path) -> tuple[Any, int]:
    import numpy as np

    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        sample_width = stream.getsampwidth()
        sample_rate = stream.getframerate()
        frame_count = stream.getnframes()
        raw = stream.readframes(frame_count)
    if channels < 1 or sample_rate < 8_000 or sample_width not in (1, 2, 3, 4):
        raise RuntimeError("audio must be PCM WAV with 8/16/24/32-bit samples")
    if sample_width == 1:
        values = np.frombuffer(raw, dtype=np.uint8).astype(np.float32)
        values = (values - 128.0) / 128.0
    elif sample_width == 2:
        values = np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0
    elif sample_width == 3:
        bytes_ = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 3)
        integers = (bytes_[:, 0].astype(np.int32) |
                    (bytes_[:, 1].astype(np.int32) << 8) |
                    (bytes_[:, 2].astype(np.int32) << 16))
        integers = np.where(integers & 0x800000, integers - 0x1000000, integers)
        values = integers.astype(np.float32) / 8388608.0
    else:
        values = np.frombuffer(raw, dtype="<i4").astype(np.float32) / 2147483648.0
    values = values.reshape(-1, channels).mean(axis=1)
    return values, sample_rate


def hz_to_mel(frequency: Any) -> Any:
    import numpy as np

    return 2595.0 * np.log10(1.0 + np.asarray(frequency) / 700.0)


def mel_to_hz(mel: Any) -> Any:
    import numpy as np

    return 700.0 * (10.0 ** (np.asarray(mel) / 2595.0) - 1.0)


def audio_features_for_frames(samples: Any, sample_rate: int, frame_count: int, fps: float) -> Any:
    """Small CPU log-mel/DCT feature path; no neural model is loaded."""

    import numpy as np

    if frame_count < 1 or fps <= 0.0:
        raise ValueError("frame count and fps must be positive")
    window_length = max(128, int(round(sample_rate * 0.025)))
    fft_size = 1
    while fft_size < window_length:
        fft_size *= 2
    mel_count = 24
    coefficient_count = 13
    upper_hz = min(7_600.0, sample_rate / 2.0)
    mel_points = np.linspace(hz_to_mel(70.0), hz_to_mel(upper_hz), mel_count + 2)
    bins = np.floor((fft_size + 1) * mel_to_hz(mel_points) / sample_rate).astype(np.int32)
    filters = np.zeros((mel_count, fft_size // 2 + 1), dtype=np.float32)
    for index in range(mel_count):
        left, center, right = int(bins[index]), int(bins[index + 1]), int(bins[index + 2])
        center = max(center, left + 1)
        right = max(right, center + 1)
        for bin_index in range(left, min(center, filters.shape[1])):
            filters[index, bin_index] = (bin_index - left) / (center - left)
        for bin_index in range(center, min(right, filters.shape[1])):
            filters[index, bin_index] = (right - bin_index) / (right - center)
    dct = np.cos(
        math.pi / mel_count *
        (np.arange(mel_count, dtype=np.float32)[None, :] + 0.5) *
        np.arange(coefficient_count, dtype=np.float32)[:, None]
    )
    window = np.hanning(window_length).astype(np.float32)
    feature_rows = []
    half = window_length // 2
    padded = np.pad(np.asarray(samples, dtype=np.float32), (half, half))
    for frame_index in range(frame_count):
        center = int(round((frame_index + 0.5) * sample_rate / fps)) + half
        segment = padded[center - half:center - half + window_length]
        if segment.shape[0] < window_length:
            segment = np.pad(segment, (0, window_length - segment.shape[0]))
        emphasized = segment.copy()
        emphasized[1:] -= 0.97 * segment[:-1]
        spectrum = np.abs(np.fft.rfft(emphasized * window, n=fft_size)) ** 2
        log_mel = np.log(np.maximum(filters @ spectrum, 1e-8))
        feature_rows.append(dct @ log_mel)
    base = np.asarray(feature_rows, dtype=np.float32)
    delta = np.gradient(base, axis=0) if frame_count > 1 else np.zeros_like(base)
    combined = np.concatenate([base, delta], axis=1)
    mean = combined.mean(axis=0, keepdims=True)
    scale = combined.std(axis=0, keepdims=True)
    return (combined - mean) / np.maximum(scale, 1e-4)


def state_centroids(features: Any, labels: Sequence[int], state_count: int) -> Any:
    import numpy as np

    values = np.asarray(features, dtype=np.float32)
    labels_array = np.asarray(labels, dtype=np.int32)
    centroids = []
    overall = values.mean(axis=0)
    for state in range(state_count):
        members = values[labels_array == state]
        centroids.append(members.mean(axis=0) if len(members) else overall)
    return np.asarray(centroids, dtype=np.float32)


def nearest_centroid(features: Any, centroids: Any) -> Any:
    import numpy as np

    values = np.asarray(features, dtype=np.float32)
    prototypes = np.asarray(centroids, dtype=np.float32)
    distances = np.sum((values[:, None, :] - prototypes[None, :, :]) ** 2, axis=2)
    return np.argmin(distances, axis=1).astype(np.int32)


def visual_features(patches: Any) -> Any:
    import cv2
    import numpy as np

    rows = []
    for patch in patches:
        small = cv2.resize(patch, (48, 32), interpolation=cv2.INTER_AREA)
        lab = cv2.cvtColor(small, cv2.COLOR_BGR2LAB).astype(np.float32)
        gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY).astype(np.float32)
        gradient_x = cv2.Sobel(gray, cv2.CV_32F, 1, 0, ksize=3)
        gradient_y = cv2.Sobel(gray, cv2.CV_32F, 0, 1, ksize=3)
        edges = np.sqrt(gradient_x * gradient_x + gradient_y * gradient_y)
        feature = np.concatenate([
            lab[:, :, 0].reshape(-1) / 255.0,
            lab[:, :, 1:].reshape(-1) / 255.0,
            edges.reshape(-1) / 255.0,
        ])
        rows.append(feature)
    values = np.asarray(rows, dtype=np.float32)
    mean = values.mean(axis=0, keepdims=True)
    scale = values.std(axis=0, keepdims=True)
    return (values - mean) / np.maximum(scale, 0.03)


def blend_patch(base: Any, patch: Any, alpha: Any) -> Any:
    import numpy as np

    background = np.asarray(base, dtype=np.uint16)
    foreground = np.asarray(patch, dtype=np.uint16)
    opacity = np.asarray(alpha, dtype=np.uint16)[:, :, None]
    return ((foreground * opacity + background * (255 - opacity) + 127) // 255).astype(np.uint8)


def make_contact_sheet(atlas: Any, alpha: Any, counts: Sequence[int], medoids: Sequence[int]) -> Any:
    import cv2
    import numpy as np

    patch_height, patch_width = atlas.shape[1:3]
    scale = min(2.0, 300.0 / max(patch_width, patch_height))
    display_width = max(160, int(round(patch_width * scale)))
    display_height = max(112, int(round(patch_height * scale)))
    columns = 4
    rows = math.ceil(len(atlas) / columns)
    tile_width = display_width + 24
    tile_height = display_height + 58
    sheet = np.full((rows * tile_height + 24, columns * tile_width + 24, 3), 24, np.uint8)
    checker = np.full((patch_height, patch_width, 3), 96, np.uint8)
    step = 12
    for y in range(0, patch_height, step):
        for x in range(0, patch_width, step):
            if (x // step + y // step) % 2 == 0:
                checker[y:y + step, x:x + step] = 132
    for state, patch in enumerate(atlas):
        preview = blend_patch(checker, patch, alpha)
        preview = cv2.resize(preview, (display_width, display_height), interpolation=cv2.INTER_CUBIC)
        row, column = divmod(state, columns)
        left = 24 + column * tile_width
        top = 24 + row * tile_height
        sheet[top:top + display_height, left:left + display_width] = preview
        cv2.putText(sheet, f"state {state}  frame {medoids[state]}",
                    (left, top + display_height + 22), cv2.FONT_HERSHEY_SIMPLEX,
                    0.48, (235, 235, 235), 1, cv2.LINE_AA)
        cv2.putText(sheet, f"teacher frames: {counts[state]}",
                    (left, top + display_height + 43), cv2.FONT_HERSHEY_SIMPLEX,
                    0.43, (165, 203, 255), 1, cv2.LINE_AA)
    return sheet


def make_comparison(
    portrait: Any,
    teacher: Any,
    teacher_atlas: Any,
    audio_atlas: Any,
    face_box: tuple[int, int, int, int],
) -> Any:
    import cv2
    import numpy as np

    x1, y1, x2, y2 = face_box
    width = x2 - x1
    height = y2 - y1
    x1 = max(0, x1 - width // 3)
    x2 = min(portrait.shape[1], x2 + width // 3)
    y1 = max(0, y1 - height // 5)
    y2 = min(portrait.shape[0], y2 + height // 5)
    entries = [
        ("enrollment portrait", portrait),
        ("full teacher frame", teacher),
        ("8-state atlas / visual label", teacher_atlas),
        ("8-state atlas / audio label", audio_atlas),
    ]
    panel_width, panel_height = 330, 430
    board = np.full((panel_height + 56, panel_width * len(entries), 3), 18, np.uint8)
    for index, (label, image) in enumerate(entries):
        crop = image[y1:y2, x1:x2]
        resized = cv2.resize(crop, (panel_width, panel_height), interpolation=cv2.INTER_LANCZOS4)
        left = index * panel_width
        board[:panel_height, left:left + panel_width] = resized
        cv2.putText(board, label, (left + 10, panel_height + 34), cv2.FONT_HERSHEY_SIMPLEX,
                    0.55, (235, 235, 235), 1, cv2.LINE_AA)
    return board


def make_sequence_board(
    teacher_preview: Sequence[Any],
    audio_preview: Sequence[Any],
    face_box: tuple[int, int, int, int],
) -> Any:
    import cv2
    import numpy as np

    if len(teacher_preview) != len(audio_preview) or not teacher_preview:
        raise ValueError("sequence previews must be non-empty and aligned")
    x1, y1, x2, y2 = face_box
    face_width = x2 - x1
    face_height = y2 - y1
    x1 = max(0, x1 - face_width // 4)
    x2 = min(teacher_preview[0].shape[1], x2 + face_width // 4)
    y1 = max(0, y1 - face_height // 8)
    y2 = min(teacher_preview[0].shape[0], y2 + face_height // 8)
    sample_count = min(6, len(teacher_preview))
    indices = np.rint(np.linspace(0, len(teacher_preview) - 1, sample_count)).astype(np.int32)
    panel_width, panel_height = 210, 260
    label_height = 42
    board = np.full((2 * (panel_height + label_height), sample_count * panel_width, 3), 18, np.uint8)
    for column, frame_index in enumerate(indices.tolist()):
        for row, (label, frames) in enumerate((
            ("visual", teacher_preview),
            ("audio", audio_preview),
        )):
            crop = frames[frame_index][y1:y2, x1:x2]
            resized = cv2.resize(crop, (panel_width, panel_height), interpolation=cv2.INTER_LANCZOS4)
            top = row * (panel_height + label_height)
            left = column * panel_width
            board[top:top + panel_height, left:left + panel_width] = resized
            cv2.putText(board, f"{label}  f{frame_index}",
                        (left + 8, top + panel_height + 27), cv2.FONT_HERSHEY_SIMPLEX,
                        0.48, (235, 235, 235), 1, cv2.LINE_AA)
    return board


def write_video(path: Path, frames: Sequence[Any], fps: float, width: int, height: int) -> None:
    import cv2

    writer = cv2.VideoWriter(str(path), cv2.VideoWriter_fourcc(*"mp4v"), fps, (width, height))
    if not writer.isOpened():
        raise RuntimeError("OpenCV could not open the MP4 proof writer")
    try:
        for frame in frames:
            writer.write(frame)
    finally:
        writer.release()
    if not path.is_file() or path.stat().st_size < 1024:
        raise RuntimeError("MP4 proof writer did not produce a usable file")


def main() -> int:
    import cv2
    import numpy as np

    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--portrait", type=Path, required=True)
    parser.add_argument("--teacher-video", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--face-box", type=parse_box, required=True)
    parser.add_argument("--mouth-box", type=parse_box)
    parser.add_argument("--profile-id", default="proof-only")
    parser.add_argument("--identity-revision", default="portrait-sha256")
    parser.add_argument("--teacher-id", default="external-teacher-video")
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--states", type=int, default=8)
    parser.add_argument("--benchmark-iterations", type=int, default=1000)
    args = parser.parse_args()
    if args.states < 4 or args.states > 16:
        raise RuntimeError("states must be from 4 through 16")
    if args.benchmark_iterations < 100 or args.benchmark_iterations > 100_000:
        raise RuntimeError("benchmark iterations must be from 100 through 100000")

    portrait_path = require_file(args.portrait, "portrait")
    teacher_path = require_file(args.teacher_video, "teacher video")
    audio_path = require_file(args.audio, "audio")
    output_root = require_e_temp(args.output_root)
    output_root.mkdir(parents=True)

    portrait = cv2.imread(str(portrait_path), cv2.IMREAD_COLOR)
    if portrait is None:
        raise RuntimeError("portrait could not be decoded")
    capture = cv2.VideoCapture(str(teacher_path))
    if not capture.isOpened():
        raise RuntimeError("teacher video could not be opened")
    fps = float(capture.get(cv2.CAP_PROP_FPS))
    frames = []
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        frames.append(frame)
    capture.release()
    if len(frames) < args.states:
        raise RuntimeError("teacher video has fewer frames than requested atlas states")
    if not math.isfinite(fps) or fps <= 0.0:
        raise RuntimeError("teacher video has an invalid frame rate")

    video_height, video_width = frames[0].shape[:2]
    if portrait.shape[1] != video_width or abs(portrait.shape[0] - video_height) > 2:
        raise RuntimeError("portrait and teacher video dimensions do not match")
    portrait = portrait[:video_height, :video_width]
    for frame in frames:
        if frame.shape[:2] != (video_height, video_width):
            raise RuntimeError("teacher video changes dimensions between frames")

    validate_box(args.face_box, video_width, video_height, "face box")
    mouth_box = args.mouth_box or derive_mouth_box(args.face_box, video_width, video_height)
    validate_box(mouth_box, video_width, video_height, "mouth box")
    mx1, my1, mx2, my2 = mouth_box
    portrait_patch = portrait[my1:my2, mx1:mx2]
    teacher_patches = np.stack([frame[my1:my2, mx1:mx2] for frame in frames])
    # Normalize the portrait reference and teacher patches in one feature
    # space.  Normalizing the reference separately would collapse it to zero
    # and accidentally select the sequence mean rather than the true neutral.
    all_visual_features = visual_features(np.concatenate([
        teacher_patches,
        portrait_patch[None, :, :, :],
    ], axis=0))
    features = all_visual_features[:-1]
    neutral_feature = all_visual_features[-1]
    medoids, teacher_labels = k_medoids(features, args.states, neutral_feature)
    teacher_labels = np.asarray(smooth_labels(teacher_labels.tolist()), dtype=np.int32)
    atlas = teacher_patches[medoids].copy()
    alpha = feather_mask(mx2 - mx1, my2 - my1)

    samples, sample_rate = read_wav_mono(audio_path)
    audio_features = audio_features_for_frames(samples, sample_rate, len(frames), fps)
    centroids = state_centroids(audio_features, teacher_labels, args.states)
    audio_labels = np.asarray(smooth_labels(nearest_centroid(audio_features, centroids).tolist()),
                              dtype=np.int32)
    audio_training_accuracy = float(np.mean(audio_labels == teacher_labels))

    teacher_preview = []
    audio_preview = []
    for teacher_state, audio_state in zip(teacher_labels, audio_labels):
        teacher_frame = portrait.copy()
        teacher_frame[my1:my2, mx1:mx2] = blend_patch(
            portrait_patch, atlas[int(teacher_state)], alpha
        )
        teacher_preview.append(teacher_frame)
        audio_frame = portrait.copy()
        audio_frame[my1:my2, mx1:mx2] = blend_patch(
            portrait_patch, atlas[int(audio_state)], alpha
        )
        audio_preview.append(audio_frame)

    counts = [int(np.count_nonzero(teacher_labels == state)) for state in range(args.states)]
    contact_sheet_path = output_root / "atlas-contact-sheet.png"
    if not cv2.imwrite(str(contact_sheet_path), make_contact_sheet(atlas, alpha, counts, medoids)):
        raise RuntimeError("contact sheet could not be written")
    teacher_video_path = output_root / "teacher-atlas-preview.mp4"
    audio_video_path = output_root / "audio-atlas-preview.mp4"
    write_video(teacher_video_path, teacher_preview, fps, video_width, video_height)
    write_video(audio_video_path, audio_preview, fps, video_width, video_height)

    diversity = [float(np.mean(np.abs(patch.astype(np.int16) - portrait_patch.astype(np.int16))))
                 for patch in teacher_patches]
    comparison_index = int(np.argmax(diversity))
    comparison_path = output_root / "comparison-closeup.png"
    comparison = make_comparison(
        portrait,
        frames[comparison_index],
        teacher_preview[comparison_index],
        audio_preview[comparison_index],
        args.face_box,
    )
    if not cv2.imwrite(str(comparison_path), comparison):
        raise RuntimeError("comparison close-up could not be written")
    sequence_board_path = output_root / "sequence-board.png"
    if not cv2.imwrite(
        str(sequence_board_path),
        make_sequence_board(teacher_preview, audio_preview, args.face_box),
    ):
        raise RuntimeError("sequence board could not be written")

    pack_path = output_root / "atlas-proof.npz"
    np.savez_compressed(
        pack_path,
        atlas_bgr=atlas,
        alpha=alpha,
        audio_centroids=centroids,
        medoid_frame_indices=np.asarray(medoids, dtype=np.int32),
    )
    premultiplied_bgr = (
        atlas.astype(np.uint16) * alpha[None, :, :, None].astype(np.uint16) + 127
    ) // 255
    atlas_bgra = np.concatenate([
        premultiplied_bgr.astype(np.uint8),
        np.broadcast_to(alpha[None, :, :, None], (*atlas.shape[:3], 1)),
    ], axis=3)
    binary_path = output_root / "atlas-bgra8-premultiplied.bin"
    binary_path.write_bytes(atlas_bgra.tobytes(order="C"))
    state_bytes = int(atlas_bgra.shape[1] * atlas_bgra.shape[2] * atlas_bgra.shape[3])
    portrait_digest = sha256(portrait_path)
    teacher_digest = sha256(teacher_path)
    audio_digest = sha256(audio_path)
    atlas_manifest = {
        "schemaVersion": 1,
        "artifactType": "character-mouth-atlas",
        "status": "prototype",
        "characterBinding": {
            "profileId": args.profile_id,
            "identityRevision": (
                portrait_digest if args.identity_revision == "portrait-sha256"
                else args.identity_revision
            ),
            "portraitSha256": portrait_digest,
        },
        "source": {
            "teacherId": args.teacher_id,
            "teacherVideoSha256": teacher_digest,
            "audioSha256": audio_digest,
            "faceBox": list(args.face_box),
            "mouthBox": list(mouth_box),
        },
        "texture": {
            "file": binary_path.name,
            "sha256": sha256(binary_path),
            "bytes": binary_path.stat().st_size,
            "pixelFormat": "bgra8-unorm",
            "alphaMode": "premultiplied",
            "width": int(atlas_bgra.shape[2]),
            "height": int(atlas_bgra.shape[1]),
            "strideBytes": int(atlas_bgra.shape[2] * 4),
            "stateCount": args.states,
            "stateBytes": state_bytes,
            "neutralStateIndex": 0,
        },
        "states": [
            {
                "index": state,
                "byteOffset": state * state_bytes,
                "medoidFrameIndex": int(medoids[state]),
                "teacherFrameCount": counts[state],
            }
            for state in range(args.states)
        ],
        "driver": {
            "kind": "same-clip-audio-centroids-proof",
            "audioFeatureDimensions": int(audio_features.shape[1]),
            "crossUtteranceQualified": False,
        },
    }
    atlas_manifest_path = output_root / "atlas-manifest.json"
    atlas_manifest_path.write_text(
        json.dumps(atlas_manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    # Benchmark the exact CPU operations used by the proof: one 26-value audio
    # distance lookup and one pre-bounded mouth-patch composite.  Full-frame
    # capture/copy/presentation is intentionally excluded and must be measured
    # by the native product benchmark.
    timings_ms = []
    checksum = 0
    for iteration in range(args.benchmark_iterations + 20):
        feature = audio_features[iteration % len(audio_features)]
        base = portrait_patch.copy()
        started = time.perf_counter_ns()
        state = int(nearest_centroid(feature[None, :], centroids)[0])
        blended = blend_patch(base, atlas[state], alpha)
        elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000.0
        checksum = (checksum * 131 + int(blended[blended.shape[0] // 2,
                                                       blended.shape[1] // 2, 1])) & 0xFFFFFFFFFFFFFFFF
        if iteration >= 20:
            timings_ms.append(elapsed_ms)

    report = {
        "schema": SCHEMA,
        "status": "prototype-measured",
        "scope": "static-enrollment-portrait-atlas-proof-not-current-game-frame-not-app-e2e",
        "inputs": {
            "portrait_sha256": portrait_digest,
            "teacher_video_sha256": teacher_digest,
            "audio_sha256": audio_digest,
            "portrait_size": [video_width, video_height],
            "teacher_frame_count": len(frames),
            "fps": round(fps, 3),
            "face_box": list(args.face_box),
            "mouth_box": list(mouth_box),
            "mouth_box_source": "explicit" if args.mouth_box else "bounded-face-ratios",
        },
        "atlas": {
            "state_count": args.states,
            "medoid_frame_indices": medoids,
            "teacher_frame_count_by_state": counts,
            "raw_runtime_bytes": int(atlas.nbytes + alpha.nbytes + centroids.nbytes),
            "compressed_pack_bytes": pack_path.stat().st_size,
            "audio_feature_dimensions": int(audio_features.shape[1]),
            "same-clip_audio_centroid_accuracy": round(audio_training_accuracy, 4),
            "same_clip_accuracy_is_qualification": False,
        },
        "runtime_microbenchmark": {
            "implementation": "python-numpy-nearest-centroid-plus-roi-alpha-composite",
            "iterations": args.benchmark_iterations,
            "mean_ms": round(statistics.fmean(timings_ms), 6),
            "p50_ms": round(percentile(timings_ms, 0.50), 6),
            "p95_ms": round(percentile(timings_ms, 0.95), 6),
            "p99_ms": round(percentile(timings_ms, 0.99), 6),
            "equivalent_fps_from_mean": round(1000.0 / statistics.fmean(timings_ms), 3),
            "checksum": checksum,
            "includes_capture_or_presentation": False,
        },
        "artifacts": {
            path.name: {"bytes": path.stat().st_size, "sha256": sha256(path)}
            for path in (
                pack_path,
                binary_path,
                atlas_manifest_path,
                contact_sheet_path,
                comparison_path,
                sequence_board_path,
                teacher_video_path,
                audio_video_path,
            )
        },
        "claims": {
            "heavy_model_required_at_runtime": False,
            "teacher_used_only_for_enrollment": True,
            "static_portrait_mechanics_proof": True,
            "outside_mouth_pixels_modified": 0,
            "current_game_frame_pipeline": False,
            "live_landmark_warp": False,
            "cross_utterance_viseme_accuracy": False,
            "installed_app_e2e": False,
            "qualified_pack": False,
        },
        "limitations": [
            "The visual-label preview is an upper-bound reconstruction of the same teacher clip.",
            "The audio-label preview is trained and evaluated on one short clip; its score is not generalization evidence.",
            "A production path must warp atlas patches using current-frame mouth landmarks and reject pose, occlusion, stale identity, and stale frame bindings.",
            "Atlas lighting and skin color must be adapted to the current frame or rejected outside the enrolled pose and illumination envelope.",
        ],
    }
    report_path = output_root / "qualification.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "report": str(report_path)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
