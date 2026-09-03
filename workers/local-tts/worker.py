#!/usr/bin/env python3
"""Supervised, offline Kokoro/sherpa-onnx PCM worker."""

from __future__ import annotations

import argparse
import collections
import hmac
import os
import re
import sys
import threading
import time
import traceback
from dataclasses import dataclass, field
from pathlib import Path
from typing import BinaryIO

from backend import BackendIdentity, TtsBackend
from manifest import PackManifest, load_manifest
from pack_manager import HttpsDownloader, PackLifecycleError, PackManager
from pcm_transport import PcmTransportError, PcmWriter
from protocol import (
    EndOfStream,
    FrameWriter,
    MAX_SEEN_REQUESTS,
    PROTOCOL_VERSION,
    ProtocolError,
    Request,
    event_envelope,
    read_frame,
    validate_id,
    validate_text,
)
from sherpa_backend import SherpaOnnxBackend
from voices import VOICE_BY_ID, public_voice_records

WORKER_ID_PATTERN = re.compile(r"^[A-Za-z0-9._-]{3,128}$")
SELF_TEST_TEXT = "The lantern is ready."
SELF_TEST_VOICE = "af_heart"


def install_network_denial() -> None:
    """Deny new network activity after the supervisor has verified the pack."""

    def deny(event: str, _arguments: tuple[object, ...]) -> None:
        if (
            event == "socket.__new__"
            or event.startswith("socket.connect")
            or event.startswith("socket.getaddrinfo")
            or event.startswith("urllib.")
        ):
            raise PermissionError("local TTS worker network access is disabled")

    sys.addaudithook(deny)


@dataclass(slots=True)
class SynthesisJob:
    request: Request
    stream_id: str
    cancel: threading.Event = field(default_factory=threading.Event)
    thread: threading.Thread | None = None


class WorkerController:
    def __init__(
        self,
        *,
        manifest: PackManifest,
        pack_root: Path,
        backend: TtsBackend,
        control_writer: FrameWriter,
        pcm_writer: PcmWriter,
        launch_nonce: str,
        worker_instance_id: str,
    ) -> None:
        if not WORKER_ID_PATTERN.fullmatch(worker_instance_id):
            raise ValueError("worker_instance_id is invalid")
        self.manifest = manifest
        self.pack_root = pack_root
        self.backend = backend
        self.control_writer = control_writer
        self.pcm_writer = pcm_writer
        self.launch_nonce = launch_nonce
        self.worker_instance_id = worker_instance_id
        self.lock = threading.RLock()
        self.lifecycle = "starting"
        self.current_generation = 0
        self.last_sequence = -1
        self.seen_request_ids: set[str] = set()
        self.seen_order: collections.deque[str] = collections.deque()
        self.seen_stream_ids: set[str] = set()
        self.jobs: dict[str, SynthesisJob] = {}
        self.backend_identity: BackendIdentity | None = None
        self.loaded_lease_id: str | None = None
        self.load_millis: float | None = None
        self.requests_total = 0
        self.synthesis_started = 0
        self.synthesis_completed = 0
        self.synthesis_cancelled = 0
        self.errors_total = 0
        self.should_exit = threading.Event()

    def _emit(
        self,
        request: Request,
        event: str,
        *,
        terminal: bool,
        event_index: int = 0,
        payload: dict[str, object] | None = None,
        error: ProtocolError | None = None,
    ) -> None:
        self.control_writer.write(
            event_envelope(
                request,
                self.worker_instance_id,
                event,
                terminal=terminal,
                event_index=event_index,
                payload=payload,
                error=error,
            )
        )

    def _error(self, request: Request, error: ProtocolError) -> None:
        self.errors_total += 1
        self._emit(
            request,
            "error",
            terminal=True,
            error=error,
        )

    def protocol_error(self, error: Exception) -> None:
        self.errors_total += 1
        safe = str(error)[:256] or "invalid local TTS protocol frame"
        self.control_writer.write(
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
                    "message": safe,
                    "retryable": False,
                    "details": {},
                },
            }
        )

    def _remember(self, request: Request) -> None:
        if request.request_id in self.seen_request_ids:
            raise ProtocolError(
                "duplicate_request",
                "request_id has already been used",
            )
        if request.sequence <= self.last_sequence:
            raise ProtocolError(
                "out_of_order_sequence",
                "sequence must increase monotonically",
                details={"minimum_sequence": self.last_sequence + 1},
            )
        self.last_sequence = request.sequence
        self.seen_request_ids.add(request.request_id)
        self.seen_order.append(request.request_id)
        while len(self.seen_order) > MAX_SEEN_REQUESTS:
            expired = self.seen_order.popleft()
            self.seen_request_ids.discard(expired)

    def _validate_generation(self, request: Request) -> None:
        if request.operation == "handshake" and self.lifecycle == "starting":
            if request.generation != 0:
                raise ProtocolError(
                    "generation_gap",
                    "initial handshake generation must be zero",
                )
            return
        if request.operation == "cancel":
            if request.generation < self.current_generation:
                raise ProtocolError(
                    "stale_generation",
                    "cancellation generation is stale",
                    details={"current_generation": self.current_generation},
                )
            if request.generation > self.current_generation + 1:
                raise ProtocolError(
                    "generation_gap",
                    "cancel may advance generation by exactly one",
                    details={"current_generation": self.current_generation},
                )
            return
        if request.generation < self.current_generation:
            raise ProtocolError(
                "stale_generation",
                "request generation is stale",
                details={"current_generation": self.current_generation},
            )
        if request.generation > self.current_generation:
            raise ProtocolError(
                "generation_gap",
                "only cancel may advance generation",
                details={"current_generation": self.current_generation},
            )

    def handle(self, raw: dict[str, object]) -> None:
        request: Request | None = None
        try:
            request = Request.parse(raw)
            with self.lock:
                self._remember(request)
                self.requests_total += 1
                if self.lifecycle == "starting" and request.operation != "handshake":
                    raise ProtocolError(
                        "handshake_required",
                        "handshake must be the first operation",
                    )
                if (
                    self.lifecycle != "starting"
                    and request.worker_instance_id != self.worker_instance_id
                ):
                    raise ProtocolError(
                        "instance_mismatch",
                        "worker_instance_id does not match this process",
                    )
                if self.lifecycle == "stopped":
                    raise ProtocolError(
                        "worker_stopped",
                        "worker is shutting down",
                    )
                self._validate_generation(request)
            handler = getattr(self, f"_handle_{request.operation}", None)
            if handler is None:
                raise ProtocolError(
                    "unsupported_operation",
                    "operation is not supported",
                )
            handler(request)
        except ProtocolError as error:
            if request is not None:
                self._error(request, error)
            else:
                self.protocol_error(error)
        except Exception:
            self.errors_total += 1
            if request is not None:
                self._emit(
                    request,
                    "error",
                    terminal=True,
                    error=ProtocolError(
                        "internal_error",
                        "local TTS worker operation failed",
                        retryable=True,
                    ),
                )
            traceback.print_exc(file=sys.stderr)

    def _capabilities(self) -> dict[str, object]:
        return {
            "pack_id": self.manifest.pack_id,
            "revision": self.manifest.revision,
            "manifest_sha256": self.manifest.canonical_sha256,
            "runtime_abi": self.manifest.runtime_abi,
            "worker_kind": "tts",
            "compute_backend": "cpu",
            "streaming_pcm": True,
            "cancellation": True,
            "network_access": False,
            "voice_cloning": False,
            "sample_rate_hz": 24000,
            "channels": 1,
            "sample_format": "pcm_s16le",
            "stock_voice_count": len(VOICE_BY_ID),
            "timing": {
                "pcm_sample_clock": True,
                "callback_receipt_monotonic": True,
                "word_alignment": False,
                "phoneme_alignment": False,
                "visemes": False,
                "unavailable_reason": (
                    "Pinned sherpa-onnx Kokoro C API exposes PCM/progress, "
                    "not word, phoneme or viseme timestamps."
                ),
            },
            "activation": "blocked_pending_measurement",
            "resource_envelope": {
                "status": "unmeasured",
                "admission_allowed": False,
            },
        }

    @staticmethod
    def _expect_fields(
        payload: dict[str, object],
        required: set[str],
        optional: set[str] = frozenset(),
    ) -> None:
        if not required.issubset(payload) or set(payload) - required - optional:
            raise ProtocolError(
                "invalid_payload",
                "operation payload fields are invalid",
            )

    def _handle_handshake(self, request: Request) -> None:
        self._expect_fields(
            request.payload,
            {"launch_nonce", "supervisor"},
        )
        nonce = validate_id(request.payload.get("launch_nonce"), "launch_nonce")
        validate_id(request.payload.get("supervisor"), "supervisor")
        if not hmac.compare_digest(nonce, self.launch_nonce):
            raise ProtocolError(
                "authentication_failed",
                "launch nonce does not match the supervisor",
            )
        if request.worker_instance_id not in ("", self.worker_instance_id):
            raise ProtocolError(
                "instance_mismatch",
                "initial worker_instance_id is invalid",
            )
        with self.lock:
            if self.lifecycle == "starting":
                self.lifecycle = "cold"
            elif self.lifecycle not in ("cold", "loaded"):
                raise ProtocolError(
                    "invalid_state",
                    "handshake cannot run in this lifecycle state",
                )
        self._emit(
            request,
            "completed",
            terminal=True,
            payload={
                "worker_instance_id": self.worker_instance_id,
                "lifecycle": self.lifecycle,
                "capabilities": self._capabilities(),
            },
        )

    def _handle_capabilities(self, request: Request) -> None:
        self._expect_fields(request.payload, set())
        self._emit(
            request,
            "completed",
            terminal=True,
            payload=self._capabilities(),
        )

    def _handle_discover_voices(self, request: Request) -> None:
        self._expect_fields(request.payload, set())
        self._emit(
            request,
            "completed",
            terminal=True,
            payload={
                "voices": public_voice_records(),
                "count": len(VOICE_BY_ID),
                "source": "audited_kokoro_v1.0_english_stock_allowlist",
                "unknown_voices_allowed": False,
                "voice_cloning": False,
            },
        )

    def _handle_health(self, request: Request) -> None:
        self._expect_fields(request.payload, set())
        with self.lock:
            payload: dict[str, object] = {
                "lifecycle": self.lifecycle,
                "generation": self.current_generation,
                "loaded": self.backend_identity is not None,
                "in_flight": len(self.jobs),
                "requests_total": self.requests_total,
                "synthesis_started": self.synthesis_started,
                "synthesis_completed": self.synthesis_completed,
                "synthesis_cancelled": self.synthesis_cancelled,
                "errors_total": self.errors_total,
                "load_ms": self.load_millis,
            }
        self._emit(request, "completed", terminal=True, payload=payload)

    def _handle_load(self, request: Request) -> None:
        self._expect_fields(
            request.payload,
            {"pack_id", "revision", "lease_id"},
            {"num_threads"},
        )
        if request.payload.get("pack_id") != self.manifest.pack_id or request.payload.get(
            "revision"
        ) != self.manifest.revision:
            raise ProtocolError(
                "invalid_payload",
                "load request does not select this verified pack revision",
            )
        lease_id = validate_id(request.payload.get("lease_id"), "lease_id")
        threads = request.payload.get("num_threads", 2)
        if isinstance(threads, bool) or not isinstance(threads, int) or not 1 <= threads <= 8:
            raise ProtocolError(
                "invalid_payload",
                "num_threads must be between 1 and 8",
            )
        with self.lock:
            if self.jobs:
                raise ProtocolError(
                    "worker_busy",
                    "cancel and drain synthesis before replacing a model lease",
                    retryable=True,
                )
            if self.backend_identity is not None:
                if lease_id == self.loaded_lease_id:
                    identity = self.backend_identity
                    self._emit(
                        request,
                        "completed",
                        terminal=True,
                        payload=self._identity_payload(identity, idempotent=True),
                    )
                    return
                raise ProtocolError(
                    "invalid_state",
                    "unload the current verified lease before loading another",
                )
        started = time.perf_counter()
        try:
            identity = self.backend.load(self.pack_root, num_threads=threads)
        except Exception as error:
            raise ProtocolError(
                "model_load_failed",
                "verified local TTS pack could not be loaded",
                retryable=True,
            ) from error
        if (
            identity.runtime_version != self.manifest.runtime_version
            or not self.manifest.runtime_git_sha.startswith(identity.runtime_git_sha)
            or identity.onnxruntime_version != self.manifest.onnxruntime_version
            or identity.sample_rate_hz != self.manifest.output_sample_rate_hz
            or identity.speaker_count < 53
        ):
            self.backend.unload()
            raise ProtocolError(
                "runtime_identity_mismatch",
                "loaded runtime identity differs from the trusted manifest",
            )
        with self.lock:
            self.backend_identity = identity
            self.loaded_lease_id = lease_id
            self.load_millis = (time.perf_counter() - started) * 1000.0
            self.lifecycle = "loaded"
        self._emit(
            request,
            "completed",
            terminal=True,
            payload=self._identity_payload(identity, idempotent=False),
        )

    def _identity_payload(
        self,
        identity: BackendIdentity,
        *,
        idempotent: bool,
    ) -> dict[str, object]:
        return {
            "runtime_version": identity.runtime_version,
            "runtime_git_sha": identity.runtime_git_sha,
            "onnxruntime_version": identity.onnxruntime_version,
            "sample_rate_hz": identity.sample_rate_hz,
            "speaker_count": identity.speaker_count,
            "load_ms": self.load_millis,
            "load_ms_provenance": "measured_this_process",
            "idempotent": idempotent,
            "resource_envelope_admission_allowed": False,
        }

    def _parse_synthesis(
        self,
        request: Request,
        *,
        self_test: bool,
    ) -> tuple[str, str, str, float, float]:
        required = {"audio_stream_id"} if self_test else {
            "audio_stream_id",
            "text",
            "voice_id",
        }
        optional = set() if self_test else {"speed", "silence_scale"}
        self._expect_fields(request.payload, required, optional)
        stream_id = validate_id(
            request.payload.get("audio_stream_id"),
            "audio_stream_id",
        )
        if self_test:
            return stream_id, SELF_TEST_TEXT, SELF_TEST_VOICE, 1.0, 0.2
        text = validate_text(request.payload.get("text"), "text")
        voice_id = validate_id(request.payload.get("voice_id"), "voice_id")
        if voice_id not in VOICE_BY_ID:
            raise ProtocolError(
                "unknown_voice",
                "voice_id is not in the audited stock-voice allowlist",
            )
        speed = request.payload.get("speed", 1.0)
        silence = request.payload.get("silence_scale", 0.2)
        for value, field, minimum, maximum in (
            (speed, "speed", 0.75, 1.25),
            (silence, "silence_scale", 0.0, 1.0),
        ):
            if (
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not minimum <= float(value) <= maximum
            ):
                raise ProtocolError(
                    "invalid_payload",
                    f"{field} is outside the product range",
                )
        return stream_id, text, voice_id, float(speed), float(silence)

    def _handle_synthesize(self, request: Request) -> None:
        self._start_synthesis(request, self_test=False)

    def _handle_self_test(self, request: Request) -> None:
        self._start_synthesis(request, self_test=True)

    def _start_synthesis(self, request: Request, *, self_test: bool) -> None:
        stream_id, text, voice_id, speed, silence = self._parse_synthesis(
            request,
            self_test=self_test,
        )
        with self.lock:
            if self.backend_identity is None or self.lifecycle != "loaded":
                raise ProtocolError(
                    "model_not_loaded",
                    "load a verified local TTS lease before synthesis",
                )
            if self.jobs:
                raise ProtocolError(
                    "worker_busy",
                    "local TTS worker supports one in-flight synthesis",
                    retryable=True,
                )
            if stream_id in self.seen_stream_ids:
                raise ProtocolError(
                    "duplicate_stream",
                    "audio_stream_id has already been used",
                )
            self.seen_stream_ids.add(stream_id)
            job = SynthesisJob(request=request, stream_id=stream_id)
            self.jobs[request.request_id] = job
            self.synthesis_started += 1
        self._emit(
            request,
            "accepted",
            terminal=False,
            payload={
                "audio_stream_id": stream_id,
                "voice_id": voice_id,
                "sample_rate_hz": self.backend_identity.sample_rate_hz,
                "timing_basis": "exact_output_sample_clock",
                "visemes": "unavailable",
            },
        )
        thread = threading.Thread(
            target=self._run_synthesis,
            args=(job, text, voice_id, speed, silence, self_test),
            name=f"local-tts-{request.request_id[:32]}",
            daemon=True,
        )
        job.thread = thread
        thread.start()

    def _run_synthesis(
        self,
        job: SynthesisJob,
        text: str,
        voice_id: str,
        speed: float,
        silence_scale: float,
        self_test: bool,
    ) -> None:
        request = job.request
        voice = VOICE_BY_ID[voice_id]
        started = time.perf_counter()
        first_pcm_at: float | None = None
        frames = 0
        chunk_sequence = 0
        event_index = 1
        clipping = 0
        nonzero = False

        def audio_callback(samples: list[float], progress: float) -> bool:
            nonlocal frames, chunk_sequence, event_index, first_pcm_at, clipping, nonzero
            if job.cancel.is_set() or request.generation != self.current_generation:
                return False
            if request.deadline_unix_ms and request.deadline_unix_ms <= int(
                time.time() * 1000
            ):
                job.cancel.set()
                return False
            if first_pcm_at is None and samples:
                first_pcm_at = time.perf_counter()
            nonzero = nonzero or any(abs(sample) > 1e-6 for sample in samples)
            receipts = self.pcm_writer.write_samples(
                stream_id=job.stream_id,
                request_id=request.request_id,
                generation=request.generation,
                chunk_sequence=chunk_sequence,
                sample_start=frames,
                sample_rate_hz=self.backend_identity.sample_rate_hz,  # type: ignore[union-attr]
                samples=samples,
            )
            for receipt in receipts:
                clipping += receipt.clipped_samples
                self._emit(
                    request,
                    "pcm_chunk",
                    terminal=False,
                    event_index=event_index,
                    payload={
                        **receipt.public_metadata(),
                        "progress_fraction": max(0.0, min(1.0, progress)),
                    },
                )
                event_index += 1
            frames += len(samples)
            chunk_sequence += len(receipts)
            return not job.cancel.is_set()

        status = "failed"
        try:
            summary = self.backend.synthesize(
                text,
                speaker_id=voice.speaker_id,
                speed=speed,
                silence_scale=silence_scale,
                callback=audio_callback,
                cancel=job.cancel,
            )
            cancelled = (
                summary.cancelled
                or job.cancel.is_set()
                or request.generation != self.current_generation
            )
            if cancelled:
                status = "cancelled"
                with self.lock:
                    self.synthesis_cancelled += 1
                return
            if summary.generated_frames != frames:
                raise RuntimeError("backend and streamed frame counts differ")
            if self_test and (
                frames < 2400 or not nonzero or clipping != 0
            ):
                raise RuntimeError("local TTS structural self-test failed")
            status = "completed"
            finished = time.perf_counter()
            sample_rate = self.backend_identity.sample_rate_hz  # type: ignore[union-attr]
            audio_ms = frames * 1000.0 / sample_rate
            total_ms = (finished - started) * 1000.0
            first_pcm_ms = (
                (first_pcm_at - started) * 1000.0
                if first_pcm_at is not None
                else None
            )
            result_event = "self_test_result" if self_test else "tts_result"
            self._emit(
                request,
                result_event,
                terminal=True,
                event_index=event_index,
                payload={
                    "audio_stream_id": job.stream_id,
                    "voice_id": voice_id,
                    "frames": frames,
                    "sample_rate_hz": sample_rate,
                    "audio_duration_ms": audio_ms,
                    "first_pcm_ms": first_pcm_ms,
                    "total_synthesis_ms": total_ms,
                    "realtime_factor": total_ms / audio_ms if audio_ms else None,
                    "callbacks": summary.callbacks,
                    "clipped_samples": clipping,
                    "non_silent": nonzero,
                    "measurement_provenance": "measured_this_process",
                    "timing": {
                        "kind": "exact_pcm_sample_clock",
                        "word_alignment": "unavailable",
                        "phoneme_alignment": "unavailable",
                    },
                    "visemes": {
                        "availability": "unavailable",
                        "reason": (
                            "Pinned sherpa-onnx Kokoro C API does not expose "
                            "honest phoneme, word or viseme timestamps."
                        ),
                    },
                    "self_test_passed": self_test,
                    "resource_envelope_admission_allowed": False,
                },
            )
            with self.lock:
                self.synthesis_completed += 1
        except Exception:
            if not job.cancel.is_set() and request.generation == self.current_generation:
                self.errors_total += 1
                self._emit(
                    request,
                    "error",
                    terminal=True,
                    event_index=event_index,
                    error=ProtocolError(
                        "inference_failed",
                        "local TTS synthesis failed",
                        retryable=True,
                    ),
                )
            traceback.print_exc(file=sys.stderr)
        finally:
            try:
                self.pcm_writer.finish(
                    stream_id=job.stream_id,
                    request_id=request.request_id,
                    generation=request.generation,
                    final_frames=frames,
                    status=status,
                )
            except PcmTransportError:
                traceback.print_exc(file=sys.stderr)
            with self.lock:
                self.jobs.pop(request.request_id, None)

    def _handle_cancel(self, request: Request) -> None:
        self._expect_fields(request.payload, set())
        started = time.perf_counter()
        with self.lock:
            if request.generation == self.current_generation + 1:
                self.current_generation = request.generation
            cancelled_ids: list[str] = []
            for request_id, job in self.jobs.items():
                if job.request.generation < self.current_generation:
                    job.cancel.set()
                    cancelled_ids.append(request_id)
        self._emit(
            request,
            "completed",
            terminal=True,
            payload={
                "current_generation": self.current_generation,
                "cancelled_request_ids": sorted(cancelled_ids),
                "cancel_signal_ms": (time.perf_counter() - started) * 1000.0,
                "drain_complete": not bool(cancelled_ids),
                "note": (
                    "PCM end records provide authoritative per-stream drain; "
                    "the cancel response only proves the callback signal."
                ),
            },
        )

    def _handle_unload(self, request: Request) -> None:
        self._expect_fields(request.payload, set())
        with self.lock:
            if self.jobs:
                raise ProtocolError(
                    "worker_busy",
                    "cancel and wait for PCM end before unloading",
                    retryable=True,
                )
        self.backend.unload()
        with self.lock:
            self.backend_identity = None
            self.loaded_lease_id = None
            self.load_millis = None
            self.lifecycle = "cold"
        self._emit(
            request,
            "completed",
            terminal=True,
            payload={"lifecycle": self.lifecycle},
        )

    def _handle_shutdown(self, request: Request) -> None:
        self._expect_fields(request.payload, set())
        with self.lock:
            active = list(self.jobs.values())
            for job in active:
                job.cancel.set()
        for job in active:
            if job.thread is not None:
                job.thread.join(timeout=5.0)
        with self.lock:
            if self.jobs:
                raise ProtocolError(
                    "worker_busy",
                    "synthesis did not drain; supervisor may terminate the process",
                    retryable=False,
                )
        self.backend.unload()
        with self.lock:
            self.backend_identity = None
            self.loaded_lease_id = None
            self.lifecycle = "stopped"
        self._emit(
            request,
            "completed",
            terminal=True,
            payload={"lifecycle": "stopped"},
        )
        self.should_exit.set()

    def cancel_and_drain(self, timeout_seconds: float = 5.0) -> bool:
        """Cancel native callbacks and wait before a parent-disconnect unload."""

        with self.lock:
            active = list(self.jobs.values())
            for job in active:
                job.cancel.set()
        deadline = time.monotonic() + max(0.0, timeout_seconds)
        for job in active:
            if job.thread is not None:
                job.thread.join(timeout=max(0.0, deadline - time.monotonic()))
        with self.lock:
            return not self.jobs


def _open_read(path: str | None) -> tuple[BinaryIO, bool]:
    if (
        not isinstance(path, str)
        or os.name != "nt"
        or not path.lower().startswith("\\\\.\\pipe\\")
    ):
        raise ValueError("input pipe must be a supervisor-created Windows named pipe")
    return open(path, "rb", buffering=0), True


def _open_write(path: str | None, *, pcm: bool = False) -> tuple[BinaryIO, bool]:
    if (
        not isinstance(path, str)
        or os.name != "nt"
        or not path.lower().startswith("\\\\.\\pipe\\")
    ):
        output = "PCM output" if pcm else "output"
        raise ValueError(
            f"{output} pipe must be a supervisor-created Windows named pipe"
        )
    return open(path, "wb", buffering=0), True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--pack-root", required=True, type=Path)
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--launch-nonce", required=True)
    parser.add_argument("--worker-instance-id", required=True)
    parser.add_argument("--input-pipe", required=True)
    parser.add_argument("--output-pipe", required=True)
    parser.add_argument("--pcm-output-pipe", required=True)
    args = parser.parse_args(argv)

    manifest = load_manifest(args.manifest)
    if not hmac.compare_digest(
        manifest.canonical_sha256,
        args.expected_manifest_sha256,
    ):
        raise SystemExit("trusted manifest digest mismatch")
    pack_root = args.pack_root.resolve(strict=True)
    expected_root = (
        pack_root.parents[1]
        / manifest.pack_id
        / manifest.revision
    ).resolve(strict=True)
    if pack_root != expected_root:
        raise SystemExit("pack root does not match the trusted pack identity")
    manager = PackManager(pack_root.parents[1], HttpsDownloader())
    try:
        manager.verify(manifest)
    except PackLifecycleError as error:
        raise SystemExit(f"verified pack required: {error.code}") from error

    control_input, close_input = _open_read(args.input_pipe)
    control_output, close_output = _open_write(args.output_pipe)
    pcm_output, close_pcm = _open_write(args.pcm_output_pipe, pcm=True)
    install_network_denial()
    backend = SherpaOnnxBackend(
        expected_version=manifest.runtime_version,
        expected_git_sha=manifest.runtime_git_sha,
        expected_onnxruntime_version=manifest.onnxruntime_version,
    )
    controller = WorkerController(
        manifest=manifest,
        pack_root=pack_root,
        backend=backend,
        control_writer=FrameWriter(control_output),
        pcm_writer=PcmWriter(pcm_output),
        launch_nonce=validate_id(args.launch_nonce, "launch_nonce"),
        worker_instance_id=args.worker_instance_id,
    )
    try:
        while not controller.should_exit.is_set():
            try:
                controller.handle(read_frame(control_input))
            except EndOfStream:
                break
            except ProtocolError as error:
                controller.protocol_error(error)
                break
    finally:
        # A native callback may still hold the engine.  Never destroy it until
        # cancellation has drained; a hung process is the supervisor job
        # object's responsibility.
        if controller.cancel_and_drain():
            backend.unload()
        if close_input:
            control_input.close()
        if close_output:
            control_output.close()
        if close_pcm:
            pcm_output.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
