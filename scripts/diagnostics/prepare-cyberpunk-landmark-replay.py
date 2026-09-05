#!/usr/bin/env python3
"""Prepare model-derived YuNet + OpenSeeFace native replay evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import re
import statistics
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable

import cv2
import numpy as np
import onnxruntime as ort


EXPECTED_YUNET_SHA256 = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"
EXPECTED_LM1_SHA256 = "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f"
KNOWN_LANDMARK_MODELS = {
    EXPECTED_LM1_SHA256: (
        "lm1", "lm_model1_opt.onnx", "85aa70fc67582d046e771ea73625182a0d8f7475"
    ),
    "5aa0ab2594acaf2e1d3916a8de592cffeb1f1a7a7f778cad2f37948bff6549d1": (
        "lm3", "lm_model3_opt.onnx", "85aa70fc67582d046e771ea73625182a0d8f7475"
    ),
    "2aae9c99a700f756e71781f114ebeac87cdf635f9275a71df80456388c29ceef": (
        "lm4", "lm_model4_opt.onnx", "85aa70fc67582d046e771ea73625182a0d8f7475"
    ),
}
OPENCV_REPOSITORY_REVISION = "47534e27c9851bb1128ccc0102f1145e27f23f98"
FRAME_RATE = 30
DEFAULT_FRAME_COUNT = 282
MAX_FRAME_COUNT = 18_000
DEFAULT_ACTOR_LABEL = "pepe"
DEFAULT_MANUAL_FACE = (390.0, 138.0, 138.0, 212.0)
ACTOR_LABEL_PATTERN = re.compile(r"[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?")


@dataclass(frozen=True)
class Face:
    x: float
    y: float
    width: float
    height: float
    confidence: float

    @property
    def right(self) -> float:
        return self.x + self.width

    @property
    def bottom(self) -> float:
        return self.y + self.height

    @property
    def center(self) -> tuple[float, float]:
        return (self.x + self.width * 0.5, self.y + self.height * 0.5)


@dataclass
class ResolutionState:
    resolution: int
    detector: object
    previous: Face | None = None
    selected: list[Face | None] | None = None
    timings_ms: list[float] | None = None
    candidate_counts: list[int] | None = None

    def __post_init__(self) -> None:
        self.selected = []
        self.timings_ms = []
        self.candidate_counts = []


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def percentile(values: Iterable[float], quantile: float) -> float | None:
    ordered = sorted(values)
    if not ordered:
        return None
    position = (len(ordered) - 1) * quantile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def intersection_area(first: Face, second: Face) -> float:
    width = max(0.0, min(first.right, second.right) - max(first.x, second.x))
    height = max(0.0, min(first.bottom, second.bottom) - max(first.y, second.y))
    return width * height


def expanded(face: Face, margin: float, frame_width: int, frame_height: int) -> Face:
    return Face(
        max(0.0, face.x - face.width * margin),
        max(0.0, face.y - face.height * margin),
        min(float(frame_width), face.right + face.width * margin)
        - max(0.0, face.x - face.width * margin),
        min(float(frame_height), face.bottom + face.height * margin)
        - max(0.0, face.y - face.height * margin),
        face.confidence,
    )


def candidate_matches(candidate: Face, prior: Face, manual: Face,
                      frame_width: int, frame_height: int) -> bool:
    prior_region = expanded(prior, 0.75, frame_width, frame_height)
    manual_region = expanded(manual, 1.25, frame_width, frame_height)
    candidate_area = candidate.width * candidate.height
    if candidate_area <= 0.0:
        return False
    prior_overlap = intersection_area(candidate, prior_region) / candidate_area
    manual_overlap = intersection_area(candidate, manual_region) / candidate_area
    dx = candidate.center[0] - prior.center[0]
    dy = candidate.center[1] - prior.center[1]
    prior_diagonal = math.hypot(prior.width, prior.height)
    return (
        prior_overlap >= 0.50
        and manual_overlap >= 0.50
        and prior_diagonal > 0.0
        and math.hypot(dx, dy) / prior_diagonal <= 1.0
    )


def select_face(rows: np.ndarray | None, scale_x: float, scale_y: float,
                prior: Face, manual: Face, frame_width: int,
                frame_height: int) -> tuple[Face | None, int]:
    if rows is None:
        return None, 0
    candidates: list[Face] = []
    for row in rows:
        face = Face(
            float(row[0]) * scale_x,
            float(row[1]) * scale_y,
            float(row[2]) * scale_x,
            float(row[3]) * scale_y,
            float(row[-1]),
        )
        if (face.width >= 4.0 and face.height >= 4.0
                and candidate_matches(face, prior, manual, frame_width, frame_height)):
            candidates.append(face)
    if not candidates:
        return None, int(len(rows))

    def rank(candidate: Face) -> tuple[float, float]:
        overlap = intersection_area(candidate, prior)
        union = (candidate.width * candidate.height + prior.width * prior.height - overlap)
        iou = overlap / union if union > 0.0 else 0.0
        return (candidate.confidence + iou * 0.25, candidate.confidence)

    return max(candidates, key=rank), int(len(rows))


def detect_frame(state: ResolutionState, frame: np.ndarray, manual: Face,
                 frame_width: int, frame_height: int) -> Face | None:
    target_width = state.resolution
    target_height = int(round(frame.shape[0] * target_width / frame.shape[1]))
    resized = cv2.resize(frame, (target_width, target_height), interpolation=cv2.INTER_LINEAR)
    state.detector.setInputSize((target_width, target_height))
    started = time.perf_counter_ns()
    _, rows = state.detector.detect(resized)
    state.timings_ms.append((time.perf_counter_ns() - started) / 1_000_000.0)
    selected, count = select_face(
        rows,
        frame.shape[1] / target_width,
        frame.shape[0] / target_height,
        state.previous or manual,
        manual,
        frame_width,
        frame_height,
    )
    state.candidate_counts.append(count)
    state.selected.append(selected)
    if selected is not None:
        state.previous = selected
    return selected


def warm_detector(state: ResolutionState, frame: np.ndarray) -> float:
    target_width = state.resolution
    target_height = int(round(frame.shape[0] * target_width / frame.shape[1]))
    resized = cv2.resize(frame, (target_width, target_height), interpolation=cv2.INTER_LINEAR)
    state.detector.setInputSize((target_width, target_height))
    started = time.perf_counter_ns()
    state.detector.detect(resized)
    return (time.perf_counter_ns() - started) / 1_000_000.0


def native_lm_tensor(frame: np.ndarray, face: Face) -> tuple[np.ndarray, tuple[int, int, int, int]]:
    left = max(0, math.floor(face.x - face.width * 0.10))
    top = max(0, math.floor(face.y - face.height * 0.125))
    right = min(frame.shape[1], math.floor(face.right + face.width * 0.10))
    bottom = min(frame.shape[0], math.floor(face.bottom + face.height * 0.125))
    if right - left < 4 or bottom - top < 4:
        raise ValueError("provider_landmark_crop_invalid")

    source_height = frame.shape[0]
    source_width = frame.shape[1]
    ys = top + (np.arange(224, dtype=np.float64) + 0.5) * (bottom - top) / 224.0 - 0.5
    xs = left + (np.arange(224, dtype=np.float64) + 0.5) * (right - left) / 224.0 - 0.5
    y_floor = np.floor(ys)
    x_floor = np.floor(xs)
    y0 = np.clip(y_floor, 0, source_height - 1).astype(np.intp)
    x0 = np.clip(x_floor, 0, source_width - 1).astype(np.intp)
    y1 = np.minimum(y0 + 1, source_height - 1)
    x1 = np.minimum(x0 + 1, source_width - 1)
    fy = np.clip(ys - y_floor, 0.0, 1.0).astype(np.float32)[:, None, None]
    fx = np.clip(xs - x_floor, 0.0, 1.0).astype(np.float32)[None, :, None]
    top_pixels = frame[y0[:, None], x0[None, :], :].astype(np.float32) * (1.0 - fx) + \
        frame[y0[:, None], x1[None, :], :].astype(np.float32) * fx
    bottom_pixels = frame[y1[:, None], x0[None, :], :].astype(np.float32) * (1.0 - fx) + \
        frame[y1[:, None], x1[None, :], :].astype(np.float32) * fx
    resized_bgr = top_pixels * (1.0 - fy) + bottom_pixels * fy
    rgb = resized_bgr[:, :, ::-1]
    mean = np.array([-0.485 / 0.229, -0.456 / 0.224, -0.406 / 0.225], dtype=np.float32)
    scale = np.array(
        [1.0 / (0.229 * 255.0), 1.0 / (0.224 * 255.0), 1.0 / (0.225 * 255.0)],
        dtype=np.float32,
    )
    tensor = (rgb * scale + mean).transpose(2, 0, 1)[None, :, :, :].astype(np.float32)
    return tensor, (left, top, right, bottom)


def decode_lm(output: np.ndarray, crop: tuple[int, int, int, int],
              width: int, height: int) -> dict[str, object]:
    data = output.reshape(198, 28, 28)
    cells = 28 * 28
    points: list[tuple[float, float, float]] = []
    confidence_sum = 0.0
    mouth_confidence_sum = 0.0
    visible = 0
    left, top, right, bottom = crop
    crop_width = right - left
    crop_height = bottom - top
    for landmark in range(66):
        heatmap = data[landmark].reshape(cells)
        index = int(np.argmax(heatmap))
        raw_offset_x = float(np.clip(data[66 + landmark].reshape(cells)[index], 1.0e-7, 0.9999999))
        raw_offset_y = float(np.clip(data[132 + landmark].reshape(cells)[index], 1.0e-7, 0.9999999))
        offset_x = 223.0 * math.log(raw_offset_x / (1.0 - raw_offset_x)) / 16.0
        offset_y = 223.0 * math.log(raw_offset_y / (1.0 - raw_offset_y)) / 16.0
        image_y = top + crop_height / 224.0 * (223.0 * (index // 28) / 27.0 + offset_x)
        image_x = left + crop_width / 224.0 * (223.0 * (index % 28) / 27.0 + offset_y)
        confidence = min(1.0, max(0.0, float(heatmap[index])))
        in_frame = math.isfinite(image_x) and math.isfinite(image_y) and \
            0.0 <= image_x <= width and 0.0 <= image_y <= height
        points.append((min(1.0, max(0.0, image_x / width)),
                       min(1.0, max(0.0, image_y / height)), confidence))
        confidence_sum += confidence
        if confidence >= 0.55 and in_frame:
            visible += 1
        if landmark >= 48:
            mouth_confidence_sum += confidence

    def eye_center(first: int) -> tuple[float, float]:
        return (
            sum(points[index][0] for index in range(first, first + 6)) / 6.0,
            sum(points[index][1] for index in range(first, first + 6)) / 6.0,
        )

    left_eye = eye_center(36)
    right_eye = eye_center(42)
    eye_dx = right_eye[0] - left_eye[0]
    eye_dy = right_eye[1] - left_eye[1]
    eye_distance = math.hypot(eye_dx, eye_dy)
    eye_mid_x = (left_eye[0] + right_eye[0]) * 0.5
    yaw = min(60.0, max(-60.0, (points[30][0] - eye_mid_x) / eye_distance * 35.0)) \
        if eye_distance > 1.0e-6 else 999.0
    roll = math.degrees(math.atan2(eye_dy, eye_dx))
    mouth_confidence = mouth_confidence_sum / 18.0
    return {
        "points": points,
        "landmarkConfidence": confidence_sum / 66.0,
        "mouthConfidence": mouth_confidence,
        "visibilityRatio": visible / 66.0,
        "yaw": yaw,
        "pitch": 0.0,
        "roll": roll,
        "mouthOccluded": mouth_confidence < 0.55 or eye_distance <= 1.0e-6
        or abs(yaw) > 35.0 or abs(roll) > 45.0,
    }


def packet_policy(face: Face, decoded: dict[str, object], frame_width: int,
                  frame_height: int) -> tuple[bool, str]:
    points = decoded["points"]
    assert isinstance(points, list)
    if face.confidence < 0.70:
        return False, "detector_below_0.70"
    if float(decoded["landmarkConfidence"]) < 0.82:
        return False, "landmarks_below_0.82"
    if float(decoded["visibilityRatio"]) < 0.72:
        return False, "visibility_below_0.72"
    if bool(decoded["mouthOccluded"]):
        return False, "mouth_occluded_or_pose"
    face_normalized = Face(face.x / frame_width, face.y / frame_height,
                           face.width / frame_width, face.height / frame_height,
                           face.confidence)
    mouth = points[48:66]
    mouth_left = min(point[0] for point in mouth)
    mouth_top = min(point[1] for point in mouth)
    mouth_right = max(point[0] for point in mouth)
    mouth_bottom = max(point[1] for point in mouth)
    if mouth_right <= mouth_left or mouth_bottom <= mouth_top:
        return False, "invalid_mouth_contour"
    if (mouth_left < face_normalized.x or mouth_top < face_normalized.y
            or mouth_right > face_normalized.right or mouth_bottom > face_normalized.bottom):
        return False, "mouth_outside_face"
    if points[58][0] == points[62][0]:
        return False, "unordered_corners"
    upper = sum(points[index][1] for index in (59, 60, 61)) / 3.0
    lower = sum(points[index][1] for index in (63, 64, 65)) / 3.0
    if upper >= lower and upper - lower > (mouth_bottom - mouth_top) * 0.10:
        return False, "material_inner_lip_crossing"
    return True, "accepted"


def format_number(value: float) -> str:
    return format(value, ".10g")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--frames", type=Path, required=True)
    parser.add_argument("--yunet-model", type=Path, required=True)
    parser.add_argument("--lm1-model", "--landmark-model", dest="landmark_model",
                        type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--actor-label", default=DEFAULT_ACTOR_LABEL)
    parser.add_argument("--manual-seed", nargs=4, type=float,
                        default=DEFAULT_MANUAL_FACE,
                        metavar=("X", "Y", "WIDTH", "HEIGHT"))
    parser.add_argument("--frame-count", type=int, default=DEFAULT_FRAME_COUNT)
    parser.add_argument("--detector-resolution", "--chosen-resolution",
                        dest="detector_resolution", choices=("auto", "640", "960"),
                        default="auto")
    parser.add_argument("--max-frames", type=int)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not ACTOR_LABEL_PATTERN.fullmatch(args.actor_label):
        raise RuntimeError("actor label must be a lowercase alphanumeric slug with optional hyphens")
    if args.frame_count <= 0 or args.frame_count > MAX_FRAME_COUNT:
        raise RuntimeError(f"frame count must be between 1 and {MAX_FRAME_COUNT}")
    if args.max_frames is not None:
        if args.max_frames <= 0 or args.max_frames > MAX_FRAME_COUNT:
            raise RuntimeError(f"max frames must be between 1 and {MAX_FRAME_COUNT}")
        if args.max_frames > args.frame_count:
            raise RuntimeError("max frames cannot exceed frame count")
    if not all(math.isfinite(value) for value in args.manual_seed):
        raise RuntimeError("manual seed values must be finite")
    paths = sorted(args.frames.glob("frame-*.ppm"))
    if args.max_frames is not None:
        paths = paths[: args.max_frames]
    if not paths:
        raise RuntimeError("no frame-*.ppm inputs found")
    if args.max_frames is None and len(paths) != args.frame_count:
        raise RuntimeError(f"expected {args.frame_count} frames, found {len(paths)}")
    args.output.mkdir(parents=True, exist_ok=True)

    yunet_sha = sha256_file(args.yunet_model)
    landmark_sha = sha256_file(args.landmark_model)
    if yunet_sha != EXPECTED_YUNET_SHA256:
        raise RuntimeError(f"YuNet SHA-256 mismatch: {yunet_sha}")
    if landmark_sha not in KNOWN_LANDMARK_MODELS:
        raise RuntimeError(f"unrecognized landmark model SHA-256: {landmark_sha}")
    landmark_label, landmark_filename, openseeface_revision = KNOWN_LANDMARK_MODELS[landmark_sha]

    first_frame = cv2.imread(str(paths[0]), cv2.IMREAD_COLOR)
    if first_frame is None:
        raise RuntimeError("first input frame did not decode")
    frame_height, frame_width = first_frame.shape[:2]
    if frame_width < 4 or frame_height < 4:
        raise RuntimeError("input frame dimensions are invalid")
    manual = Face(*args.manual_seed, 1.0)
    if (manual.x < 0.0 or manual.y < 0.0 or manual.width < 4.0 or manual.height < 4.0
            or manual.right > frame_width or manual.bottom > frame_height):
        raise RuntimeError("manual seed must be a valid pixel rectangle inside the input frame")
    states: dict[int, ResolutionState] = {}
    load_timings: dict[int, float] = {}
    warm_timings: dict[int, float] = {}
    detector_resolutions = (640, 960) if args.detector_resolution == "auto" else (
        int(args.detector_resolution),
    )
    for resolution in detector_resolutions:
        started = time.perf_counter_ns()
        detector = cv2.FaceDetectorYN.create(
            str(args.yunet_model), "", (resolution, frame_height * resolution // frame_width),
            0.50, 0.30, 5000,
        )
        load_timings[resolution] = (time.perf_counter_ns() - started) / 1_000_000.0
        states[resolution] = ResolutionState(resolution, detector)
        warm_timings[resolution] = warm_detector(states[resolution], first_frame)

    frame_manifest: list[dict[str, object]] = []
    frames: list[np.ndarray] = []
    for index, path in enumerate(paths):
        frame = cv2.imread(str(path), cv2.IMREAD_COLOR)
        if frame is None or frame.shape[:2] != (frame_height, frame_width):
            raise RuntimeError(f"frame {path} dimensions differ from the first frame")
        frames.append(frame)
        frame_manifest.append({
            "index": index,
            "file": path.name,
            "bytes": path.stat().st_size,
            "sha256": sha256_file(path),
        })
        for state in states.values():
            detect_frame(state, frame, manual, frame_width, frame_height)

    if args.detector_resolution == "auto":
        def resolution_rank(resolution: int) -> tuple[int, float, float]:
            state = states[resolution]
            selected = [face for face in state.selected if face is not None]
            confidence = statistics.fmean(face.confidence for face in selected) if selected else 0.0
            timing = percentile(state.timings_ms, 0.50) or math.inf
            return (len(selected), confidence, -timing)
        chosen_resolution = max(states, key=resolution_rank)
    else:
        chosen_resolution = int(args.detector_resolution)
    chosen = states[chosen_resolution]

    session_options = ort.SessionOptions()
    session_options.inter_op_num_threads = 1
    session_options.intra_op_num_threads = 1
    session_options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    session_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    session_options.log_severity_level = 3
    lm_load_started = time.perf_counter_ns()
    session = ort.InferenceSession(str(args.landmark_model), sess_options=session_options,
                                   providers=["CPUExecutionProvider"])
    lm_load_ms = (time.perf_counter_ns() - lm_load_started) / 1_000_000.0
    input_name = session.get_inputs()[0].name

    packets: list[dict[str, object] | None] = []
    lm_timings: list[float] = []
    policy_reasons: dict[str, int] = {}
    replay_lines = [
        f"npc-landmark-replay-v1 {frame_width} {frame_height} {len(paths)} {FRAME_RATE}"
    ]
    for index, (frame, face) in enumerate(zip(frames, chosen.selected)):
        if face is None:
            packets.append(None)
            policy_reasons["detector_or_identity_selection_failed"] = \
                policy_reasons.get("detector_or_identity_selection_failed", 0) + 1
            replay_lines.append(f"{index} 0")
            continue
        tensor, crop = native_lm_tensor(frame, face)
        started = time.perf_counter_ns()
        output = session.run(None, {input_name: tensor})[0]
        lm_timings.append((time.perf_counter_ns() - started) / 1_000_000.0)
        decoded = decode_lm(output, crop, frame_width, frame_height)
        accepted, reason = packet_policy(face, decoded, frame_width, frame_height)
        policy_reasons[reason] = policy_reasons.get(reason, 0) + 1
        packet = {"face": face, "decoded": decoded, "accepted": accepted, "reason": reason}
        packets.append(packet)
        values = [
            str(index), "1", format_number(face.confidence),
            format_number(float(decoded["landmarkConfidence"])),
            format_number(float(decoded["visibilityRatio"])),
            format_number(float(decoded["yaw"])),
            format_number(float(decoded["pitch"])),
            format_number(float(decoded["roll"])),
            format_number(face.x / frame_width), format_number(face.y / frame_height),
            format_number(face.width / frame_width), format_number(face.height / frame_height),
            "1" if decoded["mouthOccluded"] else "0",
        ]
        for point in decoded["points"]:
            values.extend(format_number(float(value)) for value in point)
        replay_lines.append(" ".join(values))

    replay_path = args.output / f"{args.actor_label}-yunet-{landmark_label}-landmarks.tsv"
    replay_path.write_text("\n".join(replay_lines) + "\n", encoding="utf-8", newline="\n")
    manifest_path = args.output / "input-frame-manifest.json"
    aggregate = hashlib.sha256()
    for entry in frame_manifest:
        aggregate.update(f"{entry['index']}\t{entry['file']}\t{entry['bytes']}\t{entry['sha256']}\n".encode())
    manifest = {
        "schema": "interactive-npcs-input-frame-manifest/v1",
        "width": frame_width,
        "height": frame_height,
        "fps": FRAME_RATE,
        "frames": frame_manifest,
        "canonicalEntryDigestSha256": aggregate.hexdigest(),
    }
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    sheet_indices = sorted(set(int(value) for value in np.linspace(
        0, len(frames) - 1, min(12, len(frames))
    )))
    sheet_tiles: list[np.ndarray] = []
    for index in sheet_indices:
        frame = frames[index].copy()
        packet = packets[index]
        face = chosen.selected[index]
        if packet is None or face is None:
            tile = cv2.resize(frame, (300, 250), interpolation=cv2.INTER_AREA)
            reason = "no identity-selected face"
            metrics = "packet missing"
            color = (0, 0, 220)
        else:
            decoded = packet["decoded"]
            assert isinstance(decoded, dict)
            points = decoded["points"]
            assert isinstance(points, list)
            color = (0, 180, 0) if packet["accepted"] else (0, 0, 220)
            cv2.rectangle(frame, (round(face.x), round(face.y)),
                          (round(face.right), round(face.bottom)), color, 2)
            for point_index, point in enumerate(points):
                point_color = (0, 220, 255) if point_index >= 48 else (255, 200, 0)
                cv2.circle(frame, (round(point[0] * frame_width), round(point[1] * frame_height)),
                           2 if point_index >= 48 else 1, point_color, -1, cv2.LINE_AA)
            crop_face = expanded(face, 0.55, frame_width, frame_height)
            crop = frame[
                max(0, math.floor(crop_face.y)):min(frame_height, math.ceil(crop_face.bottom)),
                max(0, math.floor(crop_face.x)):min(frame_width, math.ceil(crop_face.right)),
            ]
            tile = cv2.resize(crop, (300, 250), interpolation=cv2.INTER_AREA)
            reason = str(packet["reason"])
            metrics = (f"det {face.confidence:.3f}  all {decoded['landmarkConfidence']:.3f}  "
                       f"mouth {decoded['mouthConfidence']:.3f}")
        canvas = np.full((300, 300, 3), 24, dtype=np.uint8)
        canvas[:250] = tile
        cv2.putText(canvas, f"frame {index}: {reason}", (6, 270),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.40, color, 1, cv2.LINE_AA)
        cv2.putText(canvas, metrics, (6, 289), cv2.FONT_HERSHEY_SIMPLEX,
                    0.37, (225, 225, 225), 1, cv2.LINE_AA)
        sheet_tiles.append(canvas)
    while len(sheet_tiles) % 4:
        sheet_tiles.append(np.full((300, 300, 3), 24, dtype=np.uint8))
    sheet_rows = [np.hstack(sheet_tiles[index:index + 4])
                  for index in range(0, len(sheet_tiles), 4)]
    contact_sheet = np.vstack(sheet_rows)
    contact_sheet_path = args.output / f"{args.actor_label}-yunet-{landmark_label}-contact-sheet.jpg"
    if not cv2.imwrite(str(contact_sheet_path), contact_sheet, [cv2.IMWRITE_JPEG_QUALITY, 94]):
        raise RuntimeError("failed to write landmark contact sheet")

    comparisons: dict[str, object] = {}
    for resolution, state in states.items():
        selected = [face for face in state.selected if face is not None]
        comparisons[str(resolution)] = {
            "inputSize": [resolution, frame_height * resolution // frame_width],
            "modelLoadMs": round(load_timings[resolution], 3),
            "warmupMs": round(warm_timings[resolution], 3),
            "frames": len(paths),
            "identitySelectedFrames": len(selected),
            "droppedFrames": len(paths) - len(selected),
            "detectorConfidenceMean": round(statistics.fmean(face.confidence for face in selected), 6)
            if selected else None,
            "detectorConfidenceMin": round(min(face.confidence for face in selected), 6)
            if selected else None,
            "detectorP50Ms": round(percentile(state.timings_ms, 0.50) or 0.0, 3),
            "detectorP95Ms": round(percentile(state.timings_ms, 0.95) or 0.0, 3),
            "candidateCountMaximum": max(state.candidate_counts) if state.candidate_counts else 0,
        }

    model_packets = [packet for packet in packets if packet is not None]
    global_confidences = [float(packet["decoded"]["landmarkConfidence"])
                          for packet in model_packets]
    mouth_confidences = [float(packet["decoded"]["mouthConfidence"])
                         for packet in model_packets]
    visibility_ratios = [float(packet["decoded"]["visibilityRatio"])
                         for packet in model_packets]
    report = {
        "schema": "interactive-npcs-cyberpunk-yunet-openseeface-replay/v1",
        "createdAt": datetime.now(timezone.utc).isoformat(),
        "status": "experimental-model-packets-not-installed-pack",
        "source": str(args.frames),
        "actorLabel": args.actor_label,
        "frames": len(paths),
        "frameDimensions": [frame_width, frame_height],
        "manualActorSeedPixels": list(args.manual_seed),
        "identitySelection": {
            "method": "manual-seed plus sticky prior overlap and center-distance gate",
            "switchingAllowed": False,
        },
        "models": {
            "yunet": {
                "path": str(args.yunet_model),
                "sha256": yunet_sha,
                "opencvRepositoryRevision": OPENCV_REPOSITORY_REVISION,
            },
            "openseefaceLandmarks": {
                "label": landmark_label,
                "path": str(args.landmark_model),
                "sha256": landmark_sha,
                "repositoryRevision": openseeface_revision,
                "sourceUrl": (
                    "https://raw.githubusercontent.com/emilianavt/OpenSeeFace/"
                    f"{openseeface_revision}/models/{landmark_filename}"
                ),
            },
        },
        "runtime": {
            "python": platform.python_version(),
            "opencv": cv2.__version__,
            "onnxruntime": ort.__version__,
            "providers": session.get_providers(),
        },
        "detectorComparison": comparisons,
        "chosenDetectorResolution": chosen_resolution,
        "landmarks": {
            "sessionLoadMs": round(lm_load_ms, 3),
            "inferenceSamples": len(lm_timings),
            "inferenceP50Ms": round(percentile(lm_timings, 0.50) or 0.0, 3),
            "inferenceP95Ms": round(percentile(lm_timings, 0.95) or 0.0, 3),
            "modelPackets": sum(packet is not None for packet in packets),
            "missingPackets": sum(packet is None for packet in packets),
            "staticNativePolicyAccepted": sum(
                packet is not None and bool(packet["accepted"]) for packet in packets
            ),
            "staticNativePolicyDropped": sum(
                packet is None or not bool(packet["accepted"]) for packet in packets
            ),
            "policyReasons": policy_reasons,
            "globalConfidence": {
                "mean": round(statistics.fmean(global_confidences), 6),
                "minimum": round(min(global_confidences), 6),
                "p50": round(percentile(global_confidences, 0.50) or 0.0, 6),
                "maximum": round(max(global_confidences), 6),
            } if global_confidences else None,
            "mouthOnlyConfidence": {
                "mean": round(statistics.fmean(mouth_confidences), 6),
                "minimum": round(min(mouth_confidences), 6),
                "p50": round(percentile(mouth_confidences, 0.50) or 0.0, 6),
                "maximum": round(max(mouth_confidences), 6),
            } if mouth_confidences else None,
            "visibilityRatio": {
                "mean": round(statistics.fmean(visibility_ratios), 6),
                "minimum": round(min(visibility_ratios), 6),
            } if visibility_ratios else None,
        },
        "artifacts": {
            "replay": str(replay_path),
            "replaySha256": sha256_file(replay_path),
            "inputManifest": str(manifest_path),
            "inputManifestSha256": sha256_file(manifest_path),
            "inputFramesCanonicalEntryDigestSha256": aggregate.hexdigest(),
            "landmarkContactSheet": str(contact_sheet_path),
            "landmarkContactSheetSha256": sha256_file(contact_sheet_path),
        },
        "boundaries": [
            "Detector and landmark confidences are actual model outputs.",
            "No native confidence, geometry, identity, occlusion, or freshness gate is weakened.",
            "Static policy counts do not replace the stateful native replay qualification.",
            "This is CPU-only experimental tracker evidence, not an installed model pack.",
        ],
    }
    report_path = args.output / "qualification.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(report_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
