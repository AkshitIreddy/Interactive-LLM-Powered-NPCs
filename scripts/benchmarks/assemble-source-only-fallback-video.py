#!/usr/bin/env python3
"""Encode a headless source/basic/full temporal mouth review.

The three columns use the same moving source frame and prerecorded audio. The
script refuses an unmarked no-pack replay, never calls a model or provider, and
writes hashes and stream metadata beside the review video.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


SOURCE_ONLY_APPEARANCE = (
    "source-only current pixels; no character pack or invented oral texture"
)
REVIEWED_APPEARANCE = "identity-bound schema-4 oral-reference pack"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def read_events(path: Path) -> list[dict]:
    events = [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    if not events or [event["outputFrame"] for event in events] != list(range(len(events))):
        raise ValueError("event ledger must be non-empty and contiguous")
    return events


def source_frame_path(root: Path, source_index: int) -> Path:
    zero_based = (root / "frame-00000.ppm").exists()
    path = root / f"frame-{source_index if zero_based else source_index + 1:05d}.ppm"
    if not path.exists():
        raise FileNotFoundError(f"missing source frame {source_index}")
    return path


def output_frame_path(root: Path, output_index: int) -> Path:
    path = root / "frames" / f"frame-{output_index:05d}.ppm"
    if not path.exists():
        raise FileNotFoundError(f"missing replay frame {output_index} in {root}")
    return path


def crop_box(event: dict, width: int, height: int) -> tuple[int, int, int, int]:
    x, y, w, h = event["bounds"]
    center_x = (x + w * 0.5) * width
    center_y = (y + h * 0.5) * height
    crop_width = max(180.0, w * width * 3.0)
    crop_height = max(120.0, h * height * 2.6)
    # Preserve the 4:3 review-tile ratio rather than stretching faces.
    if crop_width / crop_height < 4.0 / 3.0:
        crop_width = crop_height * 4.0 / 3.0
    else:
        crop_height = crop_width * 3.0 / 4.0
    left = max(0, int(round(center_x - crop_width * 0.5)))
    top = max(0, int(round(center_y - crop_height * 0.5)))
    right = min(width, int(round(center_x + crop_width * 0.5)))
    bottom = min(height, int(round(center_y + crop_height * 0.5)))
    return left, top, right, bottom


def load_tile(path: Path, box: tuple[int, int, int, int]) -> Image.Image:
    with Image.open(path) as image:
        return image.convert("RGB").crop(box).resize((480, 360), Image.Resampling.LANCZOS)


def command_output(command: list[str]) -> str:
    completed = subprocess.run(
        command, check=True, capture_output=True, text=True, encoding="utf-8"
    )
    return completed.stdout.strip()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--actor", required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--source-only", type=Path, required=True)
    parser.add_argument("--reviewed", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("output must be a fresh directory")
    if not args.audio.is_file():
        parser.error("audio must be a readable prerecorded file")

    source_only_report_path = args.source_only / "current-pixel-replay-report.json"
    reviewed_report_path = args.reviewed / "current-pixel-replay-report.json"
    source_only_events_path = args.source_only / "frames.jsonl"
    reviewed_events_path = args.reviewed / "frames.jsonl"
    source_only_report = read_json(source_only_report_path)
    reviewed_report = read_json(reviewed_report_path)
    if (
        source_only_report.get("status") != "passed"
        or source_only_report.get("mouthAppearance") != SOURCE_ONLY_APPEARANCE
    ):
        parser.error("source-only replay is not a passed no-pack result")
    if (
        reviewed_report.get("status") != "passed"
        or reviewed_report.get("mouthAppearance") != REVIEWED_APPEARANCE
    ):
        parser.error("reviewed replay is not a passed schema-four result")
    events = read_events(source_only_events_path)
    reviewed_events = read_events(reviewed_events_path)
    if len(events) != len(reviewed_events):
        parser.error("replays must contain the same output-frame count")
    if any(
        first["sourceFrame"] != second["sourceFrame"]
        for first, second in zip(events, reviewed_events, strict=True)
    ):
        parser.error("replays do not reference the same source sequence")

    args.output.mkdir(parents=True)
    frame_root = args.output / "review-frames"
    frame_root.mkdir()
    font = ImageFont.load_default(size=22)
    small = ImageFont.load_default(size=15)
    labels = (
        ("SOURCE GAME FRAME", (235, 239, 239)),
        ("BASIC SOURCE MOTION", (95, 216, 222)),
        ("FULL MOUTH PACK", (240, 79, 95)),
    )
    selected_input_hashes: dict[str, str] = {}
    for output_index, event in enumerate(events):
        paths = (
            source_frame_path(args.source, event["sourceFrame"]),
            output_frame_path(args.source_only, output_index),
            output_frame_path(args.reviewed, output_index),
        )
        with Image.open(paths[0]) as source_image:
            box = crop_box(event, source_image.width, source_image.height)
        canvas = Image.new("RGB", (1_440, 414), (7, 10, 14))
        draw = ImageDraw.Draw(canvas)
        for column, (path, (label, colour)) in enumerate(zip(paths, labels, strict=True)):
            selected_input_hashes.setdefault(str(path), sha256(path))
            canvas.paste(load_tile(path, box), (column * 480, 54))
            draw.text((column * 480 + 16, 10), label, fill=colour, font=font)
        draw.text(
            (1_424, 19), f"{output_index:02d}", fill=(130, 147, 151), font=small, anchor="ra"
        )
        canvas.save(frame_root / f"frame-{output_index:05d}.png", optimize=True)

    ffmpeg = shutil.which(args.ffmpeg) or args.ffmpeg
    ffprobe = shutil.which(args.ffprobe) or args.ffprobe
    video_path = args.output / f"{args.actor}-source-basic-full.mp4"
    duration_seconds = len(events) / 15.0
    subprocess.run(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            "15",
            "-start_number",
            "0",
            "-i",
            str(frame_root / "frame-%05d.png"),
            "-i",
            str(args.audio),
            "-frames:v",
            str(len(events)),
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-t",
            f"{duration_seconds:.6f}",
            "-movflags",
            "+faststart",
            str(video_path),
        ],
        check=True,
    )
    probe = json.loads(
        command_output(
            [
                ffprobe,
                "-v",
                "error",
                "-show_entries",
                "format=duration:stream=codec_name,codec_type,width,height,r_frame_rate,nb_frames",
                "-of",
                "json",
                str(video_path),
            ]
        )
    )
    receipt = {
        "schema": "interactive-npcs-source-basic-full-temporal-review/v1",
        "scope": "headless offline native replay comparison; same moving source and prerecorded audio; manual actor binding; not live game qualification",
        "actor": args.actor,
        "columns": ["source game frame", "basic source motion", "full mouth pack"],
        "frameCount": len(events),
        "fps": 15,
        "cropStrategy": "per-frame current residual bounds, expanded to a 4:3 face context",
        "sourceOnlyReportSha256": sha256(source_only_report_path),
        "reviewedReportSha256": sha256(reviewed_report_path),
        "sourceOnlyEventsSha256": sha256(source_only_events_path),
        "reviewedEventsSha256": sha256(reviewed_events_path),
        "audioSha256": sha256(args.audio),
        "selectedInputFramesSha256": selected_input_hashes,
        "video": str(video_path),
        "videoSha256": sha256(video_path),
        "probe": probe,
    }
    receipt_path = args.output / "verification.json"
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"video": str(video_path), "sha256": receipt["videoSha256"],
                      "receipt": str(receipt_path)}))


if __name__ == "__main__":
    main()
