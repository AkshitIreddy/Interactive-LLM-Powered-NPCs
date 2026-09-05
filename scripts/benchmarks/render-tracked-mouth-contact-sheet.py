"""Inspect moving-mouth pixels at model-derived positions without opening a UI."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--landmarks", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--before", type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--frames", default="0,8,16,24,32,40,46,48,50,54,60,70,80,89")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    lines = args.landmarks.read_text(encoding="utf-8").splitlines()
    magic, sw, sh, total, fps = lines[0].split()
    width, height, count = int(sw), int(sh), int(total)
    if magic != "npc-landmark-replay-v1" or fps != "30" or len(lines) != count + 1:
        raise ValueError("invalid landmark sequence")
    indices = [int(item) for item in args.frames.split(",")]
    if not indices or any(index < 0 or index >= count for index in indices):
        raise ValueError("selected frame is outside the sequence")
    sources = [("SOURCE", args.source)]
    if args.before:
        sources.append(("BEFORE", args.before))
    sources.append(("UPDATED", args.after))
    columns = min(7, len(indices))
    cell_width, cell_height, label_height = 200, 132, 24
    groups = (len(indices) + columns - 1) // columns
    board = Image.new("RGB", (columns * cell_width, groups * (len(sources) * cell_height + label_height)), "#12191e")
    draw = ImageDraw.Draw(board)
    font = ImageFont.truetype("C:/Windows/Fonts/arial.ttf", 13)
    selected = []
    for position, index in enumerate(indices):
        tokens = lines[index + 1].split()
        if int(tokens[0]) != index or tokens[1] != "1" or len(tokens) != 211:
            raise ValueError("selected frame needs a complete measured landmark packet")
        points = np.asarray([float(value) for value in tokens[13:]], dtype=float).reshape(66, 3)
        if not np.isfinite(points).all() or np.any(points < 0) or np.any(points > 1):
            raise ValueError("landmarks must be finite normalized values")
        mouth = points[48:66, :2] * [width, height]
        center = (mouth.min(axis=0) + mouth.max(axis=0)) / 2
        crop_width = min(width, max(65, int(np.ceil(np.ptp(mouth[:, 0]) * 2.5))))
        crop_height = min(height, max(1, int(round(crop_width * .54))))
        left = max(0, min(width - crop_width, int(round(center[0] - crop_width / 2))))
        top = max(0, min(height - crop_height, int(round(center[1] - crop_height / 2))))
        crop = (left, top, left + crop_width, top + crop_height)
        col, group = position % columns, position // columns
        x, y = col * cell_width, group * (len(sources) * cell_height + label_height)
        draw.text((x + 6, y + 4), f"Frame {index} / {index / 30:.3f}s", font=font, fill="#e5e9e9")
        for row, (label, folder) in enumerate(sources):
            image = Image.open(folder / f"frame-{index:05}.ppm").convert("RGB")
            if image.size != (width, height):
                raise ValueError("frame dimensions differ from landmark sequence")
            image = image.crop(crop).resize((cell_width, 108), Image.Resampling.NEAREST)
            row_y = y + label_height + row * cell_height
            board.paste(image, (x, row_y))
            draw.text((x + 6, row_y + 111), label, font=font, fill="#b8c6ce")
        selected.append({"frame": index, "crop": crop})
    args.out.parent.mkdir(parents=True, exist_ok=True)
    board.save(args.out)
    args.out.with_suffix(".json").write_text(json.dumps({
        "scope": "sampled model-centered visual review, not a perceptual score or identity test",
        "landmarks": str(args.landmarks), "selectedFrames": selected,
        "source": str(args.source), "before": str(args.before) if args.before else None,
        "after": str(args.after),
    }, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
