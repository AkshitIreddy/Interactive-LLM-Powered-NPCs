"""OpenCV CPU backend and an isolated deterministic test double.

Third-party modules are imported only after Model Manager has supplied a
verified artifact lease.  Starting the worker therefore neither loads a model
nor initializes a GPU runtime.
"""

from __future__ import annotations

import hashlib
import mmap
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

from model_spec import (
    DETECTOR_ID,
    DETECTOR_REVISION,
    EMBEDDING_DIMENSIONS,
    PREPROCESSING,
    PixelLease,
    PackSpec,
    SpecError,
    normalize_vector,
)

PINNED_OPENCV_VERSION = "5.0.0"
PINNED_NUMPY_VERSION = "2.5.2"


class BackendError(RuntimeError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class Cancelled(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class FaceObservation:
    detector_local_id: str
    x: float
    y: float
    width: float
    height: float
    confidence: float
    embedding: tuple[float, ...]


class Backend(Protocol):
    backend_id: str

    def load(self, artifact_root: Path, pack: PackSpec, *, cpu_threads: int) -> None: ...
    def unload(self) -> None: ...
    def infer(self, lease: PixelLease, *, frame_sequence: int, cancelled: threading.Event) -> list[FaceObservation]: ...
    def cancel_active(self) -> None: ...


def verify_artifact(path: Path, expected_size: int, expected_sha256: str) -> None:
    if path.is_symlink() or not path.is_file():
        raise BackendError("artifact_missing", "a verified identity artifact is unavailable")
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            size += len(chunk)
            if size > expected_size:
                raise BackendError("artifact_corrupt", "identity artifact exceeds its immutable size")
            digest.update(chunk)
    if size != expected_size or digest.hexdigest() != expected_sha256:
        raise BackendError("artifact_corrupt", "identity artifact no longer matches its immutable digest")


class OpenCvSFaceBackend:
    backend_id = "cpu"

    def __init__(self) -> None:
        self._cv = None
        self._np = None
        self._detector = None
        self._recognizer = None

    def load(self, artifact_root: Path, pack: PackSpec, *, cpu_threads: int) -> None:
        if not 1 <= cpu_threads <= 4:
            raise BackendError("invalid_threads", "identity CPU threads must be in 1..=4")
        try:
            import cv2 as cv
            import numpy as np
        except ImportError as exc:
            raise BackendError("runtime_missing", "the pinned OpenCV identity runtime is not installed") from exc
        if getattr(cv, "__version__", "") != PINNED_OPENCV_VERSION or getattr(np, "__version__", "") != PINNED_NUMPY_VERSION:
            raise BackendError("runtime_revision_mismatch", "OpenCV or NumPy differs from the reviewed worker ABI")
        if not hasattr(cv, "FaceDetectorYN") or not hasattr(cv, "FaceRecognizerSF"):
            raise BackendError("runtime_abi_mismatch", "OpenCV face DNN APIs are unavailable")
        detector_artifact = pack.artifact("yunet-2026may-onnx")
        recognizer_artifact = pack.artifact("sface-2021dec-onnx")
        detector_path = artifact_root.joinpath(*detector_artifact.destination.parts)
        recognizer_path = artifact_root.joinpath(*recognizer_artifact.destination.parts)
        verify_artifact(detector_path, detector_artifact.size_bytes, detector_artifact.sha256)
        verify_artifact(recognizer_path, recognizer_artifact.size_bytes, recognizer_artifact.sha256)
        try:
            cv.setNumThreads(cpu_threads)
            if hasattr(cv, "ocl"):
                cv.ocl.setUseOpenCL(False)
            detector = cv.FaceDetectorYN.create(
                str(detector_path), "", (320, 320), 0.82, 0.30, 512,
                cv.dnn.DNN_BACKEND_OPENCV, cv.dnn.DNN_TARGET_CPU,
            )
            recognizer = cv.FaceRecognizerSF.create(
                str(recognizer_path), "", cv.dnn.DNN_BACKEND_OPENCV, cv.dnn.DNN_TARGET_CPU,
            )
        except Exception as exc:
            raise BackendError("load_failed", "OpenCV could not load the verified identity pack") from exc
        self._cv = cv
        self._np = np
        self._detector = detector
        self._recognizer = recognizer

    def unload(self) -> None:
        self._recognizer = None
        self._detector = None
        self._np = None
        self._cv = None

    @staticmethod
    def _read_mapping(lease: PixelLease) -> bytes:
        try:
            mapping = mmap.mmap(-1, lease.byte_length, tagname=lease.shared_memory_name, access=mmap.ACCESS_READ)
        except (OSError, TypeError) as exc:
            raise BackendError("lease_unavailable", "the broker pixel lease could not be opened") from exc
        try:
            value = mapping.read(lease.byte_length)
        finally:
            mapping.close()
        if len(value) != lease.byte_length or hashlib.sha256(value).hexdigest() != lease.content_sha256:
            raise BackendError("lease_digest_mismatch", "the broker pixel lease failed content verification")
        return value

    def infer(self, lease: PixelLease, *, frame_sequence: int, cancelled: threading.Event) -> list[FaceObservation]:
        cv, np, detector, recognizer = self._cv, self._np, self._detector, self._recognizer
        if cv is None or np is None or detector is None or recognizer is None:
            raise BackendError("model_not_loaded", "identity model pack is not loaded")
        if cancelled.is_set():
            raise Cancelled()
        pixels = self._read_mapping(lease)
        try:
            rows = np.frombuffer(pixels, dtype=np.uint8).reshape((lease.height, lease.stride_bytes // 4, 4))
            bgr = np.ascontiguousarray(rows[:, : lease.width, :3])
            detector.setInputSize((lease.width, lease.height))
            _, faces = detector.detect(bgr)
        except Exception as exc:
            raise BackendError("inference_failed", "YuNet face detection failed safely") from exc
        if cancelled.is_set():
            raise Cancelled()
        if faces is None:
            return []
        if len(faces) > 64:
            raise BackendError("too_many_faces", "detector output exceeds the bounded face count")
        ordered = sorted(faces, key=lambda face: (-float(face[14]), float(face[0]), float(face[1])))
        result: list[FaceObservation] = []
        for index, face in enumerate(ordered):
            if cancelled.is_set():
                raise Cancelled()
            try:
                aligned = recognizer.alignCrop(bgr, face)
                raw = recognizer.feature(aligned).reshape(-1).tolist()
                embedding = normalize_vector(raw)
            except SpecError as exc:
                raise BackendError("model_abi_mismatch", "SFace embedding shape or values changed") from exc
            except Exception as exc:
                raise BackendError("inference_failed", "SFace feature extraction failed safely") from exc
            x = max(0.0, min(float(face[0]), float(lease.width - 1)))
            y = max(0.0, min(float(face[1]), float(lease.height - 1)))
            width = max(1.0, min(float(face[2]), float(lease.width) - x))
            height = max(1.0, min(float(face[3]), float(lease.height) - y))
            result.append(FaceObservation(f"{frame_sequence}:{index}", x, y, width, height, float(face[14]), embedding))
        return result

    def cancel_active(self) -> None:
        # OpenCV's synchronous DNN call has no cancellation primitive.  The
        # scheduler suppresses its result and the supervisor terminates this
        # process if the bounded generation barrier is missed.
        return None


class DeterministicFixtureBackend:
    """No-model backend used only when tests inject it explicitly."""

    backend_id = "fixture"

    def __init__(self, frames: dict[str, list[FaceObservation]] | None = None, *, block: threading.Event | None = None) -> None:
        self.frames = frames or {}
        self.loaded = False
        self.block = block

    def load(self, artifact_root: Path, pack: PackSpec, *, cpu_threads: int) -> None:
        self.loaded = True

    def unload(self) -> None:
        self.loaded = False

    def infer(self, lease: PixelLease, *, frame_sequence: int, cancelled: threading.Event) -> list[FaceObservation]:
        if not self.loaded:
            raise BackendError("model_not_loaded", "fixture backend is not loaded")
        while self.block is not None and not self.block.wait(0.002):
            if cancelled.is_set():
                raise Cancelled()
        if cancelled.is_set():
            raise Cancelled()
        return list(self.frames.get(lease.content_sha256, []))

    def cancel_active(self) -> None:
        return None


def observation_payload(observation: FaceObservation, *, frame_sequence: int, content_sha256: str) -> dict[str, object]:
    if len(observation.embedding) != EMBEDDING_DIMENSIONS:
        raise BackendError("model_abi_mismatch", "fixture observation embedding dimensions changed")
    return {
        "detector_local_id": observation.detector_local_id,
        "bounds": {
            "x": observation.x,
            "y": observation.y,
            "width": observation.width,
            "height": observation.height,
        },
        "confidence": observation.confidence,
        "embedding": {
            "schema_version": 1,
            "model": {
                "provider": "opencv-zoo",
                "model_id": "sface-2021dec-mobilefacenet",
                "revision": "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
                "dimensions": EMBEDDING_DIMENSIONS,
            },
            "metadata": {
                "source_frame_index": frame_sequence,
                "crop_bounds": {
                    "x": observation.x,
                    "y": observation.y,
                    "width": observation.width,
                    "height": observation.height,
                },
                "detector_id": DETECTOR_ID,
                "detector_revision": DETECTOR_REVISION,
                "preprocessing": PREPROCESSING,
                "source_digest_sha256": content_sha256,
            },
            "values": list(observation.embedding),
        },
    }

