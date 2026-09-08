#!/usr/bin/env python3
"""Assemble the schema-4 native current-pixel review without opening a window.

The input is the immutable native replay corpus produced on 2026-09-07.  This
tool verifies the replay receipts against their lossless source/output frames,
writes per-character temporal boards, and pipes a clearly labelled comparison
to ffmpeg.  It does not infer landmarks, render a mouth, capture a game, or
claim that the offline component replay is a live integration test.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import wave

from PIL import Image, ImageChops, ImageDraw, ImageEnhance, ImageFont


FPS = 15
CANVAS = (1920, 1080)
EXPECTED_SCHEMA = "interactive-npcs-current-pixel-replay/v1"
DEFAULT_INPUT = Path(r"E:\temp\InteractiveNPCs\integration-20260907")
DEFAULT_MARA_SOURCE = Path(r"E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1")
DEFAULT_SARAH_AUDIO = Path(
    r"E:\temp\InteractiveNPCs\user-cyberpunk-lipsync-20260905"
    r"\sarah-first-sentence-3s.wav"
)


CASES = (
    {
        "slug": "misty",
        "name": "Misty Olszewski",
        "kind": "Cyberpunk 2077 recorded fixture",
        "replay": "misty-native-v2",
        "source": "misty-native-source",
        "landmarks": "misty-native-landmarks-v2.json",
        "audio": "sarah",
        "frames": 45,
    },
    {
        "slug": "claire",
        "name": "Claire Russell",
        "kind": "Cyberpunk 2077 recorded fixture",
        "replay": "claire-native-v2",
        "source": "claire-native-source",
        "landmarks": "claire-native-landmarks-v2.json",
        "audio": "sarah",
        "frames": 45,
    },
    {
        "slug": "johnny",
        "name": "Johnny Silverhand",
        "kind": "Cyberpunk 2077 stress fixture",
        "replay": "johnny-native-v2",
        "source": "johnny-native-source",
        "landmarks": "johnny-native-landmarks-v2.json",
        "audio": "sarah",
        "frames": 45,
    },
    {
        "slug": "mara",
        "name": "Mara",
        "kind": "Synthetic component fixture",
        "replay": "mara-native-v1",
        "source": None,
        "landmarks": "mara-native-landmarks-v1.json",
        "audio": "mara",
        "frames": 21,
    },
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def sequence_sha256(paths: list[Path]) -> str:
    digest = hashlib.sha256()
    for path in paths:
        digest.update(path.name.encode("utf-8"))
        digest.update(bytes.fromhex(sha256(path)))
    return digest.hexdigest()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def read_jsonl(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]


def sorted_frames(path: Path) -> list[Path]:
    require(path.is_dir(), f"missing frame directory: {path}")
    frames = sorted(item for item in path.iterdir() if item.is_file() and item.suffix.lower() == ".ppm")
    require(frames, f"no lossless PPM frames: {path}")
    return frames


def wav_receipt(path: Path, expected_seconds: float) -> dict:
    require(path.is_file(), f"missing WAV: {path}")
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        sample_width = stream.getsampwidth()
        sample_rate = stream.getframerate()
        samples = stream.getnframes()
    seconds = samples / sample_rate
    require(channels == 1 and sample_width == 2, f"WAV must be mono PCM16: {path}")
    require(math.isclose(seconds, expected_seconds, abs_tol=0.5 / sample_rate),
            f"unexpected WAV duration for {path}: {seconds}")
    return {
        "path": str(path.resolve()),
        "sha256": sha256(path),
        "bytes": path.stat().st_size,
        "sampleRate": sample_rate,
        "channels": channels,
        "sampleWidthBytes": sample_width,
        "samples": samples,
        "durationSeconds": seconds,
    }


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = (
        Path(r"C:\Windows\Fonts\segoeuib.ttf") if bold else Path(r"C:\Windows\Fonts\segoeui.ttf"),
        Path(r"C:\Windows\Fonts\arialbd.ttf") if bold else Path(r"C:\Windows\Fonts\arial.ttf"),
    )
    for candidate in candidates:
        if candidate.is_file():
            return ImageFont.truetype(str(candidate), size=size)
    return ImageFont.load_default()


FONT_18 = font(18)
FONT_20 = font(20)
FONT_22_BOLD = font(22, True)
FONT_26_BOLD = font(26, True)
FONT_34_BOLD = font(34, True)


def normalized_rect_to_pixels(rect: list[float], size: tuple[int, int]) -> tuple[float, float, float, float]:
    x, y, width, height = (float(value) for value in rect)
    return x * size[0], y * size[1], (x + width) * size[0], (y + height) * size[1]


def contour_rect(frame: dict, size: tuple[int, int]) -> tuple[float, float, float, float]:
    points = frame.get("contour") or frame.get("rawContour")
    if not points:
        points = frame.get("packetLandmarks", [])[48:66]
    require(len(points) == 18, "full-66 landmark receipt lacks the mouth contour")
    xs = [float(point[0]) * size[0] for point in points]
    ys = [float(point[1]) * size[1] for point in points]
    return min(xs), min(ys), max(xs), max(ys)


def face_rect(frame: dict, size: tuple[int, int]) -> tuple[float, float, float, float]:
    rect = frame.get("face") or frame.get("rawFace")
    require(rect is not None and len(rect) == 4, "landmark receipt lacks a face rectangle")
    return normalized_rect_to_pixels(rect, size)


def expanded_crop(rect: tuple[float, float, float, float], size: tuple[int, int],
                  aspect: float, scale: float) -> tuple[int, int, int, int]:
    x0, y0, x1, y1 = rect
    width = max(2.0, (x1 - x0) * scale)
    height = max(2.0, (y1 - y0) * scale)
    if width / height < aspect:
        width = height * aspect
    else:
        height = width / aspect
    center_x = (x0 + x1) * 0.5
    center_y = (y0 + y1) * 0.5
    left = center_x - width * 0.5
    top = center_y - height * 0.5
    right = center_x + width * 0.5
    bottom = center_y + height * 0.5
    if left < 0:
        right -= left
        left = 0
    if top < 0:
        bottom -= top
        top = 0
    if right > size[0]:
        left -= right - size[0]
        right = size[0]
    if bottom > size[1]:
        top -= bottom - size[1]
        bottom = size[1]
    left = max(0, left)
    top = max(0, top)
    return round(left), round(top), round(right), round(bottom)


def cover(image: Image.Image, box: tuple[int, int, int, int]) -> Image.Image:
    target_width = box[2] - box[0]
    target_height = box[3] - box[1]
    scale = max(target_width / image.width, target_height / image.height)
    resized = image.resize((round(image.width * scale), round(image.height * scale)), Image.Resampling.LANCZOS)
    left = (resized.width - target_width) // 2
    top = (resized.height - target_height) // 2
    return resized.crop((left, top, left + target_width, top + target_height))


def paste_crop(canvas: Image.Image, image: Image.Image, crop: tuple[int, int, int, int],
               target: tuple[int, int, int, int]) -> None:
    canvas.paste(cover(image.crop(crop), target), target[:2])


def pill(draw: ImageDraw.ImageDraw, xy: tuple[int, int, int, int], text: str,
         fill: tuple[int, int, int], ink: tuple[int, int, int] = (243, 246, 250)) -> None:
    draw.rounded_rectangle(xy, radius=15, fill=fill)
    bounds = draw.textbbox((0, 0), text, font=FONT_18)
    x = xy[0] + (xy[2] - xy[0] - (bounds[2] - bounds[0])) // 2
    y = xy[1] + (xy[3] - xy[1] - (bounds[3] - bounds[1])) // 2 - 1
    draw.text((x, y), text, font=FONT_18, fill=ink)


def render_panel(case: dict, source: Image.Image, native: Image.Image,
                 landmark: dict, event: dict, frame_index: int) -> Image.Image:
    canvas = Image.new("RGB", CANVAS, (13, 16, 21))
    draw = ImageDraw.Draw(canvas)
    accent = (240, 180, 55) if case["slug"] == "johnny" else (56, 205, 163)
    draw.rectangle((0, 0, CANVAS[0], 86), fill=(22, 27, 35))
    draw.rectangle((0, 84, CANVAS[0], 87), fill=accent)
    draw.text((34, 20), case["name"], font=FONT_34_BOLD, fill=(247, 249, 252))
    draw.text((34, 61), case["kind"] + "  •  offline native component replay",
              font=FONT_18, fill=(167, 177, 190))
    if case["slug"] == "johnny":
        pill(draw, (1380, 20, 1882, 65), "NATIVE BYPASS • EXACT SOURCE", (126, 75, 22))
    else:
        pill(draw, (1512, 20, 1882, 65), "NATIVE SCHEMA 4 • 15 Hz", (22, 101, 82))

    draw.text((34, 104), "SOURCE • exact gameplay frame", font=FONT_22_BOLD, fill=(208, 216, 226))
    draw.text((980, 104), "NATIVE • current-pixel result", font=FONT_22_BOLD, fill=(208, 216, 226))
    left_face = (34, 140, 940, 742)
    right_face = (980, 140, 1886, 742)
    draw.rounded_rectangle((30, 136, 944, 746), radius=12, fill=(39, 45, 55))
    draw.rounded_rectangle((976, 136, 1890, 746), radius=12, fill=accent)
    face = expanded_crop(face_rect(landmark, source.size), source.size,
                         (left_face[2] - left_face[0]) / (left_face[3] - left_face[1]), 2.0)
    paste_crop(canvas, source, face, left_face)
    paste_crop(canvas, native, face, right_face)

    draw.text((34, 770), "MOUTH DETAIL • same frame / same crop", font=FONT_20, fill=(167, 177, 190))
    left_mouth = (34, 804, 940, 982)
    right_mouth = (980, 804, 1886, 982)
    mouth = expanded_crop(contour_rect(landmark, source.size), source.size,
                          (left_mouth[2] - left_mouth[0]) / (left_mouth[3] - left_mouth[1]), 4.6)
    paste_crop(canvas, source, mouth, left_mouth)
    paste_crop(canvas, native, mouth, right_mouth)

    source_exact = bool(event["sourceExact"])
    reason = str(event["reason"])
    status = (
        "Source preserved — landmark confidence below the 0.82 admission floor"
        if case["slug"] == "johnny"
        else "Source-preserving neutral/contact frame" if source_exact
        else "Native current-pixel residual rendered from this exact source frame"
    )
    draw.text((34, 1006), status, font=FONT_22_BOLD,
              fill=(244, 190, 73) if case["slug"] == "johnny" else (93, 226, 186))
    draw.text((34, 1040),
              f"frame {frame_index + 1}/{case['frames']}  •  reason: {reason}  •  "
              "15 Hz replay  •  audio reused  •  timing excludes inference/capture/game load",
              font=FONT_18, fill=(146, 156, 169))
    return canvas


def make_temporal_board(panels: list[Image.Image], indices: list[int], output: Path) -> None:
    board = Image.new("RGB", CANVAS, (9, 11, 15))
    slots = ((0, 0), (960, 0), (0, 540), (960, 540))
    for index, (x, y) in zip(indices, slots):
        board.paste(panels[index].resize((960, 540), Image.Resampling.LANCZOS), (x, y))
    board.save(output, format="PNG", optimize=True)


def make_mouth_contact_board(case: dict, output: Path) -> None:
    """Write ten paired oral crops suitable for anatomy and contact review."""
    width, height = 3200, 620
    board = Image.new("RGB", (width, height), (10, 13, 18))
    draw = ImageDraw.Draw(board)
    accent = (240, 180, 55) if case["slug"] == "johnny" else (56, 205, 163)
    draw.rectangle((0, 0, width, 88), fill=(22, 27, 35))
    draw.rectangle((0, 86, width, 89), fill=accent)
    draw.text((28, 16), case["name"] + " • oral anatomy over time", font=FONT_26_BOLD,
              fill=(247, 249, 252))
    exposure = 2 if case["slug"] == "claire" else 0
    note = (
        "INSPECTION EXPOSURE +2 STOPS • applied equally to source and native • video remains original"
        if exposure else
        "NATIVE BYPASS • exact source preserved • landmark confidence below 0.82"
        if case["slug"] == "johnny" else
        "Exact source/native pairs • crop centered on OpenSeeFace 58–65 • original exposure"
    )
    draw.text((28, 53), note, font=FONT_18,
              fill=(244, 190, 73) if exposure else (167, 177, 190))
    indices = [round(index * (case["frames"] - 1) / 9) for index in range(10)]
    for position, frame_index in enumerate(indices):
        row, column = divmod(position, 5)
        origin_x = 20 + column * 636
        origin_y = 106 + row * 252
        source, native, landmark, event = case["loadedFrames"][frame_index]
        points = landmark.get("packetLandmarks", [])[58:66]
        require(len(points) == 8, f"{case['name']} lacks OpenSeeFace 58–65")
        xs = [float(point[0]) * source.width for point in points]
        ys = [float(point[1]) * source.height for point in points]
        center_x = (min(xs) + max(xs)) * 0.5
        center_y = (min(ys) + max(ys)) * 0.5
        mouth_width = max(4.0, max(xs) - min(xs))
        crop_width = mouth_width * 3.0
        crop_height = mouth_width * 1.8
        crop = expanded_crop(
            (center_x - crop_width * 0.5, center_y - crop_height * 0.5,
             center_x + crop_width * 0.5, center_y + crop_height * 0.5),
            source.size, 300 / 180, 1.0,
        )
        pair = []
        for image in (source, native):
            oral = cover(image.crop(crop), (0, 0, 300, 180))
            if exposure:
                oral = ImageEnhance.Brightness(oral).enhance(4.0)
            pair.append(oral)
        board.paste(pair[0], (origin_x, origin_y + 42))
        board.paste(pair[1], (origin_x + 308, origin_y + 42))
        draw.text((origin_x, origin_y), f"t={frame_index / FPS:.2f}s  •  {event['reason']}",
                  font=FONT_18, fill=(211, 218, 228))
        draw.text((origin_x, origin_y + 21), "SOURCE", font=FONT_18, fill=(150, 160, 173))
        draw.text((origin_x + 308, origin_y + 21), "NATIVE", font=FONT_18, fill=accent)
        draw.rectangle((origin_x - 1, origin_y + 41, origin_x + 301, origin_y + 223),
                       outline=(64, 72, 83), width=2)
        draw.rectangle((origin_x + 307, origin_y + 41, origin_x + 609, origin_y + 223),
                       outline=accent, width=2)
    board.save(output, format="PNG", optimize=True)


def load_case(case_spec: dict, input_root: Path, mara_source: Path) -> dict:
    case = dict(case_spec)
    replay = input_root / case["replay"]
    report_path = replay / "current-pixel-replay-report.json"
    events_path = replay / "frames.jsonl"
    landmarks_path = input_root / case["landmarks"]
    source_dir = mara_source if case["source"] is None else input_root / case["source"]
    report = read_json(report_path)
    events = read_jsonl(events_path)
    landmarks_document = read_json(landmarks_path)
    outputs = sorted_frames(replay / "frames")
    sources = sorted_frames(source_dir)
    require(report.get("schema") == EXPECTED_SCHEMA, f"wrong replay schema: {report_path}")
    require(report.get("outputFps") == FPS and report.get("admittedSignalRateHz") == FPS,
            f"{case['name']} is not an honest native 15 Hz replay")
    require(report.get("sourceFrameStep") == 2, f"{case['name']} must sample current frames 0,2,4...")
    require(report.get("outputFrames") == case["frames"] == len(outputs) == len(events),
            f"{case['name']} output/event count mismatch")
    require(len(sources) == report.get("sourceInputFrames"), f"{case['name']} source count mismatch")
    landmarks = landmarks_document.get("frames", [])
    require(len(landmarks) == len(sources), f"{case['name']} landmark/source count mismatch")

    loaded_frames = []
    changed = 0
    exact = 0
    for index, (native_path, event) in enumerate(zip(outputs, events)):
        require(event.get("outputFrame") == index, f"{case['name']} event index mismatch")
        source_index = int(event["sourceFrame"])
        require(source_index == index * 2 and 0 <= source_index < len(sources),
                f"{case['name']} current-frame mapping mismatch")
        source = Image.open(sources[source_index]).convert("RGB")
        native = Image.open(native_path).convert("RGB")
        require(source.size == native.size, f"{case['name']} frame dimensions changed")
        difference = ImageChops.difference(source, native)
        difference_box = difference.getbbox()
        is_exact = difference_box is None
        require(is_exact == bool(event["sourceExact"]), f"{case['name']} sourceExact mismatch at {index}")
        require(bool(event["residual"]) or is_exact, f"{case['name']} bypass changed pixels at {index}")
        if difference_box is not None:
            changed += 1
            bounds = event.get("bounds", [0, 0, 0, 0])
            bx0, by0, bx1, by1 = normalized_rect_to_pixels(bounds, source.size)
            tolerance = 2
            require(difference_box[0] >= math.floor(bx0) - tolerance and
                    difference_box[1] >= math.floor(by0) - tolerance and
                    difference_box[2] <= math.ceil(bx1) + tolerance and
                    difference_box[3] <= math.ceil(by1) + tolerance,
                    f"{case['name']} changed pixels escaped reported support at {index}")
        else:
            exact += 1
        loaded_frames.append((source, native, landmarks[source_index], event))
    require(changed == report.get("changedFrames"), f"{case['name']} changed-frame summary mismatch")
    require(exact == case["frames"] - report.get("changedFrames"),
            f"{case['name']} exact-frame summary mismatch")
    if case["slug"] == "johnny":
        require(report.get("status") == "failed" and report.get("bypassFrames") == case["frames"] and exact == case["frames"],
                "Johnny must remain explicitly labelled as native fail-open bypass")
        require(all(event["reason"] == "bypass_invalid_packet" for event in events),
                "Johnny bypass reason changed")
    else:
        require(report.get("status") == "passed", f"{case['name']} replay did not pass")
    case.update({
        "replayPath": replay,
        "reportPath": report_path,
        "eventsPath": events_path,
        "landmarksPath": landmarks_path,
        "sourcePath": source_dir,
        "report": report,
        "events": events,
        "sources": sources,
        "outputs": outputs,
        "loadedFrames": loaded_frames,
        "changed": changed,
        "exact": exact,
    })
    return case


def encode_video(cases: list[dict], output: Path, ffmpeg: Path,
                 sarah_audio: Path, mara_audio: Path) -> Path:
    video = output / "native-current-pixel-four-character-review.mp4"
    duration = sum(case["frames"] for case in cases) / FPS
    command = [
        str(ffmpeg), "-nostdin", "-hide_banner", "-loglevel", "error",
        "-i", str(sarah_audio), "-i", str(sarah_audio), "-i", str(sarah_audio),
        "-i", str(mara_audio),
        "-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", "1920x1080",
        "-framerate", str(FPS), "-i", "pipe:0",
        "-filter_complex", "[0:a:0][1:a:0][2:a:0][3:a:0]concat=n=4:v=0:a=1[a]",
        "-map", "4:v:0", "-map", "[a]", "-t", f"{duration:.6f}",
        "-c:v", "libx264", "-preset", "fast", "-crf", "17", "-pix_fmt", "yuv420p",
        "-threads", "2", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart",
        str(video),
    ]
    with (output / "encode.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=log,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        try:
            require(process.stdin is not None, "ffmpeg stdin was not created")
            for case in cases:
                for index, (source, native, landmark, event) in enumerate(case["loadedFrames"]):
                    process.stdin.write(render_panel(case, source, native, landmark, event, index).tobytes())
            process.stdin.close()
            if process.wait(timeout=120) != 0:
                raise RuntimeError("ffmpeg encode failed; inspect encode.log")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
    return video


def probe_media(ffprobe: Path, video: Path, expected_frames: int, expected_duration: float) -> dict:
    completed = subprocess.run(
        [str(ffprobe), "-v", "error", "-count_frames", "-show_streams", "-show_format",
         "-of", "json", str(video)],
        capture_output=True, check=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    media = json.loads(completed.stdout)
    videos = [stream for stream in media["streams"] if stream["codec_type"] == "video"]
    audios = [stream for stream in media["streams"] if stream["codec_type"] == "audio"]
    require(len(videos) == 1 and len(audios) == 1, "encoded review must contain one video and one audio stream")
    require(int(videos[0]["nb_read_frames"]) == expected_frames, "encoded frame count mismatch")
    require(videos[0]["avg_frame_rate"] == f"{FPS}/1", "encoded frame rate mismatch")
    actual_duration = float(media["format"]["duration"])
    require(abs(actual_duration - expected_duration) <= 1.0 / FPS,
            f"encoded duration mismatch: {actual_duration}")
    return media


def main() -> None:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--input-root", type=Path, default=DEFAULT_INPUT)
    parser.add_argument("--mara-source", type=Path, default=DEFAULT_MARA_SOURCE)
    parser.add_argument("--sarah-audio", type=Path, default=DEFAULT_SARAH_AUDIO)
    parser.add_argument("--mara-audio", type=Path, default=DEFAULT_INPUT / "sarah-mara-42frames.wav")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--replay-dir", nargs=2, action="append", default=[],
                        metavar=("CHARACTER", "DIRECTORY"),
                        help="override a case replay directory without modifying historical defaults")
    parser.add_argument("--ffmpeg", type=Path, default=Path(r"C:\ffmpeg\bin\ffmpeg.exe"))
    parser.add_argument("--ffprobe", type=Path, default=Path(r"C:\ffmpeg\bin\ffprobe.exe"))
    parser.add_argument("--supplement-existing", action="store_true",
                        help="add/update oral boards and receipt in an already assembled output")
    args = parser.parse_args()

    input_root = args.input_root.resolve()
    output = args.output.resolve()
    if args.supplement_existing:
        require(output.is_dir(), "supplement output must already exist")
    else:
        require(not output.exists(), "output must be fresh")
    require(str(output).lower().startswith("e:\\temp\\"), "output must stay below E:\\temp")
    require(args.ffmpeg.is_file() and args.ffprobe.is_file(), "ffmpeg/ffprobe executable missing")
    sarah = wav_receipt(args.sarah_audio.resolve(), 3.0)
    mara = wav_receipt(args.mara_audio.resolve(), 1.4)
    overrides = dict(args.replay_dir)
    require(len(overrides) == len(args.replay_dir), "duplicate replay override")
    require(set(overrides) <= {spec["slug"] for spec in CASES}, "unknown replay character")
    cases = [load_case({**spec, "replay": overrides.get(spec["slug"], spec["replay"])},
                      input_root, args.mara_source.resolve()) for spec in CASES]
    for case in cases:
        expected_audio = sarah if case["audio"] == "sarah" else mara
        require(case["report"]["audioSha256"] == expected_audio["sha256"],
                f"{case['name']} report/audio binding mismatch")

    output.mkdir(parents=True, exist_ok=args.supplement_existing)
    snapshot_name = ("supplement-assembler-source.py" if args.supplement_existing
                     else "assembler-source.py")
    (output / snapshot_name).write_bytes(Path(__file__).read_bytes())
    for case in cases:
        mouth_board = output / f"{case['slug']}-mouth-contact-board.png"
        make_mouth_contact_board(case, mouth_board)
        print(json.dumps({"mouthContactBoard": str(mouth_board)}), flush=True)
    if args.supplement_existing:
        receipt_path = output / "verification.json"
        receipt = read_json(receipt_path)
        receipt_cases = {item["character"]: item for item in receipt["cases"]}
        for case in cases:
            item = receipt_cases[case["name"]]
            item["mouthContactBoardSha256"] = sha256(
                output / f"{case['slug']}-mouth-contact-board.png"
            )
            item["inspectionExposureStops"] = 2 if case["slug"] == "claire" else 0
        # Supplemental boards do not regenerate the encoded video. Preserve
        # its original assembler identity and bind this separate operation.
        receipt["supplementAssemblerSha256"] = sha256(Path(__file__))
        receipt["supplementAssemblerSourcePath"] = snapshot_name
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"supplemented": str(output), "verification": str(receipt_path)}))
        return

    for case in cases:
        panels = [render_panel(case, *loaded, index) for index, loaded in enumerate(case["loadedFrames"])]
        indices = sorted({round((case["frames"] - 1) * fraction) for fraction in (0.12, 0.38, 0.64, 0.88)})
        require(len(indices) == 4, f"{case['name']} temporal samples collapsed")
        make_temporal_board(panels, indices, output / f"{case['slug']}-temporal-board.png")
        representative = indices[1]
        panels[representative].save(output / f"{case['slug']}-comparison.png", format="PNG", optimize=True)
        print(json.dumps({"board": str(output / f"{case['slug']}-temporal-board.png"),
                          "comparison": str(output / f"{case['slug']}-comparison.png")}), flush=True)

    video = encode_video(cases, output, args.ffmpeg.resolve(), args.sarah_audio.resolve(), args.mara_audio.resolve())
    expected_frames = sum(case["frames"] for case in cases)
    expected_duration = expected_frames / FPS
    media = probe_media(args.ffprobe.resolve(), video, expected_frames, expected_duration)
    cursor = 0.0
    case_receipts = []
    for case in cases:
        duration = case["frames"] / FPS
        report = case["report"]
        case_receipts.append({
            "character": case["name"],
            "fixtureKind": case["kind"],
            "result": "native-fail-open-source-exact" if case["slug"] == "johnny" else "native-residual-component-replay",
            "startSeconds": cursor,
            "endSeconds": cursor + duration,
            "durationSeconds": duration,
            "fps": FPS,
            "frames": case["frames"],
            "changedFrames": case["changed"],
            "sourceExactFrames": case["exact"],
            "residualFrames": report["residualFrames"],
            "bypassFrames": report["bypassFrames"],
            "sourceSequenceSha256": sequence_sha256(case["sources"]),
            "nativeSequenceSha256": sequence_sha256(case["outputs"]),
            "reportSha256": sha256(case["reportPath"]),
            "eventsSha256": sha256(case["eventsPath"]),
            "landmarksSha256": sha256(case["landmarksPath"]),
            "reportLandmarkReplaySha256": report["landmarkReplaySha256"],
            "workerAndCompositeP50Ms": report["workerAndCompositeP50Ms"],
            "workerAndCompositeP95Ms": report["workerAndCompositeP95Ms"],
            "wholeFrameComponentP95Ms": report["wholeFrameComponentP95Ms"],
            "timingExcludes": report["timingExcludes"],
            "comparisonSha256": sha256(output / f"{case['slug']}-comparison.png"),
            "temporalBoardSha256": sha256(output / f"{case['slug']}-temporal-board.png"),
            "mouthContactBoardSha256": sha256(output / f"{case['slug']}-mouth-contact-board.png"),
            "inspectionExposureStops": 2 if case["slug"] == "claire" else 0,
        })
        cursor += duration
    receipt = {
        "schema": "interactive-npcs-native-current-pixel-review/v2",
        "status": "assembled",
        "scope": "headless offline native component replay comparator; not live capture, installed app, provider inference, audible playback, or game-load proof",
        "timeline": "native 15 Hz output using exact source frames 0,2,4...; no stale geometry carry and no alternating 30 Hz source/render artifact",
        "cueDelivery": "incremental single timed-viseme snapshots in the replay receipts; no full future cue schedule supplied to the worker",
        "video": {
            "path": str(video.resolve()),
            "sha256": sha256(video),
            "bytes": video.stat().st_size,
            "fps": FPS,
            "frames": expected_frames,
            "durationSeconds": expected_duration,
        },
        "audio": {"sarahThreeSeconds": sarah, "maraOnePointFourSeconds": mara},
        "cases": case_receipts,
        "mediaProbe": media,
        "assemblerSha256": sha256(Path(__file__)),
        "assemblerSourcePath": snapshot_name,
    }
    receipt_path = output / "verification.json"
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(output), "video": str(video), "verification": str(receipt_path)}))


if __name__ == "__main__":
    main()
