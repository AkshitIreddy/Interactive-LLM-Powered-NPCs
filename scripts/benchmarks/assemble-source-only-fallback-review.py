#!/usr/bin/env python3
"""Build a lossless audit board for the native no-mouth-pack fallback.

The board compares the exact game frame, the source-only current-pixel result,
and an optional reviewed character-pack result. It never runs inference or
changes exposure. The source-only replay report is required to state that no
character pack or invented oral texture was used.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_indices(value: str) -> list[int]:
    try:
        result = [int(part) for part in value.split(",")]
    except ValueError as error:
        raise argparse.ArgumentTypeError("frames must be comma-separated integers") from error
    if not result or len(result) > 12 or any(index < 0 for index in result):
        raise argparse.ArgumentTypeError("select between 1 and 12 non-negative frames")
    if len(result) != len(set(result)):
        raise argparse.ArgumentTypeError("frame indices must be unique")
    return result


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def read_events(path: Path) -> list[dict]:
    events = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()
              if line.strip()]
    if not events or [event["outputFrame"] for event in events] != list(range(len(events))):
        raise ValueError("event ledger must be non-empty and contiguous")
    return events


def source_frame_path(root: Path, source_index: int) -> Path:
    # Infer the sequence convention once from frame zero. Testing the one-based
    # name first for every index silently shifts zero-based sequences because
    # both `frame-00000` and `frame-00001` exist.
    zero_based_sequence = (root / "frame-00000.ppm").exists()
    path = root / f"frame-{source_index if zero_based_sequence else source_index + 1:05d}.ppm"
    if path.exists():
        return path
    raise FileNotFoundError(f"missing source frame {source_index}")


def output_frame_path(root: Path, output_index: int) -> Path:
    path = root / "frames" / f"frame-{output_index:05d}.ppm"
    if not path.exists():
        raise FileNotFoundError(f"missing output frame {output_index} in {root}")
    return path


def crop_box(bounds: list[float], width: int, height: int) -> tuple[int, int, int, int]:
    x, y, w, h = bounds
    center_x = (x + w * 0.5) * width
    center_y = (y + h * 0.5) * height
    crop_width = max(150.0, w * width * 3.0)
    crop_height = max(96.0, h * height * 2.35)
    left = max(0, int(round(center_x - crop_width * 0.5)))
    top = max(0, int(round(center_y - crop_height * 0.5)))
    right = min(width, int(round(center_x + crop_width * 0.5)))
    bottom = min(height, int(round(center_y + crop_height * 0.5)))
    return left, top, right, bottom


def load_crop(path: Path, box: tuple[int, int, int, int], size: tuple[int, int]) -> Image.Image:
    with Image.open(path) as image:
        return image.convert("RGB").crop(box).resize(size, Image.Resampling.NEAREST)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--source-only", type=Path, required=True)
    parser.add_argument("--reviewed", type=Path)
    parser.add_argument("--frames", type=parse_indices, default=parse_indices("1,5,10,14,20,26"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("output must be a fresh directory")

    report_path = args.source_only / "current-pixel-replay-report.json"
    events_path = args.source_only / "frames.jsonl"
    report = read_json(report_path)
    expected_appearance = "source-only current pixels; no character pack or invented oral texture"
    if report.get("status") != "passed" or report.get("mouthAppearance") != expected_appearance:
        parser.error("source-only replay is not a passed no-pack result")
    events = read_events(events_path)
    if any(index >= len(events) for index in args.frames):
        parser.error("selected frame exceeds the event ledger")

    args.output.mkdir(parents=True)
    rows = [("GAME FRAME", args.source)]
    rows.append(("NO PACK · SOURCE PIXELS", args.source_only))
    if args.reviewed:
        rows.append(("REVIEWED PACK", args.reviewed))

    tile_size = (390, 250)
    label_width = 250
    header_height = 76
    board = Image.new("RGB", (label_width + tile_size[0] * len(args.frames),
                              header_height + tile_size[1] * len(rows)), (8, 11, 15))
    draw = ImageDraw.Draw(board)
    font = ImageFont.load_default(size=18)
    small = ImageFont.load_default(size=15)
    draw.text((24, 18), "CYBERPUNK 2077 · PACKLESS MOUTH FALLBACK",
              fill=(234, 239, 238), font=font)
    draw.text((24, 46), "Nearest-neighbour enlargement · original exposure · 15 Hz native samples",
              fill=(95, 196, 201), font=small)

    selected_inputs: dict[str, str] = {}
    for column, output_index in enumerate(args.frames):
        event = events[output_index]
        source_path = source_frame_path(args.source, event["sourceFrame"])
        with Image.open(source_path) as source_image:
            box = crop_box(event["bounds"], source_image.width, source_image.height)
        draw.text((label_width + column * tile_size[0] + 12, header_height - 23),
                  f"{output_index:02d}  viseme {event['viseme']}", fill=(239, 77, 91), font=small)
        selected_inputs[str(source_path)] = sha256(source_path)
        for row_index, (label, root) in enumerate(rows):
            if label == "GAME FRAME":
                frame_path = source_path
            else:
                frame_path = output_frame_path(root, output_index)
            selected_inputs[str(frame_path)] = sha256(frame_path)
            crop = load_crop(frame_path, box, tile_size)
            x = label_width + column * tile_size[0]
            y = header_height + row_index * tile_size[1]
            board.paste(crop, (x, y))
            draw.rectangle((x, y, x + tile_size[0] - 1, y + tile_size[1] - 1),
                           outline=(46, 69, 75), width=1)
            if label == "NO PACK · SOURCE PIXELS":
                crop.save(args.output / f"source-only-frame-{output_index:02d}.png", optimize=True)

    for row_index, (label, _) in enumerate(rows):
        y = header_height + row_index * tile_size[1]
        fill = (95, 216, 218) if row_index == 1 else (226, 74, 87)
        draw.text((22, y + 104), label, fill=fill, font=font)

    board_path = args.output / "source-only-mouth-board.png"
    board.save(board_path, optimize=True)
    receipt = {
        "schema": "interactive-npcs-source-only-fallback-review/v1",
        "scope": "headless offline native replay; manual actor binding; not live capture or general NPC qualification",
        "mouthAppearance": expected_appearance,
        "sourceOnlyReport": str(report_path),
        "sourceOnlyReportSha256": sha256(report_path),
        "eventLedgerSha256": sha256(events_path),
        "selectedOutputFrames": args.frames,
        "selectedInputsSha256": selected_inputs,
        "board": str(board_path),
        "boardSha256": sha256(board_path),
        "nativeWorkerAndCompositeP95Ms": report.get("workerAndCompositeP95Ms"),
        "changedFrames": report.get("changedFrames"),
        "residualFrames": report.get("residualFrames"),
        "limits": [
            "no unseen teeth, tongue, or cavity can be produced",
            "opening is capped by current visible aperture",
            "reviewed same-character observations remain the richer path",
        ],
    }
    receipt_path = args.output / "verification.json"
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"board": str(board_path), "boardSha256": receipt["boardSha256"],
                      "receipt": str(receipt_path)}))


if __name__ == "__main__":
    main()
