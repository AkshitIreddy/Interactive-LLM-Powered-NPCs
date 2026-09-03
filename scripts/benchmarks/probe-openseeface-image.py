#!/usr/bin/env python3
"""Probe pinned OpenSeeFace detector/landmarks on one explicit image.

This is a narrow generic-image compatibility check. It does not perform actor
identity, animate pixels, or grant model-pack activation authority.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import statistics
import sys
import time
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, max(0, int(round((len(ordered) - 1) * fraction))))]


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--pack", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=30)
    args = parser.parse_args()
    if args.samples < 20:
        parser.error("at least 20 samples are required")

    implementation = Path(__file__).with_name("qualify-openseeface-visual-pack.py")
    spec = importlib.util.spec_from_file_location("npc_openseeface_qualification", implementation)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load OpenSeeFace qualification implementation")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)

    pack = args.pack.resolve(strict=True)
    image_path = args.image.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inventory = []
    for relative, (expected_size, expected_hash) in module.EXPECTED.items():
        artifact = pack / relative
        observed = (artifact.stat().st_size, sha256(artifact))
        if observed != (expected_size, expected_hash):
            raise RuntimeError(f"pinned OpenSeeFace artifact mismatch: {relative}")
        inventory.append({"path": relative, "size_bytes": observed[0], "sha256": observed[1]})

    frame = module.cv2.imread(str(image_path), module.cv2.IMREAD_COLOR)
    if frame is None:
        raise RuntimeError("image could not be decoded")
    sessions = module.load_sessions(pack)
    packets = []
    elapsed = []
    for sequence in range(1, args.samples + 1):
        started = time.perf_counter_ns()
        packet = module.run_one(sessions, frame, sequence)
        elapsed.append((time.perf_counter_ns() - started) / 1e6)
        if packet is not None:
            packets.append(packet)

    detected = len(packets) == args.samples
    best = max(packets, key=lambda item: item["detector_confidence"]) if packets else None
    proof_path = output / "openseeface-mara-venn-landmarks.png"
    if best is not None:
        module.cv2.imwrite(str(proof_path), module.draw_packet(frame, best))
    report = {
        "schema": "npc.openseeface-explicit-image-probe/v1",
        "status": "passed" if detected and best is not None and best["landmark_confidence"] >= 0.80 else "failed",
        "scope": "face-detection-and-landmarks-only-not-identity-not-animation-not-pack-admission",
        "pack": {"id": module.PACK_ID, "revision": module.PACK_REVISION, "artifacts": inventory},
        "image": {"path": str(image_path), "sha256": sha256(image_path), "width": int(frame.shape[1]), "height": int(frame.shape[0])},
        "runtime": {"onnxruntime": module.ort.__version__, "providers": sessions.detector.get_providers(), "threads": 1},
        "samples": args.samples,
        "detections": len(packets),
        "p50_operation_millis": statistics.median(elapsed),
        "p95_operation_millis": percentile(elapsed, 0.95),
        "detector_confidence": best["detector_confidence"] if best else None,
        "landmark_confidence": best["landmark_confidence"] if best else None,
        "mouth_confidence": best["mouth_confidence"] if best else None,
        "face_bounds": best["face_bounds"] if best else None,
        "mouth_bounds_normalized": best["mouth_bounds_normalized"] if best else None,
        "pose_degrees": best["pose_degrees"] if best else None,
        "proof_path": str(proof_path) if proof_path.is_file() else None,
        "proof_sha256": sha256(proof_path) if proof_path.is_file() else None,
        "identity_recognition": False,
        "pixels_generated": False,
    }
    report_path = output / "openseeface-mara-venn-probe.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
