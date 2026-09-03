#!/usr/bin/env python3
"""Private-evaluation probe for a game-scoped synthetic reference gallery.

The probe retains no embedding vectors. It records only detector confidence,
cosine comparisons, timings, hashes, and rendered face boxes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import statistics
import time
from pathlib import Path

import cv2 as cv
import numpy as np


EXPECTED = {
    "models/face_detection_yunet_2026may.onnx": (229_738, "ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0"),
    "models/face_recognition_sface_2021dec.onnx": (38_696_353, "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def one_face(detector: object, recognizer: object, path: Path) -> tuple[np.ndarray, dict[str, object], np.ndarray]:
    frame = cv.imread(str(path), cv.IMREAD_COLOR)
    if frame is None:
        raise RuntimeError(f"could not decode gallery image: {path.name}")
    detector.setInputSize((frame.shape[1], frame.shape[0]))
    started = time.perf_counter_ns()
    _, faces = detector.detect(frame)
    detect_ms = (time.perf_counter_ns() - started) / 1e6
    if faces is None or len(faces) != 1:
        raise RuntimeError(f"gallery image must contain exactly one face: {path.name}")
    face = faces[0]
    started = time.perf_counter_ns()
    aligned = recognizer.alignCrop(frame, face)
    feature = recognizer.feature(aligned).reshape(-1).astype(np.float32)
    feature /= max(float(np.linalg.norm(feature)), 1e-12)
    embedding_ms = (time.perf_counter_ns() - started) / 1e6
    if feature.shape != (128,) or not np.isfinite(feature).all():
        raise RuntimeError("SFace embedding contract changed")
    x, y, width, height = [float(value) for value in face[:4]]
    rendered = frame.copy()
    cv.rectangle(rendered, (round(x), round(y)), (round(x + width), round(y + height)), (0, 220, 255), 5)
    cv.putText(rendered, f"YuNet {float(face[14]):.3f}", (max(8, round(x)), max(32, round(y) - 12)), cv.FONT_HERSHEY_SIMPLEX, 1.0, (0, 220, 255), 2, cv.LINE_AA)
    return feature, {
        "sha256": sha256(path),
        "detector_confidence": float(face[14]),
        "face_bounds": [x, y, width, height],
        "detect_millis": detect_ms,
        "embedding_millis": embedding_ms,
    }, rendered


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--pack", type=Path, required=True)
    parser.add_argument("--front", type=Path, required=True)
    parser.add_argument("--left", type=Path, required=True)
    parser.add_argument("--right", type=Path, required=True)
    parser.add_argument("--negative", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if cv.__version__ != "5.0.0" or np.__version__ != "2.5.2":
        raise RuntimeError("identity probe requires the exact reviewed OpenCV/NumPy ABI")
    pack = args.pack.resolve(strict=True)
    for relative, (size, digest) in EXPECTED.items():
        path = pack / relative
        if path.stat().st_size != size or sha256(path) != digest:
            raise RuntimeError(f"identity artifact mismatch: {relative}")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    cv.setNumThreads(1)
    cv.ocl.setUseOpenCL(False)
    detector = cv.FaceDetectorYN.create(str(pack / "models/face_detection_yunet_2026may.onnx"), "", (320, 320), 0.82, 0.30, 512, cv.dnn.DNN_BACKEND_OPENCV, cv.dnn.DNN_TARGET_CPU)
    recognizer = cv.FaceRecognizerSF.create(str(pack / "models/face_recognition_sface_2021dec.onnx"), "", cv.dnn.DNN_BACKEND_OPENCV, cv.dnn.DNN_TARGET_CPU)

    sources = {"front": args.front, "three_quarter_left": args.left, "three_quarter_right": args.right, "negative": args.negative}
    features: dict[str, np.ndarray] = {}
    observations: dict[str, dict[str, object]] = {}
    for name, source in sources.items():
        feature, observation, rendered = one_face(detector, recognizer, source.resolve(strict=True))
        features[name] = feature
        observations[name] = observation
        cv.imwrite(str(output / f"{name}-detection.png"), rendered)

    def cosine(left: str, right: str) -> float:
        return float(np.dot(features[left], features[right]))

    positive_pairs = {
        "front_left": cosine("front", "three_quarter_left"),
        "front_right": cosine("front", "three_quarter_right"),
        "left_right": cosine("three_quarter_left", "three_quarter_right"),
    }
    negative_pairs = {
        "front_negative": cosine("front", "negative"),
        "left_negative": cosine("three_quarter_left", "negative"),
        "right_negative": cosine("three_quarter_right", "negative"),
    }
    minimum_positive = min(positive_pairs.values())
    maximum_negative = max(negative_pairs.values())
    margin = minimum_positive - maximum_negative
    timings = [float(item["detect_millis"]) + float(item["embedding_millis"]) for item in observations.values()]
    passed = margin >= 0.10 and minimum_positive >= 0.363
    report = {
        "schema": "npc.identity-reference-gallery-probe/v1",
        "status": "passed_private_evaluation_not_admission" if passed else "failed",
        "scope": "original-synthetic-game-scoped-reference-gallery-no-activation-authority",
        "runtime": {"opencv": cv.__version__, "numpy": np.__version__, "backend": "opencv-dnn-cpu", "threads": 1},
        "model": {"pack_id": "opencv-yunet-sface-private-eval", "revision": "zoo-47534e27-opencv-5.0.0.93", "embedding_dimensions": 128},
        "observations": observations,
        "same_identity_cosine": positive_pairs,
        "different_identity_cosine": negative_pairs,
        "minimum_same_identity_cosine": minimum_positive,
        "maximum_different_identity_cosine": maximum_negative,
        "open_set_margin": margin,
        "reference_threshold_basis": "OpenCV SFace example cosine threshold 0.363 is a starting point, not this product's final calibrated threshold",
        "mean_operation_millis": statistics.fmean(timings),
        "embeddings_persisted": False,
        "identity_decision_authority": False,
        "activation_authority": False,
    }
    report_path = output / "mara-venn-reference-gallery-probe.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
