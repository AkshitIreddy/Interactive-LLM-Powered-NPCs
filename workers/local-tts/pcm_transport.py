"""Out-of-band PCM records written only to a supervisor-created endpoint."""

from __future__ import annotations

import hashlib
import json
import math
import os
import struct
import threading
from array import array
from dataclasses import dataclass
from typing import BinaryIO, Iterable

PCM_PROTOCOL = "npc.local-tts-pcm/v1"
PCM_MAGIC = b"NPCTTS01"
MAX_HEADER_BYTES = 16_384
MAX_PCM_BYTES = 262_144
MAX_CHUNK_FRAMES = 2_400


class PcmTransportError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class PcmChunkReceipt:
    stream_id: str
    request_id: str
    generation: int
    chunk_sequence: int
    sample_start: int
    frame_count: int
    sample_rate_hz: int
    pcm_sha256: str
    clipped_samples: int

    def public_metadata(self) -> dict[str, object]:
        return {
            "stream_id": self.stream_id,
            "chunk_sequence": self.chunk_sequence,
            "sample_start": self.sample_start,
            "frame_count": self.frame_count,
            "sample_rate_hz": self.sample_rate_hz,
            "channels": 1,
            "sample_format": "pcm_s16le",
            "pcm_sha256": self.pcm_sha256,
            "clipped_samples": self.clipped_samples,
            "timing_basis": "exact_output_sample_clock",
        }


def floats_to_pcm16(samples: Iterable[float]) -> tuple[bytes, int]:
    converted = array("h")
    clipped = 0
    for sample in samples:
        value = float(sample)
        if not math.isfinite(value):
            raise PcmTransportError("non-finite synthesis sample")
        if value > 1.0:
            value = 1.0
            clipped += 1
        elif value < -1.0:
            value = -1.0
            clipped += 1
        integer = -32768 if value <= -1.0 else int(round(value * 32767.0))
        converted.append(integer)
    if os.sys.byteorder != "little":
        converted.byteswap()
    return converted.tobytes(), clipped


class PcmWriter:
    def __init__(self, stream: BinaryIO) -> None:
        self.stream = stream
        self.lock = threading.Lock()
        self.closed_streams: set[str] = set()

    def _write_record(self, header: dict[str, object], pcm: bytes) -> None:
        encoded = json.dumps(
            header,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
        if not encoded or len(encoded) > MAX_HEADER_BYTES:
            raise PcmTransportError("PCM header exceeds bounded size")
        if len(pcm) > MAX_PCM_BYTES:
            raise PcmTransportError("PCM payload exceeds bounded size")
        with self.lock:
            self.stream.write(PCM_MAGIC)
            self.stream.write(struct.pack(">II", len(encoded), len(pcm)))
            self.stream.write(encoded)
            self.stream.write(pcm)
            self.stream.flush()

    def write_samples(
        self,
        *,
        stream_id: str,
        request_id: str,
        generation: int,
        chunk_sequence: int,
        sample_start: int,
        sample_rate_hz: int,
        samples: Iterable[float],
    ) -> list[PcmChunkReceipt]:
        values = list(samples)
        receipts: list[PcmChunkReceipt] = []
        offset = 0
        sequence = chunk_sequence
        while offset < len(values):
            part = values[offset : offset + MAX_CHUNK_FRAMES]
            pcm, clipped = floats_to_pcm16(part)
            receipt = PcmChunkReceipt(
                stream_id=stream_id,
                request_id=request_id,
                generation=generation,
                chunk_sequence=sequence,
                sample_start=sample_start + offset,
                frame_count=len(part),
                sample_rate_hz=sample_rate_hz,
                pcm_sha256=hashlib.sha256(pcm).hexdigest(),
                clipped_samples=clipped,
            )
            header = {
                "protocol": PCM_PROTOCOL,
                "record": "chunk",
                "stream_id": stream_id,
                "request_id": request_id,
                "generation": generation,
                **receipt.public_metadata(),
            }
            self._write_record(header, pcm)
            receipts.append(receipt)
            offset += len(part)
            sequence += 1
        return receipts

    def finish(
        self,
        *,
        stream_id: str,
        request_id: str,
        generation: int,
        final_frames: int,
        status: str,
    ) -> None:
        if status not in {"completed", "cancelled", "failed"}:
            raise PcmTransportError("invalid terminal PCM status")
        with self.lock:
            if stream_id in self.closed_streams:
                raise PcmTransportError("PCM stream already terminated")
            self.closed_streams.add(stream_id)
        self._write_record(
            {
                "protocol": PCM_PROTOCOL,
                "record": "end",
                "stream_id": stream_id,
                "request_id": request_id,
                "generation": generation,
                "final_frames": final_frames,
                "status": status,
            },
            b"",
        )


def read_pcm_record(stream: BinaryIO) -> tuple[dict[str, object], bytes]:
    magic = stream.read(len(PCM_MAGIC))
    if magic != PCM_MAGIC:
        raise PcmTransportError("invalid PCM record magic")
    prefix = stream.read(8)
    if len(prefix) != 8:
        raise PcmTransportError("truncated PCM record prefix")
    header_size, pcm_size = struct.unpack(">II", prefix)
    if not 0 < header_size <= MAX_HEADER_BYTES or pcm_size > MAX_PCM_BYTES:
        raise PcmTransportError("invalid PCM record size")
    header_bytes = stream.read(header_size)
    pcm = stream.read(pcm_size)
    if len(header_bytes) != header_size or len(pcm) != pcm_size:
        raise PcmTransportError("truncated PCM record")
    try:
        header = json.loads(header_bytes.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PcmTransportError("invalid PCM record header") from error
    if not isinstance(header, dict) or header.get("protocol") != PCM_PROTOCOL:
        raise PcmTransportError("unsupported PCM record header")
    return header, pcm
