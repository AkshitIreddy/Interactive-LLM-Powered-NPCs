"""Bounded length-delimited JSON framing for the identity worker."""

from __future__ import annotations

import json
import struct
import threading
from typing import Any, BinaryIO

from model_spec import MAX_FRAME_BYTES


class EndOfStream(EOFError):
    pass


class FramingError(ValueError):
    pass


def _read_exact(stream: BinaryIO, length: int) -> bytes:
    result = bytearray()
    while len(result) < length:
        chunk = stream.read(length - len(result))
        if not chunk:
            raise EndOfStream()
        result.extend(chunk)
    return bytes(result)


def read_frame(stream: BinaryIO) -> dict[str, Any]:
    prefix = stream.read(4)
    if not prefix:
        raise EndOfStream()
    if len(prefix) != 4:
        raise FramingError("truncated frame prefix")
    length = struct.unpack(">I", prefix)[0]
    if not 1 <= length <= MAX_FRAME_BYTES:
        raise FramingError("frame length is outside 1..=1048576 bytes")
    try:
        value = json.loads(_read_exact(stream, length))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise FramingError("frame is not UTF-8 JSON") from exc
    if not isinstance(value, dict):
        raise FramingError("frame root must be an object")
    return value


class FrameWriter:
    def __init__(self, stream: BinaryIO) -> None:
        self._stream = stream
        self._lock = threading.Lock()

    def write(self, value: dict[str, Any]) -> None:
        encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")
        if not 1 <= len(encoded) <= MAX_FRAME_BYTES:
            raise FramingError("outbound frame exceeds its bounded contract")
        with self._lock:
            self._stream.write(struct.pack(">I", len(encoded)))
            self._stream.write(encoded)
            self._stream.flush()

