#!/usr/bin/env python3
"""Translate complete native provider packets to a frame-indexed replay.

No landmarks or visibility labels are fabricated. Provider failures remain
missing rows. Replay re-evaluates native tracking/worker admission at its own
sample clock; this tool confers no production actor-selection authority.
"""
import argparse
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--width", type=int, required=True)
    parser.add_argument("--height", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("output must be fresh")
    if not 1 <= args.width <= 4096 or not 1 <= args.height <= 4096:
        parser.error("dimensions exceed replay contract")
    raw = args.dump.read_bytes()
    dump = json.loads(raw)
    frames = dump["frames"]
    if not 1 <= len(frames) <= 1800:
        parser.error("invalid replay frame count")
    lines = [f"npc-landmark-replay-v1 {args.width} {args.height} {len(frames)} 30"]
    for i, frame in enumerate(frames):
        if frame.get("file") != f"frame-{i+1:05d}.ppm":
            parser.error("native dump frames must be contiguous, one-based source PPM names")
        if "packetLandmarks" not in frame:
            if "contour" in frame or "rawContour" in frame:
                parser.error("mouth-only diagnostic dumps cannot reconstruct full provider packets")
            lines.append(f"{i} 0")
            continue
        points = frame["packetLandmarks"]
        if len(points) != 66 or any(len(point) != 3 for point in points):
            parser.error("native replay requires every measured landmark")
        face = frame.get("face", frame.get("rawFace"))
        pose = frame.get("pose", frame.get("rawPose"))
        if not face or len(face) != 4 or not pose or len(pose) != 3:
            parser.error("missing provider face or pose")
        values = [frame["detectorConfidence"], frame["landmarkConfidence"],
                  frame["visibilityRatio"], *pose, *face,
                  int(frame["packetMouthOccluded"]), *[v for point in points for v in point]]
        lines.append(f"{i} 1 " + " ".join(str(value) for value in values))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    receipt = {"scope": "native provider geometry replay; identity remains a manual review fixture",
               "sourceDumpSha256": hashlib.sha256(raw).hexdigest(),
               "replaySha256": hashlib.sha256(args.output.read_bytes()).hexdigest(),
               "frames": len(frames), "manualVisibilityMask": False}
    args.output.with_suffix(".receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
