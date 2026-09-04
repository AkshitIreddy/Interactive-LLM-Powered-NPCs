#!/usr/bin/env python3
"""Reject detached, scanline-flat mouth residuals in a headless frame sequence.

This fixture-level audit complements the native correctness tests.  It compares
rendered P6 PPM frames with their moving source frames, isolates the changed
mouth residual, and rejects the two geometric signatures of the v32 failure:

* most active columns beginning or ending on one identical horizontal row;
* no materially open frame becoming tall enough relative to mouth width.

The audit intentionally does not claim perceptual realism.  It is a narrow
regression gate for the pasted-on horizontal cavity that passed the older
whole-mouth motion metric.
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import statistics
import sys


def _token(stream) -> bytes:
    token = bytearray()
    while True:
        byte = stream.read(1)
        if not byte:
            raise ValueError("unexpected end of PPM header")
        if byte == b"#":
            stream.readline()
            continue
        if not byte.isspace():
            token.extend(byte)
            break
    while True:
        byte = stream.read(1)
        if not byte or byte.isspace():
            return bytes(token)
        token.extend(byte)


def read_ppm(path: Path) -> tuple[int, int, bytes]:
    with path.open("rb") as stream:
        if _token(stream) != b"P6":
            raise ValueError(f"{path} is not a binary P6 PPM")
        width = int(_token(stream))
        height = int(_token(stream))
        maximum = int(_token(stream))
        if maximum != 255:
            raise ValueError(f"{path} uses unsupported PPM maximum {maximum}")
        pixels = stream.read()
    expected = width * height * 3
    if len(pixels) != expected:
        raise ValueError(f"{path} has {len(pixels)} pixel bytes; expected {expected}")
    return width, height, pixels


def parse_roi(value: str) -> tuple[int, int, int, int]:
    parts = [int(part.strip()) for part in value.split(",")]
    if len(parts) != 4:
        raise argparse.ArgumentTypeError("ROI must be x0,y0,x1,y1")
    x0, y0, x1, y1 = parts
    if x0 < 0 or y0 < 0 or x1 <= x0 or y1 <= y0:
        raise argparse.ArgumentTypeError("ROI bounds must be positive and ordered")
    return x0, y0, x1, y1


def analyse_pair(
    source_path: Path,
    output_path: Path,
    roi: tuple[int, int, int, int],
    difference_threshold: int,
) -> dict[str, float | int | str] | None:
    source_width, source_height, source = read_ppm(source_path)
    output_width, output_height, output = read_ppm(output_path)
    if (source_width, source_height) != (output_width, output_height):
        raise ValueError(f"frame size mismatch: {source_path} vs {output_path}")

    x0, y0, x1, y1 = roi
    if x1 > source_width or y1 > source_height:
        raise ValueError(f"ROI {roi} exceeds {source_width}x{source_height} frames")

    top_by_x: dict[int, int] = {}
    bottom_by_x: dict[int, int] = {}
    changed = 0
    minimum_x = x1
    maximum_x = x0
    minimum_y = y1
    maximum_y = y0

    for y in range(y0, y1):
        row = y * source_width * 3
        for x in range(x0, x1):
            offset = row + x * 3
            delta = max(
                abs(source[offset] - output[offset]),
                abs(source[offset + 1] - output[offset + 1]),
                abs(source[offset + 2] - output[offset + 2]),
            )
            if delta <= difference_threshold:
                continue
            changed += 1
            minimum_x = min(minimum_x, x)
            maximum_x = max(maximum_x, x)
            minimum_y = min(minimum_y, y)
            maximum_y = max(maximum_y, y)
            top_by_x[x] = min(top_by_x.get(x, y), y)
            bottom_by_x[x] = max(bottom_by_x.get(x, y), y)

    if changed == 0:
        return None

    active_columns = len(top_by_x)
    top_mode_count = Counter(top_by_x.values()).most_common(1)[0][1]
    bottom_mode_count = Counter(bottom_by_x.values()).most_common(1)[0][1]
    residual_width = maximum_x - minimum_x + 1
    residual_height = maximum_y - minimum_y + 1
    return {
        "source": source_path.name,
        "output": output_path.name,
        "changedPixels": changed,
        "residualWidth": residual_width,
        "residualHeight": residual_height,
        "heightOverWidth": residual_height / residual_width,
        "topEdgeModeShare": top_mode_count / active_columns,
        "bottomEdgeModeShare": bottom_mode_count / active_columns,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--source-index-offset", type=int, default=1)
    parser.add_argument("--roi", type=parse_roi, default=(420, 340, 560, 410))
    parser.add_argument("--difference-threshold", type=int, default=5)
    parser.add_argument("--material-pixels", type=int, default=250)
    parser.add_argument("--minimum-material-frames", type=int, default=8)
    parser.add_argument("--maximum-edge-mode-share", type=float, default=0.65)
    parser.add_argument("--minimum-open-height-ratio", type=float, default=0.24)
    parser.add_argument("--output-json", type=Path)
    args = parser.parse_args()

    output_frames = sorted(args.output_dir.glob("frame-*.ppm"))
    if not output_frames:
        raise ValueError(f"no frame-*.ppm files in {args.output_dir}")

    frames: list[dict[str, float | int | str]] = []
    for output_path in output_frames:
        output_index = int(output_path.stem.split("-")[-1])
        source_index = output_index + args.source_index_offset
        source_path = args.source_dir / f"frame-{source_index:05d}.ppm"
        if not source_path.is_file():
            raise FileNotFoundError(source_path)
        result = analyse_pair(
            source_path,
            output_path,
            args.roi,
            args.difference_threshold,
        )
        if result is not None and result["changedPixels"] >= args.material_pixels:
            frames.append(result)

    failures: list[str] = []
    if len(frames) < args.minimum_material_frames:
        failures.append(
            f"only {len(frames)} materially articulated frames; "
            f"expected at least {args.minimum_material_frames}"
        )

    if frames:
        median_top = statistics.median(float(frame["topEdgeModeShare"]) for frame in frames)
        median_bottom = statistics.median(
            float(frame["bottomEdgeModeShare"]) for frame in frames
        )
        maximum_open = max(float(frame["heightOverWidth"]) for frame in frames)
        if median_top > args.maximum_edge_mode_share:
            failures.append(
                f"median top-edge scanline share {median_top:.3f} exceeds "
                f"{args.maximum_edge_mode_share:.3f}"
            )
        if median_bottom > args.maximum_edge_mode_share:
            failures.append(
                f"median bottom-edge scanline share {median_bottom:.3f} exceeds "
                f"{args.maximum_edge_mode_share:.3f}"
            )
        if maximum_open < args.minimum_open_height_ratio:
            failures.append(
                f"maximum residual height/width {maximum_open:.3f} is below "
                f"{args.minimum_open_height_ratio:.3f}"
            )
    else:
        median_top = 0.0
        median_bottom = 0.0
        maximum_open = 0.0

    report = {
        "schema": "interactive-npcs-mouth-render-quality/v1",
        "status": "failed" if failures else "passed",
        "purpose": "reject detached scanline-flat mouth residuals",
        "materialFrames": len(frames),
        "medianTopEdgeModeShare": round(median_top, 6),
        "medianBottomEdgeModeShare": round(median_bottom, 6),
        "maximumResidualHeightOverWidth": round(maximum_open, 6),
        "thresholds": {
            "maximumEdgeModeShare": args.maximum_edge_mode_share,
            "minimumOpenHeightRatio": args.minimum_open_height_ratio,
        },
        "failures": failures,
        "frames": frames,
    }
    encoded = json.dumps(report, indent=2)
    if args.output_json is not None:
        args.output_json.parent.mkdir(parents=True, exist_ok=True)
        args.output_json.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)
    return 1 if failures else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"quality audit error: {error}", file=sys.stderr)
        sys.exit(2)
