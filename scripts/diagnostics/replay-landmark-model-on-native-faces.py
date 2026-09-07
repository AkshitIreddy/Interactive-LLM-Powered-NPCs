#!/usr/bin/env python3
"""Replay a pinned OpenSeeFace landmark model on native provider face boxes.

This diagnostic isolates the landmark model from face detection and tracking by
reusing the normalized face box recorded by an existing native dump.  Pixel
preprocessing, landmark decoding, and static packet policy come from
prepare-cyberpunk-landmark-replay.py so the comparison has one implementation.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import platform
import statistics
import sys
import time
from pathlib import Path
from types import ModuleType

import cv2
import numpy as np
import onnxruntime as ort


def load_replay_helpers() -> ModuleType:
    path = Path(__file__).resolve().parents[1] / "diagnostics" / \
        "prepare-cyberpunk-landmark-replay.py"
    spec = importlib.util.spec_from_file_location("npc_prepare_landmark_replay", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load replay helpers: {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-report", type=Path, required=True)
    parser.add_argument("--frames", type=Path, required=True)
    parser.add_argument("--landmark-model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--landmark-board",
        type=Path,
        help="write an enlarged diagnostic board for evenly spaced replay frames",
    )
    parser.add_argument("--actor-label", required=True)
    parser.add_argument("--max-frames", type=int)
    parser.add_argument(
        "--photometric",
        choices=("none", "y-equalize", "clahe", "gamma-1.4", "gamma-1.8"),
        default="none",
    )
    parser.add_argument("--horizontal-margin", type=float, default=0.10)
    parser.add_argument("--vertical-margin", type=float, default=0.125)
    parser.add_argument(
        "--crop-face-source",
        choices=("actual-inference", "recorded-packet"),
        default="actual-inference",
        help=("reconstruct tracked inference from the preceding packet box, or use "
              "the current frame's recorded output box"),
    )
    return parser.parse_args()


def adjusted_for_landmark_crop(helpers: ModuleType, face: object,
                               horizontal_margin: float,
                               vertical_margin: float) -> object:
    """Compensate for the helper's fixed upstream margins to request new margins."""
    width = face.width * (1.0 + 2.0 * horizontal_margin) / 1.20
    height = face.height * (1.0 + 2.0 * vertical_margin) / 1.25
    return helpers.Face(
        face.x + (face.width - width) * 0.5,
        face.y + (face.height - height) * 0.5,
        width,
        height,
        face.confidence,
    )


def apply_photometric(frame: np.ndarray, method: str) -> np.ndarray:
    if method == "none":
        return frame
    if method in ("y-equalize", "clahe"):
        yuv = cv2.cvtColor(frame, cv2.COLOR_BGR2YUV)
        if method == "y-equalize":
            yuv[:, :, 0] = cv2.equalizeHist(yuv[:, :, 0])
        else:
            clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))
            yuv[:, :, 0] = clahe.apply(yuv[:, :, 0])
        return cv2.cvtColor(yuv, cv2.COLOR_YUV2BGR)
    gamma = float(method.removeprefix("gamma-"))
    table = np.clip(
        255.0 * np.power(np.arange(256, dtype=np.float32) / 255.0, 1.0 / gamma),
        0.0,
        255.0,
    ).astype(np.uint8)
    return cv2.LUT(frame, table)


def render_landmark_board(
    path: Path,
    frames_dir: Path,
    results: list[dict[str, object]],
) -> None:
    """Render geometry evidence; confidence acceptance remains a separate gate."""
    chosen = sorted(set(
        round(index) for index in np.linspace(0, len(results) - 1, min(12, len(results)))
    ))
    tiles: list[np.ndarray] = []
    for index in chosen:
        result = results[index]
        frame = cv2.imread(str(frames_dir / str(result["file"])), cv2.IMREAD_COLOR)
        if frame is None:
            raise RuntimeError(f"cannot decode board source frame: {result['file']}")
        height, width = frame.shape[:2]
        face = [float(value) for value in result["face"]]
        left = max(0, math.floor((face[0] - face[2] * 0.75) * width))
        top = max(0, math.floor((face[1] - face[3] * 0.60) * height))
        right = min(width, math.ceil((face[0] + face[2] * 1.75) * width))
        bottom = min(height, math.ceil((face[1] + face[3] * 1.60) * height))
        for point_index, point in enumerate(result["landmarks"]):
            px = round(float(point[0]) * width)
            py = round(float(point[1]) * height)
            color = (0, 220, 255) if point_index >= 48 else (255, 200, 0)
            cv2.circle(frame, (px, py), 3 if point_index >= 48 else 2,
                       color, -1, cv2.LINE_AA)
        crop = frame[top:bottom, left:right]
        if crop.size == 0:
            raise RuntimeError(f"empty board crop for {result['file']}")
        tile = np.full((420, 480, 3), 20, dtype=np.uint8)
        scale = min(480 / crop.shape[1], 350 / crop.shape[0])
        resized = cv2.resize(
            crop,
            (max(1, round(crop.shape[1] * scale)), max(1, round(crop.shape[0] * scale))),
            interpolation=cv2.INTER_CUBIC,
        )
        x = (480 - resized.shape[1]) // 2
        y = (350 - resized.shape[0]) // 2
        tile[y:y + resized.shape[0], x:x + resized.shape[1]] = resized
        accepted = bool(result["accepted"])
        label_color = (80, 220, 100) if accepted else (80, 80, 240)
        cv2.putText(tile, f"frame {index + 1:02d}  {'pass' if accepted else 'bypass'}",
                    (10, 378), cv2.FONT_HERSHEY_SIMPLEX, 0.63,
                    label_color, 2, cv2.LINE_AA)
        cv2.putText(
            tile,
            (f"all {float(result['landmarkConfidence']):.3f}  "
             f"mouth {float(result['mouthConfidence']):.3f}  gate 0.820"),
            (10, 408), cv2.FONT_HERSHEY_SIMPLEX, 0.53,
            (225, 225, 225), 1, cv2.LINE_AA,
        )
        tiles.append(tile)
    while len(tiles) % 4:
        tiles.append(np.full((420, 480, 3), 20, dtype=np.uint8))
    rows = [np.hstack(tiles[start:start + 4]) for start in range(0, len(tiles), 4)]
    board = np.vstack(rows)
    path.parent.mkdir(parents=True, exist_ok=True)
    if not cv2.imwrite(str(path), board, [cv2.IMWRITE_JPEG_QUALITY, 95]):
        raise RuntimeError(f"cannot write landmark board: {path}")


def main() -> int:
    args = parse_args()
    if not 0.0 <= args.horizontal_margin <= 0.5:
        raise RuntimeError("horizontal margin must be between 0 and 0.5")
    if not 0.0 <= args.vertical_margin <= 0.5:
        raise RuntimeError("vertical margin must be between 0 and 0.5")
    helpers = load_replay_helpers()
    native = json.loads(args.native_report.read_text(encoding="utf-8"))
    native_frames = native.get("frames")
    if not isinstance(native_frames, list) or not native_frames:
        raise RuntimeError("native report has no frames")
    if args.max_frames is not None:
        if args.max_frames <= 0:
            raise RuntimeError("max frames must be positive")
        native_frames = native_frames[: args.max_frames]

    landmark_sha = helpers.sha256_file(args.landmark_model)
    if landmark_sha not in helpers.KNOWN_LANDMARK_MODELS:
        raise RuntimeError(f"unrecognized landmark model SHA-256: {landmark_sha}")
    label, filename, revision = helpers.KNOWN_LANDMARK_MODELS[landmark_sha]

    session_options = ort.SessionOptions()
    session_options.inter_op_num_threads = 1
    session_options.intra_op_num_threads = 1
    session_options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    session_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    session_options.log_severity_level = 3
    load_started = time.perf_counter_ns()
    session = ort.InferenceSession(
        str(args.landmark_model),
        sess_options=session_options,
        providers=["CPUExecutionProvider"],
    )
    load_ms = (time.perf_counter_ns() - load_started) / 1_000_000.0
    input_name = session.get_inputs()[0].name

    results: list[dict[str, object]] = []
    timings: list[float] = []
    photometric_timings: list[float] = []
    tensor_timings: list[float] = []
    decode_timings: list[float] = []
    model_path_timings: list[float] = []
    previous_normalized: list[float] | None = None
    for native_frame in native_frames:
        file_name = str(native_frame.get("file", ""))
        if not file_name:
            raise RuntimeError("native frame is missing its file name")
        frame_path = args.frames / file_name
        frame = cv2.imread(str(frame_path), cv2.IMREAD_COLOR)
        if frame is None:
            raise RuntimeError(f"cannot decode source frame: {frame_path}")
        height, width = frame.shape[:2]
        normalized = native_frame.get("face") or native_frame.get("rawFace")
        if not isinstance(normalized, list) or len(normalized) < 4:
            raise RuntimeError(f"native frame has no face box: {file_name}")
        packet_face = helpers.Face(
            float(normalized[0]) * width,
            float(normalized[1]) * height,
            float(normalized[2]) * width,
            float(normalized[3]) * height,
            float(native_frame.get("detectorConfidence", 0.0)),
        )
        crop_normalized = normalized
        crop_face_source = "recorded-packet"
        if (args.crop_face_source == "actual-inference"
                and bool(native_frame.get("usedTrackedRoi"))
                and not bool(native_frame.get("detectorRan"))
                and previous_normalized is not None):
            crop_normalized = previous_normalized
            crop_face_source = "previous-locked-track"
        crop_face = helpers.Face(
            float(crop_normalized[0]) * width,
            float(crop_normalized[1]) * height,
            float(crop_normalized[2]) * width,
            float(crop_normalized[3]) * height,
            packet_face.confidence,
        )
        previous_normalized = [float(value) for value in normalized[:4]]
        landmark_face = adjusted_for_landmark_crop(
            helpers, crop_face, args.horizontal_margin, args.vertical_margin
        )
        model_path_started = time.perf_counter_ns()
        photometric_started = time.perf_counter_ns()
        prepared_frame = apply_photometric(frame, args.photometric)
        photometric_timings.append(
            (time.perf_counter_ns() - photometric_started) / 1_000_000.0
        )
        tensor_started = time.perf_counter_ns()
        tensor, crop = helpers.native_lm_tensor(prepared_frame, landmark_face)
        tensor_timings.append((time.perf_counter_ns() - tensor_started) / 1_000_000.0)
        started = time.perf_counter_ns()
        output = session.run(None, {input_name: tensor})[0]
        timings.append((time.perf_counter_ns() - started) / 1_000_000.0)
        decode_started = time.perf_counter_ns()
        decoded = helpers.decode_lm(output, crop, width, height)
        decode_timings.append((time.perf_counter_ns() - decode_started) / 1_000_000.0)
        model_path_timings.append(
            (time.perf_counter_ns() - model_path_started) / 1_000_000.0
        )
        accepted, reason = helpers.packet_policy(packet_face, decoded, width, height)
        results.append({
            "file": file_name,
            "accepted": accepted,
            "reason": reason,
            "detectorConfidence": packet_face.confidence,
            "landmarkConfidence": decoded["landmarkConfidence"],
            "mouthConfidence": decoded["mouthConfidence"],
            "visibilityRatio": decoded["visibilityRatio"],
            "face": [packet_face.x / width, packet_face.y / height,
                     packet_face.width / width, packet_face.height / height],
            "cropFaceSource": crop_face_source,
            "cropPixels": list(crop),
            "landmarks": decoded["points"],
        })

    confidences = [float(frame["landmarkConfidence"]) for frame in results]
    mouth_confidences = [float(frame["mouthConfidence"]) for frame in results]
    reasons: dict[str, int] = {}
    for frame in results:
        reason = str(frame["reason"])
        reasons[reason] = reasons.get(reason, 0) + 1
    report = {
        "schema": "interactive-npcs-landmark-model-native-face-replay/v1",
        "status": "offline-model-comparison-not-native-provider-qualification",
        "actorLabel": args.actor_label,
        "sourceNativeReport": str(args.native_report),
        "sourceNativeReportSha256": helpers.sha256_file(args.native_report),
        "sourceFrames": str(args.frames),
        "frames": len(results),
        "landmarkModel": {
            "label": label,
            "filename": filename,
            "path": str(args.landmark_model),
            "sha256": landmark_sha,
            "openSeeFaceRevision": revision,
        },
        "preprocessing": {
            "photometric": args.photometric,
            "horizontalMargin": args.horizontal_margin,
            "verticalMargin": args.vertical_margin,
            "upstreamDefaultMargins": [0.10, 0.125],
            "cropFaceSource": args.crop_face_source,
        },
        "runtime": {
            "python": platform.python_version(),
            "opencv": cv2.__version__,
            "onnxruntime": ort.__version__,
            "providers": session.get_providers(),
            "sessionLoadMs": load_ms,
            "inferenceP50Ms": helpers.percentile(timings, 0.50),
            "inferenceP95Ms": helpers.percentile(timings, 0.95),
            "photometricP50Ms": helpers.percentile(photometric_timings, 0.50),
            "photometricP95Ms": helpers.percentile(photometric_timings, 0.95),
            "tensorPreparationP50Ms": helpers.percentile(tensor_timings, 0.50),
            "tensorPreparationP95Ms": helpers.percentile(tensor_timings, 0.95),
            "decodeP50Ms": helpers.percentile(decode_timings, 0.50),
            "decodeP95Ms": helpers.percentile(decode_timings, 0.95),
            "modelPathP50Ms": helpers.percentile(model_path_timings, 0.50),
            "modelPathP95Ms": helpers.percentile(model_path_timings, 0.95),
            "timingBoundary": (
                "CPU wall time on this isolated Python replay; photometric timing covers "
                "the full decoded source frame, model path covers photometric transform, "
                "landmark tensor construction, ORT inference, and decode, and excludes PPM decode, "
                "face detection/tracking, adapter, compositor, capture, and contention"
            ),
        },
        "staticPolicy": {
            "wholeFaceConfidenceThreshold": 0.82,
            "accepted": sum(bool(frame["accepted"]) for frame in results),
            "dropped": sum(not bool(frame["accepted"]) for frame in results),
            "reasons": dict(sorted(reasons.items())),
        },
        "confidence": {
            "wholeFaceMean": statistics.fmean(confidences),
            "wholeFaceMinimum": min(confidences),
            "wholeFaceMedian": statistics.median(confidences),
            "wholeFaceMaximum": max(confidences),
            "mouthMean": statistics.fmean(mouth_confidences),
            "mouthMinimum": min(mouth_confidences),
            "mouthMedian": statistics.median(mouth_confidences),
            "mouthMaximum": max(mouth_confidences),
        },
        "framesDetail": results,
        "boundaries": [
            "Native detector and tracker face boxes are replayed without spatial edits.",
            "Tracked inference reconstructs the preceding locked box when requested.",
            "The 0.82 whole-face confidence gate is unchanged.",
            "Non-default preprocessing variants are diagnostic comparisons, not qualified defaults.",
            "This Python CPU replay isolates landmark-model behavior and does not prove native provider integration.",
            "Static packet policy does not replace stateful adapter or rendered visual qualification.",
        ],
    }
    if args.landmark_board:
        render_landmark_board(args.landmark_board, args.frames, results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
