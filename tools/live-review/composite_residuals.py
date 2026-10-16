"""Composite the mouth worker's debug residual dumps over their source frames.

The broker shows residuals in an overlay window that is excluded from screen
capture, so screenshots never contain the presented mouth. Debug worker builds
started with NPC_MOUTH_WORKER_DEBUG_RESIDUAL_DIR write each proposed residual
(raw premultiplied BGRA + JSON) and, every so often, the source frame it was
rendered for. This rebuilds what the overlay put on screen.

Usage:
  python -I composite_residuals.py --dir RUN\\residuals [--sheet RUN\\mouth-sheet.png]

Prints, per residual, the jaw/lip coefficients and how much the composited mouth
area differs from the untouched source (mean absolute difference, 0-255).
"""

from __future__ import annotations

import argparse
import json
import pathlib

import numpy as np
from PIL import Image


def load_bgra(path: pathlib.Path, width: int, height: int, stride: int) -> np.ndarray:
    raw = np.frombuffer(path.read_bytes(), dtype=np.uint8)
    return raw.reshape(height, stride)[:, : width * 4].reshape(height, width, 4).astype(np.float32)


def composite(source: np.ndarray, residual: np.ndarray, left: int, top: int) -> np.ndarray:
    """Premultiplied-alpha 'over' of the residual onto a copy of the source."""
    out = source.copy()
    height, width = residual.shape[:2]
    region = out[top : top + height, left : left + width, :3]
    alpha = residual[..., 3:4] / 255.0
    region[...] = residual[..., :3] + region * (1.0 - alpha)
    return out


def to_rgb_image(bgra: np.ndarray) -> Image.Image:
    return Image.fromarray(np.clip(bgra[..., [2, 1, 0]], 0, 255).astype(np.uint8), "RGB")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", required=True)
    parser.add_argument("--sheet")
    parser.add_argument("--columns", type=int, default=8)
    parser.add_argument("--scale", type=int, default=3)
    parser.add_argument("--margin", type=int, default=12, help="pixels kept around each patch")
    args = parser.parse_args()
    directory = pathlib.Path(args.dir)
    entries = sorted(directory.glob("*.json"))
    if not entries:
        raise SystemExit(f"no residual dumps in {directory}")

    source = None
    crops: list[Image.Image] = []
    changes: list[float] = []
    for entry in entries:
        meta = json.loads(entry.read_text(encoding="utf-8"))
        stem = entry.stem
        if meta["source"]:
            source = load_bgra(directory / f"{stem}-source.bgra", meta["source_width"],
                               meta["source_height"], meta["source_stride_bytes"])
        if source is None:
            continue
        residual = load_bgra(directory / f"{stem}-residual.bgra", meta["width"], meta["height"],
                             meta["stride_bytes"])
        x, y, _, _ = meta["bounds"]
        left = int(round(x * source.shape[1]))
        top = int(round(y * source.shape[0]))
        left = min(max(0, left), source.shape[1] - residual.shape[1])
        top = min(max(0, top), source.shape[0] - residual.shape[0])
        shown = composite(source, residual, left, top)
        box = (slice(top, top + residual.shape[0]), slice(left, left + residual.shape[1]))
        change = float(np.mean(np.abs(shown[box][..., :3] - source[box][..., :3])))
        changes.append(change)
        coverage = float(np.mean(residual[..., 3] > 8))
        print(f"{stem} seq={meta['frame_sequence']:>6} jaw_open={meta['jaw_open']:.3f} "
              f"lip_close={meta['lip_close']:.3f} patch={meta['width']}x{meta['height']}@{left},{top} "
              f"alpha_coverage={coverage:.2f} mouth_change={change:.2f}")
        m = args.margin
        crop_box = (max(0, left - m), max(0, top - m),
                    min(source.shape[1], left + residual.shape[1] + m),
                    min(source.shape[0], top + residual.shape[0] + m))
        crops.append(to_rgb_image(shown).crop(crop_box))

    if not changes:
        raise SystemExit("no residual had a source frame to composite over")
    print(f"residuals: {len(changes)}  mean mouth change: {np.mean(changes):.2f}  "
          f"max: {np.max(changes):.2f}  min: {np.min(changes):.2f}")
    if args.sheet and crops:
        width = max(crop.width for crop in crops)
        height = max(crop.height for crop in crops)
        columns = min(args.columns, len(crops))
        rows = (len(crops) + columns - 1) // columns
        sheet = Image.new("RGB", (columns * width, rows * height))
        for index, crop in enumerate(crops):
            sheet.paste(crop, ((index % columns) * width, (index // columns) * height))
        sheet = sheet.resize((sheet.width * args.scale, sheet.height * args.scale), Image.NEAREST)
        sheet.save(args.sheet)
        print(f"sheet: {args.sheet}")


if __name__ == "__main__":
    main()
