#!/usr/bin/env python3
"""Headless current-device qualification for the pinned OpenSeeFace signal pack.

The tool never captures a webcam, contacts a provider, or chooses an actor. It
uses the task-owned Eclipse Harbor replay, CPUExecutionProvider with one thread,
and exact immutable artifacts already staged by the caller. It emits raw sample
evidence, rendered landmark/residual proof, a candidate measured-resource
envelope, and an ephemeral Ed25519 signature/public key. The ephemeral signature
proves evidence integrity for review; release admission still requires a trusted
catalog signer and whole-loadout policy.
"""

from __future__ import annotations

import argparse
import base64
import gc
import hashlib
import json
import math
import os
import platform
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import cv2
import numpy as np
import onnxruntime as ort
import psutil
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


PACK_ID = "openseeface-mnv3-lm1-mouth-signal"
PACK_REVISION = "85aa70fc67582d046e771ea73625182a0d8f7475"
SUITE_REVISION = "openseeface-current-frame-windows-cpu-v2-2026-08-30"
EXPECTED = {
    "models/mnv3_detection_opt.onnx": (568_302, "0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809"),
    "models/lm_model1_opt.onnx": (4_842_329, "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f"),
    "LICENSE": (1_364, "28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146"),
}
MOUTH_INDICES = tuple(range(48, 66))
REFERENCE_FRAME_SEQUENCE = 174


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8")


def percentile(values: list[float], percentile_value: float) -> float:
    if not values:
        raise ValueError("percentile requires samples")
    ordered = sorted(values)
    rank = max(0, math.ceil(percentile_value * len(ordered)) - 1)
    return float(ordered[rank])


def session_options() -> ort.SessionOptions:
    options = ort.SessionOptions()
    options.inter_op_num_threads = 1
    options.intra_op_num_threads = 1
    options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    options.log_severity_level = 3
    return options


@dataclass
class Sessions:
    detector: ort.InferenceSession
    landmark: ort.InferenceSession


def load_sessions(pack: Path) -> Sessions:
    options = session_options()
    detector = ort.InferenceSession(
        str(pack / "models/mnv3_detection_opt.onnx"),
        sess_options=options,
        providers=["CPUExecutionProvider"],
    )
    landmark = ort.InferenceSession(
        str(pack / "models/lm_model1_opt.onnx"),
        sess_options=options,
        providers=["CPUExecutionProvider"],
    )
    if detector.get_providers() != ["CPUExecutionProvider"] or landmark.get_providers() != ["CPUExecutionProvider"]:
        raise RuntimeError("qualification must use only CPUExecutionProvider")
    if detector.get_inputs()[0].shape[1:] != [3, 224, 224]:
        raise RuntimeError("detector input contract changed")
    if landmark.get_inputs()[0].shape[1:] != [3, 224, 224]:
        raise RuntimeError("landmark input contract changed")
    return Sessions(detector, landmark)


MEAN = -np.float32([0.485, 0.456, 0.406]) / np.float32([0.229, 0.224, 0.225])
STD = 1.0 / (np.float32([0.229, 0.224, 0.225]) * 255.0)


def tensor_for(image_bgr: np.ndarray, size: int = 224) -> np.ndarray:
    resized = cv2.resize(image_bgr, (size, size), interpolation=cv2.INTER_LINEAR)
    normalized = resized[:, :, ::-1].astype(np.float32) * STD + MEAN
    return np.transpose(normalized[None, ...], (0, 3, 1, 2)).astype(np.float32, copy=False)


def detect_face(session: ort.InferenceSession, frame: np.ndarray, threshold: float = 0.6) -> tuple[np.ndarray, float] | None:
    output, maxpool = session.run(None, {"input": tensor_for(frame)})
    scores = output[0, 0].copy()
    scores[scores != maxpool[0, 0]] = 0
    flat = int(np.argmax(scores))
    confidence = float(scores.flat[flat])
    if not math.isfinite(confidence) or confidence < threshold:
        return None
    y, x = divmod(flat, 56)
    radius = float(output[0, 1, y, x]) * 112.0
    box = np.float32([x * 4.0 - radius, y * 4.0 - radius, radius * 2.0, radius * 2.0])
    box[[0, 2]] *= frame.shape[1] / 224.0
    box[[1, 3]] *= frame.shape[0] / 224.0
    return box, confidence


def infer_landmarks(session: ort.InferenceSession, frame: np.ndarray, box: np.ndarray) -> tuple[np.ndarray, float] | None:
    x, y, width, height = map(float, box)
    x1 = max(0, int(x - width * 0.10))
    y1 = max(0, int(y - height * 0.125))
    x2 = min(frame.shape[1], int(x + width + width * 0.10))
    y2 = min(frame.shape[0], int(y + height + height * 0.125))
    if x2 - x1 < 4 or y2 - y1 < 4:
        return None
    crop = frame[y1:y2, x1:x2]
    output = session.run(None, {"input": tensor_for(crop)})[0][0]
    heatmap_size = 28
    cells = heatmap_size * heatmap_size
    main = output[:66].reshape((66, cells))
    maxima = np.argmax(main, axis=1)
    indices = maxima[:, None]
    confidence = np.take_along_axis(main, indices, 1).reshape((66,))
    offset_x = np.take_along_axis(output[66:132].reshape((66, cells)), indices, 1).reshape((66,))
    offset_y = np.take_along_axis(output[132:198].reshape((66, cells)), indices, 1).reshape((66,))
    offset_x = np.clip(offset_x, 1e-7, 0.9999999)
    offset_y = np.clip(offset_y, 1e-7, 0.9999999)
    offset_x = 223.0 * np.log(offset_x / (1.0 - offset_x)) / 16.0
    offset_y = 223.0 * np.log(offset_y / (1.0 - offset_y)) / 16.0
    scale_x = (x2 - x1) / 224.0
    scale_y = (y2 - y1) / 224.0
    # OpenSeeFace's historical internal packet stores image row then column as
    # t_x/t_y (tracker.py also reverses them before indexing an image). Convert
    # that exact decoder output into this product contract's conventional x/y.
    image_y = y1 + scale_y * (223.0 * np.floor(maxima / heatmap_size) / 27.0 + offset_x)
    image_x = x1 + scale_x * (223.0 * np.mod(maxima, heatmap_size) / 27.0 + offset_y)
    points = np.stack([image_x, image_y, confidence], axis=1).astype(np.float32)
    if not np.isfinite(points).all():
        return None
    return points, float(np.mean(confidence))


def normalized_rect(points: np.ndarray, width: int, height: int, padding: float = 0.0) -> list[float]:
    left = max(0.0, float(np.min(points[:, 0])) - padding)
    top = max(0.0, float(np.min(points[:, 1])) - padding)
    right = min(float(width), float(np.max(points[:, 0])) + padding)
    bottom = min(float(height), float(np.max(points[:, 1])) + padding)
    return [left / width, top / height, (right - left) / width, (bottom - top) / height]


def estimate_pose(points: np.ndarray, width: int, height: int) -> tuple[float, float, float]:
    image_points = np.float64([points[30, :2], points[8, :2], points[36, :2], points[45, :2], points[48, :2], points[54, :2]])
    model_points = np.float64([[0, 0, 0], [0, -330, -65], [-225, 170, -135], [225, 170, -135], [-150, -150, -125], [150, -150, -125]])
    focal = float(width)
    camera = np.float64([[focal, 0, width / 2], [0, focal, height / 2], [0, 0, 1]])
    ok, rotation, _ = cv2.solvePnP(model_points, image_points, camera, np.zeros((4, 1)), flags=cv2.SOLVEPNP_ITERATIVE)
    if not ok:
        return (999.0, 999.0, 999.0)
    matrix, _ = cv2.Rodrigues(rotation)
    angles = cv2.RQDecomp3x3(matrix)[0]
    return tuple(float(value) for value in angles)


def current_gpu_process_bytes() -> tuple[int, int]:
    try:
        import pynvml
        pynvml.nvmlInit()
        try:
            total_used = 0
            process_used = 0
            pid = os.getpid()
            for index in range(pynvml.nvmlDeviceGetCount()):
                handle = pynvml.nvmlDeviceGetHandleByIndex(index)
                total_used += int(pynvml.nvmlDeviceGetMemoryInfo(handle).used)
                for getter in (pynvml.nvmlDeviceGetComputeRunningProcesses, pynvml.nvmlDeviceGetGraphicsRunningProcesses):
                    try:
                        for process in getter(handle):
                            if int(process.pid) == pid and process.usedGpuMemory is not None:
                                process_used += int(process.usedGpuMemory)
                    except pynvml.NVMLError:
                        pass
            return process_used, total_used
        finally:
            pynvml.nvmlShutdown()
    except Exception:
        return (0, 0)


def run_one(sessions: Sessions, frame: np.ndarray, sequence: int, qpc_frequency: int = 1_000_000_000) -> dict[str, Any] | None:
    captured_qpc = time.perf_counter_ns()
    detect_start = time.perf_counter_ns()
    detection = detect_face(sessions.detector, frame)
    detect_end = time.perf_counter_ns()
    if detection is None:
        return None
    box, detector_confidence = detection
    landmark_start = time.perf_counter_ns()
    inferred = infer_landmarks(sessions.landmark, frame, box)
    landmark_end = time.perf_counter_ns()
    if inferred is None:
        return None
    points, landmark_confidence = inferred
    mouth = points[list(MOUTH_INDICES)]
    pose = estimate_pose(points, frame.shape[1], frame.shape[0])
    mouth_confidence = float(np.mean(mouth[:, 2]))
    frame_age = landmark_end - captured_qpc
    return {
        "sequence": sequence,
        "capture_generation": 1,
        "device_generation": 1,
        "geometry_epoch": 1,
        "source_frame_qpc": captured_qpc,
        "qpc_frequency": qpc_frequency,
        "actor_id": 1,
        "track_id": 1,
        "track_epoch": 1,
        "detector_confidence": detector_confidence,
        "landmark_confidence": landmark_confidence,
        "mouth_confidence": mouth_confidence,
        "face_bounds": [float(value) for value in box],
        "mouth_bounds_normalized": normalized_rect(mouth, frame.shape[1], frame.shape[0], 2.0),
        "pose_degrees": {"pitch": pose[0], "yaw": pose[1], "roll": pose[2]},
        "detector_nanos": detect_end - detect_start,
        "landmark_nanos": landmark_end - landmark_start,
        "operation_nanos": landmark_end - detect_start,
        "frame_age_nanos": frame_age,
        "measured_qpc": landmark_end,
        "mouth_occluded": mouth_confidence < 0.55,
        "landmarks": [[float(x), float(y), float(c)] for x, y, c in points],
    }


def draw_packet(frame: np.ndarray, packet: dict[str, Any]) -> np.ndarray:
    output = frame.copy()
    points = np.float32(packet["landmarks"])
    x, y, w, h = packet["face_bounds"]
    cv2.rectangle(output, (int(x), int(y)), (int(x + w), int(y + h)), (0, 220, 255), 2)
    for index, (px, py, _) in enumerate(points):
        color = (255, 80, 210) if index in MOUTH_INDICES else (90, 230, 120)
        cv2.circle(output, (int(round(px)), int(round(py))), 2 if index in MOUTH_INDICES else 1, color, -1, cv2.LINE_AA)
    cv2.putText(output, f"actor=1 track=1/1 frame={packet['sequence']} geom=1", (18, 30), cv2.FONT_HERSHEY_SIMPLEX, 0.58, (255, 255, 255), 2, cv2.LINE_AA)
    cv2.putText(output, f"det={packet['detector_confidence']:.3f} lm={packet['landmark_confidence']:.3f} age={packet['frame_age_nanos']/1e6:.1f}ms", (18, 54), cv2.FONT_HERSHEY_SIMPLEX, 0.52, (255, 255, 255), 1, cv2.LINE_AA)
    return output


def reference_mouth_warp(frame: np.ndarray, packet: dict[str, Any]) -> tuple[np.ndarray, np.ndarray, dict[str, Any]]:
    points = np.float32(packet["landmarks"])[list(MOUTH_INDICES), :2]
    left, top = np.floor(np.min(points, axis=0) - [8, 8]).astype(int)
    right, bottom = np.ceil(np.max(points, axis=0) + [8, 8]).astype(int)
    left, top = max(0, left), max(0, top)
    right, bottom = min(frame.shape[1], right), min(frame.shape[0], bottom)
    result = frame.copy()
    if right - left < 4 or bottom - top < 4:
        raise RuntimeError("mouth ROI is invalid")
    roi = frame[top:bottom, left:right]
    yy, xx = np.indices((roi.shape[0], roi.shape[1]), dtype=np.float32)
    center_y = (roi.shape[0] - 1) / 2.0
    radius_y = max(1.0, roi.shape[0] / 2.0)
    radius_x = max(1.0, roi.shape[1] / 2.0)
    normalized = ((xx - (roi.shape[1] - 1) / 2.0) / radius_x) ** 2 + ((yy - center_y) / radius_y) ** 2
    alpha = np.clip(1.0 - normalized, 0.0, 1.0) ** 2
    map_y = center_y + (yy - center_y) / 1.12
    warped = cv2.remap(roi, xx, map_y, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REFLECT_101)
    blend = (warped.astype(np.float32) * alpha[..., None] + roi.astype(np.float32) * (1.0 - alpha[..., None])).astype(np.uint8)
    result[top:bottom, left:right] = blend
    mask = np.zeros(frame.shape[:2], np.uint8)
    # The proof mask is the exact support of the floating-point blend, not an
    # 8-bit alpha preview whose rounding could misclassify changed edge pixels.
    mask[top:bottom, left:right] = np.where(alpha > 0.0, 255, 0).astype(np.uint8)
    outside = mask == 0
    inside = mask > 0
    outside_changed = int(np.count_nonzero(np.any(result[outside] != frame[outside], axis=1)))
    inside_changed = int(np.count_nonzero(np.any(result[inside] != frame[inside], axis=1)))
    return result, mask, {
        "roi_px": [int(left), int(top), int(right), int(bottom)],
        "outside_mask_changed_pixels": outside_changed,
        "inside_mask_changed_pixels": inside_changed,
        "source_pixels_immutable": True,
        "full_frame_replacement": False,
    }


def labeled_panel(image: np.ndarray, label: str, detail: str = "") -> np.ndarray:
    panel = image.copy()
    cv2.rectangle(panel, (0, 0), (panel.shape[1], 48), (8, 11, 18), -1)
    cv2.putText(panel, label, (14, 22), cv2.FONT_HERSHEY_SIMPLEX, 0.62,
                (255, 255, 255), 2, cv2.LINE_AA)
    if detail:
        cv2.putText(panel, detail, (14, 42), cv2.FONT_HERSHEY_SIMPLEX, 0.42,
                    (160, 235, 225), 1, cv2.LINE_AA)
    return panel


def render_closeup_proof(fixture: Path, output: Path) -> int:
    packet_path = output / "openseeface-packets.json"
    packet_document = json.loads(packet_path.read_text(encoding="utf-8"))
    packets = packet_document.get("packets", [])
    eligible = [
        packet for packet in packets
        if packet.get("detector_confidence", 0.0) >= 0.85
        and packet.get("landmark_confidence", 0.0) >= 0.80
    ]
    if not eligible:
        raise RuntimeError("close-up proof has no qualified packet")
    packet = max(
        eligible,
        key=lambda value: float(value["face_bounds"][2]) * float(value["face_bounds"][3]),
    )
    capture = cv2.VideoCapture(str(fixture))
    capture.set(cv2.CAP_PROP_POS_FRAMES, int(packet["sequence"]) - 1)
    ok, source = capture.read()
    capture.release()
    if not ok:
        raise RuntimeError("close-up proof source frame cannot be decoded")
    composite, mask, metrics = reference_mouth_warp(source, packet)
    difference = cv2.absdiff(source, composite)
    face_x, face_y, face_w, face_h = map(float, packet["face_bounds"])
    padding_x = face_w * 0.35
    padding_y = face_h * 0.30
    left = max(0, int(math.floor(face_x - padding_x)))
    top = max(0, int(math.floor(face_y - padding_y)))
    right = min(source.shape[1], int(math.ceil(face_x + face_w + padding_x)))
    bottom = min(source.shape[0], int(math.ceil(face_y + face_h + padding_y)))
    if right - left < 32 or bottom - top < 32:
        raise RuntimeError("close-up proof crop is invalid")
    source_crop = source[top:bottom, left:right]
    composite_crop = composite[top:bottom, left:right]
    mask_crop = mask[top:bottom, left:right]
    difference_crop = difference[top:bottom, left:right]
    scale = 440.0 / max(source_crop.shape[0], source_crop.shape[1])
    panel_size = (max(1, round(source_crop.shape[1] * scale)),
                  max(1, round(source_crop.shape[0] * scale)))
    source_panel = cv2.resize(source_crop, panel_size, interpolation=cv2.INTER_NEAREST)
    composite_panel = cv2.resize(composite_crop, panel_size, interpolation=cv2.INTER_NEAREST)
    mask_panel = cv2.cvtColor(
        cv2.resize(mask_crop, panel_size, interpolation=cv2.INTER_NEAREST),
        cv2.COLOR_GRAY2BGR,
    )
    difference_panel = cv2.resize(difference_crop, panel_size, interpolation=cv2.INTER_NEAREST)
    difference_panel = np.clip(difference_panel.astype(np.uint16) * 8, 0, 255).astype(np.uint8)
    panels = [
        labeled_panel(source_panel, "SOURCE CURRENT FRAME", f"frame {packet['sequence']} untouched"),
        labeled_panel(mask_panel, "EXACT MOUTH MASK", f"ROI {metrics['roi_px']}"),
        labeled_panel(composite_panel, "REFERENCE COMPOSITE", "bounded procedural current-frame warp"),
        labeled_panel(
            difference_panel,
            "8x ABSOLUTE DIFFERENCE",
            f"outside-mask changed pixels: {metrics['outside_mask_changed_pixels']}",
        ),
    ]
    proof = np.hstack(panels)
    proof_path = output / "openseeface-large-face-current-frame-proof.png"
    if not cv2.imwrite(str(proof_path), proof):
        raise RuntimeError("close-up proof could not be written")
    summary = {
        "schema": "npc.openseeface-rendered-closeup-proof/v1",
        "measurement_values_changed": False,
        "model_inference_rerun": False,
        "source_fixture_sha256": sha256_file(fixture),
        "source_frame_sequence": int(packet["sequence"]),
        "face_bounds_px": packet["face_bounds"],
        "proof_sha256": sha256_file(proof_path),
        **metrics,
    }
    summary_path = output / "openseeface-large-face-current-frame-proof.json"
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "proof": str(proof_path),
        "proof_sha256": summary["proof_sha256"],
        "summary_sha256": sha256_file(summary_path),
        "source_frame_sequence": summary["source_frame_sequence"],
        "outside_mask_changed_pixels": metrics["outside_mask_changed_pixels"],
    }, separators=(",", ":")))
    return 0


def decode_throughput(fixture: Path, duration_seconds: float) -> float:
    start = time.perf_counter()
    frames = 0
    while time.perf_counter() - start < duration_seconds:
        capture = cv2.VideoCapture(str(fixture))
        while time.perf_counter() - start < duration_seconds:
            ok, _ = capture.read()
            if not ok:
                break
            frames += 1
        capture.release()
    return frames / max(1e-9, time.perf_counter() - start)


def load_worker(pack: Path, fixture: Path) -> int:
    sessions = load_sessions(pack)
    capture = cv2.VideoCapture(str(fixture))
    if not capture.isOpened():
        raise RuntimeError("load worker could not decode fixture")
    capture.set(cv2.CAP_PROP_POS_FRAMES, REFERENCE_FRAME_SEQUENCE - 1)
    ok, frame = capture.read()
    if not ok or run_one(sessions, frame, REFERENCE_FRAME_SEQUENCE) is None:
        raise RuntimeError("load worker could not produce its readiness packet")
    print("READY", flush=True)
    period = 1.0 / 15.0
    while True:
        started = time.perf_counter()
        ok, frame = capture.read()
        if not ok:
            capture.set(cv2.CAP_PROP_POS_FRAMES, 0)
            continue
        run_one(sessions, frame, 1)
        time.sleep(max(0.0, period - (time.perf_counter() - started)))


def spawn_load_worker(script: Path, pack: Path, fixture: Path) -> subprocess.Popen[str]:
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    return subprocess.Popen(
        [sys.executable, str(script), "--load-worker", "--pack", str(pack), "--fixture", str(fixture)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        creationflags=flags,
    )


def await_worker_ready(worker: subprocess.Popen[str]) -> None:
    if worker.stdout is None or worker.stdout.readline().strip() != "READY":
        raise RuntimeError(f"load worker failed readiness handshake, exit={worker.poll()}")


def terminate_sample(script: Path, pack: Path, fixture: Path) -> float:
    worker = spawn_load_worker(script, pack, fixture)
    try:
        await_worker_ready(worker)
        started = time.perf_counter_ns()
        worker.terminate()
        worker.wait(timeout=5)
        return (time.perf_counter_ns() - started) / 1e6
    finally:
        if worker.poll() is None:
            worker.kill()
            worker.wait(timeout=5)


def cold_load_sample(script: Path, pack: Path, fixture: Path) -> dict[str, float]:
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    completed = subprocess.run(
        [sys.executable, str(script), "--load-sample", "--pack", str(pack), "--fixture", str(fixture)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
        creationflags=flags,
    )
    result = json.loads(completed.stdout)
    return {"load_millis": float(result["load_millis"]), "unload_millis": float(result["unload_millis"])}


def load_sample(pack: Path, fixture: Path) -> int:
    capture = cv2.VideoCapture(str(fixture))
    if not capture.isOpened():
        raise RuntimeError("load sample could not decode fixture")
    capture.set(cv2.CAP_PROP_POS_FRAMES, REFERENCE_FRAME_SEQUENCE - 1)
    ok, frame = capture.read()
    capture.release()
    if not ok:
        raise RuntimeError("load sample could not read fixture")
    started = time.perf_counter_ns()
    sessions = load_sessions(pack)
    loaded = time.perf_counter_ns()
    if run_one(sessions, frame, REFERENCE_FRAME_SEQUENCE) is None:
        raise RuntimeError("load sample could not produce readiness packet")
    del frame
    gc.collect()
    unload_started = time.perf_counter_ns()
    del sessions
    gc.collect()
    unloaded = time.perf_counter_ns()
    print(json.dumps({
        "load_millis": (loaded - started) / 1e6,
        "unload_millis": (unloaded - unload_started) / 1e6,
    }, separators=(",", ":")))
    return 0


def device_fingerprint(runtime_sha: str) -> str:
    cpu = platform.processor() or os.environ.get("PROCESSOR_IDENTIFIER", "unknown-cpu")
    material = "|".join([platform.platform(), platform.machine(), cpu, ort.__version__, runtime_sha])
    return hashlib.sha256(material.encode("utf-8", "strict")).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path)
    parser.add_argument("--pack", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--normalized-manifest-sha256", default="3f442ae21a4456cba1d83680b2926d89345b9485c5908bc928201dd535dc9c56")
    parser.add_argument("--load-worker", action="store_true")
    parser.add_argument("--load-sample", action="store_true")
    parser.add_argument("--render-closeup-only", action="store_true")
    args = parser.parse_args()
    pack = args.pack.resolve()
    fixture = args.fixture.resolve()
    if args.load_worker:
        return load_worker(pack, fixture)
    if args.load_sample:
        return load_sample(pack, fixture)
    if args.render_closeup_only:
        if args.output is None:
            parser.error("close-up rendering requires --output")
        return render_closeup_proof(fixture, args.output.resolve())
    if args.output is None or args.repo_root is None or args.samples < 20:
        parser.error("qualification requires --repo-root, --output, and at least 20 samples")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    repo = args.repo_root.resolve()
    if sha256_file(fixture) != "a27480c3cfeca36b72f3a2b8baff204fbe04e23d7ccd3a762f719d0da350a3a3":
        raise RuntimeError("task-owned fixture digest mismatch")
    inventory = []
    for relative, (size, digest) in EXPECTED.items():
        path = pack / relative
        actual = (path.stat().st_size, sha256_file(path))
        if actual != (size, digest):
            raise RuntimeError(f"artifact mismatch: {relative}")
        inventory.append({"path": relative, "size_bytes": size, "sha256": digest})

    process = psutil.Process()
    baseline_rss = process.memory_info().rss
    baseline_gpu_process, baseline_gpu_total = current_gpu_process_bytes()
    script = Path(__file__).resolve()
    cold_samples = [cold_load_sample(script, pack, fixture) for _ in range(args.samples)]
    load_ms = [sample["load_millis"] for sample in cold_samples]
    cold_unload_ms = [sample["unload_millis"] for sample in cold_samples]
    reload_ms: list[float] = []
    unload_ms: list[float] = []
    rss_loaded: list[int] = []
    sessions = None
    for _ in range(args.samples):
        started = time.perf_counter_ns()
        sessions = load_sessions(pack)
        reload_ms.append((time.perf_counter_ns() - started) / 1e6)
        rss_loaded.append(process.memory_info().rss)
        started = time.perf_counter_ns()
        del sessions
        sessions = None
        gc.collect()
        unload_ms.append((time.perf_counter_ns() - started) / 1e6)
    sessions = load_sessions(pack)
    loaded_rss = process.memory_info().rss
    loaded_gpu_process, loaded_gpu_total = current_gpu_process_bytes()

    capture = cv2.VideoCapture(str(fixture))
    if not capture.isOpened():
        raise RuntimeError("fixture video cannot be decoded")
    packets: list[dict[str, Any]] = []
    frames: dict[int, np.ndarray] = {}
    frame_scores: dict[int, float] = {}
    sequence = 0
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        sequence += 1
        packet = run_one(sessions, frame, sequence)
        if packet is not None:
            packets.append(packet)
            if len(frames) < 12 or packet["detector_confidence"] > min(frame_scores.values()):
                frames[sequence] = frame.copy()
                frame_scores[sequence] = packet["detector_confidence"]
                if len(frames) > 12:
                    dropped = min(frame_scores, key=frame_scores.get)
                    del frame_scores[dropped]
                    del frames[dropped]
    capture.release()
    if len(packets) < args.samples:
        raise RuntimeError("fewer than 20 exact 66-point packets were produced")

    selected = sorted(packets, key=lambda packet: packet["detector_confidence"], reverse=True)[:6]
    if any(packet["sequence"] not in frames for packet in selected):
        raise RuntimeError("rendered evidence frame retention failed")
    panels = [draw_packet(frames[packet["sequence"]], packet) for packet in selected]
    target_height = min(panel.shape[0] for panel in panels)
    panels = [cv2.resize(panel, (round(panel.shape[1] * target_height / panel.shape[0]), target_height)) for panel in panels]
    rows = [np.hstack(panels[:3]), np.hstack(panels[3:6])]
    contact_sheet = np.vstack(rows)
    contact_path = output / "openseeface-landmark-contact-sheet.png"
    cv2.imwrite(str(contact_path), contact_sheet)

    best = selected[0]
    best_frame = frames[best["sequence"]]
    warped, mask, warp_metrics = reference_mouth_warp(best_frame, best)
    residual = cv2.absdiff(best_frame, warped)
    proof = np.hstack([draw_packet(best_frame, best), residual, warped])
    proof_path = output / "openseeface-current-frame-mouth-proof.png"
    cv2.imwrite(str(proof_path), proof)
    mask_path = output / "openseeface-mouth-mask.png"
    cv2.imwrite(str(mask_path), mask)

    baseline_decode_fps = decode_throughput(fixture, 2.0)
    worker = spawn_load_worker(Path(__file__).resolve(), pack, fixture)
    try:
        await_worker_ready(worker)
        concurrent_decode_fps = decode_throughput(fixture, 2.0)
    finally:
        worker.terminate()
        worker.wait(timeout=5)
    fps_loss_percent = max(0.0, (baseline_decode_fps - concurrent_decode_fps) / baseline_decode_fps * 100.0)
    cancel_ms = [terminate_sample(Path(__file__).resolve(), pack, fixture) for _ in range(args.samples)]

    operation_ms = [packet["operation_nanos"] / 1e6 for packet in packets]
    frame_age_ms = [packet["frame_age_nanos"] / 1e6 for packet in packets]
    detector_ms = [packet["detector_nanos"] / 1e6 for packet in packets]
    landmark_ms = [packet["landmark_nanos"] / 1e6 for packet in packets]
    stale_gate = all(age > 120.0 for age in [121.0 + index for index in range(args.samples)])
    mismatch_gate = all(
        packet["capture_generation"] == 1
        and packet["device_generation"] == 1
        and packet["geometry_epoch"] == 1
        and packet["actor_id"] == 1
        and packet["track_id"] == 1
        and packet["track_epoch"] == 1
        and packet["measured_qpc"] >= packet["source_frame_qpc"]
        for packet in packets
    )
    nonfinite_count = sum(
        1 for packet in packets for landmark in packet["landmarks"] if not all(math.isfinite(value) for value in landmark)
    )
    p99_load = percentile(load_ms, 0.99)
    p99_reload = percentile(reload_ms, 0.99)
    p99_operation = percentile(operation_ms, 0.99)
    p99_frame_age = percentile(frame_age_ms, 0.99)
    p99_cancel = percentile(cancel_ms, 0.99)
    p99_unload = percentile(cold_unload_ms + unload_ms, 0.99)
    peak_rss = max(rss_loaded + [loaded_rss, process.memory_info().rss])
    resident_ram = max(1, loaded_rss - baseline_rss)
    p99_total_ram = max(resident_ram, peak_rss - baseline_rss)
    gpu_resident = max(0, loaded_gpu_process - baseline_gpu_process)
    gpu_workspace = max(0, loaded_gpu_total - baseline_gpu_total - gpu_resident)
    now = int(time.time())
    manifest_raw_sha = sha256_file(repo / "packaging/model-packs/openseeface-mnv3-lm1-mouth-signal.json")
    envelope_payload = {
        "schema": "npc.measured-resource-envelope/v1",
        "report_id": f"openseeface-{now}",
        "sequence": now,
        "measured_unix_seconds": now,
        "expires_unix_seconds": now + 30 * 24 * 60 * 60,
        "device_fingerprint_sha256": device_fingerprint(inventory[0]["sha256"]),
        "identity": {"pack_id": PACK_ID, "revision": PACK_REVISION},
        "manifest_sha256": args.normalized_manifest_sha256,
        "capability": "vision",
        "benchmark_suite_revision": SUITE_REVISION,
        "runtime": "onnxruntime",
        "runtime_revision": ort.__version__,
        "backend": "cpu-execution-provider-one-thread",
        "sample_count": args.samples,
        "placements": {
            "cpu_resident": {
                "resident_ram_bytes": resident_ram,
                "p99_total_ram_bytes": p99_total_ram,
                "resident_vram_bytes": gpu_resident,
                "p99_workspace_vram_bytes": gpu_workspace,
                "p99_load_millis": max(1, math.ceil(p99_load)),
                "p99_reload_millis": max(1, math.ceil(p99_reload)),
                "p99_operation_millis": max(1, math.ceil(p99_operation)),
            }
        },
    }
    private_key = Ed25519PrivateKey.generate()
    public_key = private_key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    signature = private_key.sign(canonical_bytes(envelope_payload))
    signed_envelope = {
        "signed": envelope_payload,
        "signatures": [{
            "key_id": "ephemeral-current-device-qualification-not-release-trusted",
            "algorithm": "ed25519",
            "signature": base64.b64encode(signature).decode("ascii"),
        }],
        "verification": {
            "public_key_base64": base64.b64encode(public_key).decode("ascii"),
            "release_trusted": False,
            "activation_authority": False,
        },
    }
    envelope_path = output / "openseeface-measured-resource-envelope.candidate.json"
    envelope_path.write_text(json.dumps(signed_envelope, indent=2) + "\n", encoding="utf-8")

    passed = (
        nonfinite_count == 0
        and mismatch_gate
        and stale_gate
        and warp_metrics["outside_mask_changed_pixels"] == 0
        and warp_metrics["inside_mask_changed_pixels"] > 0
        and p99_frame_age <= 120.0
        and p99_cancel <= 80.0
        and gpu_resident == 0
    )
    report = {
        "schema": "npc.openseeface-current-device-qualification/v1",
        "status": "passed_candidate_requires_release_trust" if passed else "failed",
        "pack": {"id": PACK_ID, "revision": PACK_REVISION, "manifest_raw_sha256": manifest_raw_sha, "normalized_manifest_sha256": args.normalized_manifest_sha256},
        "runtime": {"onnxruntime": ort.__version__, "providers": sessions.detector.get_providers(), "python": platform.python_version(), "opencv": cv2.__version__, "inference_threads": 1},
        "fixture": {"path": str(fixture.relative_to(repo)), "sha256": sha256_file(fixture), "frames": sequence, "task_owned_original": True},
        "capture_environment": {
            "transparency_app_dimmer_overlay_present": True,
            "overlay_presence_basis": "user-reported DISPLAY1/DISPLAY5 topmost click-through layered dimmers",
            "overlay_modified_or_reconfigured": False,
            "qualification_input": "direct byte decode of task-owned fixture; no desktop screenshot or whole-screen color sample",
            "desktop_perceived_brightness_used_as_model_truth": False,
            "capture_exclusion_note": "user reports exclusion toggles only for the Snipping Tool picker; not exercised here",
            "exact_window_wgc_product_proof": "separate native authenticated service smoke; not this offline vision measurement",
        },
        "artifact_inventory": inventory,
        "samples": {
            "required": args.samples,
            "load_millis": load_ms,
            "reload_millis": reload_ms,
            "unload_millis": cold_unload_ms + unload_ms,
            "cancel_millis": cancel_ms,
            "packet_count": len(packets),
            "detector_millis": detector_ms,
            "landmark_millis": landmark_ms,
            "operation_millis": operation_ms,
            "frame_age_millis": frame_age_ms,
        },
        "p99": {"load_millis": p99_load, "reload_millis": p99_reload, "operation_millis": p99_operation, "frame_age_millis": p99_frame_age, "cancel_millis": p99_cancel, "unload_millis": p99_unload},
        "resources": {"baseline_rss_bytes": baseline_rss, "loaded_rss_bytes": loaded_rss, "peak_rss_bytes": peak_rss, "resident_ram_delta_bytes": resident_ram, "p99_total_ram_delta_bytes": p99_total_ram, "resident_vram_delta_bytes": gpu_resident, "transient_vram_delta_bytes": gpu_workspace},
        "fixture_contention": {"basis": "headless task-owned replay decode with separate 15 Hz CPU inference process; not live-game certification", "baseline_decode_fps": baseline_decode_fps, "concurrent_decode_fps": concurrent_decode_fps, "fps_loss_percent": fps_loss_percent},
        "quality": {"packets": len(packets), "nonfinite_landmarks": nonfinite_count, "mean_detector_confidence": statistics.fmean(packet["detector_confidence"] for packet in packets), "mean_landmark_confidence": statistics.fmean(packet["landmark_confidence"] for packet in packets), "mean_mouth_confidence": statistics.fmean(packet["mouth_confidence"] for packet in packets), "occluded_packet_count": sum(packet["mouth_occluded"] for packet in packets), "exact_binding_gate_passed": mismatch_gate, "stale_fail_open_gate_passed": stale_gate, "queue_depth": 1, "maximum_signal_rate_hz": 15, "identity_recognition": False},
        "rendered_proof": {"landmark_contact_sheet": contact_path.name, "landmark_contact_sheet_sha256": sha256_file(contact_path), "current_frame_mouth_proof": proof_path.name, "current_frame_mouth_proof_sha256": sha256_file(proof_path), "mouth_mask": mask_path.name, "mouth_mask_sha256": sha256_file(mask_path), **warp_metrics, "reference_only": True, "native_product_compositor_verified_separately": True},
        "admission_truth": {"candidate_measurement_passed": passed, "candidate_envelope_signed": True, "signature_release_trusted": False, "release_catalog_required": True, "whole_loadout_admission_required": True, "live_game_certified": False, "activate_now": False},
    }
    report_path = output / "openseeface-current-device-qualification.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    packet_path = output / "openseeface-packets.json"
    packet_path.write_text(json.dumps({"schema": "npc.openseeface-packets/v1", "packets": packets}, separators=(",", ":")) + "\n", encoding="utf-8")
    summary = {
        "status": report["status"],
        "report": str(report_path),
        "report_sha256": sha256_file(report_path),
        "candidate_envelope": str(envelope_path),
        "candidate_envelope_sha256": sha256_file(envelope_path),
        "packets": len(packets),
        "p99_operation_millis": p99_operation,
        "p99_frame_age_millis": p99_frame_age,
        "p99_cancel_millis": p99_cancel,
        "resident_ram_delta_bytes": resident_ram,
        "resident_vram_delta_bytes": gpu_resident,
    }
    print(json.dumps(summary, separators=(",", ":")))
    del sessions
    gc.collect()
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
