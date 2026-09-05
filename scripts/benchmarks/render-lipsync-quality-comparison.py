"""Encode an honest before/after visual review from exact native frame outputs.

No display or audio device is opened. Inputs must share geometry/frame count;
the driving WAV sets duration. The side-by-side is a headless comparison only.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import wave
from PIL import Image, ImageDraw, ImageFont


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--audio", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--subtitle", default="Same voice and source motion. Experimental mouth rendering.")
    parser.add_argument("--title", default="LIP-SYNC COMPARISON")
    parser.add_argument("--before-label", default="BEFORE")
    parser.add_argument("--after-label", default="UPDATED")
    parser.add_argument("--mouth-roi", default="390,240,240,112")
    parser.add_argument("--footer", default="Synthetic character and stock voice  |  Headless file render  |  Live-game timing remains unverified")
    args = parser.parse_args()
    before = sorted(args.before.glob("frame-*.ppm"))
    after = sorted(args.after.glob("frame-*.ppm"))
    with wave.open(str(args.audio), "rb") as wav:
        duration = wav.getnframes() / wav.getframerate()
    expected = __import__("math").ceil(duration * 30)
    if len(before) != expected or len(after) != expected:
        raise ValueError(f"complete equal frame sequences required: {len(before)}, {len(after)}, expected {expected}")
    frame_size = Image.open(before[0]).size
    roi = tuple(int(part) for part in args.mouth_roi.split(","))
    if len(roi) != 4 or min(roi) < 0 or roi[2] == 0 or roi[3] == 0 or roi[0] + roi[2] > frame_size[0] or roi[1] + roi[3] > frame_size[1]:
        raise ValueError("mouth ROI must stay within the source frame")
    for files in (before, after):
        for index, path in enumerate(files):
            if path.name != f"frame-{index:05}.ppm" or Image.open(path).size != frame_size:
                raise ValueError("comparison needs contiguous equal-size native frames at 30 fps")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    bg = Image.new("RGB", (1600, 980), "#11181d")
    draw = ImageDraw.Draw(bg)
    def font(size: int, bold: bool = False):
        return ImageFont.truetype("C:/Windows/Fonts/arialbd.ttf" if bold else "C:/Windows/Fonts/arial.ttf", size)
    draw.text((32, 22), args.title, font=font(28, True), fill="#f1f4ef")
    draw.text((32, 62), args.subtitle, font=font(19), fill="#b2c0c6")
    for x, label in ((32, args.before_label), (816, args.after_label)):
        draw.text((x, 104), label, font=font(21, True), fill="#b3d7c3" if x == 816 else "#c6cdd1")
        draw.text((x, 618), "Mouth detail", font=font(18), fill="#a8b7be")
    draw.text((32, 939), args.footer, font=font(18), fill="#a8b7be")
    background = args.out.with_suffix(".background.png")
    bg.save(background)
    graph = (
        "[0:v]split=2[bf][bm];[1:v]split=2[af][am];"
        "[bf]scale=752:470:force_original_aspect_ratio=decrease:flags=lanczos,pad=752:470:(ow-iw)/2:(oh-ih)/2[bfull];"
        "[af]scale=752:470:force_original_aspect_ratio=decrease:flags=lanczos,pad=752:470:(ow-iw)/2:(oh-ih)/2[afull];"
        f"[bm]crop={roi[2]}:{roi[3]}:{roi[0]}:{roi[1]},scale=600:280:force_original_aspect_ratio=decrease:flags=neighbor,pad=600:280:(ow-iw)/2:(oh-ih)/2[bmouth];"
        f"[am]crop={roi[2]}:{roi[3]}:{roi[0]}:{roi[1]},scale=600:280:force_original_aspect_ratio=decrease:flags=neighbor,pad=600:280:(ow-iw)/2:(oh-ih)/2[amouth];"
        "[2:v][bfull]overlay=32:136[t1];[t1][afull]overlay=816:136[t2];"
        "[t2][bmouth]overlay=108:650[t3];[t3][amouth]overlay=892:650:shortest=1[out]"
    )
    command = ["C:/ffmpeg/bin/ffmpeg.exe", "-hide_banner", "-loglevel", "error", "-y",
               "-filter_complex_threads", "2",
               "-framerate", "30", "-i", str(args.before / "frame-%05d.ppm"),
               "-framerate", "30", "-i", str(args.after / "frame-%05d.ppm"),
               "-loop", "1", "-framerate", "30", "-i", str(background), "-i", str(args.audio),
               "-filter_complex", graph, "-map", "[out]", "-map", "3:a:0",
               "-c:v", "libx264", "-threads", "2", "-preset", "fast", "-crf", "17",
               "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-t", str(expected / 30),
               "-movflags", "+faststart", str(args.out)]
    subprocess.run(command, check=True, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
    args.out.with_suffix(".json").write_text(json.dumps({
        "before": str(args.before), "after": str(args.after), "audio": str(args.audio),
        "audioSha256": hashlib.sha256(args.audio.read_bytes()).hexdigest(),
        "videoSha256": hashlib.sha256(args.out.read_bytes()).hexdigest(),
        "sourceFramesPerSide": expected, "fps": 30, "audioDurationSeconds": duration,
        "sourceSize": frame_size, "mouthRoi": roi,
        "scope": "offline visual comparison; no real-time playback or game latency claim"
    }, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
