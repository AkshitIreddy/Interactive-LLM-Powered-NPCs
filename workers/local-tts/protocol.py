"""Bounded control-plane framing for npc.local-tts-worker/v1."""

from __future__ import annotations

import json
import struct
import threading
import time
from dataclasses import dataclass
from typing import Any, BinaryIO

PROTOCOL_VERSION = "npc.local-tts-worker/v1"
MAX_FRAME_BYTES = 1_048_576
MAX_TEXT_UTF8_BYTES = 262_144
MAX_ID_UTF8_BYTES = 128
MAX_SEEN_REQUESTS = 4096


class ProtocolError(ValueError):
    def __init__(
        self,
        code: str,
        message: str,
        *,
        retryable: bool = False,
        details: dict[str, object] | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.retryable = retryable
        self.details = details or {}


class EndOfStream(EOFError):
    pass


def validate_text(
    value: object,
    field: str,
    *,
    maximum: int = MAX_TEXT_UTF8_BYTES,
    allow_empty: bool = False,
) -> str:
    if not isinstance(value, str):
        raise ProtocolError("invalid_payload", f"{field} must be text")
    size = len(value.encode("utf-8"))
    if (not allow_empty and size == 0) or size > maximum:
        raise ProtocolError("invalid_payload", f"{field} exceeds its bounded text contract")
    if "\x00" in value:
        raise ProtocolError("invalid_payload", f"{field} cannot contain NUL")
    return value


def validate_id(value: object, field: str, *, allow_empty: bool = False) -> str:
    value = validate_text(
        value,
        field,
        maximum=MAX_ID_UTF8_BYTES,
        allow_empty=allow_empty,
    )
    if any(ord(character) < 0x20 for character in value):
        raise ProtocolError("invalid_payload", f"{field} contains control characters")
    return value


def _read_exact(stream: BinaryIO, size: int) -> bytes:
    chunks: list[bytes] = []
    remaining = size
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            if remaining == size:
                raise EndOfStream
            raise ProtocolError("invalid_frame", "truncated local TTS frame")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_frame(stream: BinaryIO) -> dict[str, Any]:
    prefix = _read_exact(stream, 4)
    size = struct.unpack(">I", prefix)[0]
    if size == 0 or size > MAX_FRAME_BYTES:
        raise ProtocolError("invalid_frame", "local TTS frame size is invalid")
    payload = _read_exact(stream, size)
    try:
        raw = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ProtocolError("invalid_frame", "local TTS frame is not UTF-8 JSON") from error
    if not isinstance(raw, dict):
        raise ProtocolError("invalid_frame", "local TTS frame root must be an object")
    return raw


class FrameWriter:
    def __init__(self, stream: BinaryIO) -> None:
        self.stream = stream
        self.lock = threading.Lock()

    def write(self, payload: dict[str, object]) -> None:
        encoded = json.dumps(
            payload,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
        if not encoded or len(encoded) > MAX_FRAME_BYTES:
            raise ProtocolError("event_limit_exceeded", "local TTS event exceeds frame limit")
        with self.lock:
            self.stream.write(struct.pack(">I", len(encoded)))
            self.stream.write(encoded)
            self.stream.flush()


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
    def parse(cls, raw: dict[str, Any]) -> "Request":
        expected = {
            "protocol_version",
            "worker_instance_id",
            "request_id",
            "sequence",
            "generation",
            "deadline_unix_ms",
            "operation",
            "payload",
        }
        if set(raw) != expected:
            raise ProtocolError("invalid_request", "request envelope fields are invalid")
        if raw.get("protocol_version") != PROTOCOL_VERSION:
            raise ProtocolError("unsupported_protocol", "local TTS protocol version is unsupported")
        sequence = raw.get("sequence")
        generation = raw.get("generation")
        deadline = raw.get("deadline_unix_ms")
        for value, field in (
            (sequence, "sequence"),
            (generation, "generation"),
            (deadline, "deadline_unix_ms"),
        ):
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ProtocolError("invalid_request", f"{field} must be a non-negative integer")
        payload = raw.get("payload")
        if not isinstance(payload, dict):
            raise ProtocolError("invalid_payload", "payload must be an object")
        request = cls(
            protocol_version=PROTOCOL_VERSION,
            worker_instance_id=validate_id(
                raw.get("worker_instance_id"),
                "worker_instance_id",
                allow_empty=True,
            ),
            request_id=validate_id(raw.get("request_id"), "request_id"),
            sequence=sequence,
            generation=generation,
            deadline_unix_ms=deadline,
            operation=validate_id(raw.get("operation"), "operation"),
            payload=payload,
        )
        if request.deadline_unix_ms and request.deadline_unix_ms <= int(time.time() * 1000):
            raise ProtocolError("deadline_exceeded", "request deadline already elapsed")
        return request


def event_envelope(
    request: Request,
    worker_instance_id: str,
    event: str,
    *,
    terminal: bool,
    event_index: int,
    payload: dict[str, object] | None = None,
    error: ProtocolError | None = None,
) -> dict[str, object]:
    result: dict[str, object] = {
        "protocol_version": PROTOCOL_VERSION,
        "worker_instance_id": worker_instance_id,
        "request_id": request.request_id,
        "sequence": request.sequence,
        "generation": request.generation,
        "event_index": event_index,
        "event": event,
        "terminal": terminal,
        "payload": payload or {},
    }
    if error is not None:
        result["error"] = {
            "code": error.code,
            "message": str(error),
            "retryable": error.retryable,
            "details": error.details,
        }
    return result
