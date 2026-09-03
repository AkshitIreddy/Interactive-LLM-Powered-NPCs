from __future__ import annotations

from collections import OrderedDict
import os
import threading
from typing import BinaryIO

from .contract import MAX_BUFFERED_CHUNKS, PcmFrame, ProtocolFault
from .framing import read_pcm_frame


class PcmFrameStore:
    def __init__(self, maximum_frames: int = MAX_BUFFERED_CHUNKS) -> None:
        self._maximum_frames = maximum_frames
        self._frames: OrderedDict[str, PcmFrame] = OrderedDict()
        self._seen: set[str] = set()
        self._fault: ProtocolFault | None = None
        self._closed = False
        self._lock = threading.Lock()

    def put(self, frame: PcmFrame) -> None:
        with self._lock:
            if self._closed:
                raise ProtocolFault("pcm_channel_closed", "PCM channel is closed")
            if frame.chunk_id in self._seen:
                raise ProtocolFault("duplicate_pcm_chunk", "duplicate PCM chunk was rejected")
            if len(self._frames) >= self._maximum_frames:
                raise ProtocolFault("pcm_backpressure", "PCM ingress queue is full", retryable=True)
            self._frames[frame.chunk_id] = frame
            self._seen.add(frame.chunk_id)

    def take(self, chunk_id: str) -> PcmFrame:
        with self._lock:
            if self._fault is not None:
                raise self._fault
            frame = self._frames.pop(chunk_id, None)
            if frame is None:
                raise ProtocolFault(
                    "pcm_chunk_not_ready",
                    "committed PCM chunk has not arrived on the inherited channel",
                    retryable=True,
                )
            return frame

    def clear(self) -> None:
        with self._lock:
            self._frames.clear()

    def fail(self, fault: ProtocolFault) -> None:
        with self._lock:
            self._fault = fault
            self._frames.clear()

    def close(self) -> None:
        with self._lock:
            self._closed = True
            # Preserve already-delivered frames so their matching control
            # commits can drain after the producer closes its write handle.
            # The queue is bounded and runtime shutdown/cancel calls clear().

    def snapshot(self) -> dict[str, int | bool | str | None]:
        with self._lock:
            return {
                "bufferedFrames": len(self._frames),
                "seenFrames": len(self._seen),
                "closed": self._closed,
                "faultCode": self._fault.code if self._fault else None,
            }


class PcmReaderThread:
    def __init__(self, stream: BinaryIO, store: PcmFrameStore) -> None:
        self._stream = stream
        self._store = store
        self._thread = threading.Thread(target=self._run, name="npc-local-stt-pcm", daemon=True)

    def start(self) -> None:
        self._thread.start()

    def _run(self) -> None:
        try:
            while True:
                frame = read_pcm_frame(self._stream)
                if frame is None:
                    self._store.close()
                    return
                self._store.put(frame)
        except ProtocolFault as fault:
            self._store.fail(fault)
        except (EOFError, OSError) as exc:
            self._store.fail(ProtocolFault("pcm_channel_failed", "PCM channel terminated unexpectedly"))


def open_inherited_pcm_stream() -> BinaryIO | None:
    raw_handle = os.environ.get("NPC_STT_PCM_HANDLE")
    if raw_handle is None:
        return None
    if os.name != "nt":
        raise ProtocolFault("invalid_launch", "inherited PCM handles are supported only on Windows")
    try:
        handle = int(raw_handle, 10)
    except ValueError as exc:
        raise ProtocolFault("invalid_launch", "inherited PCM handle is malformed") from exc
    if handle <= 0:
        raise ProtocolFault("invalid_launch", "inherited PCM handle is invalid")
    import msvcrt

    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
    file_descriptor = msvcrt.open_osfhandle(handle, flags)
    return os.fdopen(file_descriptor, "rb", buffering=0, closefd=True)
