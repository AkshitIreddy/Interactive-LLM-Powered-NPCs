#!/usr/bin/env python3
"""Render a headless same-identity mouth-atlas proof over moving RGB frames.

This is a deliberately dependency-light qualification tool.  It never invents
teeth or an oral cavity: every visible mouth pixel comes from the enrolled
actor's own observed frames.  A masked exterior ring tracks the mouth between
frames, while the audio envelope causally blends a small set of enrolled mouth
appearances.  Outputs are confined to E:\\temp.

The fixed enrollment anchors below belong only to the checked-in Pexels proof
fixture.  Product enrollment obtains equivalent anchors from the admitted
landmark provider and stores them in the character mouth-atlas manifest.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import math
from pathlib import Path
import statistics
import sys
import time
import wave

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont


@dataclass(frozen=True)
class AtlasState:
    name: str
    path: Path
    center: tuple[float, float]
    crop_size: tuple[int, int]
    openness: float


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    if resolved.drive.lower() != "e:" or "temp" not in [part.lower() for part in resolved.parts]:
        raise ValueError("output must remain under E:\\temp")
    return resolved


def read_pcm16_mono(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        sample_rate = stream.getframerate()
        sample_width = stream.getsampwidth()
        frames = stream.readframes(stream.getnframes())
    if sample_width != 2:
        raise ValueError("proof audio must be PCM16")
    pcm = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    if channels > 1:
        pcm = pcm.reshape((-1, channels)).mean(axis=1)
    return pcm, sample_rate


def audio_features(pcm: np.ndarray, sample_rate: int, frame_count: int, fps: int) -> list[tuple[float, float]]:
    raw: list[tuple[float, float]] = []
    rms_values: list[float] = []
    for index in range(frame_count):
        first = index * sample_rate // fps
        last = min(len(pcm), (index + 1) * sample_rate // fps)
        window = pcm[first:last]
        if len(window) == 0:
            raw.append((0.0, 0.0))
            rms_values.append(0.0)
            continue
        rms = float(np.sqrt(np.mean(window * window)))
        signs = np.signbit(window)
        zcr = float(np.count_nonzero(signs[1:] != signs[:-1])) / max(1, len(window) - 1)
        raw.append((rms, zcr))
        rms_values.append(rms)

    noise = float(np.percentile(rms_values, 15))
    speech = max(noise + 1.0e-5, float(np.percentile(rms_values, 92)))
    span = max(1.0e-5, speech - noise)
    smoothed = 0.0
    previous_open = 0.0
    result: list[tuple[float, float]] = []
    for rms, zcr in raw:
        target = max(0.0, min(1.0, (rms - noise) / span))
        # Fast attack, slower release; still causal and with no future samples.
        alpha = 0.76 if target >= smoothed else 0.43
        smoothed += (target - smoothed) * alpha
        # Compress weak speech while keeping decisive mouth closures.
        desired_open = max(0.0, min(1.0, (smoothed - 0.055) / 0.945))
        # Prevent one audio window from jumping directly from a sealed mouth to
        # a teeth state.  At 30 FPS this is an approximately 130 ms full-range
        # attack and 170 ms release, while bilabial provider cues can still use
        # the native timed-viseme path for a faster explicit closure.
        openness = max(previous_open - 0.18, min(previous_open + 0.22, desired_open))
        previous_open = openness
        result.append((openness, max(0.0, min(1.0, zcr / 0.18))))
    return result


def crop(image: Image.Image, center: tuple[float, float], size: tuple[int, int]) -> Image.Image:
    width, height = size
    left = round(center[0] - width / 2)
    top = round(center[1] - height / 2)
    return image.crop((left, top, left + width, top + height))


def tracking_ring(size: tuple[int, int]) -> np.ndarray:
    width, height = size
    yy, xx = np.mgrid[0:height, 0:width]
    nx = (xx + 0.5 - width / 2) / (width / 2)
    ny = (yy + 0.5 - height / 2) / (height / 2)
    radius = nx * nx + ny * ny
    # Track moustache, beard and cheek texture, while excluding the deforming lips.
    ring = (radius <= 0.96) & ((radius >= 0.32) | (np.abs(ny) >= 0.36))
    return ring.astype(np.float32)


def gray_array(image: Image.Image) -> np.ndarray:
    return np.asarray(image.convert("L"), dtype=np.float32)


def tracked_centers(frames: list[Image.Image], initial: tuple[float, float]) -> list[tuple[float, float]]:
    template_size = (112, 76)
    mask = tracking_ring(template_size)
    template = gray_array(crop(frames[0], initial, template_size))
    template_mean = float(np.sum(template * mask) / np.sum(mask))
    template = (template - template_mean) * mask
    centers = [initial]
    current = initial
    for frame in frames[1:]:
        best_score = math.inf
        best = current
        # Bounded translation-only propagation is intentional for this proof:
        # the admitted landmark worker supplies scale/roll in the native path.
        for dy in range(-4, 5):
            for dx in range(-5, 6):
                candidate = (current[0] + dx, current[1] + dy)
                patch = gray_array(crop(frame, candidate, template_size))
                mean = float(np.sum(patch * mask) / np.sum(mask))
                centered = (patch - mean) * mask
                score = float(np.sum(np.abs(centered - template) * mask) / np.sum(mask))
                if score < best_score:
                    best_score = score
                    best = candidate
        # Keep a tiny inertial term so one noisy frame cannot visibly snap the atlas.
        current = (current[0] * 0.18 + best[0] * 0.82, current[1] * 0.18 + best[1] * 0.82)
        centers.append(current)
    return centers


def lip_mask(size: tuple[int, int], opacity: float) -> Image.Image:
    width, height = size
    scale = 4
    canvas = Image.new("L", (width * scale, height * scale), 0)
    draw = ImageDraw.Draw(canvas)
    points = [
        (0.055, 0.53), (0.16, 0.25), (0.36, 0.105), (0.50, 0.075),
        (0.64, 0.105), (0.84, 0.25), (0.945, 0.53), (0.84, 0.77),
        (0.64, 0.91), (0.50, 0.94), (0.36, 0.91), (0.16, 0.77),
    ]
    draw.polygon([(round(x * width * scale), round(y * height * scale)) for x, y in points],
                 fill=round(255 * opacity))
    canvas = canvas.filter(ImageFilter.GaussianBlur(radius=4.4 * scale))
    return canvas.resize(size, Image.Resampling.LANCZOS)


def bounded_exterior_color_match(reference: Image.Image, target: Image.Image) -> Image.Image:
    ref = np.asarray(reference, dtype=np.float32).copy()
    dst = np.asarray(target, dtype=np.float32)
    height, width, _ = ref.shape
    yy, xx = np.mgrid[0:height, 0:width]
    nx = (xx + 0.5 - width / 2) / (width / 2)
    ny = (yy + 0.5 - height / 2) / (height / 2)
    radius = nx * nx + ny * ny
    ring = (radius >= 0.60) & (radius <= 0.91)
    if np.count_nonzero(ring) < 16:
        return reference
    delta = np.median(dst[ring], axis=0) - np.median(ref[ring], axis=0)
    delta = np.clip(delta, -8.0, 8.0)
    # Preserve teeth and oral interior: adaptation fades to zero at the centre.
    exterior_weight = np.clip((radius - 0.18) / 0.42, 0.0, 1.0)[..., None]
    ref += delta[None, None, :] * exterior_weight
    return Image.fromarray(np.clip(ref, 0, 255).astype(np.uint8), "RGB")


def load_landmark_records(path: Path | None) -> dict[str, dict]:
    if path is None:
        return {}
    document = json.loads(path.read_text(encoding="utf-8"))
    return {
        str(frame["file"]): frame
        for frame in document.get("frames", [])
        if frame.get("accepted")
    }


def geometry_from_record(record: dict) -> tuple[tuple[float, float], tuple[int, int]]:
    width = int(record["width"])
    height = int(record["height"])
    center = (float(record["center"][0]) * width, float(record["center"][1]) * height)
    canonical_width = max(24, round(float(record["cornerWidth"]) * width * 1.34))
    return center, (canonical_width, max(20, round(canonical_width * 0.62)))


def state_patch(state: AtlasState, size: tuple[int, int], records: dict[str, dict]) -> Image.Image:
    image = Image.open(state.path).convert("RGB")
    record = records.get(state.path.name)
    center, crop_size = geometry_from_record(record) if record else (state.center, state.crop_size)
    return crop(image, center, crop_size).resize(size, Image.Resampling.LANCZOS)


def blend_states(states: list[AtlasState], openness: float, high_frequency: float,
                 size: tuple[int, int], records: dict[str, dict]) -> tuple[Image.Image, str]:
    # A little high-frequency energy chooses the narrower teeth state instead of
    # turning every consonant into an open vowel.
    adjusted = max(0.0, min(1.0, openness * (1.0 - 0.20 * high_frequency)))
    if adjusted <= states[0].openness:
        return state_patch(states[0], size, records), states[0].name
    for low, high in zip(states, states[1:]):
        if adjusted <= high.openness:
            amount = (adjusted - low.openness) / max(1.0e-6, high.openness - low.openness)
            first = state_patch(low, size, records)
            second = state_patch(high, size, records)
            return Image.blend(first, second, amount), f"{low.name}->{high.name}"
    return state_patch(states[-1], size, records), states[-1].name


def save_ppm(path: Path, image: Image.Image) -> None:
    image.convert("RGB").save(path, format="PPM")


def make_board(frames: list[Image.Image], source: list[Image.Image], features: list[tuple[float, float]],
               centers: list[tuple[float, float]], path: Path) -> None:
    picks = sorted(set([0, 4, 8, 12, 16, 20, 24, 28, 32, 36, len(frames) - 1]))
    cell_w, cell_h = 420, 285
    cells: list[Image.Image] = []
    font = ImageFont.load_default()
    for index in picks:
        pair = Image.new("RGB", (cell_w, cell_h), (18, 18, 18))
        review_left = round(centers[index][0] - 105)
        review_top = round(centers[index][1] - 78)
        review_box = (review_left, review_top, review_left + 210, review_top + 156)
        source_crop = source[index].crop(review_box).resize((210, 264), Image.Resampling.LANCZOS)
        output_crop = frames[index].crop(review_box).resize((210, 264), Image.Resampling.LANCZOS)
        pair.paste(source_crop, (0, 0))
        pair.paste(output_crop, (210, 0))
        draw = ImageDraw.Draw(pair)
        draw.text((6, 268), f"{index:02d} source", fill=(235, 235, 235), font=font)
        draw.text((216, 268), f"output open={features[index][0]:.2f}", fill=(235, 235, 235), font=font)
        cells.append(pair)
    columns = 4
    rows = math.ceil(len(cells) / columns)
    board = Image.new("RGB", (columns * cell_w, rows * cell_h), (10, 10, 10))
    for index, cell in enumerate(cells):
        board.paste(cell, ((index % columns) * cell_w, (index // columns) * cell_h))
    board.save(path)


def make_all_frame_board(frames: list[Image.Image], features: list[tuple[float, float]],
                         centers: list[tuple[float, float]], path: Path) -> None:
    cell_w, cell_h = 300, 218
    columns = 8
    rows = math.ceil(len(frames) / columns)
    board = Image.new("RGB", (columns * cell_w, rows * cell_h), (10, 10, 10))
    font = ImageFont.load_default()
    for index, frame in enumerate(frames):
        center = centers[index]
        box = (round(center[0] - 108), round(center[1] - 72),
               round(center[0] + 108), round(center[1] + 72))
        mouth = frame.crop(box).resize((cell_w, 200), Image.Resampling.LANCZOS)
        cell = Image.new("RGB", (cell_w, cell_h), (18, 18, 18))
        cell.paste(mouth, (0, 0))
        draw = ImageDraw.Draw(cell)
        draw.text((6, 203), f"{index:02d}  open={features[index][0]:.2f}",
                  fill=(238, 238, 238), font=font)
        board.paste(cell, ((index % columns) * cell_w, (index // columns) * cell_h))
    board.save(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fps", type=int, default=30)
    parser.add_argument("--frames", type=int, default=40)
    parser.add_argument("--source-landmarks", type=Path)
    parser.add_argument("--reference-landmarks", type=Path)
    parser.add_argument(
        "--fixture",
        choices=("pexels-6682450", "pexels-4994154"),
        default="pexels-6682450",
    )
    args = parser.parse_args()

    output = require_e_temp(args.output)
    frames_dir = output / "frames"
    frames_dir.mkdir(parents=True, exist_ok=True)
    source_paths = sorted(args.source_dir.glob("frame-*.ppm"))[: args.frames]
    if not source_paths:
        raise ValueError("no source PPM frames found")
    source_frames = [Image.open(path).convert("RGB") for path in source_paths]
    pcm, sample_rate = read_pcm16_mono(args.audio)
    features = audio_features(pcm, sample_rate, len(source_frames), args.fps)
    source_records = load_landmark_records(args.source_landmarks)
    reference_records = load_landmark_records(args.reference_landmarks)

    if args.fixture == "pexels-4994154":
        initial_center = (270.0, 590.0)
        base_size = (180, 108)
        states = [
            AtlasState("rest", args.references / "frame-00073.ppm", (270.0, 515.0), (180, 120), 0.00),
            AtlasState("rounded", args.references / "frame-00064.ppm", (270.0, 470.0), (180, 120), 0.28),
            AtlasState("labiodental", args.references / "frame-00068.ppm", (270.0, 485.0), (180, 120), 0.48),
            AtlasState("open", args.references / "frame-00078.ppm", (270.0, 500.0), (180, 120), 0.73),
            AtlasState("wide", args.references / "frame-00086.ppm", (270.0, 480.0), (180, 120), 1.00),
        ]
    else:
        initial_center = (486.0, 378.0)
        base_size = (92, 52)
        states = [
            AtlasState("rest", args.references / "frame-00003.ppm", (486.0, 378.0), (102, 60), 0.00),
            AtlasState("slight", args.references / "frame-00013.ppm", (500.0, 379.0), (112, 68), 0.24),
            AtlasState("teeth", args.references / "frame-00014.ppm", (502.0, 381.0), (112, 68), 0.57),
            AtlasState("open", args.references / "frame-00017.ppm", (507.0, 383.0), (116, 72), 1.00),
        ]
    missing = [str(state.path) for state in states if not state.path.is_file()]
    if missing:
        raise ValueError("missing atlas states: " + ", ".join(missing))

    if source_records and all(path.name in source_records for path in source_paths):
        target_geometry = [geometry_from_record(source_records[path.name]) for path in source_paths]
        centers = [geometry[0] for geometry in target_geometry]
        target_sizes = [geometry[1] for geometry in target_geometry]
    else:
        centers = tracked_centers(source_frames, initial_center)
        target_sizes = [base_size] * len(centers)
    rendered: list[Image.Image] = []
    chosen_states: list[str] = []
    hot_path_ms: list[float] = []
    for index, (source, center, target_size, feature) in enumerate(
        zip(source_frames, centers, target_sizes, features)
    ):
        started = time.perf_counter()
        openness, high_frequency = feature
        # Follow real speech jaw range while avoiding the oversized smile of the
        # raw enrollment frame.  Alpha becomes exactly zero at true rest.
        width, height = target_size
        destination_size = (width, height)
        reference, selected = blend_states(
            states, openness, high_frequency, destination_size, reference_records
        )
        target_patch = crop(source, center, destination_size)
        reference = bounded_exterior_color_match(reference, target_patch)
        alpha = lip_mask(destination_size, 0.96)
        alpha = alpha.point(lambda value: round(value * min(1.0, openness / 0.10)))
        blended = Image.composite(reference, target_patch, alpha)
        result = source.copy()
        left = round(center[0] - width / 2)
        top = round(center[1] - height / 2)
        result.paste(blended, (left, top))
        hot_path_ms.append((time.perf_counter() - started) * 1000.0)
        rendered.append(result)
        chosen_states.append(selected)
        save_ppm(frames_dir / f"frame-{index:05d}.ppm", result)

    make_board(rendered, source_frames, features, centers,
               output / "source-output-mouth-board.png")
    make_all_frame_board(rendered, features, centers, output / "all-output-mouth-frames.png")
    centers_delta = [
        math.hypot(centers[i][0] - centers[i - 1][0], centers[i][1] - centers[i - 1][1])
        for i in range(1, len(centers))
    ]
    manifest = {
        "schema": "interactive-npcs-observed-mouth-atlas-proof/v1",
        "status": "rendered-not-qualified",
        "source": str(args.source_dir.resolve()),
        "audio": str(args.audio.resolve()),
        "frames": len(rendered),
        "fps": args.fps,
        "atlasStates": [state.name for state in states],
        "allVisibleOralPixelsAreIdentityObserved": True,
        "meanTrackedCenterDeltaPixels": statistics.fmean(centers_delta) if centers_delta else 0.0,
        "maximumTrackedCenterDeltaPixels": max(centers_delta, default=0.0),
        "maximumOpenness": max((value[0] for value in features), default=0.0),
        "maximumAdjacentOpennessDelta": max(
            (abs(features[index][0] - features[index - 1][0])
             for index in range(1, len(features))),
            default=0.0,
        ),
        "hotPathMilliseconds": {
            "scope": "atlas lookup, resize, blend, color match, mask, and source composite; excludes PPM and board writes",
            "mean": statistics.fmean(hot_path_ms) if hot_path_ms else 0.0,
            "p50": float(np.percentile(hot_path_ms, 50)) if hot_path_ms else 0.0,
            "p95": float(np.percentile(hot_path_ms, 95)) if hot_path_ms else 0.0,
            "maximum": max(hot_path_ms, default=0.0),
        },
        "selectedStates": chosen_states,
    }
    (output / "observed-atlas-proof.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(output / "source-output-mouth-board.png")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, wave.Error) as error:
        print(f"observed atlas proof error: {error}", file=sys.stderr)
        sys.exit(2)
