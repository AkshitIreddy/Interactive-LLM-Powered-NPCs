#!/usr/bin/env python3
"""Render a source-preserving observed-lip proof over moving frames.

Unlike the rejected rectangular atlas prototype, this proof never resizes a
mouth rectangle and never averages two different tooth layouts.  It transfers
an observed state's tracked deformation onto the current frame, warps the
current frame's own lip pixels, and imports only the oral interior that a closed
source cannot provide.  All artifacts remain under E:\\temp.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import math
from pathlib import Path
import statistics
import subprocess
import sys
import time
import wave

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont


LANDMARK_SCHEMA = "interactive-npcs-mediapipe-mouth-landmarks/v1"


@dataclass(frozen=True)
class ObservedState:
    index: int
    path: Path
    record: dict
    raw_aperture: float
    openness: float


@dataclass(frozen=True)
class RenderGeometry:
    source_lip: np.ndarray
    destination_lip: np.ndarray
    destination_inner: np.ndarray
    source_controls: np.ndarray
    destination_controls: np.ndarray


@dataclass(frozen=True)
class ObservedFlow:
    x0: int
    y0: int
    vectors: np.ndarray


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    if resolved.drive.lower() != "e:" or "temp" not in [part.lower() for part in resolved.parts]:
        raise ValueError("inputs and outputs must remain under E:\\temp")
    return resolved


def frame_index(path_or_name: Path | str) -> int:
    stem = Path(path_or_name).stem
    return int(stem.split("-")[-1])


def load_landmarks(path: Path) -> tuple[dict, dict[str, dict]]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != LANDMARK_SCHEMA:
        raise ValueError(f"unsupported landmark schema in {path}")
    records = {
        str(frame["file"]): frame
        for frame in document.get("frames", [])
        if frame.get("accepted")
    }
    return document, records


def read_pcm16_mono(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        sample_rate = stream.getframerate()
        sample_width = stream.getsampwidth()
        payload = stream.readframes(stream.getnframes())
    if sample_width != 2:
        raise ValueError("proof audio must be PCM16")
    pcm = np.frombuffer(payload, dtype="<i2").astype(np.float32) / 32768.0
    if channels > 1:
        pcm = pcm.reshape((-1, channels)).mean(axis=1)
    return pcm, sample_rate


def audio_openness(
    pcm: np.ndarray,
    sample_rate: int,
    frame_count: int,
    fps: int,
) -> list[float]:
    rms_values: list[float] = []
    for index in range(frame_count):
        first = index * sample_rate // fps
        last = min(len(pcm), (index + 1) * sample_rate // fps)
        window = pcm[first:last]
        rms_values.append(float(np.sqrt(np.mean(window * window))) if len(window) else 0.0)

    noise = float(np.percentile(rms_values, 12))
    speech = max(noise + 1.0e-5, float(np.percentile(rms_values, 94)))
    span = max(1.0e-5, speech - noise)
    smoothed = 0.0
    previous = 0.0
    openness: list[float] = []
    for rms in rms_values:
        target = max(0.0, min(1.0, (rms - noise) / span))
        smoothed += (target - smoothed) * (0.78 if target >= smoothed else 0.52)
        desired = max(0.0, min(1.0, (smoothed - 0.035) / 0.965))
        value = max(previous - 0.22, min(previous + 0.26, desired))
        if value < 0.035:
            value = 0.0
        openness.append(value)
        previous = value
    return openness


def hz_to_mel(frequency: np.ndarray | float) -> np.ndarray:
    return 2595.0 * np.log10(1.0 + np.asarray(frequency) / 700.0)


def mel_to_hz(mel: np.ndarray | float) -> np.ndarray:
    return 700.0 * (10.0 ** (np.asarray(mel) / 2595.0) - 1.0)


def audio_features_for_frames(
    samples: np.ndarray,
    sample_rate: int,
    frame_count: int,
    fps: float,
) -> np.ndarray:
    """Match the atlas enrolment's tiny 26-value CPU log-mel feature path."""

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
        math.pi / mel_count
        * (np.arange(mel_count, dtype=np.float32)[None, :] + 0.5)
        * np.arange(coefficient_count, dtype=np.float32)[:, None]
    )
    window = np.hanning(window_length).astype(np.float32)
    half = window_length // 2
    padded = np.pad(np.asarray(samples, dtype=np.float32), (half, half))
    rows: list[np.ndarray] = []
    for frame in range(frame_count):
        center = int(round((frame + 0.5) * sample_rate / fps)) + half
        segment = padded[center - half:center - half + window_length]
        if len(segment) < window_length:
            segment = np.pad(segment, (0, window_length - len(segment)))
        emphasized = segment.copy()
        emphasized[1:] -= 0.97 * segment[:-1]
        spectrum = np.abs(np.fft.rfft(emphasized * window, n=fft_size)) ** 2
        rows.append(dct @ np.log(np.maximum(filters @ spectrum, 1.0e-8)))
    base = np.asarray(rows, dtype=np.float32)
    delta = np.gradient(base, axis=0) if frame_count > 1 else np.zeros_like(base)
    combined = np.concatenate((base, delta), axis=1)
    return (combined - combined.mean(axis=0, keepdims=True)) / np.maximum(
        combined.std(axis=0, keepdims=True), 1.0e-4
    )


def smooth_labels(labels: list[int]) -> list[int]:
    result = labels.copy()
    for index in range(1, len(result) - 1):
        if result[index - 1] == result[index + 1] != result[index]:
            result[index] = result[index - 1]
    return result


def spectral_state_indices(
    pcm: np.ndarray,
    sample_rate: int,
    frame_count: int,
    fps: int,
    atlas_pack: Path,
) -> list[int]:
    with np.load(atlas_pack) as pack:
        centroids = np.asarray(pack["audio_centroids"], dtype=np.float32)
        medoids = np.asarray(pack["medoid_frame_indices"], dtype=np.int32)
    features = audio_features_for_frames(pcm, sample_rate, frame_count, float(fps))
    if centroids.ndim != 2 or centroids.shape[1] != features.shape[1]:
        raise ValueError("atlas audio centroid shape does not match the CPU feature path")
    distances = np.sum(
        (features[:, None, :] - centroids[None, :, :]) ** 2,
        axis=2,
    )
    labels = smooth_labels(np.argmin(distances, axis=1).astype(int).tolist())
    # The enrolment stores zero-based video indices; extracted PPMs are one-based.
    return [int(medoids[label]) + 1 for label in labels]


def record_points(record: dict, key: str) -> np.ndarray:
    width = float(record["width"])
    height = float(record["height"])
    return np.asarray(
        [[float(point[0]) * width, float(point[1]) * height] for point in record[key]],
        dtype=np.float32,
    )


def outer_polygon(record: dict) -> np.ndarray:
    upper = record_points(record, "outerUpper")
    lower = record_points(record, "outerLower")
    return np.concatenate((upper, lower[-2:0:-1]), axis=0)


def inner_polygon(record: dict) -> np.ndarray:
    upper = record_points(record, "innerUpper")
    lower = record_points(record, "innerLower")
    return np.concatenate((upper, lower[-2:0:-1]), axis=0)


def lip_control_points(record: dict) -> np.ndarray:
    """Return corresponding tracked controls without duplicated corners."""

    return np.concatenate(
        (
            record_points(record, "outerUpper"),
            record_points(record, "outerLower")[1:-1],
            record_points(record, "innerUpper"),
            record_points(record, "innerLower")[1:-1],
        ),
        axis=0,
    )


def mouth_center(record: dict) -> np.ndarray:
    return np.asarray(
        [
            float(record["center"][0]) * float(record["width"]),
            float(record["center"][1]) * float(record["height"]),
        ],
        dtype=np.float64,
    )


def corner_width_pixels(record: dict) -> float:
    return float(record["cornerWidth"]) * float(record["width"])


def aperture(record: dict) -> float:
    upper = record_points(record, "innerUpper")
    lower = record_points(record, "innerLower")
    center_aperture = float(np.mean(lower[2:9, 1] - upper[2:9, 1]))
    return max(0.0, center_aperture / max(1.0, corner_width_pixels(record)))


def build_states(
    frame_root: Path,
    records: dict[str, dict],
    first_index: int,
    last_index: int,
) -> list[ObservedState]:
    candidates: list[tuple[int, Path, dict, float]] = []
    for name, record in records.items():
        index = frame_index(name)
        path = frame_root / name
        if first_index <= index <= last_index and path.is_file():
            candidates.append((index, path, record, aperture(record)))
    candidates.sort(key=lambda value: (value[3], value[0]))
    if len(candidates) < 12:
        raise ValueError("dense atlas interval contains fewer than 12 accepted states")

    # Sort by measured geometry, never by a manually assigned semantic label.
    # This prevents the state inversions that made the rejected v37 proof map a
    # nominally "wide" state to a physically narrower mouth than "rounded".
    minimum = candidates[0][3]
    maximum = candidates[-1][3]
    if maximum - minimum < 0.06:
        raise ValueError("dense atlas interval has insufficient real articulation")
    return [
        ObservedState(
            index=index,
            path=path,
            record=record,
            raw_aperture=measured,
            openness=(measured - minimum) / (maximum - minimum),
        )
        for index, path, record, measured in candidates
    ]


def canonical_points(record: dict, points: np.ndarray) -> np.ndarray:
    centered = points.astype(np.float64) - mouth_center(record)
    angle = -float(record["rollRadians"])
    cosine = math.cos(angle)
    sine = math.sin(angle)
    rotation = np.asarray([[cosine, -sine], [sine, cosine]], dtype=np.float64)
    return centered @ rotation.T / max(1.0, corner_width_pixels(record))


def from_canonical_vectors(record: dict, vectors: np.ndarray) -> np.ndarray:
    angle = float(record["rollRadians"])
    cosine = math.cos(angle)
    sine = math.sin(angle)
    rotation = np.asarray([[cosine, -sine], [sine, cosine]], dtype=np.float64)
    return vectors @ rotation.T * corner_width_pixels(record)


def choose_state(
    states: list[ObservedState],
    desired: float,
    previous: ObservedState | None,
) -> ObservedState:
    """Match aperture while suppressing visible same-openness shape pops."""

    nearest_error = min(abs(state.openness - desired) for state in states)
    candidates = [
        state
        for state in states
        if abs(state.openness - desired) <= nearest_error + 0.06
    ]
    if previous is None:
        return min(candidates, key=lambda state: (abs(state.openness - desired), state.index))

    previous_shape = canonical_points(previous.record, outer_polygon(previous.record))

    def cost(state: ObservedState) -> tuple[float, float, int]:
        shape = canonical_points(state.record, outer_polygon(state.record))
        shape_jump = float(np.sqrt(np.mean(np.sum((shape - previous_shape) ** 2, axis=1))))
        return shape_jump + abs(state.openness - desired) * 0.24, shape_jump, state.index

    return min(candidates, key=cost)


def render_geometry(
    neutral: ObservedState,
    state: ObservedState,
    target_record: dict,
    desired: float,
) -> RenderGeometry:
    neutral_controls = lip_control_points(neutral.record)
    state_controls = lip_control_points(state.record)
    current_controls = lip_control_points(target_record).astype(np.float64)
    neutral_canonical = canonical_points(neutral.record, neutral_controls)
    state_canonical = canonical_points(state.record, state_controls)
    amount = min(1.08, desired / max(0.055, state.openness))
    canonical_delta = (state_canonical - neutral_canonical) * amount

    # Speech changes lip width far less than the rejected whole-mouth paste did.
    # State geometry contributes roundedness and subtle curvature, while the
    # continuous aperture channel below owns vertical opening. This prevents a
    # low-gap consonant exemplar from randomly collapsing an equally loud vowel.
    canonical_delta[:, 0] *= 0.52
    canonical_delta[:, 1] *= 0.65
    destination_controls = current_controls + from_canonical_vectors(
        target_record, canonical_delta
    )
    outer_count = 11 + 9
    inner_upper_count = 11
    inner_lower_count = 9

    # Make aperture a stable continuous channel instead of trusting whichever
    # phoneme state happened to have the largest measured gap. The selected
    # state still supplies roundedness and oral appearance; this bounded lower-
    # jaw-biased correction supplies the requested amount of opening.
    angle = float(target_record["rollRadians"])
    vertical = np.asarray([-math.sin(angle), math.cos(angle)], dtype=np.float64)
    upper_inner = slice(outer_count + 1, outer_count + inner_upper_count - 1)
    lower_start = outer_count + inner_upper_count
    lower_inner = slice(lower_start, lower_start + inner_lower_count)
    current_gap = float(
        np.mean((destination_controls[lower_inner] - destination_controls[upper_inner]) @ vertical)
    )
    # Perceptual easing prevents medium-energy syllables from collapsing into a
    # closed-looking seam while keeping the fully open bound essentially fixed.
    # Preserve the anatomy of a high-quality open reference at full drive. The
    # previous 0.15-width ceiling vertically crushed teeth, cavity, and tongue
    # into parallel scanlines. A 0.21-width ceiling remains conservative for an
    # "ah" mouth while leaving enough pixels for distinct oral structures.
    target_gap = corner_width_pixels(target_record) * (0.018 + 0.195 * desired)
    correction = target_gap - current_gap
    destination_controls[upper_inner] -= vertical * correction * 0.34
    destination_controls[lower_inner] += vertical * correction * 0.66
    destination_controls[1:10] -= vertical * correction * 0.10
    destination_controls[11:outer_count] += vertical * correction * 0.48
    destination_outer_upper = destination_controls[:11]
    destination_outer_lower = destination_controls[11:outer_count]
    destination_outer = np.concatenate(
        (
            destination_outer_upper,
            destination_outer_lower[::-1],
        ),
        axis=0,
    )

    # Reconstruct the destination inner contour from the corresponding control
    # ranges. Its corners are the deformed outer corners, as in MediaPipe.
    destination_inner_upper = destination_controls[
        outer_count:outer_count + inner_upper_count
    ]
    destination_inner_lower = destination_controls[
        lower_start:lower_start + inner_lower_count
    ]
    destination_inner = np.concatenate(
        (destination_inner_upper, destination_inner_lower[::-1]), axis=0
    )
    return RenderGeometry(
        source_lip=outer_polygon(target_record),
        destination_lip=destination_outer.astype(np.float32),
        destination_inner=destination_inner.astype(np.float32),
        source_controls=current_controls.astype(np.float32),
        destination_controls=destination_controls.astype(np.float32),
    )


def support_anchors(points: np.ndarray) -> np.ndarray:
    minimum = points.min(axis=0)
    maximum = points.max(axis=0)
    width = max(20.0, maximum[0] - minimum[0])
    height = max(12.0, maximum[1] - minimum[1])
    x0, x1 = minimum[0] - width * 0.24, maximum[0] + width * 0.24
    y0, y1 = minimum[1] - height * 0.72, maximum[1] + height * 0.72
    return np.asarray(
        [
            [x0, y0], [(x0 + x1) * 0.5, y0], [x1, y0],
            [x1, (y0 + y1) * 0.5], [x1, y1], [(x0 + x1) * 0.5, y1],
            [x0, y1], [x0, (y0 + y1) * 0.5],
        ],
        dtype=np.float32,
    )


def warp_from_controls(
    image: np.ndarray,
    source_controls: np.ndarray,
    destination_controls: np.ndarray,
    region_points: np.ndarray,
    padding: int,
) -> tuple[np.ndarray, tuple[int, int, int, int]]:
    """Inverse-distance warp a small ROI while matching every lip control."""

    height, width = image.shape[:2]
    minimum = np.floor(region_points.min(axis=0)).astype(int) - padding
    maximum = np.ceil(region_points.max(axis=0)).astype(int) + padding
    x0 = max(0, int(minimum[0]))
    y0 = max(0, int(minimum[1]))
    x1 = min(width, int(maximum[0]) + 1)
    y1 = min(height, int(maximum[1]) + 1)
    if x1 <= x0 or y1 <= y0:
        raise ValueError("mouth warp ROI is empty")

    grid_x, grid_y = np.meshgrid(
        np.arange(x0, x1, dtype=np.float32),
        np.arange(y0, y1, dtype=np.float32),
    )
    query = np.stack((grid_x, grid_y), axis=-1)
    difference = query[:, :, None, :] - destination_controls[None, None, :, :]
    distance_squared = np.sum(difference * difference, axis=3)
    weights = 1.0 / np.maximum(distance_squared, 0.75)
    weights *= weights
    displacement = source_controls - destination_controls
    weighted = np.sum(weights[:, :, :, None] * displacement[None, None, :, :], axis=2)
    weighted /= np.maximum(np.sum(weights, axis=2)[:, :, None], 1.0e-8)
    map_x = (grid_x + weighted[:, :, 0]).astype(np.float32)
    map_y = (grid_y + weighted[:, :, 1]).astype(np.float32)
    warped = cv2.remap(
        image,
        map_x,
        map_y,
        interpolation=cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    return warped, (x0, y0, x1, y1)


def warp_affine_to_region(
    image: np.ndarray,
    source_controls: np.ndarray,
    destination_controls: np.ndarray,
    region_points: np.ndarray,
    output_shape: tuple[int, int],
    padding: int,
) -> tuple[np.ndarray, tuple[int, int, int, int]]:
    """Fit a full affine oral-interior map and render only its target ROI."""

    output_height, output_width = output_shape
    minimum = np.floor(region_points.min(axis=0)).astype(int) - padding
    maximum = np.ceil(region_points.max(axis=0)).astype(int) + padding
    x0 = max(0, int(minimum[0]))
    y0 = max(0, int(minimum[1]))
    x1 = min(output_width, int(maximum[0]) + 1)
    y1 = min(output_height, int(maximum[1]) + 1)
    design = np.concatenate(
        (source_controls.astype(np.float64), np.ones((len(source_controls), 1))),
        axis=1,
    )
    coefficients, _, _, _ = np.linalg.lstsq(
        design, destination_controls.astype(np.float64), rcond=None
    )
    matrix = coefficients.T.astype(np.float64)
    local_matrix = matrix.copy()
    local_matrix[0, 2] -= x0
    local_matrix[1, 2] -= y0
    warped = cv2.warpAffine(
        image,
        local_matrix,
        (x1 - x0, y1 - y0),
        flags=cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    return warped, (x0, y0, x1, y1)


def fit_affine(source_controls: np.ndarray, destination_controls: np.ndarray) -> np.ndarray:
    design = np.concatenate(
        (source_controls.astype(np.float64), np.ones((len(source_controls), 1))),
        axis=1,
    )
    coefficients, _, _, _ = np.linalg.lstsq(
        design, destination_controls.astype(np.float64), rcond=None
    )
    return coefficients.T.astype(np.float32)


def transform_points(matrix: np.ndarray, points: np.ndarray) -> np.ndarray:
    homogeneous = np.concatenate(
        (points.astype(np.float32), np.ones((len(points), 1), dtype=np.float32)),
        axis=1,
    )
    return homogeneous @ matrix.T


def build_observed_flow(
    neutral_image: np.ndarray,
    active_image: np.ndarray,
    neutral_record: dict,
    active_record: dict,
) -> ObservedFlow:
    """Precompute backward active-to-neutral flow around one observed mouth."""

    region = np.concatenate(
        (
            outer_polygon(neutral_record),
            outer_polygon(active_record),
            inner_polygon(neutral_record),
            inner_polygon(active_record),
        ),
        axis=0,
    )
    minimum = np.floor(region.min(axis=0)).astype(int) - 18
    maximum = np.ceil(region.max(axis=0)).astype(int) + 18
    x0 = max(0, int(minimum[0]))
    y0 = max(0, int(minimum[1]))
    x1 = min(neutral_image.shape[1], int(maximum[0]) + 1)
    y1 = min(neutral_image.shape[0], int(maximum[1]) + 1)
    if x1 - x0 < 24 or y1 - y0 < 16:
        raise ValueError("observed-flow mouth ROI is too small")
    neutral_gray = cv2.cvtColor(
        neutral_image[y0:y1, x0:x1], cv2.COLOR_BGR2GRAY
    )
    active_gray = cv2.cvtColor(active_image[y0:y1, x0:x1], cv2.COLOR_BGR2GRAY)
    # Flow is active(destination) -> neutral(source), directly usable by a
    # backward image remap. This is enrollment work, not part of the frame path.
    vectors = cv2.calcOpticalFlowFarneback(
        active_gray,
        neutral_gray,
        None,
        0.5,
        4,
        21,
        5,
        7,
        1.5,
        cv2.OPTFLOW_FARNEBACK_GAUSSIAN,
    )
    return ObservedFlow(x0=x0, y0=y0, vectors=vectors.astype(np.float32))


def render_flow_delta(
    source: np.ndarray,
    target_record: dict,
    neutral: ObservedState,
    neutral_observed: np.ndarray,
    state: ObservedState,
    observed: np.ndarray,
    observed_flow: ObservedFlow,
    strength: float,
) -> tuple[np.ndarray, int]:
    """Apply precomputed teacher motion while retaining current-frame detail."""

    height, width = source.shape[:2]
    neutral_controls = lip_control_points(neutral.record)
    destination_controls = (
        mouth_center(target_record)[None, :]
        + from_canonical_vectors(
            target_record,
            canonical_points(neutral.record, neutral_controls),
        )
    ).astype(np.float32)
    matrix = fit_affine(neutral_controls, destination_controls)
    inverse = cv2.invertAffineTransform(matrix)

    flow_height, flow_width = observed_flow.vectors.shape[:2]
    flow_corners = np.asarray(
        [
            [observed_flow.x0, observed_flow.y0],
            [observed_flow.x0 + flow_width - 1, observed_flow.y0],
            [observed_flow.x0 + flow_width - 1, observed_flow.y0 + flow_height - 1],
            [observed_flow.x0, observed_flow.y0 + flow_height - 1],
        ],
        dtype=np.float32,
    )
    target_corners = transform_points(matrix, flow_corners)
    minimum = np.floor(target_corners.min(axis=0)).astype(int) - 2
    maximum = np.ceil(target_corners.max(axis=0)).astype(int) + 2
    x0 = max(0, int(minimum[0]))
    y0 = max(0, int(minimum[1]))
    x1 = min(width, int(maximum[0]) + 1)
    y1 = min(height, int(maximum[1]) + 1)
    if x1 <= x0 or y1 <= y0:
        raise ValueError("flow-delta target ROI is empty")

    grid_x, grid_y = np.meshgrid(
        np.arange(x0, x1, dtype=np.float32),
        np.arange(y0, y1, dtype=np.float32),
    )
    teacher_x = inverse[0, 0] * grid_x + inverse[0, 1] * grid_y + inverse[0, 2]
    teacher_y = inverse[1, 0] * grid_x + inverse[1, 1] * grid_y + inverse[1, 2]
    flow_x = cv2.remap(
        observed_flow.vectors[:, :, 0],
        teacher_x - float(observed_flow.x0),
        teacher_y - float(observed_flow.y0),
        interpolation=cv2.INTER_LINEAR,
        borderMode=cv2.BORDER_REPLICATE,
    )
    flow_y = cv2.remap(
        observed_flow.vectors[:, :, 1],
        teacher_x - float(observed_flow.x0),
        teacher_y - float(observed_flow.y0),
        interpolation=cv2.INTER_LINEAR,
        borderMode=cv2.BORDER_REPLICATE,
    )
    neutral_x = teacher_x + flow_x
    neutral_y = teacher_y + flow_y
    current_x = matrix[0, 0] * neutral_x + matrix[0, 1] * neutral_y + matrix[0, 2]
    current_y = matrix[1, 0] * neutral_x + matrix[1, 1] * neutral_y + matrix[1, 2]

    warped_current = cv2.remap(
        source,
        current_x,
        current_y,
        interpolation=cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    active_sample = cv2.remap(
        observed,
        teacher_x,
        teacher_y,
        interpolation=cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    neutral_sample = cv2.remap(
        neutral_observed,
        neutral_x,
        neutral_y,
        interpolation=cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    candidate = np.clip(
        warped_current.astype(np.float32)
        + active_sample.astype(np.float32)
        - neutral_sample.astype(np.float32),
        0.0,
        255.0,
    )

    destination_neutral_lip = transform_points(matrix, outer_polygon(neutral.record))
    destination_state_lip = transform_points(matrix, outer_polygon(state.record))
    motion_mask = np.zeros((height, width), dtype=np.uint8)
    cv2.fillPoly(
        motion_mask,
        [np.rint(destination_neutral_lip).astype(np.int32)],
        255,
    )
    cv2.fillPoly(
        motion_mask,
        [np.rint(destination_state_lip).astype(np.int32)],
        255,
    )
    motion_mask = cv2.dilate(
        motion_mask,
        cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 5)),
    )
    motion_mask = cv2.GaussianBlur(motion_mask, (0, 0), 1.15)
    alpha = np.clip(
        motion_mask[y0:y1, x0:x1, None].astype(np.float32) / 255.0 * strength,
        0.0,
        1.0,
    )
    result = source.copy()
    result[y0:y1, x0:x1] = np.clip(
        source[y0:y1, x0:x1].astype(np.float32) * (1.0 - alpha)
        + candidate * alpha,
        0.0,
        255.0,
    ).astype(np.uint8)
    return result, int(np.count_nonzero(motion_mask > 5))


def contour_mask(
    shape: tuple[int, int],
    source_polygon: np.ndarray,
    destination_polygon: np.ndarray,
    strength: float,
) -> np.ndarray:
    height, width = shape
    mask = np.zeros((height, width), dtype=np.uint8)
    cv2.fillPoly(mask, [np.rint(source_polygon).astype(np.int32)], 255)
    cv2.fillPoly(mask, [np.rint(destination_polygon).astype(np.int32)], 255)
    # A two-pixel close fills sub-pixel cracks between the old and new contour;
    # the tiny Gaussian edge hides antialiasing without replacing face skin.
    mask = cv2.morphologyEx(
        mask,
        cv2.MORPH_CLOSE,
        cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (3, 3)),
    )
    mask = cv2.GaussianBlur(mask, (0, 0), 1.15)
    return np.clip(mask.astype(np.float32) / 255.0 * strength, 0.0, 1.0)


def polygon_mask(
    shape: tuple[int, int],
    polygon: np.ndarray,
    strength: float,
    feather: float,
) -> np.ndarray:
    mask = np.zeros(shape, dtype=np.uint8)
    cv2.fillPoly(mask, [np.rint(polygon).astype(np.int32)], 255)
    if feather > 0.0:
        mask = cv2.GaussianBlur(mask, (0, 0), feather)
    return np.clip(mask.astype(np.float32) / 255.0 * strength, 0.0, 1.0)


def composite_roi(
    background: np.ndarray,
    foreground_roi: np.ndarray,
    bounds: tuple[int, int, int, int],
    mask: np.ndarray,
) -> np.ndarray:
    x0, y0, x1, y1 = bounds
    result = background.copy()
    alpha = mask[y0:y1, x0:x1, None]
    result[y0:y1, x0:x1] = np.clip(
        result[y0:y1, x0:x1].astype(np.float32) * (1.0 - alpha)
        + foreground_roi.astype(np.float32) * alpha,
        0.0,
        255.0,
    ).astype(np.uint8)
    return result


def render_frame(
    source: np.ndarray,
    target_record: dict,
    neutral: ObservedState,
    neutral_observed: np.ndarray,
    state: ObservedState,
    observed: np.ndarray,
    desired: float,
    strength: float,
    transfer_mode: str,
    observed_flow: ObservedFlow | None,
) -> tuple[np.ndarray, int]:
    if strength <= 0.0:
        return source.copy(), 0
    if transfer_mode == "flow-delta":
        if observed_flow is None:
            raise ValueError("flow-delta mode requires a precomputed observed flow")
        return render_flow_delta(
            source,
            target_record,
            neutral,
            neutral_observed,
            state,
            observed,
            observed_flow,
            strength,
        )
    height, width = source.shape[:2]
    if transfer_mode == "observed-delta":
        # Both teacher images share one static face coordinate system. Map the
        # neutral teacher to the current moving mouth with one similarity-like
        # affine, then add only the active-minus-neutral appearance delta. If
        # the current frame equals the enrolled neutral this reconstructs the
        # teacher state, while otherwise retaining current-frame texture and
        # illumination instead of pasting a second set of lips.
        neutral_controls = lip_control_points(neutral.record)
        destination_neutral_controls = (
            mouth_center(target_record)[None, :]
            + from_canonical_vectors(
                target_record,
                canonical_points(neutral.record, neutral_controls),
            )
        ).astype(np.float32)
        destination_neutral_lip = (
            mouth_center(target_record)[None, :]
            + from_canonical_vectors(
                target_record,
                canonical_points(neutral.record, outer_polygon(neutral.record)),
            )
        ).astype(np.float32)
        destination_state_lip = (
            mouth_center(target_record)[None, :]
            + from_canonical_vectors(
                target_record,
                canonical_points(neutral.record, outer_polygon(state.record)),
            )
        ).astype(np.float32)
        destination_region = np.concatenate(
            (destination_neutral_lip, destination_state_lip), axis=0
        )
        active_warp, active_bounds = warp_affine_to_region(
            observed,
            neutral_controls,
            destination_neutral_controls,
            destination_region,
            (height, width),
            padding=5,
        )
        neutral_warp, neutral_bounds = warp_affine_to_region(
            neutral_observed,
            neutral_controls,
            destination_neutral_controls,
            destination_region,
            (height, width),
            padding=5,
        )
        if active_bounds != neutral_bounds:
            raise ValueError("teacher delta warps produced inconsistent bounds")
        delta_mask = np.zeros((height, width), dtype=np.uint8)
        cv2.fillPoly(
            delta_mask,
            [np.rint(destination_neutral_lip).astype(np.int32)],
            255,
        )
        cv2.fillPoly(
            delta_mask,
            [np.rint(destination_state_lip).astype(np.int32)],
            255,
        )
        delta_mask = cv2.dilate(
            delta_mask,
            cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 5)),
        )
        delta_mask = cv2.GaussianBlur(delta_mask, (0, 0), 1.25)
        delta_mask_float = np.clip(
            delta_mask.astype(np.float32) / 255.0 * strength, 0.0, 1.0
        )
        x0, y0, x1, y1 = active_bounds
        result = source.copy()
        delta = active_warp.astype(np.float32) - neutral_warp.astype(np.float32)
        alpha = delta_mask_float[y0:y1, x0:x1, None]
        result[y0:y1, x0:x1] = np.clip(
            source[y0:y1, x0:x1].astype(np.float32) + delta * alpha,
            0.0,
            255.0,
        ).astype(np.uint8)
        return result, int(np.count_nonzero(delta_mask_float > 0.02))

    geometry = render_geometry(neutral, state, target_record, desired)
    observed_controls = lip_control_points(state.record)
    if transfer_mode == "rigid-observed-lip":
        # Preserve the teacher state's exact morphology. Its canonical points
        # are mapped only by the current frame's mouth centre, scale, and roll;
        # no local mesh can stretch teeth or bend one lip independently.
        destination_controls = (
            mouth_center(target_record)[None, :]
            + from_canonical_vectors(
                target_record,
                canonical_points(state.record, observed_controls),
            )
        ).astype(np.float32)
        destination_lip = (
            mouth_center(target_record)[None, :]
            + from_canonical_vectors(
                target_record,
                canonical_points(state.record, outer_polygon(state.record)),
            )
        ).astype(np.float32)
    else:
        destination_controls = geometry.destination_controls
        destination_lip = geometry.destination_lip

    anchors = support_anchors(
        np.concatenate((geometry.source_lip, destination_lip), axis=0)
    )
    source_controls = np.concatenate((geometry.source_controls, anchors), axis=0)
    destination_support_controls = np.concatenate((destination_controls, anchors), axis=0)
    warped_source, source_bounds = warp_from_controls(
        source,
        source_controls,
        destination_support_controls,
        np.concatenate((geometry.source_lip, destination_lip, anchors), axis=0),
        padding=3,
    )
    lip_mask = contour_mask(
        (height, width), geometry.source_lip, destination_lip, strength
    )
    result = composite_roi(source, warped_source, source_bounds, lip_mask)

    if transfer_mode == "rigid-observed-lip":
        observed_warp, observed_bounds = warp_affine_to_region(
            observed,
            observed_controls,
            destination_controls,
            destination_lip,
            (height, width),
            padding=3,
        )
        observed_mask = polygon_mask(
            (height, width), destination_lip, strength, feather=0.62
        )
    elif transfer_mode == "full-observed-lip":
        # The atlas state belongs to this exact enrolled identity. Transfer the
        # coherent lip + oral appearance as one tracked mesh, but clip it to the
        # destination outer-lip contour. This retains the teacher's natural
        # teeth/lip relationship without ever pasting its surrounding skin.
        observed_warp, observed_bounds = warp_from_controls(
            observed,
            observed_controls,
            destination_controls,
            destination_lip,
            padding=3,
        )
        observed_mask = polygon_mask(
            (height, width), destination_lip, strength, feather=0.72
        )
    else:
        # Conservative cross-source mode: preserve all current-frame lip pixels
        # and import only oral pixels exposed by the destination inner contour.
        inner_offset = 20
        observed_inner_controls = observed_controls[inner_offset:]
        destination_inner_controls = geometry.destination_controls[inner_offset:]
        observed_warp, observed_bounds = warp_affine_to_region(
            observed,
            observed_inner_controls,
            destination_inner_controls,
            geometry.destination_inner,
            (height, width),
            padding=3,
        )
        observed_mask = polygon_mask(
            (height, width), geometry.destination_inner, strength, feather=0.62
        )
    result = composite_roi(result, observed_warp, observed_bounds, observed_mask)
    combined_mask = np.maximum(lip_mask, observed_mask)
    return result, int(np.count_nonzero(combined_mask > 0.02))


def crop_mouth(image: np.ndarray, record: dict, scale: float = 1.75) -> np.ndarray:
    center = mouth_center(record)
    width = max(48, round(corner_width_pixels(record) * scale))
    height = max(34, round(width * 0.62))
    x0 = max(0, round(center[0] - width / 2))
    y0 = max(0, round(center[1] - height / 2))
    x1 = min(image.shape[1], x0 + width)
    y1 = min(image.shape[0], y0 + height)
    return image[y0:y1, x0:x1]


def make_boards(
    sources: list[np.ndarray],
    outputs: list[np.ndarray],
    records: list[dict],
    openness: list[float],
    state_indices: list[int],
    output: Path,
) -> None:
    font = ImageFont.load_default()
    columns = 8
    cell_width, image_height, label_height = 300, 190, 18
    rows = math.ceil(len(outputs) / columns)
    board = Image.new("RGB", (columns * cell_width, rows * (image_height + label_height)), (8, 8, 8))
    for index, (frame, record) in enumerate(zip(outputs, records)):
        mouth = cv2.cvtColor(crop_mouth(frame, record), cv2.COLOR_BGR2RGB)
        tile = Image.fromarray(mouth).resize((cell_width, image_height), Image.Resampling.LANCZOS)
        x = index % columns * cell_width
        y = index // columns * (image_height + label_height)
        board.paste(tile, (x, y))
        ImageDraw.Draw(board).text(
            (x + 5, y + image_height + 2),
            f"{index:02d} open={openness[index]:.2f} state={state_indices[index]:03d}",
            fill=(238, 238, 238),
            font=font,
        )
    board.save(output / "all-output-mouth-frames.png")

    picks = sorted(set([0, 4, 8, 12, 16, 20, 24, 28, 32, len(outputs) - 1]))
    pair_width, pair_height = 600, 210
    pair_board = Image.new("RGB", (pair_width * 2, math.ceil(len(picks) / 2) * pair_height), (8, 8, 8))
    for position, index in enumerate(picks):
        source_mouth = cv2.cvtColor(crop_mouth(sources[index], records[index]), cv2.COLOR_BGR2RGB)
        output_mouth = cv2.cvtColor(crop_mouth(outputs[index], records[index]), cv2.COLOR_BGR2RGB)
        source_tile = Image.fromarray(source_mouth).resize((300, 190), Image.Resampling.LANCZOS)
        output_tile = Image.fromarray(output_mouth).resize((300, 190), Image.Resampling.LANCZOS)
        pair = Image.new("RGB", (pair_width, pair_height), (14, 14, 14))
        pair.paste(source_tile, (0, 0))
        pair.paste(output_tile, (300, 0))
        draw = ImageDraw.Draw(pair)
        draw.text((5, 193), f"{index:02d} source", fill=(238, 238, 238), font=font)
        draw.text(
            (305, 193),
            f"output open={openness[index]:.2f} observed={state_indices[index]:03d}",
            fill=(238, 238, 238),
            font=font,
        )
        x = position % 2 * pair_width
        y = position // 2 * pair_height
        pair_board.paste(pair, (x, y))
    pair_board.save(output / "source-output-mouth-board.png")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-frames", type=Path, required=True)
    parser.add_argument("--source-landmarks", type=Path, required=True)
    parser.add_argument("--atlas-frames", type=Path, required=True)
    parser.add_argument("--atlas-landmarks", type=Path, required=True)
    parser.add_argument("--single-open-image", type=Path)
    parser.add_argument("--single-open-landmarks", type=Path)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--atlas-pack", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-first", type=int, default=1)
    parser.add_argument("--source-count", type=int, default=40)
    parser.add_argument("--atlas-first", type=int, default=1)
    parser.add_argument("--atlas-last", type=int, default=10_000)
    parser.add_argument("--fps", type=int, default=25)
    parser.add_argument("--video-name", default="mara-jason-source-preserving-lips.mp4")
    parser.add_argument(
        "--transfer-mode",
        choices=(
            "flow-delta",
            "observed-delta",
            "rigid-observed-lip",
            "full-observed-lip",
            "oral-interior",
        ),
        default="oral-interior",
        help=(
            "oral-interior is the qualified source-preserving path; the other modes are "
            "retained only to reproduce rejected research comparisons"
        ),
    )
    args = parser.parse_args()

    source_root = require_e_temp(args.source_frames)
    source_landmark_path = require_e_temp(args.source_landmarks)
    atlas_root = require_e_temp(args.atlas_frames)
    atlas_landmark_path = require_e_temp(args.atlas_landmarks)
    single_open_image = require_e_temp(args.single_open_image) if args.single_open_image else None
    single_open_landmarks = (
        require_e_temp(args.single_open_landmarks)
        if args.single_open_landmarks
        else None
    )
    if (single_open_image is None) != (single_open_landmarks is None):
        raise ValueError(
            "--single-open-image and --single-open-landmarks must be supplied together"
        )
    audio_path = require_e_temp(args.audio)
    atlas_pack_path = require_e_temp(args.atlas_pack) if args.atlas_pack else None
    output = require_e_temp(args.output)
    frames_output = output / "frames"
    if output.exists():
        raise ValueError(f"refusing to overwrite existing proof: {output}")
    frames_output.mkdir(parents=True)

    source_landmark_document, source_records = load_landmarks(source_landmark_path)
    atlas_landmark_document, atlas_records = load_landmarks(atlas_landmark_path)
    source_paths = [
        source_root / f"frame-{index:05d}.ppm"
        for index in range(args.source_first, args.source_first + args.source_count)
    ]
    missing = [
        str(path)
        for path in source_paths
        if not path.is_file() or path.name not in source_records
    ]
    if missing:
        raise ValueError("source frames or accepted landmarks missing: " + ", ".join(missing[:4]))
    if single_open_image is not None and single_open_landmarks is not None:
        _, single_records = load_landmarks(single_open_landmarks)
        open_record = single_records.get(single_open_image.name)
        if open_record is None:
            raise ValueError("single open reference has no accepted landmark record")
        neutral_record = source_records[source_paths[0].name]
        neutral_raw_aperture = aperture(neutral_record)
        open_raw_aperture = aperture(open_record)
        if open_raw_aperture <= neutral_raw_aperture + 0.045:
            raise ValueError("single open reference does not add enough articulation")
        states = [
            ObservedState(
                index=0,
                path=source_paths[0],
                record=neutral_record,
                raw_aperture=neutral_raw_aperture,
                openness=0.0,
            ),
            ObservedState(
                index=1,
                path=single_open_image,
                record=open_record,
                raw_aperture=open_raw_aperture,
                openness=1.0,
            ),
        ]
    else:
        states = build_states(atlas_root, atlas_records, args.atlas_first, args.atlas_last)
    neutral = states[0]
    observed_cache: dict[int, np.ndarray] = {}
    for state in states:
        observed = cv2.imread(str(state.path), cv2.IMREAD_COLOR)
        if observed is None:
            raise ValueError(f"cannot decode atlas state {state.path}")
        observed_cache[state.index] = observed
    pcm, sample_rate = read_pcm16_mono(audio_path)
    openness = audio_openness(pcm, sample_rate, len(source_paths), args.fps)
    spectral_indices = (
        spectral_state_indices(
            pcm, sample_rate, len(source_paths), args.fps, atlas_pack_path
        )
        if atlas_pack_path is not None
        else None
    )
    states_by_index = {state.index: state for state in states}
    if spectral_indices is not None:
        missing_spectral = sorted(set(spectral_indices) - states_by_index.keys())
        if missing_spectral:
            raise ValueError(f"spectral atlas states are missing: {missing_spectral}")

    sources: list[np.ndarray] = []
    outputs: list[np.ndarray] = []
    target_records: list[dict] = []
    state_indices: list[int] = []
    mask_pixels: list[int] = []
    hot_path_ms: list[float] = []
    selected_openness: list[float] = []
    flow_cache: dict[int, ObservedFlow] = {}
    previous_state: ObservedState | None = None
    for sequence, (path, desired) in enumerate(zip(source_paths, openness)):
        source = cv2.imread(str(path), cv2.IMREAD_COLOR)
        if source is None:
            raise ValueError(f"cannot decode source frame {path}")
        target_record = source_records[path.name]
        state = (
            states[1]
            if single_open_image is not None and desired > 0.0
            else (
                states_by_index[spectral_indices[sequence]]
                if spectral_indices is not None and desired > 0.0
                else choose_state(states, desired, previous_state)
            )
        )
        strength = min(1.0, desired / 0.075) if desired > 0.0 else 0.0
        observed_flow = None
        if args.transfer_mode == "flow-delta" and strength > 0.0:
            observed_flow = flow_cache.get(state.index)
            if observed_flow is None:
                observed_flow = build_observed_flow(
                    observed_cache[neutral.index],
                    observed_cache[state.index],
                    neutral.record,
                    state.record,
                )
                flow_cache[state.index] = observed_flow
        started = time.perf_counter()
        rendered, changed_mask_pixels = render_frame(
            source,
            target_record,
            neutral,
            observed_cache[neutral.index],
            state,
            observed_cache[state.index],
            desired,
            strength,
            args.transfer_mode,
            observed_flow,
        )
        hot_path_ms.append((time.perf_counter() - started) * 1000.0)
        output_path = frames_output / f"frame-{sequence:05d}.ppm"
        if not cv2.imwrite(str(output_path), rendered):
            raise OSError(f"cannot write {output_path}")
        sources.append(source)
        outputs.append(rendered)
        target_records.append(target_record)
        state_indices.append(state.index)
        selected_openness.append(state.openness)
        mask_pixels.append(changed_mask_pixels)
        previous_state = state

    make_boards(sources, outputs, target_records, openness, state_indices, output)
    if Path(args.video_name).name != args.video_name or not args.video_name.lower().endswith(".mp4"):
        raise ValueError("--video-name must be one MP4 filename")
    video_path = output / args.video_name
    duration = len(outputs) / args.fps
    subprocess.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-framerate",
            str(args.fps),
            "-i",
            str(frames_output / "frame-%05d.ppm"),
            "-i",
            str(audio_path),
            "-af",
            "apad",
            "-t",
            f"{duration:.6f}",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "17",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            str(video_path),
        ],
        check=True,
    )

    sorted_hot = sorted(hot_path_ms)
    p95_index = max(0, math.ceil(len(sorted_hot) * 0.95) - 1)
    manifest = {
        "schema": "interactive-npcs-dense-observed-lip-proof/v1",
        "status": "rendered-not-qualified",
        "source": str(source_root),
        "sourceFrames": [source_paths[0].name, source_paths[-1].name],
        "atlas": str(atlas_root),
        "atlasFrames": [states[0].index, states[-1].index],
        "acceptedSourceTrackerFrames": source_landmark_document.get("accepted"),
        "acceptedAtlasTrackerFrames": atlas_landmark_document.get("accepted"),
        "audio": str(audio_path),
        "audioStateDriver": (
            "cpu-log-mel-centroids" if spectral_indices is not None else "rms-aperture-only"
        ),
        "atlasPack": str(atlas_pack_path) if atlas_pack_path is not None else None,
        "singleOpenReference": (
            str(single_open_image) if single_open_image is not None else None
        ),
        "fps": args.fps,
        "frames": len(outputs),
        "atlasStateCount": len(states),
        "geometry": (
            "precomputed dense teacher motion plus appearance residual on the moving frame"
            if args.transfer_mode == "flow-delta"
            else (
                "same-identity observed active-minus-neutral delta on the moving frame"
                if args.transfer_mode == "observed-delta"
                else (
                    "current-frame support warp plus rigid same-identity observed lip morphology"
                    if args.transfer_mode == "rigid-observed-lip"
                    else (
                        "current-frame support deformation plus same-identity observed full-lip mesh"
                        if args.transfer_mode == "full-observed-lip"
                        else "current-frame lip deformation plus observed oral-interior transfer"
                    )
                )
            )
        ),
        "transferMode": args.transfer_mode,
        "pixelCrossfades": 0,
        "rectangleResizes": 0,
        "gpuVramBytes": 0,
        "maximumOpenness": max(openness),
        "requestedOpenness": openness,
        "selectedStateOpenness": selected_openness,
        "maximumAdjacentOpennessDelta": max(
            abs(current - previous) for previous, current in zip(openness, openness[1:])
        ),
        "distinctObservedStates": len(set(state_indices)),
        "maximumObservedStateJump": max(
            abs(current - previous)
            for previous, current in zip(state_indices, state_indices[1:])
        ),
        "meanMaskPixels": statistics.fmean(mask_pixels),
        "hotPathMilliseconds": {
            "scope": (
                "current-frame dense lip deformation, cached observed-state oral-interior "
                "remap, exact contour masks, and source composite"
                if args.transfer_mode == "oral-interior"
                else (
                    "rejected comparison mode: cached local remap and source composite"
                )
            ),
            "mean": statistics.fmean(hot_path_ms),
            "p50": statistics.median(hot_path_ms),
            "p95": sorted_hot[p95_index],
            "maximum": max(hot_path_ms),
        },
        "selectedStateIndices": state_indices,
        "video": str(video_path),
    }
    (output / "dense-observed-lip-proof.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (cv2.error, json.JSONDecodeError, OSError, ValueError, wave.Error) as error:
        print(f"dense observed lip proof error: {error}", file=sys.stderr)
        sys.exit(2)
