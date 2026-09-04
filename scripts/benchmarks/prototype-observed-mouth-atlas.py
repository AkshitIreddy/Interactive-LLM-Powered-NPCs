#!/usr/bin/env python3
"""Build a visual specimen board for identity-observed mouth atlas blending.

This is deliberately a fast, headless design probe.  It uses mouth appearances
observed from the same authorized source video, aligns each appearance to one
neutral target frame, and renders several mask/geometry choices side by side.
It does not select an actor or qualify the native product path.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageStat


@dataclass(frozen=True)
class Variant:
    name: str
    reference: Path
    reference_center: tuple[float, float]
    reference_size: tuple[int, int]
    destination_center: tuple[float, float]
    destination_size: tuple[int, int]
    mask_inset: tuple[int, int]
    feather: float
    opacity: float


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    if resolved.drive.lower() != "e:" or "temp" not in [part.lower() for part in resolved.parts]:
        raise ValueError("output must remain under E:\\temp")
    return resolved


def crop_around(
    image: Image.Image,
    center: tuple[float, float],
    size: tuple[int, int],
) -> Image.Image:
    width, height = size
    left = round(center[0] - width / 2)
    top = round(center[1] - height / 2)
    return image.crop((left, top, left + width, top + height))


def mean_rgb(image: Image.Image, mask: Image.Image) -> tuple[float, float, float]:
    stats = ImageStat.Stat(image, mask=mask)
    return tuple(float(value) for value in stats.mean[:3])


def color_match(reference: Image.Image, target: Image.Image, mask: Image.Image) -> Image.Image:
    """Apply a bounded per-channel affine match over the feathered patch."""
    ref_stats = ImageStat.Stat(reference, mask=mask)
    target_stats = ImageStat.Stat(target, mask=mask)
    ref_mean = ref_stats.mean[:3]
    target_mean = target_stats.mean[:3]
    ref_std = [max(8.0, value) for value in ref_stats.stddev[:3]]
    target_std = [max(8.0, value) for value in target_stats.stddev[:3]]
    pixels = bytearray(reference.tobytes())
    for offset in range(0, len(pixels), 3):
        for channel in range(3):
            gain = max(0.82, min(1.18, target_std[channel] / ref_std[channel]))
            value = (pixels[offset + channel] - ref_mean[channel]) * gain + target_mean[channel]
            pixels[offset + channel] = max(0, min(255, round(value)))
    return Image.frombytes("RGB", reference.size, bytes(pixels))


def ellipse_mask(
    size: tuple[int, int],
    inset: tuple[int, int],
    feather: float,
    opacity: float,
) -> Image.Image:
    width, height = size
    inset_x, inset_y = inset
    mask = Image.new("L", size, 0)
    draw = ImageDraw.Draw(mask)
    draw.ellipse(
        (inset_x, inset_y, width - 1 - inset_x, height - 1 - inset_y),
        fill=max(0, min(255, round(255 * opacity))),
    )
    return mask.filter(ImageFilter.GaussianBlur(radius=feather))


def render_variant(target: Image.Image, variant: Variant) -> Image.Image:
    reference = Image.open(variant.reference).convert("RGB")
    reference_patch = crop_around(
        reference,
        variant.reference_center,
        variant.reference_size,
    ).resize(variant.destination_size, Image.Resampling.LANCZOS)
    destination_patch = crop_around(
        target,
        variant.destination_center,
        variant.destination_size,
    )
    mask = ellipse_mask(
        variant.destination_size,
        variant.mask_inset,
        variant.feather,
        variant.opacity,
    )
    reference_patch = color_match(reference_patch, destination_patch, mask)
    blended = Image.composite(reference_patch, destination_patch, mask)
    result = target.copy()
    left = round(variant.destination_center[0] - variant.destination_size[0] / 2)
    top = round(variant.destination_center[1] - variant.destination_size[1] / 2)
    result.paste(blended, (left, top))
    return result


def labelled_crop(image: Image.Image, name: str) -> Image.Image:
    crop = image.crop((405, 325, 575, 430)).resize((680, 420), Image.Resampling.LANCZOS)
    canvas = Image.new("RGB", (680, 458), (20, 20, 20))
    canvas.paste(crop, (0, 0))
    draw = ImageDraw.Draw(canvas)
    draw.text((12, 430), name, fill=(245, 245, 245), font=ImageFont.load_default())
    return canvas


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = require_e_temp(args.output)
    output.mkdir(parents=True, exist_ok=True)
    target = Image.open(args.target).convert("RGB")

    # These anchors are measured on the fixed 960x720 Pexels proof fixture.
    # The production implementation obtains both sets from the landmark worker.
    target_center = (486.0, 378.0)
    variants = [
        Variant("A · slight / broad feather", args.references / "frame-00013.ppm",
                (500.0, 379.0), (112, 68), target_center, (100, 60), (5, 4), 6.5, 0.92),
        Variant("B · teeth / broad feather", args.references / "frame-00014.ppm",
                (502.0, 381.0), (116, 72), target_center, (102, 64), (6, 5), 7.0, 0.94),
        Variant("C · teeth / lip-local", args.references / "frame-00014.ppm",
                (502.0, 381.0), (108, 62), target_center, (96, 58), (5, 4), 5.0, 0.98),
        Variant("D · open / narrower speech", args.references / "frame-00017.ppm",
                (507.0, 383.0), (122, 76), target_center, (92, 68), (4, 5), 6.0, 0.96),
        Variant("E · open / compact", args.references / "frame-00017.ppm",
                (507.0, 383.0), (112, 68), target_center, (88, 62), (4, 4), 4.5, 0.98),
        Variant("F · open / soft identity", args.references / "frame-00016.ppm",
                (505.0, 382.0), (118, 72), target_center, (96, 66), (6, 5), 8.0, 0.88),
    ]

    rendered = [("source", target)]
    for variant in variants:
        image = render_variant(target, variant)
        image.save(output / f"{variant.name[0].lower()}-variant.png")
        rendered.append((variant.name, image))

    cells = [labelled_crop(image, name) for name, image in rendered]
    columns = 4
    rows = (len(cells) + columns - 1) // columns
    board = Image.new("RGB", (columns * 680, rows * 458), (12, 12, 12))
    for index, cell in enumerate(cells):
        board.paste(cell, ((index % columns) * 680, (index // columns) * 458))
    board.save(output / "observed-atlas-variant-board.png")
    print(output / "observed-atlas-variant-board.png")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"prototype error: {error}", file=sys.stderr)
        sys.exit(2)
