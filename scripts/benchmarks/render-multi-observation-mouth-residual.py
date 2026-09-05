#!/usr/bin/env python3
"""Render a bounded multi-observation mouth residual on fresh source frames.

The atlas is private proof data derived from a pinned neural teacher. Runtime
selection uses only the current declared audio interval and current source-mouth
geometry. Pixels outside the warped lip mask remain byte-identical.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import time
import wave

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


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
        raise RuntimeError(f"could not decode {path}")
    return frames, fps


def read_audio(path: Path) -> tuple[np.ndarray, int, int]:
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        rate = stream.getframerate()
        width = stream.getsampwidth()
        raw = stream.readframes(stream.getnframes())
    if width != 2:
        raise RuntimeError("proof accepts signed 16-bit PCM WAV only")
    pcm = np.frombuffer(raw, dtype="<i2").astype(np.float32).reshape(-1, channels)
    return pcm.mean(axis=1) / 32768.0, rate, channels


def classify_descriptor(descriptor: np.ndarray) -> str:
    rms, zcr, centroid, low, mid, high = [float(value) for value in descriptor]
    if rms < 0.008:
        return "silence"
    if zcr > 0.20 or high > 0.22:
        return "fricative"
    if low > 0.72 and centroid < 0.12:
        return "rounded"
    if mid > 0.30:
        return "spread"
    return "open"


def audio_descriptor(samples: np.ndarray, rate: int) -> tuple[np.ndarray, str]:
    if samples.size == 0:
        return np.zeros(6, dtype=np.float32), "silence"
    windowed = samples * np.hanning(samples.size).astype(np.float32)
    spectrum = np.abs(np.fft.rfft(windowed))
    frequencies = np.fft.rfftfreq(samples.size, 1.0 / rate)
    energy = spectrum * spectrum
    total = float(energy.sum()) + 1e-9
    rms = float(np.sqrt(np.mean(samples * samples)))
    zcr = float(np.mean(np.signbit(samples[1:]) != np.signbit(samples[:-1]))) if samples.size > 1 else 0.0
    centroid = float((frequencies * energy).sum() / total) / (rate * 0.5)
    low = float(energy[frequencies < 800].sum() / total)
    mid = float(energy[(frequencies >= 800) & (frequencies < 2500)].sum() / total)
    high = float(energy[frequencies >= 2500].sum() / total)
    descriptor = np.asarray([rms, zcr, centroid, low, mid, high], dtype=np.float32)
    return descriptor, classify_descriptor(descriptor)


def landmark_geometry(records: list[dict], time_seconds: float, source_fps: float) -> dict[str, float]:
    index = min(len(records) - 1, max(0, round(time_seconds * source_fps)))
    record = records[index]
    center = np.asarray(record["center"], dtype=np.float64)
    width = float(record["cornerWidth"])
    return {
        "record_index": index,
        "center_x": float(center[0]),
        "center_y": float(center[1]),
        "width": width,
        "roll": float(record["rollRadians"]),
    }


def affine_for(
    candidate: dict[str, float], target: dict[str, float],
    candidate_width: int, candidate_height: int,
    target_width: int, target_height: int,
) -> np.ndarray:
    scale = (target["width"] * target_width) / max(
        candidate["width"] * candidate_width, 1e-6
    )
    angle = target["roll"] - candidate["roll"]
    cosine, sine = math.cos(angle) * scale, math.sin(angle) * scale
    source_center = np.asarray(
        [candidate["center_x"] * candidate_width, candidate["center_y"] * candidate_height]
    )
    target_center = np.asarray(
        [target["center_x"] * target_width, target["center_y"] * target_height]
    )
    matrix = np.asarray([[cosine, -sine, 0.0], [sine, cosine, 0.0]], dtype=np.float32)
    matrix[:, 2] = target_center - matrix[:, :2] @ source_center
    return matrix


def read_headless_sequence(
    path: Path,
) -> tuple[list[np.ndarray], float, list[dict[str, float]], list[int], dict]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        document.get("schema") != "interactive-npcs-headless-pre-lip-sequence/v1"
        or document.get("status") != "passed"
        or document.get("render_stage") != "post-controlled-blink-and-breath-pre-lip-articulation"
        or document.get("renderer_mouth_pixels_preserved") is not True
        or document.get("window_created") is not False
    ):
        raise RuntimeError("playback sequence did not satisfy the headless pre-lip contract")
    width, height = int(document["rendered_width"]), int(document["rendered_height"])
    fps = float(document["rendered_frame_rate"])
    frames: list[np.ndarray] = []
    geometries: list[dict[str, float]] = []
    content_indices: list[int] = []
    for expected_index, record in enumerate(document["frames"]):
        frame_path = path.parent / record["file"]
        if int(record["frame_index"]) != expected_index or sha256(frame_path) != record["file_sha256"]:
            raise RuntimeError("playback sequence frame identity mismatch")
        frame = cv2.imread(str(frame_path), cv2.IMREAD_COLOR)
        if frame is None or frame.shape[:2] != (height, width):
            raise RuntimeError("playback sequence frame dimensions mismatch")
        geometry = record["mouth_geometry"]
        if geometry.get("accepted") is not True:
            raise RuntimeError("playback sequence has unaccepted mouth geometry")
        frames.append(frame)
        geometries.append({
            "record_index": expected_index,
            "center_x": float(geometry["center"][0]) / width,
            "center_y": float(geometry["center"][1]) / height,
            "width": float(geometry["corner_width_pixels"]) / width,
            "roll": float(geometry["roll_radians"]),
        })
        content_indices.append(int(record["content_frame_index"]))
    if len(frames) != int(document["export_frame_count"]) or fps <= 0:
        raise RuntimeError("playback sequence frame count/cadence mismatch")
    return frames, fps, geometries, content_indices, document


def lip_mask(shape: tuple[int, int], geometry: dict[str, float]) -> np.ndarray:
    height, width = shape
    center = (round(geometry["center_x"] * width), round(geometry["center_y"] * height))
    mouth_width = geometry["width"] * width
    mask = np.zeros((height, width), dtype=np.uint8)
    cv2.ellipse(
        mask,
        center,
        (max(4, round(mouth_width * 0.86)), max(3, round(mouth_width * 0.48))),
        math.degrees(geometry["roll"]), 0, 360, 255, -1, lineType=cv2.LINE_AA,
    )
    return cv2.GaussianBlur(mask, (13, 13), 0)


def target_mouth_region(
    shape: tuple[int, int], geometry: dict[str, float]
) -> tuple[int, int, int, int, np.ndarray]:
    height, width = shape
    center_x = round(geometry["center_x"] * width)
    center_y = round(geometry["center_y"] * height)
    mouth_width = geometry["width"] * width
    axis_x = max(4, round(mouth_width * 0.66))
    axis_y = max(3, round(mouth_width * 0.34))
    radius = max(axis_x, axis_y) + 10
    x0, y0 = max(0, center_x - radius), max(0, center_y - radius)
    x1, y1 = min(width, center_x + radius + 1), min(height, center_y + radius + 1)
    mask = np.zeros((y1 - y0, x1 - x0), dtype=np.uint8)
    cv2.ellipse(
        mask,
        (center_x - x0, center_y - y0),
        (axis_x, axis_y),
        math.degrees(geometry["roll"]), 0, 360, 255, -1, lineType=cv2.LINE_AA,
    )
    return x0, y0, x1, y1, cv2.GaussianBlur(mask, (9, 9), 0)


def normalize_descriptors(values: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    minimum = values.min(axis=0)
    span = np.maximum(values.max(axis=0) - minimum, 1e-6)
    return (values - minimum) / span, minimum, span


def diverse_indices(descriptors: np.ndarray, sharpness: np.ndarray, limit: int) -> list[int]:
    normalized, _, _ = normalize_descriptors(descriptors)
    first = int(np.argmax(sharpness))
    selected = [first]
    while len(selected) < min(limit, len(descriptors)):
        distances = np.min(
            np.linalg.norm(normalized[:, None, :] - normalized[selected][None, :, :], axis=2),
            axis=1,
        )
        distances[selected] = -1.0
        quality = distances * (0.75 + 0.25 * sharpness / max(float(sharpness.max()), 1e-6))
        selected.append(int(np.argmax(quality)))
    return sorted(selected)


def read_frame_directory(path: Path) -> list[np.ndarray]:
    files = sorted(item for item in path.iterdir() if item.suffix.lower() in {".png", ".ppm"})
    frames = [cv2.imread(str(item), cv2.IMREAD_COLOR) for item in files]
    if not frames or any(frame is None for frame in frames):
        raise RuntimeError(f"could not decode baseline frame directory {path}")
    return frames


def make_board(
    source: list[np.ndarray],
    output: list[np.ndarray],
    destination: Path,
    baseline: list[np.ndarray] | None = None,
    baseline_fps: float = 30.0,
    output_fps: float = 25.0,
) -> None:
    chosen = [3, 10, 18, 26]
    x, y, crop_width, crop_height, scale = 390, 285, 200, 150, 3
    label_height = 38
    cell_width, image_height = crop_width * scale, crop_height * scale
    rows: list[tuple[str, list[np.ndarray], float]] = [("SOURCE", source, output_fps)]
    if baseline is not None:
        rows.append(("REJECTED V66", baseline, baseline_fps))
    rows.append(("MULTI-OBS RESIDUAL", output, output_fps))
    board = Image.new(
        "RGB", (cell_width * len(chosen), (image_height + label_height) * len(rows)), (17, 19, 22)
    )
    draw = ImageDraw.Draw(board)
    font = ImageFont.load_default(size=22)
    for column, index in enumerate(chosen):
        for row, (label, frames, frame_fps) in enumerate(rows):
            row_index = min(len(frames) - 1, round(index / output_fps * frame_fps))
            rgb = cv2.cvtColor(frames[row_index][y : y + crop_height, x : x + crop_width], cv2.COLOR_BGR2RGB)
            image = Image.fromarray(rgb).resize((cell_width, image_height), Image.Resampling.LANCZOS)
            top = row * (image_height + label_height)
            left = column * cell_width
            board.paste(image, (left, top + label_height))
            draw.text((left + 10, top + 8), f"{label} · t{index / output_fps:.2f}s", fill=(238, 241, 244), font=font)
    board.save(destination)


def make_display_board(
    source: list[np.ndarray],
    output: list[np.ndarray],
    destination: Path,
    baseline: list[np.ndarray] | None = None,
    baseline_fps: float = 30.0,
    output_fps: float = 25.0,
) -> None:
    chosen = [3, min(len(output) - 1, 18)]
    rows: list[tuple[str, list[np.ndarray], float]] = [("SOURCE", source, output_fps)]
    if baseline is not None:
        rows.append(("REJECTED V66", baseline, baseline_fps))
    rows.append(("MULTI-OBS RESIDUAL", output, output_fps))
    cell_width, cell_height, label_height = 480, 360, 34
    board = Image.new(
        "RGB", (cell_width * len(chosen), (cell_height + label_height) * len(rows)), (17, 19, 22)
    )
    draw = ImageDraw.Draw(board)
    font = ImageFont.load_default(size=20)
    for column, index in enumerate(chosen):
        for row, (label, frames, frame_fps) in enumerate(rows):
            row_index = min(len(frames) - 1, round(index / output_fps * frame_fps))
            rgb = cv2.cvtColor(frames[row_index], cv2.COLOR_BGR2RGB)
            image = Image.fromarray(rgb).resize((cell_width, cell_height), Image.Resampling.LANCZOS)
            left, top = column * cell_width, row * (cell_height + label_height)
            board.paste(image, (left, top + label_height))
            draw.text(
                (left + 10, top + 7), f"{label} · t{index / output_fps:.2f}s",
                fill=(238, 241, 244), font=font,
            )
    board.save(destination)


def make_transition_board(
    source: list[np.ndarray],
    output: list[np.ndarray],
    selections: list[dict[str, object]],
    destination: Path,
    output_fps: float,
) -> None:
    window = min(6, len(output))
    states = [item["selected_atlas_frame_index"] for item in selections]
    start = max(
        range(len(output) - window + 1),
        key=lambda candidate: sum(
            states[index] != states[index - 1]
            for index in range(candidate + 1, candidate + window)
        ),
    )
    chosen = list(range(start, start + window))
    x, y, crop_width, crop_height, scale = 390, 285, 200, 150, 2
    label_height = 42
    cell_width, image_height = crop_width * scale, crop_height * scale
    board = Image.new(
        "RGB", (cell_width * window, (image_height + label_height) * 2), (17, 19, 22)
    )
    draw = ImageDraw.Draw(board)
    font = ImageFont.load_default(size=18)
    for column, index in enumerate(chosen):
        state = selections[index]["selected_atlas_frame_index"]
        viseme = selections[index]["viseme_class"]
        for row, (label, frames) in enumerate((("SOURCE", source), ("RESIDUAL", output))):
            rgb = cv2.cvtColor(
                frames[index][y : y + crop_height, x : x + crop_width], cv2.COLOR_BGR2RGB
            )
            image = Image.fromarray(rgb).resize((cell_width, image_height), Image.Resampling.LANCZOS)
            left, top = column * cell_width, row * (image_height + label_height)
            board.paste(image, (left, top + label_height))
            suffix = f" · s{state} · {viseme}" if row else ""
            draw.text(
                (left + 8, top + 9), f"{label} · t{index / output_fps:.2f}s{suffix}",
                fill=(238, 241, 244), font=font,
            )
    board.save(destination)


def make_atlas_board(
    teacher: list[np.ndarray],
    selected: list[int],
    visemes: list[str],
    destination: Path,
) -> None:
    columns = 6
    rows = math.ceil(len(selected) / columns)
    x, y, crop_width, crop_height, scale = 390, 285, 200, 150, 2
    label_height = 38
    cell_width, image_height = crop_width * scale, crop_height * scale
    board = Image.new(
        "RGB", (cell_width * columns, (image_height + label_height) * rows), (17, 19, 22)
    )
    draw = ImageDraw.Draw(board)
    font = ImageFont.load_default(size=18)
    for position, index in enumerate(selected):
        row, column = divmod(position, columns)
        rgb = cv2.cvtColor(
            teacher[index][y : y + crop_height, x : x + crop_width], cv2.COLOR_BGR2RGB
        )
        image = Image.fromarray(rgb).resize((cell_width, image_height), Image.Resampling.LANCZOS)
        left, top = column * cell_width, row * (image_height + label_height)
        board.paste(image, (left, top + label_height))
        draw.text(
            (left + 8, top + 8), f"STATE {index} · {visemes[index]}",
            fill=(238, 241, 244), font=font,
        )
    board.save(destination)


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--source-video", type=Path, required=True)
    parser.add_argument("--teacher-video", type=Path, required=True)
    parser.add_argument("--enrollment-audio", type=Path, required=True)
    parser.add_argument("--playback-audio", type=Path)
    parser.add_argument("--landmarks", type=Path, required=True)
    parser.add_argument("--landmark-fps", type=float, default=30.0)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--atlas-limit", type=int, default=12)
    parser.add_argument("--baseline-frame-dir", type=Path)
    parser.add_argument("--baseline-fps", type=float, default=30.0)
    parser.add_argument("--ping-pong-playback-source", action="store_true")
    parser.add_argument("--playback-sequence-json", type=Path)
    args = parser.parse_args()
    if args.output.exists() or not 4 <= args.atlas_limit <= 24:
        raise RuntimeError("use a fresh output directory and an atlas limit from 4 through 24")
    args.output.mkdir(parents=True)
    frame_dir = args.output / "frames"
    frame_dir.mkdir()

    decoded_source, enrollment_fps = read_video(args.source_video)
    teacher, teacher_fps = read_video(args.teacher_video)
    enrollment_audio, enrollment_rate, enrollment_channels = read_audio(args.enrollment_audio)
    playback_path = args.playback_audio or args.enrollment_audio
    playback_audio, sample_rate, channels = read_audio(playback_path)
    if enrollment_rate != sample_rate or enrollment_channels != channels:
        raise RuntimeError("enrollment and playback WAV formats must match")
    landmark_document = json.loads(args.landmarks.read_text(encoding="utf-8"))
    accepted = [frame for frame in landmark_document["frames"] if frame.get("accepted")]
    if len(accepted) != len(landmark_document["frames"]):
        raise RuntimeError("proof requires accepted geometry for every source landmark frame")
    playback_sequence_document = None
    if args.playback_sequence_json:
        if args.ping_pong_playback_source:
            raise RuntimeError("headless sequence playback and source ping-pong are mutually exclusive")
        (
            playback_source, playback_fps, playback_geometry_override,
            playback_content_indices, playback_sequence_document,
        ) = read_headless_sequence(args.playback_sequence_json)
    else:
        playback_source = decoded_source
        playback_fps = enrollment_fps
        playback_geometry_override = None
        playback_content_indices = list(range(len(decoded_source)))

    atlas_count = min(
        len(decoded_source), len(teacher),
        int(math.floor(len(enrollment_audio) * enrollment_fps / enrollment_rate)),
    )
    desired_count = int(math.ceil(len(playback_audio) * playback_fps / sample_rate))
    if args.ping_pong_playback_source:
        cycle = list(range(len(playback_source))) + list(range(len(playback_source) - 2, 0, -1))
        source_indices = [cycle[index % len(cycle)] for index in range(desired_count)]
    else:
        source_indices = list(range(min(len(playback_source), desired_count)))
    count = len(source_indices)
    if atlas_count < 8 or count < 8 or abs(enrollment_fps - teacher_fps) > 1e-6:
        raise RuntimeError("source and teacher cadence is incompatible")
    atlas_source = decoded_source[:atlas_count]
    source = [playback_source[index] for index in source_indices]
    teacher = teacher[:atlas_count]
    height, width = source[0].shape[:2]
    atlas_height, atlas_width = teacher[0].shape[:2]

    descriptors: list[np.ndarray] = []
    visemes: list[str] = []
    geometries: list[dict[str, float]] = []
    sharpness: list[float] = []
    enrollment_intervals: list[tuple[int, int, int]] = []
    for index in range(atlas_count):
        first = index * enrollment_rate // round(enrollment_fps)
        end = min(len(enrollment_audio), (index + 1) * enrollment_rate // round(enrollment_fps))
        descriptor, viseme = audio_descriptor(enrollment_audio[first:end], enrollment_rate)
        geometry = landmark_geometry(accepted, index / enrollment_fps, args.landmark_fps)
        center_x = round(geometry["center_x"] * atlas_width)
        center_y = round(geometry["center_y"] * atlas_height)
        radius = max(24, round(geometry["width"] * atlas_width))
        roi = teacher[index][max(0, center_y - radius) : min(atlas_height, center_y + radius),
                             max(0, center_x - radius) : min(atlas_width, center_x + radius)]
        descriptors.append(descriptor)
        visemes.append(viseme)
        geometries.append(geometry)
        sharpness.append(float(cv2.Laplacian(cv2.cvtColor(roi, cv2.COLOR_BGR2GRAY), cv2.CV_64F).var()))
        enrollment_intervals.append((first, end - first, max(first, end - 1)))

    playback_descriptors: list[np.ndarray] = []
    playback_visemes: list[str] = []
    playback_geometries: list[dict[str, float]] = []
    intervals: list[tuple[int, int, int]] = []
    for index in range(count):
        first = index * sample_rate // round(playback_fps)
        end = min(len(playback_audio), (index + 1) * sample_rate // round(playback_fps))
        descriptor, viseme = audio_descriptor(playback_audio[first:end], sample_rate)
        playback_descriptors.append(descriptor)
        playback_visemes.append(viseme)
        playback_geometries.append(
            playback_geometry_override[source_indices[index]]
            if playback_geometry_override is not None
            else landmark_geometry(
                accepted, source_indices[index] / enrollment_fps, args.landmark_fps
            )
        )
        intervals.append((first, end - first, max(first, end - 1)))
    raw_playback_descriptors = playback_descriptors
    playback_descriptors = []
    for index, descriptor in enumerate(raw_playback_descriptors):
        causal = descriptor * 0.60
        causal += raw_playback_descriptors[max(0, index - 1)] * 0.30
        causal += raw_playback_descriptors[max(0, index - 2)] * 0.10
        playback_descriptors.append(causal)
    playback_visemes = [classify_descriptor(descriptor) for descriptor in playback_descriptors]

    descriptor_matrix = np.stack(descriptors)
    sharpness_array = np.asarray(sharpness)
    selected = diverse_indices(descriptor_matrix, sharpness_array, args.atlas_limit)
    selected_descriptors = descriptor_matrix[selected]
    _, descriptor_minimum, descriptor_span = normalize_descriptors(selected_descriptors)
    outputs: list[np.ndarray] = []
    selections: list[dict[str, object]] = []
    render_ms: list[float] = []
    bypasses = 0
    outside_exact = True
    previous_state: int | None = None
    previous_state_age = 0
    width_support = [geometries[index]["width"] for index in selected]
    roll_support = [geometries[index]["roll"] for index in selected]

    def render(index: int, *, occluded: bool = False, roll_override: float | None = None) -> tuple[np.ndarray, np.ndarray, str, int | None]:
        target = dict(playback_geometries[index])
        if roll_override is not None:
            target["roll"] = roll_override
        if occluded:
            return source[index].copy(), np.zeros((height, width), dtype=np.uint8), "bypass_occluded", None
        if target["width"] < min(width_support) * 0.92 or target["width"] > max(width_support) * 1.08 or target["roll"] < min(roll_support) - 0.08 or target["roll"] > max(roll_support) + 0.08:
            return source[index].copy(), np.zeros((height, width), dtype=np.uint8), "bypass_pose_coverage", None
        target_audio = np.clip(
            (playback_descriptors[index] - descriptor_minimum) / descriptor_span, 0.0, 1.0
        )
        atlas_audio = (selected_descriptors - descriptor_minimum) / descriptor_span
        audio_cost = np.linalg.norm(atlas_audio - target_audio[None, :], axis=1)
        pose_cost = np.asarray([
            abs(geometries[state]["roll"] - target["roll"]) * 2.0
            + abs(math.log(max(geometries[state]["width"], 1e-6) / target["width"]))
            for state in selected
        ])
        class_cost = np.asarray([0.0 if visemes[state] == playback_visemes[index] else 0.65 for state in selected])
        continuity = np.asarray([0.0 if previous_state is None else min(abs(state - previous_state) / atlas_count, 0.35) for state in selected])
        costs = audio_cost + pose_cost + class_cost + continuity
        local = int(np.argmin(costs))
        if previous_state in selected:
            previous_local = selected.index(previous_state)
            if previous_state_age < 2 or costs[local] + 0.25 >= costs[previous_local]:
                local = previous_local
        state = selected[local]
        matrix = affine_for(
            geometries[state], target, atlas_width, atlas_height, width, height
        )
        x0, y0, x1, y1, local_mask = target_mouth_region((height, width), target)
        local_matrix = matrix.copy()
        local_matrix[:, 2] -= np.asarray([x0, y0], dtype=np.float32)
        warped = cv2.warpAffine(
            teacher[state], local_matrix, (x1 - x0, y1 - y0),
            flags=cv2.INTER_LANCZOS4, borderMode=cv2.BORDER_REFLECT_101,
        )
        alpha = local_mask.astype(np.float32)[..., None] / 255.0
        source_roi = source[index][y0:y1, x0:x1]
        result = source[index].copy()
        result[y0:y1, x0:x1] = np.clip(
            source_roi.astype(np.float32) * (1.0 - alpha) + warped.astype(np.float32) * alpha,
            0, 255,
        ).astype(np.uint8)
        mask = np.zeros((height, width), dtype=np.uint8)
        mask[y0:y1, x0:x1] = local_mask
        return result, mask, "residual_ready", state

    for index in range(count):
        render_started = time.perf_counter()
        result, mask, disposition, state = render(index)
        render_ms.append((time.perf_counter() - render_started) * 1000.0)
        if state is not None:
            previous_state_age = previous_state_age + 1 if state == previous_state else 1
            previous_state = state
        else:
            bypasses += 1
            previous_state_age = 0
        outside_exact &= bool(np.array_equal(result[mask == 0], source[index][mask == 0]))
        outputs.append(result)
        first, sample_count, playback = intervals[index]
        selections.append({
            "source_frame_index": index,
            "source_pts_seconds": round(index / playback_fps, 6),
            "disposition": disposition,
            "selected_atlas_frame_index": state,
            "viseme_class": playback_visemes[index],
            "audio_binding": {
                "stream_generation": 1, "segment_id": 1, "first_sample_index": first,
                "sample_count": sample_count, "playback_sample_index": playback,
                "sample_rate": sample_rate, "channels": channels,
            },
            "changed_mask_pixels": int(np.count_nonzero(mask)),
        })
        cv2.imwrite(str(frame_dir / f"frame-{index + 1:05d}.png"), result)

    # Deterministic fail-open probes are separate from the delivered sequence.
    occluded, _, occluded_disposition, _ = render(min(10, count - 1), occluded=True)
    unsupported, _, unsupported_disposition, _ = render(
        min(10, count - 1),
        roll_override=playback_geometries[min(10, count - 1)]["roll"] + math.pi / 2,
    )
    fail_open_verified = (
        occluded_disposition == "bypass_occluded"
        and unsupported_disposition == "bypass_pose_coverage"
        and np.array_equal(occluded, source[min(10, count - 1)])
        and np.array_equal(unsupported, source[min(10, count - 1)])
    )

    video_path = args.output / "mara-multi-observation-residual.mp4"
    subprocess.run(
        [
            str(args.ffmpeg), "-hide_banner", "-nostdin", "-loglevel", "error", "-y",
            "-framerate", f"{playback_fps:g}", "-i", str(frame_dir / "frame-%05d.png"),
            "-i", str(playback_path), "-map", "0:v:0", "-map", "1:a:0", "-c:v", "libx264",
            "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a", "aac",
            "-b:a", "192k", "-shortest", "-movflags", "+faststart", str(video_path),
        ],
        stdin=subprocess.DEVNULL, capture_output=True, check=True,
    )
    board_path = args.output / "source-output-enlarged-mouth-board.png"
    baseline = read_frame_directory(args.baseline_frame_dir) if args.baseline_frame_dir else None
    make_board(source, outputs, board_path, baseline, args.baseline_fps, playback_fps)
    display_board_path = args.output / "display-size-comparison-board.png"
    make_display_board(source, outputs, display_board_path, baseline, args.baseline_fps, playback_fps)
    transition_board_path = args.output / "temporal-state-transition-board.png"
    make_transition_board(source, outputs, selections, transition_board_path, playback_fps)
    atlas_board_path = args.output / "atlas-state-board.png"
    make_atlas_board(teacher, selected, visemes, atlas_board_path)
    held_out_playback = sha256(args.enrollment_audio) != sha256(playback_path)
    playback_duration_seconds = len(playback_audio) / sample_rate
    rendered_duration_seconds = count / playback_fps
    playback_truncated_to_source = not args.ping_pong_playback_source and count < desired_count
    atlas = []
    for index in selected:
        first, sample_count, playback = enrollment_intervals[index]
        atlas.append({
            "state_index": index, "source_frame_sha256": sha256_bytes(atlas_source[index].tobytes()),
            "teacher_frame_sha256": sha256_bytes(teacher[index].tobytes()),
            "geometry": geometries[index], "viseme_class": visemes[index],
            "audio_descriptor": [round(float(value), 7) for value in descriptors[index]],
            "sharpness": round(sharpness[index], 4),
            "audio_binding": {"first_sample_index": first, "sample_count": sample_count, "playback_sample_index": playback},
        })
    (args.output / "atlas-manifest.json").write_text(json.dumps({
        "schema": "interactive-npcs-private-generated-teacher-mouth-atlas/v1",
        "scope": "private-synthetic-mara-proof-not-product-pack",
        "maximum_states": args.atlas_limit, "states": atlas,
    }, indent=2) + "\n", encoding="utf-8")
    (args.output / "proof.json").write_text(json.dumps({
        "schema": "interactive-npcs-multi-observation-residual-proof/v1",
        "status": "rendered-not-qualified",
        "scope": (
            "private-synthetic-controlled-blink-breath-source-proof"
            if playback_sequence_document is not None
            else "private-synthetic-static-actor-zoompan-source-proof"
        ),
        "source": str(args.source_video.resolve(strict=True)),
        "teacher": str(args.teacher_video.resolve(strict=True)),
        "enrollment_audio": str(args.enrollment_audio.resolve(strict=True)),
        "enrollment_audio_sha256": sha256(args.enrollment_audio),
        "playback_audio": str(playback_path.resolve(strict=True)),
        "playback_audio_sha256": sha256(playback_path),
        "held_out_playback": held_out_playback,
        "playback_duration_seconds": round(playback_duration_seconds, 6),
        "rendered_duration_seconds": round(rendered_duration_seconds, 6),
        "playback_truncated_to_source": playback_truncated_to_source,
        "enrollment_source_fps": enrollment_fps,
        "source_fps": playback_fps, "output_frames": count,
        "playback_source_schedule": (
            "headless-controlled-blink-breath"
            if playback_sequence_document is not None
            else ("smooth-ping-pong" if args.ping_pong_playback_source else "finite-forward")
        ),
        "playback_source_indices": (
            [playback_content_indices[index] for index in source_indices]
            if playback_sequence_document is not None else source_indices
        ),
        "playback_sequence_json": (
            str(args.playback_sequence_json.resolve(strict=True))
            if args.playback_sequence_json else None
        ),
        "playback_sequence_sha256": (
            sha256(args.playback_sequence_json) if args.playback_sequence_json else None
        ),
        "atlas_state_count": len(selected), "atlas_state_indices": selected,
        "selection": "causal current-plus-two-past audio intervals plus current pose/scale and two-frame state hysteresis",
        "runtime_render_ms": {
            "samples": len(render_ms),
            "median": round(float(np.median(render_ms)), 3),
            "p95": round(float(np.percentile(render_ms, 95)), 3),
            "maximum": round(float(np.max(render_ms)), 3),
            "scope": "selection+mouth-ROI affine warp+mouth-ROI mask+ROI composite; excludes tracking, audio capture, PNG, and video encoding",
        },
        "outside_zero_mask_pixels_byte_exact": outside_exact,
        "delivered_bypasses": bypasses,
        "simulated_fail_open_verified": fail_open_verified,
        "simulated_dispositions": [occluded_disposition, unsupported_disposition],
        "video": str(video_path), "video_sha256": sha256(video_path),
        "board": str(board_path), "display_board": str(display_board_path),
        "transition_board": str(transition_board_path), "atlas_board": str(atlas_board_path),
        "selections": selections,
        "limitations": [
            (
                "The playback source contains project-controlled synthetic blink/breath deformation over "
                "camera-transformed portrait frames; it is not natural actor or game animation."
                if playback_sequence_document is not None
                else "The source actor is static; only zoom/pan changes the source frames."
            ),
            "Atlas textures are generated by an offline MuseTalk teacher and are private proof data.",
            "The audio-state classifier is heuristic and not a phoneme alignment model.",
            (
                "A distinct audio file supplies playback; selection reuses enrollment states causally, but "
                "the heuristic acoustic descriptor is not a phoneme recognizer and perceptual sync is unqualified."
                if held_out_playback
                else "The same utterance supplies enrollment and playback, so generalization is unproven."
            ),
            *(
                ["The playback proof ends when the finite source sequence ends and does not cover the utterance tail."]
                if playback_truncated_to_source else []
            ),
        ],
    }, indent=2) + "\n", encoding="utf-8")
    print(board_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
