#!/usr/bin/env python3
"""Runnable deterministic Worker Control v1 process.

This process is a protocol fixture. It never imports or imitates a third-party
inference runtime and it installs an audit hook that rejects socket operations.
"""

from __future__ import annotations

import argparse
import os
import sys
import threading
import time
import traceback
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, BinaryIO

try:  # Supports `python worker.py` and `python -m workers.stubs.worker`.
    from .contract import (
        ContractError,
        Descriptor,
        MAX_EVENTS_PER_REQUEST,
        PROTOCOL_VERSION,
        Request,
        validate_generation,
        validate_text,
    )
    from .engines import ENGINES
    from .framing import EndOfStream, FrameWriter, FramingError, read_frame
except ImportError:
    from contract import (
        ContractError,
        Descriptor,
        MAX_EVENTS_PER_REQUEST,
        PROTOCOL_VERSION,
        Request,
        validate_generation,
        validate_text,
    )
    from engines import ENGINES
    from framing import EndOfStream, FrameWriter, FramingError, read_frame


def install_network_denial() -> None:
    """Deny socket creation/connects as defense in depth for development stubs."""

    def deny_socket_events(event: str, _arguments: tuple[Any, ...]) -> None:
        if event == "socket.__new__" or event.startswith("socket.connect") or event.startswith("socket.getaddrinfo"):
            raise PermissionError("worker network access is disabled")

    sys.addaudithook(deny_socket_events)


@dataclass(slots=True)
class Job:
    request: Request
    cancelled: threading.Event = field(default_factory=threading.Event)


class WorkerController:
    def __init__(self, descriptor: Descriptor, writer: FrameWriter, launch_nonce: str) -> None:
        self.descriptor = descriptor
        self.writer = writer
        self.launch_nonce = launch_nonce
        self.lock = threading.RLock()
        self.lifecycle = "starting"
        self.warmed = False
        self.loaded_model_id: str | None = None
        self.loaded_lease_id: str | None = None
        self.current_generation = 0
        self.last_sequence = -1
        self.seen_ids: set[str] = set()
        self.seen_order: deque[str] = deque()
        self.jobs: dict[str, Job] = {}
        self.requests_total = 0
        self.inferences_started = 0
        self.inferences_completed = 0
        self.inferences_cancelled = 0
        self.errors_total = 0
        self.should_exit = threading.Event()

    def _event(
        self,
        request: Request,
        event: str,
        payload: dict[str, Any] | None = None,
        *,
        terminal: bool,
        event_index: int = 0,
        error: ContractError | None = None,
    ) -> None:
        envelope: dict[str, Any] = {
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": self.descriptor.worker_id,
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
                "message": error.message,
                "retryable": error.retryable,
                "details": error.details,
            }
        self.writer.write(envelope)

    def protocol_error(self, error: Exception) -> None:
        self.errors_total += 1
        message = str(error)[:256] or "invalid protocol frame"
        envelope = {
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": self.descriptor.worker_id,
            "request_id": "unattributed",
            "sequence": 0,
            "generation": self.current_generation,
            "event_index": 0,
            "event": "error",
            "terminal": True,
            "payload": {},
            "error": {
                "code": "invalid_frame",
                "message": message,
                "retryable": False,
                "details": {},
            },
        }
        self.writer.write(envelope)

    @staticmethod
    def _best_effort_identity(raw: dict[str, Any]) -> Request | None:
        """Recover safe correlation fields when full request parsing fails."""

        request_id = raw.get("request_id")
        sequence = raw.get("sequence")
        generation = raw.get("generation")
        operation = raw.get("operation")
        instance_id = raw.get("worker_instance_id", "")
        if (
            not isinstance(request_id, str)
            or not request_id
            or len(request_id.encode("utf-8")) > 128
            or isinstance(sequence, bool)
            or not isinstance(sequence, int)
            or sequence < 0
            or isinstance(generation, bool)
            or not isinstance(generation, int)
            or generation < 0
            or not isinstance(operation, str)
            or not isinstance(instance_id, str)
        ):
            return None
        return Request(
            protocol_version=PROTOCOL_VERSION,
            worker_instance_id=instance_id[:128],
            request_id=request_id,
            sequence=sequence,
            generation=generation,
            deadline_unix_ms=0,
            operation=operation[:32],
            payload={},
        )

    def _remember_request(self, request: Request) -> None:
        if request.request_id in self.seen_ids:
            raise ContractError("duplicate_request", "request_id has already been used")
        if request.sequence <= self.last_sequence:
            raise ContractError(
                "out_of_order_sequence",
                "sequence must increase monotonically",
                details={"minimum_sequence": self.last_sequence + 1},
            )
        self.last_sequence = request.sequence
        self.seen_ids.add(request.request_id)
        self.seen_order.append(request.request_id)
        while len(self.seen_order) > 4_096:
            expired = self.seen_order.popleft()
            self.seen_ids.discard(expired)

    def _validate_generation(self, request: Request) -> None:
        if request.operation == "handshake" and self.lifecycle == "starting":
            if request.generation != 0:
                raise ContractError("generation_gap", "initial handshake generation must be zero")
            return
        if request.operation == "cancel":
            if request.generation < self.current_generation:
                raise ContractError(
                    "stale_generation",
                    "cancellation generation is stale",
                    details={"current_generation": self.current_generation},
                )
            if request.generation > self.current_generation + 1:
                raise ContractError(
                    "generation_gap",
                    "cancel may advance generation by exactly one",
                    details={"current_generation": self.current_generation},
                )
            return
        if request.generation < self.current_generation:
            raise ContractError(
                "stale_generation",
                "request generation is stale",
                details={"current_generation": self.current_generation},
            )
        if request.generation > self.current_generation:
            raise ContractError(
                "generation_gap",
                "only cancel may advance generation",
                details={"current_generation": self.current_generation},
            )

    def handle(self, raw: dict[str, Any]) -> None:
        request: Request | None = None
        try:
            request = Request.parse(raw)
            with self.lock:
                self._remember_request(request)
                self.requests_total += 1
                if self.lifecycle == "starting" and request.operation != "handshake":
                    raise ContractError("handshake_required", "handshake must be the first operation")
                if self.lifecycle != "starting" and request.worker_instance_id != self.descriptor.worker_id:
                    raise ContractError("instance_mismatch", "worker_instance_id does not match this process")
                if self.lifecycle == "stopped":
                    raise ContractError("worker_stopped", "worker is shutting down")
                self._validate_generation(request)
            operation = getattr(self, f"_handle_{request.operation}")
            operation(request)
        except ContractError as error:
            self.errors_total += 1
            if request is None:
                request = self._best_effort_identity(raw)
            if request is None:
                self.protocol_error(error)
            else:
                self._event(request, "error", terminal=True, error=error)
        except Exception:
            self.errors_total += 1
            if request is not None:
                self._event(
                    request,
                    "error",
                    terminal=True,
                    error=ContractError("internal_error", "worker operation failed", retryable=True),
                )
            traceback.print_exc(file=sys.stderr)

    def _capability_payload(self) -> dict[str, Any]:
        return {
            "worker_id": self.descriptor.worker_id,
            "pack_id": self.descriptor.pack_id,
            "worker_kind": self.descriptor.kind,
            "engine": self.descriptor.engine,
            "operations": sorted(
                {"handshake", "capabilities", "health", "warm", "load", "unload", "infer", "cancel", "shutdown"}
            ),
            **self.descriptor.capabilities,
            "limits": {
                "max_frame_bytes": 1_048_576,
                "max_text_bytes": 262_144,
                "max_inline_binary_bytes": 524_288,
                "max_batch_entries": 256,
                "max_embedding_dimensions": 4_096,
                "max_events_per_request": MAX_EVENTS_PER_REQUEST,
            },
            "resource_estimate": self.descriptor.resources,
            "development_stub": True,
            "third_party_payloads_bundled": False,
            "production_installation_owner": "model_manager",
        }

    def _handle_handshake(self, request: Request) -> None:
        nonce = validate_text(request.payload.get("launch_nonce"), "launch_nonce")
        if nonce != self.launch_nonce:
            raise ContractError("authentication_failed", "launch nonce is invalid")
        with self.lock:
            if self.lifecycle == "starting":
                self.lifecycle = "cold"
            elif self.lifecycle not in {"cold", "warm", "loaded"}:
                raise ContractError("invalid_state", "handshake is not available in the current state")
        self._event(
            request,
            "completed",
            {
                "lifecycle": self.lifecycle,
                "protocol_version": PROTOCOL_VERSION,
                "capabilities": self._capability_payload(),
            },
            terminal=True,
        )

    def _handle_capabilities(self, request: Request) -> None:
        self._event(request, "completed", self._capability_payload(), terminal=True)

    def _handle_health(self, request: Request) -> None:
        with self.lock:
            payload = {
                "status": "ok" if self.lifecycle != "stopped" else "stopping",
                "lifecycle": self.lifecycle,
                "warmed": self.warmed,
                "loaded_model_id": self.loaded_model_id,
                "loaded_lease_id": self.loaded_lease_id,
                "generation": self.current_generation,
                "in_flight": len(self.jobs),
                "counters": {
                    "requests_total": self.requests_total,
                    "inferences_started": self.inferences_started,
                    "inferences_completed": self.inferences_completed,
                    "inferences_cancelled": self.inferences_cancelled,
                    "errors_total": self.errors_total,
                },
                "resource_estimate": self.descriptor.resources,
            }
        self._event(request, "completed", payload, terminal=True)

    def _handle_warm(self, request: Request) -> None:
        with self.lock:
            self.warmed = True
            if self.lifecycle == "cold":
                self.lifecycle = "warm"
            lifecycle = self.lifecycle
        self._event(request, "completed", {"lifecycle": lifecycle, "already_warm": lifecycle == "loaded"}, terminal=True)

    def _handle_load(self, request: Request) -> None:
        model_id = validate_text(request.payload.get("model_id"), "model_id")
        lease_id = validate_text(request.payload.get("lease_id"), "lease_id")
        model_path = request.payload.get("verified_model_path")
        if model_path is not None:
            model_path = validate_text(model_path, "verified_model_path")
            if "\x00" in model_path or not Path(model_path).is_absolute():
                raise ContractError("invalid_model_lease", "verified_model_path must be absolute and contain no NUL")
        with self.lock:
            active = len(self.jobs)
            if active and (model_id != self.loaded_model_id or lease_id != self.loaded_lease_id):
                raise ContractError("worker_busy", "cancel or drain active inference before replacing a model", retryable=True)
            already_loaded = model_id == self.loaded_model_id and lease_id == self.loaded_lease_id
            self.loaded_model_id = model_id
            self.loaded_lease_id = lease_id
            self.warmed = True
            self.lifecycle = "loaded"
        self._event(
            request,
            "completed",
            {"lifecycle": "loaded", "model_id": model_id, "lease_id": lease_id, "already_loaded": already_loaded},
            terminal=True,
        )

    def _handle_unload(self, request: Request) -> None:
        with self.lock:
            for job in self.jobs.values():
                job.cancelled.set()
            self.loaded_model_id = None
            self.loaded_lease_id = None
            self.lifecycle = "warm" if self.warmed else "cold"
            lifecycle = self.lifecycle
        self._event(request, "completed", {"lifecycle": lifecycle}, terminal=True)

    def _handle_infer(self, request: Request) -> None:
        fixture_delay_ms = request.payload.get("fixture_event_delay_ms", 0)
        if (
            isinstance(fixture_delay_ms, bool)
            or not isinstance(fixture_delay_ms, int)
            or not 0 <= fixture_delay_ms <= 1_000
        ):
            raise ContractError("invalid_payload", "fixture_event_delay_ms must be between 0 and 1000")
        if self.descriptor.kind == "lip_sync":
            payload_generation = validate_generation(request.payload.get("cancellation_generation"))
            if payload_generation != request.generation:
                raise ContractError(
                    "generation_mismatch",
                    "lip-sync payload cancellation_generation must match the request envelope",
                    details={"request_generation": request.generation},
                )
        with self.lock:
            if self.lifecycle != "loaded" or self.loaded_model_id is None:
                raise ContractError("model_not_loaded", "load a verified model lease before infer")
            concurrency = int(self.descriptor.resources["concurrent_requests"])
            if len(self.jobs) >= concurrency:
                raise ContractError("worker_busy", "worker concurrency limit reached", retryable=True)
            job = Job(request)
            self.jobs[request.request_id] = job
            self.inferences_started += 1
        self._event(request, "accepted", {"model_id": self.loaded_model_id}, terminal=False, event_index=0)
        thread = threading.Thread(
            target=self._run_inference,
            args=(job,),
            name=f"infer-{request.request_id[:32]}",
            daemon=True,
        )
        thread.start()

    def _run_inference(self, job: Job) -> None:
        request = job.request
        event_index = 1

        def cancelled() -> bool:
            with self.lock:
                return (
                    job.cancelled.is_set()
                    or self.should_exit.is_set()
                    or request.generation != self.current_generation
                    or request.request_id not in self.jobs
                )

        try:
            engine = ENGINES[self.descriptor.kind]
            for event, payload in engine(request.payload, cancelled):
                if cancelled():
                    return
                if event_index >= MAX_EVENTS_PER_REQUEST - 1:
                    raise ContractError("event_limit_exceeded", "inference emitted too many events")
                self._event(request, event, payload, terminal=False, event_index=event_index)
                event_index += 1
                # Yield to the input loop so a cancellation frame can supersede
                # even a deterministic worker producing very small events.
                delay_ms = int(request.payload.get("fixture_event_delay_ms", 0))
                time.sleep(delay_ms / 1_000 if delay_ms else 0)
            if cancelled():
                return
            self._event(request, "completed", {"result": "delivered"}, terminal=True, event_index=event_index)
            with self.lock:
                self.inferences_completed += 1
        except ContractError as error:
            if not cancelled():
                self.errors_total += 1
                self._event(request, "error", terminal=True, event_index=event_index, error=error)
        except Exception:
            if not cancelled():
                self.errors_total += 1
                self._event(
                    request,
                    "error",
                    terminal=True,
                    event_index=event_index,
                    error=ContractError("inference_failed", "deterministic inference failed", retryable=True),
                )
                traceback.print_exc(file=sys.stderr)
        finally:
            with self.lock:
                self.jobs.pop(request.request_id, None)

    def _handle_cancel(self, request: Request) -> None:
        with self.lock:
            previous_generation = self.current_generation
            advanced = request.generation == previous_generation + 1
            if advanced:
                self.current_generation = request.generation
            affected = 0
            for job in self.jobs.values():
                if job.request.generation < self.current_generation and not job.cancelled.is_set():
                    job.cancelled.set()
                    affected += 1
            self.inferences_cancelled += affected
        self._event(
            request,
            "completed",
            {
                "generation": self.current_generation,
                "previous_generation": previous_generation,
                "advanced": advanced,
                "cancelled_requests": affected,
            },
            terminal=True,
        )

    def _handle_shutdown(self, request: Request) -> None:
        with self.lock:
            for job in self.jobs.values():
                job.cancelled.set()
            self.loaded_model_id = None
            self.loaded_lease_id = None
            self.lifecycle = "stopped"
            self.should_exit.set()
        self._event(request, "completed", {"lifecycle": "stopped"}, terminal=True)


def _open_stream(path: str | None, *, reading: bool) -> tuple[BinaryIO, bool]:
    if path is None:
        return (sys.stdin.buffer if reading else sys.stdout.buffer), False
    mode = "rb" if reading else "wb"
    return open(path, mode, buffering=0), True


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Deterministic NPC worker protocol stub")
    parser.add_argument("--descriptor", required=True, help="Path to a development worker-pack descriptor")
    parser.add_argument("--input-pipe", help="Supervisor-created input named-pipe path")
    parser.add_argument("--output-pipe", help="Supervisor-created output named-pipe path")
    parser.add_argument(
        "--launch-nonce",
        default=os.environ.get("NPC_WORKER_LAUNCH_NONCE", "development-only-nonce"),
        help="Per-launch supervisor nonce; production should pass this out-of-band",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        descriptor = Descriptor.load(args.descriptor)
    except ContractError as exc:
        print(f"worker descriptor rejected: {exc.message}", file=sys.stderr)
        return 2
    install_network_denial()
    input_stream, close_input = _open_stream(args.input_pipe, reading=True)
    output_stream, close_output = _open_stream(args.output_pipe, reading=False)
    controller = WorkerController(descriptor, FrameWriter(output_stream), args.launch_nonce)
    try:
        while not controller.should_exit.is_set():
            try:
                raw = read_frame(input_stream)
            except EndOfStream:
                break
            except FramingError as exc:
                controller.protocol_error(exc)
                break
            controller.handle(raw)
    finally:
        controller.should_exit.set()
        with controller.lock:
            for job in controller.jobs.values():
                job.cancelled.set()
        if close_input:
            input_stream.close()
        if close_output:
            output_stream.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
