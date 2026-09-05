#!/usr/bin/env python3
"""Audit native mouth-frame containment and render targeted transition boards."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import cv2
import numpy as np


def frame_paths(directory: Path) -> list[Path]:
    paths = sorted(directory.glob("frame-*.ppm"))
    if not paths:
        raise ValueError(f"no native PPM frames in {directory}")
    return paths


def read(path: Path) -> np.ndarray:
    image = cv2.imread(str(path), cv2.IMREAD_COLOR)
    if image is None:
        raise ValueError(f"failed to decode {path}")
    return image


def parse_roi(value: str) -> tuple[int, int, int, int]:
    parts = tuple(int(part) for part in value.split(","))
    if len(parts) != 4 or min(parts) < 0:
        raise ValueError("ROI must be x,y,width,height with non-negative integers")
    return parts


def parse_groups(value: str) -> list[tuple[str, list[int]]]:
    groups: list[tuple[str, list[int]]] = []
    for raw_group in value.split(";"):
        name, raw_range = raw_group.split(":", 1)
        first, last = (int(part) for part in raw_range.split("-", 1))
        if first < 0 or last < first:
            raise ValueError(f"invalid frame group: {raw_group}")
        groups.append((name, list(range(first, last + 1))))
    return groups


def residual_delta(current: np.ndarray, source: np.ndarray) -> np.ndarray:
    return current.astype(np.int16) - source.astype(np.int16)


def transition_metrics(
    rendered: list[Path], source: list[Path], roi: tuple[int, int, int, int]
) -> list[float]:
    x, y, width, height = roi
    values: list[float] = []
    previous: np.ndarray | None = None
    for index, path in enumerate(rendered):
        current = residual_delta(read(path), read(source[index % len(source)]))[y : y + height, x : x + width]
        if previous is not None:
            values.append(float(np.mean(np.abs(current - previous))))
        previous = current
    return values


def make_board(
    before: list[Path],
    after: list[Path],
    groups: list[tuple[str, list[int]]],
    roi: tuple[int, int, int, int],
    output: Path,
    labels: tuple[str, str],
) -> None:
    x, y, width, height = roi
    cell_width, cell_height = 180, 84
    label_width, group_header = 104, 34
    widest = max(len(indices) for _, indices in groups)
    board = np.full(
        (len(groups) * (group_header + cell_height * 2), label_width + widest * cell_width, 3),
        18,
        np.uint8,
    )
    top = 0
    for name, indices in groups:
        cv2.putText(board, name, (8, top + 23), cv2.FONT_HERSHEY_SIMPLEX, 0.62, (240, 240, 240), 1, cv2.LINE_AA)
        top += group_header
        for row, (label, paths) in enumerate(zip(labels, (before, after))):
            cv2.putText(
                board,
                label,
                (7, top + row * cell_height + 47),
                cv2.FONT_HERSHEY_SIMPLEX,
                0.48,
                (210, 210, 210),
                1,
                cv2.LINE_AA,
            )
            for column, index in enumerate(indices):
                image = read(paths[index])[y : y + height, x : x + width]
                image = cv2.resize(image, (cell_width, cell_height), interpolation=cv2.INTER_NEAREST)
                x0 = label_width + column * cell_width
                y0 = top + row * cell_height
                board[y0 : y0 + cell_height, x0 : x0 + cell_width] = image
                cv2.putText(
                    board,
                    str(index),
                    (x0 + 5, y0 + 15),
                    cv2.FONT_HERSHEY_SIMPLEX,
                    0.42,
                    (250, 250, 250),
                    1,
                    cv2.LINE_AA,
                )
        top += cell_height * 2
    output.parent.mkdir(parents=True, exist_ok=True)
    if not cv2.imwrite(str(output), board):
        raise ValueError(f"failed to write {output}")


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--roi", default="390,240,240,112")
    parser.add_argument("--groups", default="FV-1:38-43;rounded:88-96;FV-2:128-133")
    parser.add_argument("--before-label", default="previous")
    parser.add_argument("--after-label", default="updated")
    parser.add_argument("--events", type=Path, help="Optional native replay frames.jsonl for dynamic containment")
    args = parser.parse_args()

    source = frame_paths(args.source)
    before = frame_paths(args.before)
    after = frame_paths(args.after)
    events = [json.loads(line) for line in args.events.read_text(encoding="utf-8").splitlines()] if args.events else None
    if events is not None and (len(events) != len(after) or len(source) != len(after)):
        raise ValueError("dynamic containment requires complete equal sequences and events")
    if len(before) != len(after):
        raise ValueError("before/after sequences must contain the same frame count")
    roi = parse_roi(args.roi)
    groups = parse_groups(args.groups)
    if max(index for _, indices in groups for index in indices) >= len(after):
        raise ValueError("transition group references a missing frame")

    sample = read(after[0])
    x, y, width, height = roi
    if x + width > sample.shape[1] or y + height > sample.shape[0]:
        raise ValueError("ROI exceeds the native frame")
    outside = np.ones(sample.shape[:2], dtype=bool)
    outside[y : y + height, x : x + width] = False
    changed_outside_pixels = 0
    maximum_outside_channel_delta = 0
    frames_exact_outside = 0
    dynamic_outside_changes = 0
    bypass_frames = 0
    bypass_changed_pixels = 0
    changed_frames = []
    for index, path in enumerate(after):
        rendered = read(path)
        source_image = read(source[index % len(source)])
        delta = np.abs(rendered.astype(np.int16) - source_image.astype(np.int16))
        changed = np.any(delta > 0, axis=2)
        if np.any(changed):
            changed_frames.append(index)
        outside_count = int(np.count_nonzero(changed & outside))
        changed_outside_pixels += outside_count
        maximum_outside_channel_delta = max(
            maximum_outside_channel_delta,
            int(delta[outside].max(initial=0)),
        )
        frames_exact_outside += outside_count == 0
        if events is not None:
            event = events[index]
            if event["frame"] != index:
                raise ValueError("events must be contiguous")
            permitted = np.zeros(changed.shape, dtype=bool)
            if event["residual"]:
                bx, by, bw, bh = event["bounds"]
                h, w = changed.shape
                # One pixel rounding allowance covers serialized float precision.
                left, top = max(0, int(np.floor(bx * w)) - 1), max(0, int(np.floor(by * h)) - 1)
                right, bottom = min(w, int(np.ceil((bx + bw) * w)) + 1), min(h, int(np.ceil((by + bh) * h)) + 1)
                permitted[top:bottom, left:right] = True
            else:
                bypass_frames += 1
                bypass_changed_pixels += int(np.count_nonzero(changed))
            dynamic_outside_changes += int(np.count_nonzero(changed & ~permitted))

    before_transitions = transition_metrics(before, source, roi)
    after_transitions = transition_metrics(after, source, roi)
    make_board(before, after, groups, roi, args.output, (args.before_label, args.after_label))
    report = {
        "schema": "interactive-npcs-native-mouth-sequence-audit/v1",
        "frameCount": len(after),
        "sourceFrameCount": len(source),
        "framesWithChangedPixels": changed_frames,
        "changedFrameCount": len(changed_frames),
        "sourceIdenticalFrameCount": len(after) - len(changed_frames),
        "mouthRoi": [x, y, width, height],
        "framesByteExactOutsideMouthRoi": frames_exact_outside,
        "changedPixelsOutsideMouthRoi": changed_outside_pixels,
        "maximumOutsideMouthChannelDelta": maximum_outside_channel_delta,
        "beforeResidualTransitionP95": round(float(np.percentile(before_transitions, 95)), 4),
        "afterResidualTransitionP95": round(float(np.percentile(after_transitions, 95)), 4),
        "beforeResidualTransitionMax": round(max(before_transitions), 4),
        "afterResidualTransitionMax": round(max(after_transitions), 4),
        "board": str(args.output),
        "dynamicBoundsChecked": events is not None,
        "changedPixelsOutsideDynamicBounds": dynamic_outside_changes if events is not None else None,
        "bypassedFrames": bypass_frames if events is not None else None,
        "changedPixelsInBypassedFrames": bypass_changed_pixels if events is not None else None,
    }
    report_path = args.output.with_suffix(".json")
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))
    # A moving actor can leave the fixed review crop; native per-frame bounds
    # define containment when available, rather than the illustration crop.
    return int((changed_outside_pixels != 0 if events is None else dynamic_outside_changes != 0)
               or bypass_changed_pixels != 0)


if __name__ == "__main__":
    raise SystemExit(main())
