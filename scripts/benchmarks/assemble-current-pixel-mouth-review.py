#!/usr/bin/env python3
"""Validate lossless proof frames and encode a labeled, headless comparison.

Manifest: {audio: WAV path, cases: [{name: label, proof: proof directory}]}.
Never presents a window or plays audio. The WAV repeats for each equal-duration
case, so visual comparisons use the identical speech rather than voice casting.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import cv2
import numpy as np


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def caption(image, text, xy, size=.8, color=(215, 217, 221)):
    cv2.putText(image, text, xy, cv2.FONT_HERSHEY_SIMPLEX, size, color, 1, cv2.LINE_AA)


def fit(image, width, height):
    scale = min(width/image.shape[1], height/image.shape[0])
    return cv2.resize(image, (round(image.shape[1]*scale), round(image.shape[0]*scale)),
                      interpolation=cv2.INTER_LINEAR)


def main():
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path, required=True)
    args = parser.parse_args()
    config = json.loads(args.manifest.read_text(encoding="utf-8-sig"))
    output = args.output.resolve()
    if output.exists() or not str(output).lower().startswith("e:\\temp\\"):
        raise ValueError("review output must be fresh and below E:\\temp")
    cases = config["cases"]
    if not 1 <= len(cases) <= 8:
        raise ValueError("invalid number of review cases")
    loaded = []
    for case in cases:
        proof = Path(case["proof"])
        report = json.loads((proof/"report.json").read_text())
        geometry = json.loads((proof/"geometry.json").read_text())
        if report["fps"] != 30 or report["frameCount"] != 90 or len(geometry) != 90:
            raise ValueError("comparison requires 90 source-bound frames at 30 fps per case")
        loaded.append((case, proof, report, geometry))
    output.mkdir(parents=True)
    video = output/"three-character-current-pixel-comparison.mp4"
    command = [str(args.ffmpeg), "-nostdin", "-hide_banner", "-loglevel", "error",
        "-stream_loop", "-1", "-i", config["audio"], "-f", "rawvideo", "-pixel_format", "bgr24",
        "-video_size", "1920x1080", "-framerate", "30", "-i", "pipe:0", "-map", "1:v",
        "-map", "0:a", "-t", str(len(cases)*3), "-c:v", "libx264", "-preset", "fast",
        "-crf", "17", "-pix_fmt", "yuv420p", "-threads", "2", "-c:a", "aac", "-b:a", "160k",
        "-movflags", "+faststart", str(video)]
    receipts = []
    with (output/"encode.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                   stderr=log, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        try:
            for case, proof, report, geometry in loaded:
                unchanged, changed, manual, outside = 0, 0, 0, 0
                sequence_hash = hashlib.sha256()
                board = []
                for index, (row, geo) in enumerate(zip(report["frames"], geometry)):
                    source_path = Path(report["sourceFrames"])/geo["file"]
                    source = cv2.imread(str(source_path))
                    rendered = cv2.imread(str(proof/"frames"/f"frame-{index:05d}.png"))
                    if source is None or rendered is None or source.shape != rendered.shape:
                        raise ValueError("missing or mismatched proof frame")
                    sequence_hash.update(source_path.name.encode()+bytes.fromhex(sha(source_path)))
                    difference = np.any(source != rendered, axis=2)
                    if int(difference.sum()) != row["changedPixels"]:
                        raise ValueError("changed-pixel receipt does not match lossless frame")
                    if row["reason"] != "current-pixel-warp" and difference.any():
                        raise ValueError("bypass altered source pixels")
                    if "pixelBounds" in row:
                        x0, y0, x1, y1 = row["pixelBounds"]
                        exterior = difference.copy()
                        exterior[y0:y1, x0:x1] = False
                        outside += int(exterior.sum())
                        if exterior.any():
                            raise ValueError("deformation escaped declared support")
                    if difference.any():
                        changed += 1
                    else:
                        unchanged += 1
                    manual += row["reason"].startswith("manual-visibility-bypass")
                    panel = np.full((1080, 1920, 3), (17, 16, 15), np.uint8)
                    caption(panel, case["name"]+"  /  same 3-second speech", (38, 42), .9)
                    caption(panel, "Original gameplay", (38, 90), .75)
                    method = "Current-frame lips + private oral reference" if report["oralReferenceSha256"] else "Current-frame lips only / reference rejected"
                    caption(panel, method, (988, 90), .75)
                    x0, y0, x1, y1 = report["faceBox"]
                    crops = []
                    for frame, start in ((source, 32), (rendered, 992)):
                        crop = fit(frame[y0:y1, x0:x1], 896, 868)
                        y = 112+(868-crop.shape[0])//2
                        x = start+(896-crop.shape[1])//2
                        panel[y:y+crop.shape[0], x:x+crop.shape[1]] = crop
                        crops.append(fit(frame[y0:y1, x0:x1], 300, 330))
                    status = "Manual source preservation: visibility annotation" if row["reason"].startswith("manual-") else (
                        "Source preserved: "+row["reason"] if row["reason"] != "current-pixel-warp" else "Offline deformation experiment")
                    caption(panel, status, (38, 1020), .7)
                    caption(panel, "Unqualified offline prototype | enlarged source crop | full cue schedule known | stock voice reused", (38, 1058), .63, (151, 155, 161))
                    process.stdin.write(panel.tobytes())
                    if index == 16:
                        cv2.imwrite(str(output/(case["name"].lower()+"-comparison.png")), panel)
                    if index in (8, 12, 16, 36, 54, 68):
                        pair = np.concatenate(crops, 1)
                        caption(pair, f"frame {index}", (6, 23), .55)
                        board.append(pair)
                if board:
                    cv2.imwrite(str(output/(case["name"].lower()+"-temporal-board.png")), np.concatenate(board, 0))
                times = [r["renderMs"]+r["landmarkMs"] for r in report["frames"]]
                receipts.append({"character": case["name"], "reportSha256": sha(proof/"report.json"),
                    "rendererSha256": report["rendererSha256"], "sourceSequenceSha256": sequence_hash.hexdigest(),
                    "frames": 90, "changed": changed, "sourceIdentical": unchanged,
                    "manualVisibilityBypass": manual, "outsideSupportChangedPixels": outside,
                    "oralReferenceSha256":report["oralReferenceSha256"],
                    "cueTrajectory":report.get("cueTrajectory","category-ema"),
                    "sourceEdgeRefinement":report.get("sourceEdgeRefinement",False),
                    "geometryFailures":sum(r["reason"]=="missing-face-geometry" for r in report["frames"]),
                    "cpuGeometryAndRenderP50Ms": float(np.percentile(times, 50)),
                    "cpuGeometryAndRenderP95Ms": float(np.percentile(times, 95)),
                    "renderP95Ms": report["renderP95Ms"]})
            process.stdin.close()
            if process.wait(timeout=60) != 0:
                raise RuntimeError("ffmpeg encoding failed; see encode.log")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
    probe = subprocess.run([str(args.ffprobe), "-v", "error", "-count_frames", "-show_streams",
        "-show_format", "-of", "json", str(video)], capture_output=True, check=True,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    media = json.loads(probe.stdout)
    streams = [stream for stream in media["streams"] if stream["codec_type"] == "video"]
    if len(streams) != 1 or int(streams[0]["nb_read_frames"]) != 90*len(cases):
        raise ValueError("encoded video frame count failed")
    if not any(stream["codec_type"] == "audio" for stream in media["streams"]):
        raise ValueError("encoded video missing shared audio")
    receipt = {"schema": "interactive-npcs-current-pixel-review/v1", "cases": receipts,
        "videoSha256": sha(video), "audioSha256": sha(config["audio"]), "mediaProbe": media,
        "scope": "headless offline comparator; not live game, automatic occlusion or production qualification",
        "timingExcludes": ["image I/O", "cue extraction", "capture", "speaker", "display", "game load"],
        "cueScheduleScope": "full utterance known offline; no streaming lookahead claim",
        "assemblerSha256": sha(__file__)}
    (output/"verification.json").write_text(json.dumps(receipt, indent=2)+"\n", encoding="utf-8")
    print(json.dumps({"video": str(video), "cases": receipts}))


if __name__ == "__main__":
    main()
