"""Strict supervisor protocol and lifecycle coordinator."""

from __future__ import annotations

import re
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from backend import Backend, BackendError, OnnxBgeBackend
from framing import FrameWriter
from model_spec import (
    DIMENSIONS,
    MAX_BATCH_ITEMS,
    MAX_FRAME_BYTES,
    MODEL_ID,
    PACK_ID,
    PACK_REVISION,
    SOURCE_REVISION,
    EmbeddingRequest,
    PackSpec,
    SpecError,
)
from pack_manager import PackLifecycle
from scheduler import EmbeddingScheduler, ScheduledRequest, SchedulerError
from telemetry import Telemetry

PROTOCOL_VERSION = "1.0"
OPERATIONS = frozenset({"handshake", "capabilities", "health", "warm", "load", "unload", "infer", "cancel", "shutdown"})
_OPAQUE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,126}[A-Za-z0-9]$")


class ProtocolError(RuntimeError):
    def __init__(self, code: str, message: str, *, retryable: bool = False, details: dict[str, object] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.retryable = retryable
        self.details = details or {}


def _uint(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= (2**63 - 1):
        raise ProtocolError("invalid_request", f"{name} must be a non-negative integer")
    return value


def _opaque(value: Any, name: str) -> str:
    if not isinstance(value, str) or not _OPAQUE_ID.fullmatch(value):
        raise ProtocolError("invalid_request", f"{name} must be an opaque identifier")
    return value


@dataclass(frozen=True, slots=True)
class Request:
    protocol_version: str
    worker_instance_id: str
    request_id: str
    sequence: int
    generation: int
    deadline_unix_ms: int
    operation: str
    payload: dict[str, Any]

    @classmethod
    def parse(cls, raw: Any) -> "Request":
        if not isinstance(raw, dict):
            raise ProtocolError("invalid_request", "request root must be an object")
        required = {
            "protocol_version",
            "worker_instance_id",
            "request_id",
            "sequence",
            "generation",
            "deadline_unix_ms",
            "operation",
            "payload",
        }
        if set(raw) != required:
            raise ProtocolError("invalid_request", "request contains missing or unknown envelope fields")
        version = raw.get("protocol_version")
        if version != PROTOCOL_VERSION:
            raise ProtocolError("unsupported_protocol", "worker protocol version is unsupported")
        instance = raw.get("worker_instance_id")
        if not isinstance(instance, str) or len(instance.encode("utf-8")) > 128:
            raise ProtocolError("invalid_request", "worker_instance_id is invalid")
        operation = raw.get("operation")
        if operation not in OPERATIONS:
            raise ProtocolError("unsupported_operation", "worker operation is unsupported")
        payload = raw.get("payload")
        if not isinstance(payload, dict):
            raise ProtocolError("invalid_request", "request payload must be an object")
        return cls(
            version,
            instance,
            _opaque(raw.get("request_id"), "request_id"),
            _uint(raw.get("sequence"), "sequence"),
            _uint(raw.get("generation"), "generation"),
            _uint(raw.get("deadline_unix_ms"), "deadline_unix_ms"),
            operation,
            payload,
        )


class WorkerController:
    def __init__(
        self,
        writer: FrameWriter,
        *,
        launch_nonce: str,
        worker_instance_id: str,
        manifest_path: Path,
        backend: Backend | None = None,
    ) -> None:
        self.writer = writer
        self.launch_nonce = launch_nonce
        self.worker_instance_id = worker_instance_id
        self.pack = PackSpec.load(manifest_path)
        self.backend = backend or OnnxBgeBackend()
        self.telemetry = Telemetry()
        self.scheduler: EmbeddingScheduler | None = None
        self.lifecycle = "starting"
        self.warmed = False
        self.loaded_lease_id: str | None = None
        self.loaded_install_dir: Path | None = None
        self.current_generation = 0
        self.last_sequence = -1
        self.seen_ids: set[str] = set()
        self.seen_order: deque[str] = deque()
        self.lock = threading.RLock()
        self.should_exit = threading.Event()
        self.requests_total = 0
        self.errors_total = 0

    def _event(
        self,
        request: Request,
        event: str,
        payload: dict[str, object] | None = None,
        *,
        terminal: bool,
        event_index: int = 0,
        error: ProtocolError | SchedulerError | None = None,
    ) -> None:
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
        self.writer.write(
            {
                "protocol_version": PROTOCOL_VERSION,
                "worker_instance_id": self.worker_instance_id,
                "request_id": "unattributed",
                "sequence": 0,
                "generation": self.current_generation,
                "event_index": 0,
                "event": "error",
                "terminal": True,
                "payload": {},
                "error": {
                    "code": "invalid_frame",
                    "message": message[:256] or "invalid protocol frame",
                    "retryable": False,
                    "details": {},
                },
            }
        )

    def _remember(self, request: Request) -> None:
        if request.request_id in self.seen_ids:
            raise ProtocolError("duplicate_request", "request_id has already been used")
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
        else:
            if request.worker_instance_id != self.worker_instance_id:
                raise ProtocolError("instance_mismatch", "worker instance binding does not match")
            if request.operation != "cancel":
                if request.generation < self.current_generation:
                    raise ProtocolError("stale_generation", "request belongs to a cancelled generation")
                if request.generation > self.current_generation:
                    raise ProtocolError("generation_gap", "request skipped the cancellation barrier")

    def handle(self, raw: dict[str, Any]) -> None:
        request: Request | None = None
        try:
            request = Request.parse(raw)
            with self.lock:
                self.requests_total += 1
                self._validate_order(request)
            handler = getattr(self, f"_handle_{request.operation}")
            handler(request)
        except (ProtocolError, SchedulerError, SpecError, BackendError) as exc:
            self.errors_total += 1
            if request is not None:
                if isinstance(exc, (SpecError, BackendError)):
                    error = ProtocolError(getattr(exc, "code", "invalid_payload"), str(exc))
                else:
                    error = exc
                self._event(request, "error", terminal=True, error=error)
            else:
                self.protocol_frame_error(str(exc))
        except Exception:
            self.errors_total += 1
            if request is not None:
                self._event(
                    request,
                    "error",
                    terminal=True,
                    error=ProtocolError("internal_error", "local embedding worker failed safely"),
                )

    def _handle_handshake(self, request: Request) -> None:
        if request.worker_instance_id not in {"", self.worker_instance_id}:
            raise ProtocolError("instance_mismatch", "handshake worker instance id is invalid")
        if set(request.payload) != {"launch_nonce", "supervisor"}:
            raise ProtocolError("invalid_payload", "handshake payload fields are invalid")
        if request.payload.get("launch_nonce") != self.launch_nonce:
            raise ProtocolError("authentication_failed", "launch nonce did not match")
        if request.payload.get("supervisor") != "npc-runtime":
            raise ProtocolError("authentication_failed", "worker may only bind to the runtime supervisor")
        self.lifecycle = "cold"
        self._event(request, "completed", self._descriptor(), terminal=True)

    def _descriptor(self) -> dict[str, object]:
        return {
            "worker_id": self.worker_instance_id,
            "kind": "embedding",
            "engine": "onnxruntime-bge-bert-cls",
            "pack_id": PACK_ID,
            "pack_revision": PACK_REVISION,
            "model_id": MODEL_ID,
            "model_revision": SOURCE_REVISION,
            "capabilities": {
                "input_modalities": ["text_batch"],
                "output_modalities": ["normalized_embedding_batch"],
                "compute_backends": ["cpu"],
                "streaming_output": False,
                "cancellation": True,
                "network_access": False,
                "hidden_batching": True,
                "background_priority_only": True,
                "maximum_batch_entries": MAX_BATCH_ITEMS,
                "dimensions": DIMENSIONS,
            },
            "limits": {"maximum_frame_bytes": MAX_FRAME_BYTES, "maximum_dimensions": 4096},
        }

    def _handle_capabilities(self, request: Request) -> None:
        if request.payload:
            raise ProtocolError("invalid_payload", "capabilities payload must be empty")
        self._event(request, "completed", self._descriptor(), terminal=True)

    def _handle_health(self, request: Request) -> None:
        if request.payload:
            raise ProtocolError("invalid_payload", "health payload must be empty")
        queue_requests, queue_items = self.scheduler.queue_depth if self.scheduler else (0, 0)
        self._event(
            request,
            "completed",
            {
                "lifecycle": self.lifecycle,
                "warmed": self.warmed,
                "loaded_model_id": MODEL_ID if self.lifecycle == "loaded" else None,
                "loaded_lease_id": self.loaded_lease_id,
                "active_generation": self.current_generation,
                "queue": {"requests": queue_requests, "items": queue_items},
                "requests_total": self.requests_total,
                "errors_total": self.errors_total,
                "resources": self.telemetry.report(backend=self.backend.backend_id if self.lifecycle == "loaded" else None, loaded=self.lifecycle == "loaded"),
            },
            terminal=True,
        )

    def _handle_warm(self, request: Request) -> None:
        if request.payload not in ({}, {"runtime_only": True}):
            raise ProtocolError("invalid_payload", "warm only accepts runtime_only=true")
        if self.lifecycle not in {"cold", "warm", "loaded"}:
            raise ProtocolError("invalid_state", "worker cannot warm from its current lifecycle")
        self.warmed = True
        if self.lifecycle == "cold":
            self.lifecycle = "warm"
        self._event(request, "completed", {"lifecycle": self.lifecycle, "warmed": True}, terminal=True)

    def _handle_load(self, request: Request) -> None:
        allowed = {"lease_id", "pack_id", "revision", "artifact_root", "manifest_sha256", "backend", "cpu_threads"}
        if set(request.payload) != allowed:
            raise ProtocolError("invalid_payload", "load payload fields are invalid")
        lease_id = _opaque(request.payload.get("lease_id"), "lease_id")
        if request.payload.get("pack_id") != PACK_ID or request.payload.get("revision") != PACK_REVISION:
            raise ProtocolError("invalid_payload", "load pack identity is incompatible")
        if request.payload.get("manifest_sha256") != self.pack.manifest_sha256:
            raise ProtocolError("invalid_payload", "load manifest digest differs from the reviewed worker manifest")
        if request.payload.get("backend") != "cpu":
            raise ProtocolError("unsupported_backend", "this measured pack revision is CPU-only")
        cpu_threads = request.payload.get("cpu_threads")
        if isinstance(cpu_threads, bool) or not isinstance(cpu_threads, int) or not 1 <= cpu_threads <= 8:
            raise ProtocolError("invalid_payload", "cpu_threads must be in 1..=8")
        root_raw = request.payload.get("artifact_root")
        if not isinstance(root_raw, str) or not root_raw or "\x00" in root_raw:
            raise ProtocolError("invalid_payload", "artifact_root is invalid")
        install_dir = Path(root_raw).resolve()
        if install_dir.is_symlink() or not install_dir.is_dir():
            raise ProtocolError("invalid_payload", "artifact_root must be a real installed directory")
        expected_suffix = Path(PACK_ID) / PACK_REVISION
        if install_dir.parts[-2:] != expected_suffix.parts:
            raise ProtocolError("invalid_payload", "artifact_root is not bound to the requested pack revision")
        if self.lifecycle == "loaded" and self.loaded_lease_id == lease_id and self.loaded_install_dir == install_dir:
            self._event(request, "completed", {"lifecycle": "loaded", "idempotent": True}, terminal=True)
            return
        if self.lifecycle == "loaded" or (self.scheduler is not None and not self.scheduler.drain(0.0)):
            raise ProtocolError("worker_busy", "cancel or unload the current model before replacement", retryable=True)
        lifecycle = PackLifecycle(install_dir.parents[1], self.pack)
        if lifecycle.target != install_dir:
            raise ProtocolError("invalid_payload", "artifact_root does not match the exact Model Manager install target")
        report = lifecycle.verify()
        if not report.healthy:
            raise ProtocolError("pack_verification_failed", "model pack failed verification; repair before activation")
        started = time.monotonic_ns()
        self.backend.load(install_dir, self.pack, cpu_threads=cpu_threads)
        self.telemetry.record_load(started)
        try:
            self_test = self.backend.self_test()
        except Exception:
            self.backend.unload()
            raise
        self.scheduler = EmbeddingScheduler(self.backend, self.telemetry)
        self.loaded_lease_id = lease_id
        self.loaded_install_dir = install_dir
        self.lifecycle = "loaded"
        self.warmed = True
        self._event(
            request,
            "completed",
            {
                "lifecycle": "loaded",
                "idempotent": False,
                "self_test": self_test,
                "resources": self.telemetry.report(backend=self.backend.backend_id, loaded=True),
            },
            terminal=True,
        )

    def _handle_unload(self, request: Request) -> None:
        if request.payload not in ({}, {"lease_id": self.loaded_lease_id}):
            raise ProtocolError("invalid_payload", "unload lease does not match the active model")
        if self.scheduler is not None:
            if not self.scheduler.stop():
                self.lifecycle = "faulted"
                self.should_exit.set()
                raise ProtocolError(
                    "cancellation_timeout",
                    "embedding backend did not stop before unload; worker will exit",
                )
            self.scheduler = None
        self.backend.unload()
        self.loaded_lease_id = None
        self.loaded_install_dir = None
        self.lifecycle = "warm" if self.warmed else "cold"
        self._event(request, "completed", {"lifecycle": self.lifecycle}, terminal=True)

    def _handle_infer(self, request: Request) -> None:
        if self.lifecycle != "loaded" or self.scheduler is None:
            raise ProtocolError("model_not_loaded", "embedding model must be loaded before inference")
        embedding = EmbeddingRequest.parse(request.payload)
        accepted = threading.Event()

        def completed(payload: dict[str, object]) -> None:
            accepted.wait()
            self._event(request, "embedding_batch", payload, terminal=False, event_index=1)
            self._event(request, "completed", {"item_count": len(embedding.items)}, terminal=True, event_index=2)

        def failed(error: SchedulerError) -> None:
            accepted.wait()
            self._event(request, "error", terminal=True, event_index=1, error=error)

        try:
            self.scheduler.submit(
                ScheduledRequest(
                    request.request_id,
                    request.generation,
                    request.deadline_unix_ms,
                    embedding,
                    completed,
                    failed,
                )
            )
            self._event(
                request,
                "accepted",
                {"queued_items": len(embedding.items), "priority": "background"},
                terminal=False,
                event_index=0,
            )
        finally:
            accepted.set()

    def _handle_cancel(self, request: Request) -> None:
        if set(request.payload) != {"cancellation_generation"}:
            raise ProtocolError("invalid_payload", "cancel payload fields are invalid")
        requested = _uint(request.payload.get("cancellation_generation"), "cancellation_generation")
        if request.generation != requested:
            raise ProtocolError("invalid_payload", "cancel envelope and payload generations must match")
        if requested == self.current_generation:
            self._event(request, "completed", {"generation": requested, "idempotent": True}, terminal=True)
            return
        if requested != self.current_generation + 1:
            raise ProtocolError("generation_gap", "cancel generation must advance by exactly one")
        if self.scheduler is not None:
            self.scheduler.cancel_to(requested)
        self.current_generation = requested
        if self.scheduler is not None and not self.scheduler.wait_generation_barrier(requested, 2.0):
            self.lifecycle = "faulted"
            self.should_exit.set()
            raise ProtocolError(
                "cancellation_timeout",
                "embedding backend did not reach the cancellation barrier; worker will exit",
            )
        self._event(request, "completed", {"generation": requested, "idempotent": False}, terminal=True)

    def _handle_shutdown(self, request: Request) -> None:
        if request.payload:
            raise ProtocolError("invalid_payload", "shutdown payload must be empty")
        if self.scheduler is not None:
            if not self.scheduler.stop():
                self.lifecycle = "faulted"
                self.should_exit.set()
                raise ProtocolError(
                    "cancellation_timeout",
                    "embedding backend did not stop before shutdown; worker will exit",
                )
            self.scheduler = None
        self.backend.unload()
        self.loaded_lease_id = None
        self.loaded_install_dir = None
        self.lifecycle = "stopped"
        self._event(request, "completed", {"lifecycle": "stopped"}, terminal=True)
        self.should_exit.set()
