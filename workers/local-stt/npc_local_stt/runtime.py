from __future__ import annotations

import hmac
import time
from typing import Any

from .backend import BackendDelta, SttBackend
from .contract import (
    PROTOCOL_VERSION,
    PcmFrame,
    ProtocolFault,
    RequestEnvelope,
    SessionStart,
    parse_load,
    parse_pcm_commit,
    parse_session_end,
    strict_object,
)
from .ingress import PcmFrameStore
from .metrics import MetricsTracker


class WorkerRuntime:
    def __init__(
        self,
        *,
        worker_instance_id: str,
        launch_nonce: str,
        backend: SttBackend,
        pcm_store: PcmFrameStore | None = None,
    ) -> None:
        self.worker_instance_id = worker_instance_id
        self._launch_nonce = launch_nonce
        self._backend = backend
        self._pcm_store = pcm_store or PcmFrameStore()
        self._state = "starting"
        self._handshaken = False
        self._last_sequence = -1
        self._seen_request_ids: set[str] = set()
        self._generation = 0
        self._active_session: SessionStart | None = None
        self._next_chunk_index = 0
        self._pack: tuple[str, str, str] | None = None
        self._metrics = MetricsTracker(evidence_class="fixture" if backend.fixture else "runtime_hook")

    @property
    def stopped(self) -> bool:
        return self._state == "stopped"

    @property
    def pcm_store(self) -> PcmFrameStore:
        return self._pcm_store

    def feed_pcm_for_test(self, frame: PcmFrame) -> None:
        if not self._backend.fixture:
            raise ProtocolFault("fixture_disabled", "direct PCM injection is available only to fixture tests")
        self._pcm_store.put(frame)

    def _event(
        self,
        request: RequestEnvelope,
        index: int,
        event: str,
        *,
        terminal: bool = False,
        payload: dict[str, Any] | None = None,
        error: ProtocolFault | None = None,
    ) -> dict[str, Any]:
        result: dict[str, Any] = {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": self.worker_instance_id,
            "requestId": request.request_id,
            "sequence": request.sequence,
            "generation": self._generation,
            "eventIndex": index,
            "event": event,
            "terminal": terminal,
            "payload": payload or {},
        }
        if error is not None:
            result["error"] = {
                "code": error.code,
                "message": error.safe_message,
                "retryable": error.retryable,
                "details": error.details,
            }
        return result

    def _validate_ordering(self, request: RequestEnvelope) -> None:
        if request.worker_instance_id != self.worker_instance_id:
            raise ProtocolFault("instance_mismatch", "worker instance identity does not match")
        if request.request_id in self._seen_request_ids:
            raise ProtocolFault("duplicate_request", "duplicate request ID was rejected")
        if request.sequence <= self._last_sequence:
            raise ProtocolFault("out_of_order_sequence", "request sequence did not increase")
        if request.operation == "cancel":
            if request.generation not in {self._generation, self._generation + 1}:
                code = "stale_generation" if request.generation < self._generation else "generation_gap"
                raise ProtocolFault(code, "cancellation generation is not contiguous")
        elif request.generation != self._generation:
            code = "stale_generation" if request.generation < self._generation else "generation_gap"
            raise ProtocolFault(code, "request generation does not match the worker generation")

    def _record_request(self, request: RequestEnvelope) -> None:
        self._seen_request_ids.add(request.request_id)
        self._last_sequence = request.sequence

    def handle(self, raw: Any) -> list[dict[str, Any]]:
        request = RequestEnvelope.parse(raw)
        try:
            self._validate_ordering(request)
            # Once identity/order/generation are accepted, consume the request
            # even if its operation later fails. This prevents an invalid or
            # unauthenticated control frame from being replayed under the same
            # request ID and sequence.
            self._record_request(request)
            if request.operation != "handshake" and not self._handshaken:
                raise ProtocolFault("handshake_required", "worker handshake is required")
            if self.stopped:
                raise ProtocolFault("invalid_state", "worker has already stopped")
            return self._dispatch(request)
        except ProtocolFault as fault:
            # Ordering failures happen before recording, so the supervisor may
            # retry with corrected order/generation. Dispatch failures are
            # consumed and require a fresh request ID/sequence. Errors are safe.
            return [self._event(request, 0, "error", terminal=True, error=fault)]

    def _dispatch(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        operation = request.operation
        if operation == "handshake":
            return self._handshake(request)
        if operation == "capabilities":
            return self._capabilities(request)
        if operation == "health":
            return self._health(request)
        if operation == "warm":
            return self._warm(request)
        if operation == "load":
            return self._load(request)
        if operation == "unload":
            return self._unload(request)
        if operation == "session_start":
            return self._session_start(request)
        if operation == "pcm_commit":
            return self._pcm_commit(request)
        if operation == "session_end":
            return self._session_end(request)
        if operation == "cancel":
            return self._cancel(request)
        if operation == "self_test":
            return self._self_test(request)
        if operation == "shutdown":
            return self._shutdown(request)
        raise ProtocolFault("unsupported_operation", "unsupported local STT operation")

    def _handshake(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        if self._handshaken:
            raise ProtocolFault("invalid_state", "worker handshake has already completed")
        p = strict_object(
            request.payload,
            "handshake payload",
            {"launchNonce", "supervisorPid", "supervisorExecutableSha256"},
        )
        if set(p) != {"launchNonce", "supervisorPid", "supervisorExecutableSha256"}:
            raise ProtocolFault("invalid_payload", "handshake payload is incomplete")
        if not isinstance(p["launchNonce"], str) or not hmac.compare_digest(
            p["launchNonce"], self._launch_nonce
        ):
            raise ProtocolFault("authentication_failed", "worker launch authentication failed")
        if isinstance(p["supervisorPid"], bool) or not isinstance(p["supervisorPid"], int) or p["supervisorPid"] <= 0:
            raise ProtocolFault("invalid_payload", "supervisorPid must be positive")
        digest = p["supervisorExecutableSha256"]
        if not isinstance(digest, str) or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise ProtocolFault("invalid_payload", "supervisor executable digest is invalid")
        self._handshaken = True
        self._state = "cold"
        return [
            self._event(
                request,
                0,
                "descriptor",
                payload={
                    "workerId": "npc-local-stt-moonshine",
                    "backend": self._backend.backend_name,
                    "fixture": self._backend.fixture,
                    "protocolVersion": PROTOCOL_VERSION,
                    "networkAccess": False,
                    "microphoneOwnership": "media_broker",
                    "computeBackend": "cpu",
                },
            ),
            self._event(request, 1, "completed", terminal=True, payload={"state": self._state}),
        ]

    def _capabilities(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "capabilities payload", set())
        return [
            self._event(
                request,
                0,
                "capabilities",
                payload={
                    "operations": sorted(
                        {
                            "health",
                            "warm",
                            "load",
                            "unload",
                            "session_start",
                            "pcm_commit",
                            "session_end",
                            "cancel",
                            "self_test",
                            "shutdown",
                        }
                    ),
                    "inputSource": "supervisor_microphone_pcm",
                    "sampleFormats": ["pcm_s16le"],
                    "sampleRatesHz": [8000, 16000, 22050, 24000, 44100, 48000],
                    "channels": [1],
                    "partialTranscripts": True,
                    "finalTranscripts": True,
                    "wordTimestamps": True,
                    "vad": True,
                    "pttKeyUpAuthoritative": True,
                    "cancellation": True,
                    "maximumConcurrentSessions": 1,
                    "maximumBufferedAudioMs": 30000,
                },
            ),
            self._event(request, 1, "completed", terminal=True),
        ]

    def _health(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "health payload", set())
        return [
            self._event(
                request,
                0,
                "health",
                payload={
                    "state": self._state,
                    "generation": self._generation,
                    "loadedPack": {
                        "packId": self._pack[0],
                        "revision": self._pack[1],
                        "modelLeaseId": self._pack[2],
                    }
                    if self._pack
                    else None,
                    "activeSessionId": self._active_session.session_id if self._active_session else None,
                    "pcmIngress": self._pcm_store.snapshot(),
                    "metrics": self._metrics.snapshot(),
                },
            ),
            self._event(request, 1, "completed", terminal=True),
        ]

    def _warm(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "warm payload", set())
        if self._state not in {"cold", "warm"}:
            raise ProtocolFault("invalid_state", "warm is valid only before model load")
        self._backend.warm()
        self._state = "warm"
        return [self._event(request, 0, "completed", terminal=True, payload={"state": "warm"})]

    def _load(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        pack = parse_load(request.payload)
        if self._active_session is not None:
            raise ProtocolFault("worker_busy", "cannot replace a model during an active session", retryable=True)
        if self._pack == pack and self._state == "loaded":
            return [self._event(request, 0, "completed", terminal=True, payload={"idempotent": True})]
        if self._state not in {"cold", "warm", "loaded"}:
            raise ProtocolFault("invalid_state", "model cannot be loaded in the current state")
        if self._state == "loaded":
            self._backend.unload()
        started = time.perf_counter()
        self._backend.load()
        load_ms = (time.perf_counter() - started) * 1000
        self._metrics.record_load(load_ms)
        self._pack = pack
        self._state = "loaded"
        return [
            self._event(request, 0, "model_loaded", payload={"loadMs": load_ms}),
            self._event(request, 1, "completed", terminal=True, payload={"state": "loaded"}),
        ]

    def _unload(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "unload payload", set())
        if self._active_session is not None:
            raise ProtocolFault("worker_busy", "cancel or end the session before unload", retryable=True)
        if self._state == "loaded":
            self._backend.unload()
        self._pack = None
        self._state = "warm"
        self._pcm_store.clear()
        return [self._event(request, 0, "completed", terminal=True, payload={"state": "warm"})]

    def _session_start(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        if self._state != "loaded":
            raise ProtocolFault("model_not_loaded", "load the local STT pack before starting a session")
        if self._active_session is not None:
            raise ProtocolFault("worker_busy", "a local STT session is already active", retryable=True)
        session = SessionStart.parse(request.payload)
        self._backend.start_session(session.session_id, session.word_timestamps)
        self._active_session = session
        self._next_chunk_index = 0
        return [
            self._event(
                request,
                0,
                "session_started",
                payload={
                    "sessionId": session.session_id,
                    "sampleClockOrigin": "first_pcm_capture_qpc",
                    "turnMode": session.turn_mode,
                },
            ),
            self._event(request, 1, "completed", terminal=True),
        ]

    def _validate_pcm_frame(self, frame: PcmFrame, session_id: str) -> None:
        session = self._active_session
        if session is None or session.session_id != session_id or frame.session_id != session_id:
            raise ProtocolFault("session_mismatch", "PCM chunk does not belong to the active session")
        if frame.worker_instance_id != self.worker_instance_id:
            raise ProtocolFault("instance_mismatch", "PCM worker identity does not match")
        if frame.generation != self._generation:
            raise ProtocolFault("stale_generation", "PCM chunk generation is stale")
        if frame.chunk_index != self._next_chunk_index:
            raise ProtocolFault("pcm_out_of_order", "PCM chunk index is not contiguous")
        if frame.sample_rate_hz != session.sample_rate_hz:
            raise ProtocolFault("pcm_format_changed", "PCM sample rate changed during the session")

    def _delta_events(self, request: RequestEnvelope, delta: BackendDelta, start_index: int) -> list[dict[str, Any]]:
        events: list[dict[str, Any]] = []
        index = start_index
        if delta.speech_started:
            events.append(self._event(request, index, "utterance_started", payload={}))
            index += 1
        saw_partial = False
        saw_final = False
        for line in delta.lines:
            event_name = "final_transcript" if line.complete else "partial_transcript"
            saw_final = saw_final or line.complete
            saw_partial = saw_partial or not line.complete
            events.append(self._event(request, index, event_name, payload=line.as_dict()))
            index += 1
        if delta.speech_ended:
            events.append(self._event(request, index, "utterance_ended", payload={}))
            index += 1
        self._metrics.record_delta(
            delta.inference_ms,
            delta.analyzed_audio_ms,
            partial=saw_partial,
            final=saw_final,
        )
        return events

    def _pcm_commit(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        session_id, chunk_id = parse_pcm_commit(request.payload)
        frame = self._pcm_store.take(chunk_id)
        self._validate_pcm_frame(frame, session_id)
        self._next_chunk_index += 1
        self._metrics.record_pcm(frame.sample_count)
        delta = self._backend.add_pcm(frame)
        events = [self._event(request, 0, "accepted", payload={"chunkId": chunk_id})]
        events.extend(self._delta_events(request, delta, 1))
        events.append(
            self._event(
                request,
                len(events),
                "completed",
                terminal=True,
                payload={
                    "chunkId": chunk_id,
                    "chunkIndex": frame.chunk_index,
                    "sampleCount": frame.sample_count,
                },
            )
        )
        return events

    def _session_end(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        session_id, reason = parse_session_end(request.payload)
        if self._active_session is None or self._active_session.session_id != session_id:
            raise ProtocolFault("session_mismatch", "session_end does not match the active session")
        self._metrics.mark_session_end()
        delta = self._backend.end_session(reason)
        events = [self._event(request, 0, "accepted", payload={"reason": reason})]
        events.extend(self._delta_events(request, delta, 1))
        self._active_session = None
        self._pcm_store.clear()
        events.append(
            self._event(
                request,
                len(events),
                "completed",
                terminal=True,
                payload={"sessionId": session_id, "reason": reason, "metrics": self._metrics.snapshot()},
            )
        )
        return events

    def _cancel(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "cancel payload", {"reason"})
        if set(request.payload) != {"reason"} or request.payload["reason"] not in {
            "barge_in",
            "route_changed",
            "runtime_shutdown",
            "deadline",
            "user_cancelled",
        }:
            raise ProtocolFault("invalid_payload", "cancel reason is invalid")
        if request.generation == self._generation:
            return [
                self._event(
                    request,
                    0,
                    "completed",
                    terminal=True,
                    payload={"generation": self._generation, "idempotent": True},
                )
            ]
        self._generation = request.generation
        self._backend.cancel()
        self._active_session = None
        self._pcm_store.clear()
        return [
            self._event(
                request,
                0,
                "cancelled",
                payload={"generation": self._generation, "reason": request.payload["reason"]},
            ),
            self._event(request, 1, "completed", terminal=True),
        ]

    def _self_test(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "self_test payload", {"level"})
        if set(request.payload) != {"level"} or request.payload["level"] not in {"protocol", "bridge"}:
            raise ProtocolFault("invalid_payload", "self-test level is invalid")
        result = self._backend.self_test()
        return [
            self._event(request, 0, "self_test_result", payload=result),
            self._event(request, 1, "completed", terminal=True),
        ]

    def _shutdown(self, request: RequestEnvelope) -> list[dict[str, Any]]:
        strict_object(request.payload, "shutdown payload", set())
        if self._active_session is not None:
            self._backend.cancel()
        if self._state == "loaded":
            self._backend.unload()
        self._active_session = None
        self._pack = None
        self._pcm_store.close()
        self._state = "stopped"
        return [self._event(request, 0, "completed", terminal=True, payload={"state": "stopped"})]
