#!/usr/bin/env python3
"""Summarize native 66-point landmark confidence without changing quality gates.

The landmark dump format stores normalized face rectangles and 66 points whose
third coordinate is OpenSeeFace's per-point confidence.  This tool compares the
whole-face confidence used by the native provider with the mouth subset and
with detector/tracker state.  It intentionally does not reinterpret, clamp, or
calibrate those scores.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Sequence


MOUTH_START = 48
MOUTH_END = 66


@dataclass(frozen=True)
class Case:
    name: str
    report: Path
    frames: Path


def percentile(values: Sequence[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    position = fraction * (len(ordered) - 1)
    low = math.floor(position)
    high = math.ceil(position)
    if low == high:
        return ordered[low]
    weight = position - low
    return ordered[low] * (1.0 - weight) + ordered[high] * weight


def stats(values: Iterable[float]) -> dict[str, float | int | None]:
    finite = [float(value) for value in values if math.isfinite(float(value))]
    return {
        "count": len(finite),
        "min": min(finite) if finite else None,
        "p05": percentile(finite, 0.05),
        "median": statistics.median(finite) if finite else None,
        "mean": statistics.fmean(finite) if finite else None,
        "p95": percentile(finite, 0.95),
        "max": max(finite) if finite else None,
    }


def pearson(left: Sequence[float], right: Sequence[float]) -> float | None:
    pairs = [
        (float(a), float(b))
        for a, b in zip(left, right)
        if math.isfinite(float(a)) and math.isfinite(float(b))
    ]
    if len(pairs) < 2:
        return None
    left_mean = statistics.fmean(a for a, _ in pairs)
    right_mean = statistics.fmean(b for _, b in pairs)
    numerator = sum((a - left_mean) * (b - right_mean) for a, b in pairs)
    left_power = sum((a - left_mean) ** 2 for a, _ in pairs)
    right_power = sum((b - right_mean) ** 2 for _, b in pairs)
    denominator = math.sqrt(left_power * right_power)
    return numerator / denominator if denominator else None


def ppm_dimensions(path: Path) -> tuple[int, int]:
    tokens: list[bytes] = []
    with path.open("rb") as stream:
        while len(tokens) < 4:
            line = stream.readline()
            if not line:
                break
            line = line.partition(b"#")[0]
            tokens.extend(line.split())
    if len(tokens) < 4 or tokens[0] not in (b"P3", b"P6"):
        raise ValueError(f"unsupported or incomplete PPM header: {path}")
    return int(tokens[1]), int(tokens[2])


def face_rect(frame: dict[str, Any]) -> Sequence[float] | None:
    value = frame.get("face") or frame.get("rawFace")
    return value if isinstance(value, list) and len(value) >= 4 else None


def point_confidences(frame: dict[str, Any]) -> list[float]:
    points = frame.get("packetLandmarks")
    if not isinstance(points, list) or len(points) < MOUTH_END:
        return []
    result: list[float] = []
    for point in points:
        if not isinstance(point, list) or len(point) < 3:
            return []
        result.append(float(point[2]))
    return result


def reason_counts(frames: Sequence[dict[str, Any]]) -> dict[str, int]:
    result: dict[str, int] = {}
    for frame in frames:
        reason = str(frame.get("reason") or frame.get("runtimeDisposition") or "accepted")
        result[reason] = result.get(reason, 0) + 1
    return dict(sorted(result.items()))


def analyze(case: Case, threshold: float) -> dict[str, Any]:
    payload = json.loads(case.report.read_text(encoding="utf-8"))
    frames = payload.get("frames")
    if not isinstance(frames, list) or not frames:
        raise ValueError(f"report has no frames: {case.report}")

    source_width, source_height = ppm_dimensions(case.frames / str(frames[0]["file"]))
    landmark_confidence: list[float] = []
    all_point_mean: list[float] = []
    mouth_mean: list[float] = []
    mouth_min: list[float] = []
    nonmouth_mean: list[float] = []
    detector_confidence: list[float] = []
    face_width_px: list[float] = []
    face_height_px: list[float] = []
    detector_frame_confidence: list[float] = []
    tracker_frame_confidence: list[float] = []
    out_of_bounds = 0

    for frame in frames:
        confidence = float(frame.get("landmarkConfidence", math.nan))
        landmark_confidence.append(confidence)
        detector_confidence.append(float(frame.get("detectorConfidence", math.nan)))
        points = point_confidences(frame)
        all_point_mean.append(statistics.fmean(points) if points else math.nan)
        mouth = points[MOUTH_START:MOUTH_END]
        nonmouth = points[:MOUTH_START]
        mouth_mean.append(statistics.fmean(mouth) if mouth else math.nan)
        mouth_min.append(min(mouth) if mouth else math.nan)
        nonmouth_mean.append(statistics.fmean(nonmouth) if nonmouth else math.nan)

        rect = face_rect(frame)
        if rect:
            x, y, width, height = map(float, rect[:4])
            face_width_px.append(width * source_width)
            face_height_px.append(height * source_height)
            if x < 0.0 or y < 0.0 or x + width > 1.0 or y + height > 1.0:
                out_of_bounds += 1
        else:
            face_width_px.append(math.nan)
            face_height_px.append(math.nan)

        if frame.get("detectorRan"):
            detector_frame_confidence.append(confidence)
        if frame.get("usedTrackedRoi"):
            tracker_frame_confidence.append(confidence)

    recomputed_passes = sum(value >= threshold for value in landmark_confidence)
    accepted = sum(bool(frame.get("accepted")) for frame in frames)
    return {
        "case": case.name,
        "report": str(case.report),
        "sourceFrames": str(case.frames),
        "sourceDimensions": [source_width, source_height],
        "totalFrames": len(frames),
        "reportedAcceptedFrames": accepted,
        "reportedAcceptanceRatio": accepted / len(frames),
        "wholeFaceThreshold": threshold,
        "wholeFaceThresholdPasses": recomputed_passes,
        "confidence": {
            "providerWholeFace": stats(landmark_confidence),
            "recomputedAll66Mean": stats(all_point_mean),
            "mouth18Mean": stats(mouth_mean),
            "mouth18Minimum": stats(mouth_min),
            "nonMouth48Mean": stats(nonmouth_mean),
            "detector": stats(detector_confidence),
            "detectorRunWholeFace": stats(detector_frame_confidence),
            "trackedRoiWholeFace": stats(tracker_frame_confidence),
        },
        "faceBoxPixels": {
            "width": stats(face_width_px),
            "height": stats(face_height_px),
            "outOfBoundsFrames": out_of_bounds,
        },
        "correlation": {
            "wholeFaceToMouthMean": pearson(landmark_confidence, mouth_mean),
            "wholeFaceToMouthMinimum": pearson(landmark_confidence, mouth_min),
            "wholeFaceToFaceWidthPixels": pearson(landmark_confidence, face_width_px),
            "wholeFaceToDetectorConfidence": pearson(
                landmark_confidence, detector_confidence
            ),
        },
        "reasons": reason_counts(frames),
    }


def parse_case(value: str) -> Case:
    parts = value.split("=", 2)
    if len(parts) != 3 or not all(parts):
        raise argparse.ArgumentTypeError("case must be NAME=REPORT_JSON=SOURCE_PPM_DIR")
    return Case(parts[0], Path(parts[1]), Path(parts[2]))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--case",
        action="append",
        type=parse_case,
        required=True,
        help="NAME=REPORT_JSON=SOURCE_PPM_DIR; repeat for each corpus",
    )
    parser.add_argument("--threshold", type=float, default=0.82)
    parser.add_argument(
        "--minimum-acceptance",
        type=float,
        help="exit 2 when any reported acceptance ratio is below this value",
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    result = {
        "schema": 1,
        "method": "offline-native-landmark-confidence-audit",
        "cases": [analyze(case, args.threshold) for case in args.case],
    }
    rendered = json.dumps(result, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)

    if args.minimum_acceptance is not None and any(
        case["reportedAcceptanceRatio"] < args.minimum_acceptance
        for case in result["cases"]
    ):
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
