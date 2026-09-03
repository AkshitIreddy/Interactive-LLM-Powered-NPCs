from __future__ import annotations

import io
import json
import struct
from typing import Any, BinaryIO

from .contract import (
    MAX_CONTROL_FRAME_BYTES,
    MAX_PCM_FRAME_BYTES,
    MAX_PCM_METADATA_BYTES,
    PcmFrame,
    ProtocolFault,
)


def _read_exact(stream: BinaryIO, length: int) -> bytes:
    chunks: list[bytes] = []
    remaining = length
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise EOFError("truncated frame")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_control_frame(stream: BinaryIO) -> dict[str, Any] | None:
    prefix = stream.read(4)
    if prefix == b"":
        return None
    if len(prefix) != 4:
        raise ProtocolFault("invalid_frame", "truncated control frame prefix")
    (length,) = struct.unpack(">I", prefix)
    if length == 0 or length > MAX_CONTROL_FRAME_BYTES:
        raise ProtocolFault("invalid_frame", "control frame length is invalid")
    try:
        payload = _read_exact(stream, length).decode("utf-8")
        value = json.loads(payload)
    except (EOFError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolFault("invalid_frame", "control frame is not valid UTF-8 JSON") from exc
    if not isinstance(value, dict):
        raise ProtocolFault("invalid_frame", "control frame root must be an object")
    return value


def write_control_frame(stream: BinaryIO, value: dict[str, Any]) -> None:
    payload = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if not payload or len(payload) > MAX_CONTROL_FRAME_BYTES:
        raise ProtocolFault("invalid_frame", "encoded response exceeds the control frame limit")
    stream.write(struct.pack(">I", len(payload)))
    stream.write(payload)
    stream.flush()


def encode_control_frame(value: dict[str, Any]) -> bytes:
    stream = io.BytesIO()
    write_control_frame(stream, value)
    return stream.getvalue()


def read_pcm_frame(stream: BinaryIO) -> PcmFrame | None:
    prefix = stream.read(8)
    if prefix == b"":
        return None
    if len(prefix) != 8:
        raise ProtocolFault("invalid_pcm_frame", "truncated PCM frame prefix")
    metadata_length, pcm_length = struct.unpack(">II", prefix)
    if metadata_length == 0 or metadata_length > MAX_PCM_METADATA_BYTES:
        raise ProtocolFault("invalid_pcm_frame", "PCM metadata length is invalid")
    if pcm_length == 0 or pcm_length > MAX_PCM_FRAME_BYTES:
        raise ProtocolFault("invalid_pcm_frame", "PCM payload length is invalid")
    try:
        metadata = json.loads(_read_exact(stream, metadata_length).decode("utf-8"))
        pcm = _read_exact(stream, pcm_length)
    except (EOFError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolFault("invalid_pcm_frame", "PCM frame is malformed") from exc
    return PcmFrame.parse(metadata, pcm)


def encode_pcm_frame(metadata: dict[str, Any], pcm: bytes) -> bytes:
    encoded_metadata = json.dumps(metadata, separators=(",", ":")).encode("utf-8")
    if not encoded_metadata or len(encoded_metadata) > MAX_PCM_METADATA_BYTES:
        raise ProtocolFault("invalid_pcm_frame", "encoded PCM metadata exceeds its limit")
    if not pcm or len(pcm) > MAX_PCM_FRAME_BYTES:
        raise ProtocolFault("invalid_pcm_frame", "encoded PCM payload exceeds its limit")
    return struct.pack(">II", len(encoded_metadata), len(pcm)) + encoded_metadata + pcm
