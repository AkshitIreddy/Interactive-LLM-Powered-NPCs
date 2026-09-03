"""Length-delimited Worker Control v1 JSON framing."""

from __future__ import annotations

import json
import struct
import threading
from typing import Any, BinaryIO

from .constants import MAX_FRAME_BYTES
from .digest import canonical_json_bytes
from .errors import LocalLlmError


class FrameIO:
    def __init__(self, input_stream: BinaryIO, output_stream: BinaryIO) -> None:
        self.input = input_stream
        self.output = output_stream
        self.output_lock = threading.Lock()

    def read(self) -> Any | None:
        header = self._read_exact(4, allow_eof=True)
        if header is None:
            return None
        length = struct.unpack(">I", header)[0]
        if length == 0 or length > MAX_FRAME_BYTES:
            raise LocalLlmError("invalid_frame", "worker frame length is invalid")
        payload = self._read_exact(length, allow_eof=False)
        assert payload is not None
        try:
            value = json.loads(payload)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise LocalLlmError("invalid_frame", "worker frame is not valid UTF-8 JSON") from error
        return value

    def write(self, value: Any) -> None:
        payload = canonical_json_bytes(value)
        if not payload or len(payload) > MAX_FRAME_BYTES:
            raise LocalLlmError("invalid_frame", "worker output frame length is invalid")
        framed = struct.pack(">I", len(payload)) + payload
        with self.output_lock:
            self.output.write(framed)
            self.output.flush()

    def _read_exact(self, length: int, *, allow_eof: bool) -> bytes | None:
        output = bytearray()
        while len(output) < length:
            chunk = self.input.read(length - len(output))
            if not chunk:
                if allow_eof and not output:
                    return None
                raise LocalLlmError("invalid_frame", "worker frame ended unexpectedly")
            output.extend(chunk)
        return bytes(output)
