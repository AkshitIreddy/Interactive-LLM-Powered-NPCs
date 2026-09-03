from __future__ import annotations

from dataclasses import dataclass
import math
import re
import time
from typing import Any, Mapping


PROTOCOL_VERSION = "npc.local-stt/v1"
MAX_CONTROL_FRAME_BYTES = 1_048_576
MAX_PCM_FRAME_BYTES = 262_144
MAX_PCM_METADATA_BYTES = 4_096
MAX_REQUEST_ID_BYTES = 128
MAX_TEXT_BYTES = 262_144
MAX_BUFFERED_CHUNKS = 8
MAX_BUFFERED_AUDIO_MS = 30_000

OPERATIONS = frozenset(
    {
        "handshake",
        "capabilities",
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
)

_ENVELOPE_KEYS = frozenset(
    {
        "protocolVersion",
        "workerInstanceId",
        "requestId",
        "sequence",
        "generation",
        "deadlineUnixMs",
        "operation",
        "payload",
    }
)
_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")


class ProtocolFault(Exception):
    def __init__(
        self,
        code: str,
        message: str,
        *,
        retryable: bool = False,
        details: Mapping[str, Any] | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.safe_message = message
        self.retryable = retryable
        self.details = dict(details or {})


def _bounded_identifier(value: Any, name: str) -> str:
    if not isinstance(value, str) or not _ID_RE.fullmatch(value):
        raise ProtocolFault("invalid_request", f"{name} is not a valid bounded identifier")
    return value


def _bounded_uint(value: Any, name: str, maximum: int = (1 << 63) - 1) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value > maximum:
        raise ProtocolFault("invalid_request", f"{name} must be an unsigned bounded integer")
    return value


def _bounded_number(value: Any, name: str, minimum: float, maximum: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ProtocolFault("invalid_payload", f"{name} must be numeric")
    result = float(value)
    if not math.isfinite(result) or result < minimum or result > maximum:
        raise ProtocolFault("invalid_payload", f"{name} is outside its allowed range")
    return result


def strict_object(value: Any, name: str, allowed: set[str] | frozenset[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProtocolFault("invalid_payload", f"{name} must be an object")
    unknown = set(value) - set(allowed)
    if unknown:
        raise ProtocolFault(
            "invalid_payload",
            f"{name} contains unsupported fields",
            details={"fieldCount": len(unknown)},
        )
    return value


@dataclass(frozen=True)
class RequestEnvelope:
    protocol_version: str
    worker_instance_id: str
    request_id: str
    sequence: int
    generation: int
    deadline_unix_ms: int
    operation: str
    payload: dict[str, Any]

    @classmethod
    def parse(cls, raw: Any) -> "RequestEnvelope":
        obj = strict_object(raw, "request", _ENVELOPE_KEYS)
        missing = _ENVELOPE_KEYS - set(obj)
        if missing:
            raise ProtocolFault(
                "invalid_request",
                "request is missing required fields",
                details={"missingFieldCount": len(missing)},
            )
        if obj["protocolVersion"] != PROTOCOL_VERSION:
            raise ProtocolFault("unsupported_protocol", "unsupported local STT protocol version")
        operation = obj["operation"]
        if operation not in OPERATIONS:
            raise ProtocolFault("unsupported_operation", "unsupported local STT operation")
        payload = obj["payload"]
        if not isinstance(payload, dict):
            raise ProtocolFault("invalid_payload", "payload must be an object")
        deadline = _bounded_uint(obj["deadlineUnixMs"], "deadlineUnixMs")
        if deadline and deadline < int(time.time() * 1000):
            raise ProtocolFault("deadline_exceeded", "request deadline has expired", retryable=True)
        return cls(
            protocol_version=PROTOCOL_VERSION,
            worker_instance_id=_bounded_identifier(obj["workerInstanceId"], "workerInstanceId"),
            request_id=_bounded_identifier(obj["requestId"], "requestId"),
            sequence=_bounded_uint(obj["sequence"], "sequence"),
            generation=_bounded_uint(obj["generation"], "generation"),
            deadline_unix_ms=deadline,
            operation=operation,
            payload=payload,
        )


@dataclass(frozen=True)
class SessionStart:
    session_id: str
    input_source: str
    turn_mode: str
    sample_rate_hz: int
    channels: int
    sample_format: str
    word_timestamps: bool

    @classmethod
    def parse(cls, payload: Any) -> "SessionStart":
        p = strict_object(
            payload,
            "session_start payload",
            {
                "sessionId",
                "inputSource",
                "turnMode",
                "sampleRateHz",
                "channels",
                "sampleFormat",
                "wordTimestamps",
            },
        )
        required = {
            "sessionId",
            "inputSource",
            "turnMode",
            "sampleRateHz",
            "channels",
            "sampleFormat",
            "wordTimestamps",
        }
        if required - set(p):
            raise ProtocolFault("invalid_payload", "session_start payload is incomplete")
        if p["inputSource"] != "supervisor_microphone_pcm":
            raise ProtocolFault("invalid_payload", "inputSource must be supervisor_microphone_pcm")
        if p["turnMode"] not in {"push_to_talk", "open_microphone"}:
            raise ProtocolFault("invalid_payload", "unsupported turn mode")
        if p["sampleFormat"] != "pcm_s16le":
            raise ProtocolFault("invalid_payload", "only pcm_s16le is accepted")
        if p["channels"] != 1:
            raise ProtocolFault("invalid_payload", "only mono PCM is accepted")
        if p["sampleRateHz"] not in {8000, 16000, 22050, 24000, 44100, 48000}:
            raise ProtocolFault("invalid_payload", "unsupported PCM sample rate")
        if not isinstance(p["wordTimestamps"], bool):
            raise ProtocolFault("invalid_payload", "wordTimestamps must be boolean")
        return cls(
            session_id=_bounded_identifier(p["sessionId"], "sessionId"),
            input_source=p["inputSource"],
            turn_mode=p["turnMode"],
            sample_rate_hz=p["sampleRateHz"],
            channels=1,
            sample_format="pcm_s16le",
            word_timestamps=p["wordTimestamps"],
        )


@dataclass(frozen=True)
class PcmFrame:
    protocol_version: str
    worker_instance_id: str
    session_id: str
    generation: int
    chunk_id: str
    chunk_index: int
    sample_rate_hz: int
    channels: int
    sample_format: str
    sample_count: int
    captured_at_qpc: int
    qpc_frequency_hz: int
    pcm: bytes

    @classmethod
    def parse(cls, metadata: Any, pcm: bytes) -> "PcmFrame":
        keys = {
            "protocolVersion",
            "workerInstanceId",
            "sessionId",
            "generation",
            "chunkId",
            "chunkIndex",
            "sampleRateHz",
            "channels",
            "sampleFormat",
            "sampleCount",
            "capturedAtQpc",
            "qpcFrequencyHz",
        }
        m = strict_object(metadata, "PCM metadata", keys)
        if keys - set(m):
            raise ProtocolFault("invalid_pcm_frame", "PCM metadata is incomplete")
        if m["protocolVersion"] != PROTOCOL_VERSION:
            raise ProtocolFault("unsupported_protocol", "unsupported PCM protocol version")
        if not isinstance(pcm, bytes) or not pcm or len(pcm) > MAX_PCM_FRAME_BYTES:
            raise ProtocolFault("invalid_pcm_frame", "PCM payload length is invalid")
        if len(pcm) % 2:
            raise ProtocolFault("invalid_pcm_frame", "pcm_s16le payload must contain whole samples")
        sample_count = _bounded_uint(m["sampleCount"], "sampleCount", MAX_PCM_FRAME_BYTES // 2)
        if sample_count * 2 != len(pcm):
            raise ProtocolFault("invalid_pcm_frame", "PCM byte length does not match sampleCount")
        if m["sampleFormat"] != "pcm_s16le" or m["channels"] != 1:
            raise ProtocolFault("invalid_pcm_frame", "only mono pcm_s16le is accepted")
        if m["sampleRateHz"] not in {8000, 16000, 22050, 24000, 44100, 48000}:
            raise ProtocolFault("invalid_pcm_frame", "unsupported PCM sample rate")
        qpc_hz = _bounded_uint(m["qpcFrequencyHz"], "qpcFrequencyHz")
        if qpc_hz == 0:
            raise ProtocolFault("invalid_pcm_frame", "qpcFrequencyHz must be positive")
        return cls(
            protocol_version=PROTOCOL_VERSION,
            worker_instance_id=_bounded_identifier(m["workerInstanceId"], "workerInstanceId"),
            session_id=_bounded_identifier(m["sessionId"], "sessionId"),
            generation=_bounded_uint(m["generation"], "generation"),
            chunk_id=_bounded_identifier(m["chunkId"], "chunkId"),
            chunk_index=_bounded_uint(m["chunkIndex"], "chunkIndex"),
            sample_rate_hz=m["sampleRateHz"],
            channels=1,
            sample_format="pcm_s16le",
            sample_count=sample_count,
            captured_at_qpc=_bounded_uint(m["capturedAtQpc"], "capturedAtQpc"),
            qpc_frequency_hz=qpc_hz,
            pcm=pcm,
        )


def parse_pcm_commit(payload: Any) -> tuple[str, str]:
    p = strict_object(payload, "pcm_commit payload", {"sessionId", "chunkId"})
    if set(p) != {"sessionId", "chunkId"}:
        raise ProtocolFault("invalid_payload", "pcm_commit payload is incomplete")
    return (
        _bounded_identifier(p["sessionId"], "sessionId"),
        _bounded_identifier(p["chunkId"], "chunkId"),
    )


def parse_session_end(payload: Any) -> tuple[str, str]:
    p = strict_object(payload, "session_end payload", {"sessionId", "reason"})
    if set(p) != {"sessionId", "reason"}:
        raise ProtocolFault("invalid_payload", "session_end payload is incomplete")
    if p["reason"] not in {"ptt_key_up", "vad_end_of_turn", "manual_stop", "shutdown"}:
        raise ProtocolFault("invalid_payload", "unsupported session end reason")
    return _bounded_identifier(p["sessionId"], "sessionId"), p["reason"]


def parse_load(payload: Any) -> tuple[str, str, str]:
    p = strict_object(payload, "load payload", {"packId", "revision", "modelLeaseId"})
    if set(p) != {"packId", "revision", "modelLeaseId"}:
        raise ProtocolFault("invalid_payload", "load payload is incomplete")
    return (
        _bounded_identifier(p["packId"], "packId"),
        _bounded_identifier(p["revision"], "revision"),
        _bounded_identifier(p["modelLeaseId"], "modelLeaseId"),
    )


def bounded_float_option(payload: dict[str, Any], key: str, minimum: float, maximum: float) -> float | None:
    if key not in payload:
        return None
    return _bounded_number(payload[key], key, minimum, maximum)
