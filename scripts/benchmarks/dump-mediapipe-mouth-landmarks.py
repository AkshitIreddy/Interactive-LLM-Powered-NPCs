#!/usr/bin/env python3
"""Dump dense, tracker-neutral mouth anchors for headless atlas enrollment.

This tool is intentionally outside the hot path.  It lets qualification compare
the existing tiny OpenSeeFace tracker with MediaPipe's denser CPU face mesh and
stores only geometry/confidence metadata beside test artifacts on E:\\temp.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import sys
import time

import cv2
import mediapipe as mp


OUTER_UPPER = [61, 185, 40, 39, 37, 0, 267, 269, 270, 409, 291]
OUTER_LOWER = [61, 146, 91, 181, 84, 17, 314, 405, 321, 375, 291]
INNER_UPPER = [78, 191, 80, 81, 82, 13, 312, 311, 310, 415, 308]
INNER_LOWER = [78, 95, 88, 178, 87, 14, 317, 402, 318, 324, 308]
ALL_MOUTH = sorted(set(OUTER_UPPER + OUTER_LOWER + INNER_UPPER + INNER_LOWER))


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    if resolved.drive.lower() != "e:" or "temp" not in [part.lower() for part in resolved.parts]:
        raise ValueError("output must remain under E:\\temp")
    return resolved


def image_paths(input_path: Path) -> list[Path]:
    if input_path.is_file():
        return [input_path]
    candidates = sorted(path for path in input_path.iterdir() if path.suffix.lower() in {".ppm", ".png", ".jpg", ".jpeg"})
    ppm_paths = [path for path in candidates if path.suffix.lower() == ".ppm"]
    paths = ppm_paths or candidates
    if not paths:
        raise ValueError("input contains no supported images")
    return paths


def point(landmarks, index: int) -> list[float]:
    value = landmarks[index]
    return [float(value.x), float(value.y), float(value.z)]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = require_e_temp(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    paths = image_paths(args.input)
    frames: list[dict] = []
    elapsed_ms: list[float] = []

    with mp.solutions.face_mesh.FaceMesh(
        static_image_mode=True,
        max_num_faces=1,
        refine_landmarks=True,
        min_detection_confidence=0.45,
    ) as mesh:
        for path in paths:
            image = cv2.imread(str(path), cv2.IMREAD_COLOR)
            if image is None:
                frames.append({"file": path.name, "accepted": False, "reason": "decode_failed"})
                continue
            started = time.perf_counter()
            result = mesh.process(cv2.cvtColor(image, cv2.COLOR_BGR2RGB))
            elapsed_ms.append((time.perf_counter() - started) * 1000.0)
            if not result.multi_face_landmarks:
                frames.append({"file": path.name, "accepted": False, "reason": "face_not_found"})
                continue
            landmarks = result.multi_face_landmarks[0].landmark
            mouth = [landmarks[index] for index in ALL_MOUTH]
            left = min(value.x for value in mouth)
            right = max(value.x for value in mouth)
            top = min(value.y for value in mouth)
            bottom = max(value.y for value in mouth)
            width = right - left
            height = bottom - top
            corner_left = landmarks[61]
            corner_right = landmarks[291]
            corner_width = math.hypot(corner_right.x - corner_left.x,
                                      corner_right.y - corner_left.y)
            accepted = (
                0.035 <= width <= 0.55
                and 0.008 <= height <= 0.35
                and 0.035 <= corner_width <= 0.55
                and -0.08 <= left <= 1.0
                and 0.0 <= right <= 1.08
                and 0.0 <= top <= 1.0
                and 0.0 <= bottom <= 1.0
            )
            center_x = (corner_left.x + corner_right.x) * 0.5
            center_y = (landmarks[0].y + landmarks[17].y) * 0.5
            frames.append({
                "file": path.name,
                "accepted": accepted,
                "reason": "accepted" if accepted else "implausible_mouth_geometry",
                "width": int(image.shape[1]),
                "height": int(image.shape[0]),
                "center": [center_x, center_y],
                "cornerWidth": corner_width,
                "rollRadians": math.atan2(corner_right.y - corner_left.y,
                                           corner_right.x - corner_left.x),
                "bounds": [left, top, width, height],
                "outerUpper": [point(landmarks, index) for index in OUTER_UPPER],
                "outerLower": [point(landmarks, index) for index in OUTER_LOWER],
                "innerUpper": [point(landmarks, index) for index in INNER_UPPER],
                "innerLower": [point(landmarks, index) for index in INNER_LOWER],
            })

    accepted_count = sum(bool(frame["accepted"]) for frame in frames)
    document = {
        "schema": "interactive-npcs-mediapipe-mouth-landmarks/v1",
        "model": "MediaPipe Face Mesh 0.10.21 refine_landmarks CPU",
        "accepted": accepted_count,
        "total": len(frames),
        "meanInferenceMs": sum(elapsed_ms) / len(elapsed_ms) if elapsed_ms else 0.0,
        "frames": frames,
    }
    output.write_text(json.dumps(document, indent=2), encoding="utf-8")
    print(f"{output} accepted={accepted_count}/{len(frames)}")
    return 0 if accepted_count == len(frames) else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"MediaPipe landmark dump error: {error}", file=sys.stderr)
        sys.exit(2)
