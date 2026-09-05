#!/usr/bin/env python3
"""Create an enlarged source/output board and decoded-frame integrity metrics."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont


def read_video(path: Path) -> tuple[list[np.ndarray], float]:
    capture = cv2.VideoCapture(str(path))
    fps = float(capture.get(cv2.CAP_PROP_FPS))
    frames: list[np.ndarray] = []
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        frames.append(cv2.cvtColor(frame, cv2.COLOR_BGR2RGB))
    capture.release()
    if not frames or fps <= 0:
        raise RuntimeError(f"could not decode {path}")
    return frames, fps


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--mouth-crop", nargs=4, type=int, required=True, metavar=("X", "Y", "W", "H"))
    args = parser.parse_args()
    destination = args.destination
    destination.mkdir(parents=True, exist_ok=True)
    source, source_fps = read_video(args.source)
    output, output_fps = read_video(args.output)
    compared = min(len(source), len(output))
    if compared == 0 or source[0].shape != output[0].shape:
        raise RuntimeError("source and output cannot be compared frame-for-frame")

    x, y, width, height = args.mouth_crop
    frame_height, frame_width = source[0].shape[:2]
    if x < 0 or y < 0 or width <= 0 or height <= 0 or x + width > frame_width or y + height > frame_height:
        raise RuntimeError("mouth crop is outside the decoded frame")
    outside = np.ones((frame_height, frame_width), dtype=bool)
    outside[y : y + height, x : x + width] = False
    per_frame: list[dict[str, float | int]] = []
    for index in range(compared):
        delta = np.abs(source[index].astype(np.int16) - output[index].astype(np.int16))
        per_frame.append(
            {
                "frame_index": index,
                "source_pts_seconds": round(index / source_fps, 6),
                "output_pts_seconds": round(index / output_fps, 6),
                "inside_crop_mean_abs_delta": round(float(delta[y : y + height, x : x + width].mean()), 4),
                "outside_crop_mean_abs_delta": round(float(delta[outside].mean()), 4),
                "outside_crop_p95_abs_delta": round(float(np.percentile(delta[outside], 95)), 4),
            }
        )

    source_motion = [
        float(np.abs(source[index].astype(np.int16) - source[index - 1].astype(np.int16)).mean())
        for index in range(1, len(source))
    ]
    output_motion = [
        float(np.abs(output[index].astype(np.int16) - output[index - 1].astype(np.int16)).mean())
        for index in range(1, len(output))
    ]

    chosen = sorted({min(compared - 1, value) for value in (3, 10, 18, 26)})
    scale = 3
    label_height = 38
    cell_width = width * scale
    cell_height = height * scale + label_height
    board = Image.new("RGB", (cell_width * len(chosen), cell_height * 2), (17, 19, 22))
    draw = ImageDraw.Draw(board)
    font = ImageFont.load_default(size=22)
    for column, index in enumerate(chosen):
        for row, (label, frames) in enumerate((("SOURCE", source), ("MUSETALK 1.5", output))):
            crop = Image.fromarray(frames[index][y : y + height, x : x + width]).resize(
                (cell_width, height * scale), Image.Resampling.LANCZOS
            )
            left = column * cell_width
            top = row * cell_height + label_height
            board.paste(crop, (left, top))
            draw.text((left + 10, row * cell_height + 8), f"{label} · f{index:02d}", fill=(238, 241, 244), font=font)
    board_path = destination / "source-output-enlarged-mouth-board.png"
    board.save(board_path)

    summary = {
        "schema": "interactive-npcs-moving-lipsync-video-audit/v1",
        "source": str(args.source.resolve(strict=True)),
        "output": str(args.output.resolve(strict=True)),
        "source_frame_count": len(source),
        "output_frame_count": len(output),
        "compared_frame_count": compared,
        "source_fps": source_fps,
        "output_fps": output_fps,
        "frame_count_match": len(source) == len(output),
        "fps_match": abs(source_fps - output_fps) < 1e-6,
        "same_index_pts_delta_seconds_max": round(
            max(abs(float(v["source_pts_seconds"]) - float(v["output_pts_seconds"])) for v in per_frame), 6
        ),
        "source_unique_decoded_frame_count": len(
            {hashlib.sha256(frame.tobytes()).hexdigest() for frame in source}
        ),
        "source_consecutive_frame_mean_abs_delta_median": round(float(np.median(source_motion)), 4),
        "output_consecutive_frame_mean_abs_delta_median": round(float(np.median(output_motion)), 4),
        "mouth_crop_pixels": [x, y, width, height],
        "outside_crop_mean_abs_delta_median": round(float(np.median([v["outside_crop_mean_abs_delta"] for v in per_frame])), 4),
        "outside_crop_p95_abs_delta_max": round(float(max(v["outside_crop_p95_abs_delta"] for v in per_frame)), 4),
        "per_frame": per_frame,
        "board": str(board_path),
    }
    (destination / "moving-video-audit.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(board_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
