"""Compare a region (e.g. the mouth box) across captured frames.

Usage:
  python -I mouth_diff.py --box x0,y0,x1,y1 --baseline quiet-*.png --frames turn-*.png \
      [--sheet out.png]

Reports the mean absolute difference of each frame's region against the median
of the baseline frames, and optionally writes an enlarged contact sheet.
"""

from __future__ import annotations

import argparse
import glob

import numpy as np
from PIL import Image


def region(path: str, box: tuple[int, int, int, int]) -> np.ndarray:
    return np.asarray(Image.open(path).convert("RGB").crop(box), dtype=np.float32)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--box", required=True)
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--frames", required=True)
    parser.add_argument("--sheet")
    parser.add_argument("--scale", type=int, default=4)
    args = parser.parse_args()
    box = tuple(int(value) for value in args.box.split(","))
    baseline_paths = sorted(glob.glob(args.baseline))
    frame_paths = sorted(glob.glob(args.frames))
    if not baseline_paths or not frame_paths:
        raise SystemExit("no baseline or frame images matched")
    baseline = np.median(np.stack([region(path, box) for path in baseline_paths]), axis=0)
    quiet = [float(np.mean(np.abs(region(path, box) - baseline))) for path in baseline_paths]
    print(f"baseline self-diff: mean={np.mean(quiet):.2f} max={np.max(quiet):.2f}")
    diffs = []
    for path in frame_paths:
        diff = float(np.mean(np.abs(region(path, box) - baseline)))
        diffs.append(diff)
        print(f"{diff:7.2f}  {path}")
    print(f"turn frames: mean={np.mean(diffs):.2f} max={np.max(diffs):.2f}")
    if args.sheet:
        crops = [Image.open(path).convert("RGB").crop(box) for path in frame_paths[:24]]
        width, height = crops[0].size
        columns = min(6, len(crops))
        rows = (len(crops) + columns - 1) // columns
        sheet = Image.new("RGB", (columns * width, rows * height))
        for index, crop in enumerate(crops):
            sheet.paste(crop, ((index % columns) * width, (index // columns) * height))
        sheet = sheet.resize((sheet.width * args.scale, sheet.height * args.scale), Image.NEAREST)
        sheet.save(args.sheet)
        print(f"sheet: {args.sheet}")


if __name__ == "__main__":
    main()
