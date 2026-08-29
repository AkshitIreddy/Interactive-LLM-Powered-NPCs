"""Bounded length-delimited JSON framing.

The length prefix is parsed before allocating the payload buffer. Protocol data
goes to the supplied binary output only; callers keep diagnostics on stderr.
"""

from __future__ import annotations

import json
import struct
import threading
from dataclasses import dataclass, field
from typing import Any, BinaryIO

MAX_FRAME_BYTES = 1_048_576
_LENGTH = struct.Struct(">I")


class FramingError(Exception):
    """Base class for framing errors."""


class EndOfStream(FramingError):
    """Raised when the input closes cleanly between frames."""


class TruncatedFrame(FramingError):
    """Raised when a stream closes part-way through a frame."""


class OversizedFrame(FramingError):
    """Raised before allocating a payload larger than the contract limit."""


class InvalidJsonFrame(FramingError):
    """Raised for invalid UTF-8, JSON, or non-object roots."""


def _read_exact(stream: BinaryIO, count: int) -> bytes:
    chunks: list[bytes] = []
    remaining = count
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise TruncatedFrame(f"stream ended with {remaining} bytes remaining")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_frame(stream: BinaryIO) -> dict[str, Any]:
    prefix = stream.read(_LENGTH.size)
    if prefix == b"":
        raise EndOfStream()
    if len(prefix) != _LENGTH.size:
        raise TruncatedFrame("truncated frame length")
    (length,) = _LENGTH.unpack(prefix)
    if length == 0:
        raise InvalidJsonFrame("empty frames are not valid")
    if length > MAX_FRAME_BYTES:
        raise OversizedFrame(f"frame size {length} exceeds {MAX_FRAME_BYTES}")
    payload = _read_exact(stream, length)
    try:
        value = json.loads(
            payload.decode("utf-8"),
            parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"non-finite number {value}")),
        )
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError, ValueError) as exc:
        raise InvalidJsonFrame("frame is not valid UTF-8 JSON") from exc
    if not isinstance(value, dict):
        raise InvalidJsonFrame("frame root must be an object")
    return value


def encode_frame(value: dict[str, Any]) -> bytes:
    try:
        payload = json.dumps(
            value,
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise InvalidJsonFrame("event is not JSON serializable") from exc
    if not payload or len(payload) > MAX_FRAME_BYTES:
        raise OversizedFrame(f"encoded frame size {len(payload)} exceeds limit")
    return _LENGTH.pack(len(payload)) + payload


@dataclass(slots=True)
class FrameWriter:
    stream: BinaryIO
    _lock: threading.Lock = field(init=False, repr=False)

    def __post_init__(self) -> None:
        self._lock = threading.Lock()

    def write(self, value: dict[str, Any]) -> None:
        frame = encode_frame(value)
        with self._lock:
            self.stream.write(frame)
            self.stream.flush()
