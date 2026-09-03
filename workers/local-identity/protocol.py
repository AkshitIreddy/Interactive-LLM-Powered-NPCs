"""Authenticated Worker Control v1 lifecycle for identity observations."""

from __future__ import annotations

import hashlib
import hmac
import re
import threading
import time
from collections import deque
from pathlib import Path
from typing import Any

from backend import Backend, BackendError, OpenCvSFaceBackend, observation_payload
from framing import FrameWriter
from model_spec import (
    DETECTOR_ID,
    DETECTOR_REVISION,
    EMBEDDING_REVISION,
    OBSERVATION_CONTRACT_VERSION,
    PACK_ID,
    PACK_REVISION,
    PORTABLE_REFERENCE_SCHEMA_VERSION,
    PREPROCESSING,
    PROTOCOL_VERSION,
    ReferenceImportRequest,
    WgcFrameRequest,
    PackSpec,
    SpecError,
    model_payload,
    opaque,
    sha256,
    tensor_bytes,
    uint,
)
from scheduler import IdentityScheduler, Job, SchedulerError

OPERATIONS = frozenset({"handshake", "capabilities", "health", "load", "unload", "infer", "cancel", "shutdown"})
_REQUEST_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,126}[A-Za-z0-9]$")


class ProtocolError(RuntimeError):
    def __init__(self, code: str, message: str, *, retryable: bool = False, details: dict[str, object] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.retryable = retryable
        self.details = details or {}


class Request:
    __slots__ = ("protocol_version", "worker_instance_id", "request_id", "sequence", "generation", "deadline_unix_ms", "operation", "payload")

    def __init__(self, protocol_version: str, worker_instance_id: str, request_id: str, sequence: int, generation: int, deadline_unix_ms: int, operation: str, payload: dict[str, Any]) -> None:
        self.protocol_version = protocol_version
        self.worker_instance_id = worker_instance_id
        self.request_id = request_id
        self.sequence = sequence
        self.generation = generation
        self.deadline_unix_ms = deadline_unix_ms
        self.operation = operation
        self.payload = payload

    @classmethod
    def parse(cls, raw: Any) -> "Request":
        required = {"protocol_version", "worker_instance_id", "request_id", "sequence", "generation", "deadline_unix_ms", "operation", "payload"}
        if not isinstance(raw, dict) or set(raw) != required:
            raise ProtocolError("invalid_request", "request envelope has missing or unknown fields")
        if raw.get("protocol_version") != PROTOCOL_VERSION:
            raise ProtocolError("unsupported_protocol", "worker protocol version is unsupported")
        instance = raw.get("worker_instance_id")
        request_id = raw.get("request_id")
        operation = raw.get("operation")
        payload = raw.get("payload")
        if not isinstance(instance, str) or len(instance.encode("utf-8")) > 128:
            raise ProtocolError("invalid_request", "worker instance id is invalid")
        if not isinstance(request_id, str) or not _REQUEST_ID.fullmatch(request_id):
            raise ProtocolError("invalid_request", "request id is invalid")
        if operation not in OPERATIONS or not isinstance(payload, dict):
            raise ProtocolError("invalid_request", "operation or payload is invalid")
        try:
            return cls(PROTOCOL_VERSION, instance, request_id, uint(raw.get("sequence"), "sequence"), uint(raw.get("generation"), "generation"), uint(raw.get("deadline_unix_ms"), "deadline"), operation, payload)
        except SpecError as exc:
            raise ProtocolError("invalid_request", str(exc)) from exc


class WorkerController:
    def __init__(self, writer: FrameWriter, *, launch_nonce: str, worker_instance_id: str, manifest_path: Path, backend: Backend | None = None) -> None:
        self.writer = writer
        self.launch_nonce = opaque(launch_nonce, "launch nonce")
        self.worker_instance_id = opaque(worker_instance_id, "worker instance id", maximum=128)
        self.pack = PackSpec.load(manifest_path)
        self.backend = backend or OpenCvSFaceBackend()
        self.lifecycle = "starting"
        self.current_generation = 0
        self.last_sequence = -1
        self.seen_ids: set[str] = set()
        self.seen_order: deque[str] = deque()
        self.scheduler: IdentityScheduler | None = None
        self.loaded_lease_id: str | None = None
        self.loaded_artifact_root: Path | None = None
        self.loaded_activation_mode: str | None = None
        self.loaded_catalog_admission_sha256: str | None = None
        self.should_exit = threading.Event()
        self.requests_total = 0
        self.errors_total = 0
        self.completed_inferences = 0
        self._lock = threading.RLock()

    def _event(self, request: Request, event: str, payload: dict[str, object] | None = None, *, terminal: bool, event_index: int = 0, error: ProtocolError | SchedulerError | None = None) -> None:
        envelope: dict[str, object] = {
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": self.worker_instance_id,
            "request_id": request.request_id,
            "sequence": request.sequence,
            "generation": request.generation,
            "event_index": event_index,
            "event": event,
            "terminal": terminal,
            "payload": payload or {},
        }
        if error is not None:
            envelope["error"] = {
                "code": error.code,
                "message": str(error),
                "retryable": error.retryable,
                "details": getattr(error, "details", {}),
            }
        self.writer.write(envelope)

    def protocol_frame_error(self, message: str) -> None:
        self.errors_total += 1
        self.writer.write({
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": self.worker_instance_id,
            "request_id": "unattributed",
            "sequence": 0,
            "generation": self.current_generation,
            "event_index": 0,
            "event": "error",
            "terminal": True,
            "payload": {},
            "error": {"code": "invalid_frame", "message": message[:256], "retryable": False, "details": {}},
        })

    def _remember(self, request: Request) -> None:
        if request.request_id in self.seen_ids:
            raise ProtocolError("duplicate_request", "request id has already been used")
        if request.sequence <= self.last_sequence:
            raise ProtocolError("out_of_order_sequence", "request sequence must increase monotonically")
        self.seen_ids.add(request.request_id)
        self.seen_order.append(request.request_id)
        while len(self.seen_order) > 4096:
            self.seen_ids.discard(self.seen_order.popleft())
        self.last_sequence = request.sequence

    def _validate_order(self, request: Request) -> None:
        self._remember(request)
        if request.deadline_unix_ms and int(time.time() * 1000) >= request.deadline_unix_ms:
            raise ProtocolError("deadline_exceeded", "request deadline has passed", retryable=True)
        if self.lifecycle == "starting":
            if request.operation != "handshake":
                raise ProtocolError("handshake_required", "handshake must be the first operation")
        elif request.worker_instance_id != self.worker_instance_id:
            raise ProtocolError("instance_mismatch", "worker instance binding does not match")
        if request.operation != "cancel":
            if request.generation < self.current_generation:
                raise ProtocolError("stale_generation", "request belongs to a cancelled generation")
            if request.generation > self.current_generation:
                raise ProtocolError("generation_gap", "only cancellation may advance a generation")

    def handle(self, raw: Any) -> None:
        request: Request | None = None
        try:
            request = Request.parse(raw)
            with self._lock:
                self.requests_total += 1
                self._validate_order(request)
            getattr(self, f"_handle_{request.operation}")(request)
        except (ProtocolError, SchedulerError, SpecError, BackendError) as exc:
            self.errors_total += 1
            if request is None:
                self.protocol_frame_error(str(exc))
            else:
                error = exc if isinstance(exc, (ProtocolError, SchedulerError)) else ProtocolError(getattr(exc, "code", "invalid_payload"), str(exc))
                self._event(request, "error", terminal=True, error=error)
        except Exception:
            self.errors_total += 1
            if request is not None:
                self._event(request, "error", terminal=True, error=ProtocolError("internal_error", "identity worker failed safely"))

    def _descriptor(self) -> dict[str, object]:
        return {
            "worker_id": self.worker_instance_id,
            "kind": "vision",
            "engine": "opencv-yunet-sface-private-evaluation",
            "pack_id": PACK_ID,
            "pack_revision": PACK_REVISION,
            "operations": sorted(OPERATIONS),
            "compute_backends": ["cpu"],
            "network_access": False,
            "queue_depth": 1,
            "cancellation": "generation_barrier_or_process_exit",
            "output_authority": "untrusted_observations_native_revalidation_required",
            "character_selection": False,
            "demographic_inference": False,
            "activation_modes": ["private_evaluation", "qualified_catalog"],
            "current_manifest_admission": "blocked_pending_measurement",
            "model": model_payload(),
            "detector": {"detector_id": DETECTOR_ID, "revision": DETECTOR_REVISION, "preprocessing": PREPROCESSING},
        }

    def _handle_handshake(self, request: Request) -> None:
        if request.worker_instance_id not in {"", self.worker_instance_id} or set(request.payload) != {"launch_nonce", "supervisor"}:
            raise ProtocolError("authentication_failed", "handshake identity is invalid")
        supplied_nonce = request.payload.get("launch_nonce")
        if (
            not isinstance(supplied_nonce, str)
            or not supplied_nonce.isascii()
            or not hmac.compare_digest(supplied_nonce, self.launch_nonce)
            or request.payload.get("supervisor") != "npc-runtime"
        ):
            raise ProtocolError("authentication_failed", "launch nonce or supervisor did not match")
        self.launch_nonce = ""
        self.lifecycle = "cold"
        self._event(request, "completed", self._descriptor(), terminal=True)

    def _handle_capabilities(self, request: Request) -> None:
        if request.payload:
            raise ProtocolError("invalid_payload", "capabilities payload must be empty")
        self._event(request, "completed", self._descriptor(), terminal=True)

    def _handle_health(self, request: Request) -> None:
        if request.payload:
            raise ProtocolError("invalid_payload", "health payload must be empty")
        self._event(request, "completed", {
            "lifecycle": self.lifecycle,
            "generation": self.current_generation,
            "busy": bool(self.scheduler and self.scheduler.busy),
            "loaded_lease_id": self.loaded_lease_id,
            "activation_mode": self.loaded_activation_mode,
            "requests_total": self.requests_total,
            "errors_total": self.errors_total,
            "completed_inferences": self.completed_inferences,
            "resources": {
                "resident_ram_bytes": None,
                "p99_operation_millis": None,
                "resident_vram_bytes": 0 if self.lifecycle == "loaded" else None,
                "measurement": "this_pc_qualification_required",
            },
        }, terminal=True)

    def _handle_load(self, request: Request) -> None:
        required = {
            "lease_id", "pack_id", "revision", "manifest_sha256",
            "artifact_root", "backend", "cpu_threads",
            "explicit_user_confirmation", "activation_mode",
            "verified_catalog_admission_sha256",
        }
        if set(request.payload) != required:
            raise ProtocolError("invalid_payload", "load payload fields are invalid")
        if request.payload.get("pack_id") != PACK_ID or request.payload.get("revision") != PACK_REVISION or request.payload.get("manifest_sha256") != self.pack.manifest_sha256:
            raise ProtocolError("invalid_payload", "load identity differs from the reviewed manifest")
        activation_mode = request.payload.get("activation_mode")
        admission_digest = request.payload.get("verified_catalog_admission_sha256")
        if request.payload.get("backend") != "cpu" or request.payload.get("explicit_user_confirmation") is not True:
            raise ProtocolError("explicit_activation_confirmation_required", "identity model load requires explicit user confirmation")
        if activation_mode == "private_evaluation":
            if admission_digest is not None:
                raise ProtocolError("invalid_payload", "private evaluation cannot claim a catalog admission")
        elif activation_mode == "qualified_catalog":
            try:
                sha256(admission_digest, "verified catalog admission digest")
            except SpecError as exc:
                raise ProtocolError("verified_catalog_admission_required", str(exc)) from exc
        else:
            raise ProtocolError("invalid_payload", "identity activation mode is unsupported")
        lease_id = opaque(request.payload.get("lease_id"), "model lease id")
        cpu_threads = uint(request.payload.get("cpu_threads"), "CPU threads", minimum=1, maximum=4)
        root_raw = request.payload.get("artifact_root")
        if not isinstance(root_raw, str) or not root_raw or "\0" in root_raw:
            raise ProtocolError("invalid_payload", "artifact root is invalid")
        supplied_root = Path(root_raw)
        if supplied_root.is_symlink():
            raise ProtocolError("invalid_payload", "artifact root must not be a symbolic link")
        try:
            root = supplied_root.resolve(strict=True)
        except (OSError, RuntimeError) as exc:
            raise ProtocolError("invalid_payload", "artifact root is unavailable") from exc
        if not root.is_dir() or root.parts[-2:] != (PACK_ID, PACK_REVISION):
            raise ProtocolError("invalid_payload", "artifact root is not the exact Model Manager pack path")
        if self.scheduler is not None and not self.scheduler.drain(0):
            raise ProtocolError("worker_busy", "cancel inference before loading a model", retryable=True)
        if (
            self.lifecycle == "loaded"
            and self.loaded_lease_id == lease_id
            and self.loaded_artifact_root == root
            and self.loaded_activation_mode == activation_mode
            and self.loaded_catalog_admission_sha256 == admission_digest
        ):
            self._event(request, "completed", {"lifecycle": "loaded", "idempotent": True}, terminal=True)
            return
        if self.lifecycle == "loaded":
            raise ProtocolError("worker_busy", "unload the active model before replacement")
        self.backend.load(root, self.pack, cpu_threads=cpu_threads)
        self.scheduler = IdentityScheduler(self.backend)
        self.loaded_lease_id = lease_id
        self.loaded_artifact_root = root
        self.loaded_activation_mode = activation_mode
        self.loaded_catalog_admission_sha256 = admission_digest
        self.lifecycle = "loaded"
        self._event(request, "completed", {
            "lifecycle": "loaded",
            "idempotent": False,
            "activation_mode": activation_mode,
            "verified_catalog_admission_sha256": admission_digest,
        }, terminal=True)

    def _handle_unload(self, request: Request) -> None:
        if request.payload not in ({}, {"lease_id": self.loaded_lease_id}):
            raise ProtocolError("invalid_payload", "unload lease does not match")
        if self.scheduler is not None and not self.scheduler.stop():
            self.lifecycle = "faulted"
            self.should_exit.set()
            raise ProtocolError("cancellation_timeout", "identity inference did not stop; worker will exit")
        self.scheduler = None
        self.backend.unload()
        self.loaded_lease_id = None
        self.loaded_artifact_root = None
        self.loaded_activation_mode = None
        self.loaded_catalog_admission_sha256 = None
        self.lifecycle = "cold"
        self._event(request, "completed", {"lifecycle": "cold"}, terminal=True)

    def _frame_result(self, frame: WgcFrameRequest, cancelled: threading.Event) -> dict[str, object]:
        faces = self.backend.infer(frame.pixel_lease, frame_sequence=frame.frame_sequence, cancelled=cancelled)
        return {
            "contract_version": OBSERVATION_CONTRACT_VERSION,
            "authority": "untrusted_worker_observations",
            "native_revalidation_required": True,
            "schema_version": 1,
            "target": frame.target.payload(),
            "frame_sequence": frame.frame_sequence,
            "device_generation": frame.device_generation,
            "geometry_epoch": frame.geometry_epoch,
            "source_frame_qpc": frame.source_frame_qpc,
            "qpc_frequency": frame.qpc_frequency,
            "captured_at_ms": frame.captured_at_ms,
            "content_sha256": frame.content_sha256,
            "advancing_frame_verified": frame.advancing_frame_verified,
            "overlay_capture_excluded": frame.overlay_capture_excluded,
            "protected_online_detected": frame.protected_online_detected,
            "anti_cheat_detected": frame.anti_cheat_detected,
            "observations": [observation_payload(face, frame_sequence=frame.frame_sequence, content_sha256=frame.content_sha256) for face in faces],
        }

    def _reference_result(self, reference: ReferenceImportRequest, cancelled: threading.Event) -> dict[str, object]:
        faces = self.backend.infer(reference.pixel_lease, frame_sequence=1, cancelled=cancelled)
        if len(faces) != 1:
            raise BackendError("reference_face_ambiguous", "reference import requires exactly one detected face")
        face = faces[0]
        observation = observation_payload(face, frame_sequence=1, content_sha256=reference.source_content_sha256)
        values = face.embedding
        tensor = tensor_bytes(values)
        return {
            "contract_version": "npc.portable-reference-import-result/v1",
            "portable_reference_import": {
                "schema_version": PORTABLE_REFERENCE_SCHEMA_VERSION,
                "provenance": {
                    "game_profile_id": reference.game_profile_id,
                    "subject_id": reference.subject_id,
                    "reference_id": reference.reference_id,
                    "source_class": reference.source_class,
                    "source_content_sha256": reference.source_content_sha256,
                    "owner_user_id": reference.owner_user_id,
                    "original_work_license": reference.original_work_license,
                    "explicit_user_consent": reference.explicit_user_consent,
                    "local_only": reference.local_only,
                    "imported_at_ms": reference.imported_at_ms,
                },
                "subject_display_name": reference.subject_display_name,
                "model": model_payload(),
                "metadata": observation["embedding"]["metadata"],
                "transport": "f32_le",
                "tensor_sha256": hashlib.sha256(tensor).hexdigest(),
                "tensor_f32le": list(tensor),
            },
        }

    def _handle_infer(self, request: Request) -> None:
        if self.lifecycle != "loaded" or self.scheduler is None:
            raise ProtocolError("model_not_loaded", "identity pack must be loaded before inference")
        mode = request.payload.get("mode")
        if mode == "wgc_frame":
            parsed = WgcFrameRequest.parse(request.payload)
            execute = lambda cancelled: self._frame_result(parsed, cancelled)
        elif mode == "reference_import":
            parsed = ReferenceImportRequest.parse(request.payload)
            execute = lambda cancelled: self._reference_result(parsed, cancelled)
        else:
            raise ProtocolError("invalid_payload", "identity infer mode is unsupported")
        accepted = threading.Event()

        def completed(payload: dict[str, object]) -> None:
            accepted.wait()
            self.completed_inferences += 1
            self._event(request, "identity_observations", payload, terminal=False, event_index=1)
            self._event(request, "completed", {"result_contract": payload["contract_version"]}, terminal=True, event_index=2)

        def failed(error: SchedulerError) -> None:
            accepted.wait()
            self._event(request, "error", terminal=True, event_index=1, error=error)

        try:
            self.scheduler.submit(Job(request.request_id, request.generation, request.deadline_unix_ms, execute, completed, failed))
            self._event(request, "accepted", {"queue_depth": 1, "mode": mode}, terminal=False, event_index=0)
        finally:
            accepted.set()

    def _handle_cancel(self, request: Request) -> None:
        if set(request.payload) != {"cancellation_generation"}:
            raise ProtocolError("invalid_payload", "cancel payload fields are invalid")
        requested = uint(request.payload.get("cancellation_generation"), "cancellation generation")
        if requested != request.generation:
            raise ProtocolError("invalid_payload", "cancel envelope and payload generations differ")
        if requested == self.current_generation:
            self._event(request, "completed", {"generation": requested, "idempotent": True}, terminal=True)
            return
        if requested != self.current_generation + 1:
            raise ProtocolError("generation_gap", "cancellation generation must advance by one")
        if self.scheduler is not None:
            self.scheduler.cancel_to(requested)
        self.current_generation = requested
        if self.scheduler is not None and not self.scheduler.wait_generation_barrier(requested, 2.0):
            self.lifecycle = "faulted"
            self.should_exit.set()
            raise ProtocolError("cancellation_timeout", "OpenCV inference missed the barrier; worker will exit")
        self._event(request, "completed", {"generation": requested, "idempotent": False}, terminal=True)

    def _handle_shutdown(self, request: Request) -> None:
        if request.payload:
            raise ProtocolError("invalid_payload", "shutdown payload must be empty")
        if self.scheduler is not None and not self.scheduler.stop():
            self.lifecycle = "faulted"
            self.should_exit.set()
            raise ProtocolError("cancellation_timeout", "identity inference did not stop; worker will exit")
        self.scheduler = None
        self.backend.unload()
        self.loaded_lease_id = None
        self.loaded_artifact_root = None
        self.loaded_activation_mode = None
        self.loaded_catalog_admission_sha256 = None
        self.lifecycle = "stopped"
        self._event(request, "completed", {"lifecycle": "stopped"}, terminal=True)
        self.should_exit.set()
