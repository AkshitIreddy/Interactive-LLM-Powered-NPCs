from __future__ import annotations

from abc import ABC, abstractmethod
import ctypes
from dataclasses import dataclass, field
import json
import math
from pathlib import Path
import struct
import threading
import time
from typing import Any

from .contract import MAX_TEXT_BYTES, PcmFrame, ProtocolFault


@dataclass(frozen=True)
class WordTiming:
    text: str
    start_ms: int
    end_ms: int
    confidence: float | None

    def as_dict(self) -> dict[str, Any]:
        return {
            "text": self.text,
            "startMs": self.start_ms,
            "endMs": self.end_ms,
            "confidence": self.confidence,
        }


@dataclass(frozen=True)
class TranscriptLine:
    utterance_id: str
    text: str
    start_ms: int
    end_ms: int
    complete: bool
    changed: bool
    upstream_latency_ms: int | None
    words: tuple[WordTiming, ...] = field(default_factory=tuple)

    def as_dict(self) -> dict[str, Any]:
        return {
            "utteranceId": self.utterance_id,
            "text": self.text,
            "startMs": self.start_ms,
            "endMs": self.end_ms,
            "complete": self.complete,
            "changed": self.changed,
            "upstreamLatencyMs": self.upstream_latency_ms,
            "words": [word.as_dict() for word in self.words],
        }


@dataclass(frozen=True)
class BackendDelta:
    lines: tuple[TranscriptLine, ...] = field(default_factory=tuple)
    speech_started: bool = False
    speech_ended: bool = False
    inference_ms: float = 0.0
    analyzed_audio_ms: int = 0


class SttBackend(ABC):
    backend_name: str
    fixture: bool = False

    @abstractmethod
    def warm(self) -> None: ...

    @abstractmethod
    def load(self) -> None: ...

    @abstractmethod
    def unload(self) -> None: ...

    @abstractmethod
    def start_session(self, session_id: str, word_timestamps: bool) -> None: ...

    @abstractmethod
    def add_pcm(self, frame: PcmFrame) -> BackendDelta: ...

    @abstractmethod
    def end_session(self, reason: str) -> BackendDelta: ...

    @abstractmethod
    def cancel(self) -> None: ...

    @abstractmethod
    def self_test(self) -> dict[str, Any]: ...


class FixtureMoonshineBackend(SttBackend):
    """Deterministic protocol backend. It never claims model quality or speed."""

    backend_name = "moonshine-fixture"
    fixture = True

    def __init__(self, transcript: str = "Can you hear me clearly") -> None:
        self._fixture_transcript = transcript
        self._loaded = False
        self._session_id: str | None = None
        self._word_timestamps = True
        self._total_samples = 0
        self._speech_samples = 0
        self._sample_rate = 16_000
        self._speech_active = False
        self._silent_chunks = 0
        self._last_text = ""
        self._utterance_index = 0
        self._cancelled = False

    def warm(self) -> None:
        return None

    def load(self) -> None:
        self._loaded = True

    def unload(self) -> None:
        self.cancel()
        self._loaded = False

    def start_session(self, session_id: str, word_timestamps: bool) -> None:
        if not self._loaded:
            raise ProtocolFault("model_not_loaded", "local STT model is not loaded")
        if self._session_id is not None:
            raise ProtocolFault("worker_busy", "a local STT session is already active", retryable=True)
        self._session_id = session_id
        self._word_timestamps = word_timestamps
        self._total_samples = 0
        self._speech_samples = 0
        self._speech_active = False
        self._silent_chunks = 0
        self._last_text = ""
        self._cancelled = False
        self._utterance_index += 1

    @staticmethod
    def _rms(pcm: bytes) -> float:
        count = len(pcm) // 2
        if not count:
            return 0.0
        squares = 0.0
        for (sample,) in struct.iter_unpack("<h", pcm):
            normalized = sample / 32768.0
            squares += normalized * normalized
        return math.sqrt(squares / count)

    def _line(self, *, complete: bool) -> TranscriptLine:
        words = self._fixture_transcript.split()
        speech_ms = max(1, round(self._speech_samples * 1000 / self._sample_rate))
        if complete:
            shown = words
        else:
            proportion = min(1.0, max(0.25, speech_ms / 800.0))
            shown = words[: max(1, math.ceil(len(words) * proportion))]
        text = " ".join(shown)
        start_ms = round((self._total_samples - self._speech_samples) * 1000 / self._sample_rate)
        word_timings: list[WordTiming] = []
        if self._word_timestamps and shown:
            width = speech_ms / len(shown)
            for index, word in enumerate(shown):
                word_timings.append(
                    WordTiming(
                        text=word,
                        start_ms=start_ms + round(index * width),
                        end_ms=start_ms + round((index + 1) * width),
                        confidence=0.99,
                    )
                )
        line = TranscriptLine(
            utterance_id=f"fixture-{self._utterance_index:06d}",
            text=text,
            start_ms=start_ms,
            end_ms=start_ms + speech_ms,
            complete=complete,
            changed=text != self._last_text or complete,
            upstream_latency_ms=0,
            words=tuple(word_timings),
        )
        self._last_text = text
        return line

    def add_pcm(self, frame: PcmFrame) -> BackendDelta:
        if self._session_id != frame.session_id:
            raise ProtocolFault("session_mismatch", "PCM frame does not belong to the active session")
        if self._cancelled:
            raise ProtocolFault("stale_generation", "PCM arrived after cancellation")
        started = time.perf_counter()
        self._sample_rate = frame.sample_rate_hz
        self._total_samples += frame.sample_count
        duration_ms = round(frame.sample_count * 1000 / frame.sample_rate_hz)
        voice = self._rms(frame.pcm) >= 0.015
        speech_started = False
        speech_ended = False
        lines: list[TranscriptLine] = []
        if voice:
            if not self._speech_active:
                self._speech_active = True
                self._speech_samples = 0
                self._last_text = ""
                speech_started = True
            self._silent_chunks = 0
            self._speech_samples += frame.sample_count
            if self._speech_samples * 1000 // frame.sample_rate_hz >= 150:
                lines.append(self._line(complete=False))
        elif self._speech_active:
            self._silent_chunks += 1
            if self._silent_chunks >= 3:
                lines.append(self._line(complete=True))
                self._speech_active = False
                self._silent_chunks = 0
                speech_ended = True
        return BackendDelta(
            lines=tuple(lines),
            speech_started=speech_started,
            speech_ended=speech_ended,
            inference_ms=(time.perf_counter() - started) * 1000,
            analyzed_audio_ms=duration_ms,
        )

    def end_session(self, reason: str) -> BackendDelta:
        if self._session_id is None:
            raise ProtocolFault("invalid_state", "no local STT session is active")
        lines: tuple[TranscriptLine, ...] = ()
        ended = False
        if self._speech_active:
            lines = (self._line(complete=True),)
            ended = True
        self._session_id = None
        self._speech_active = False
        return BackendDelta(lines=lines, speech_ended=ended, inference_ms=0.0)

    def cancel(self) -> None:
        self._cancelled = True
        self._session_id = None
        self._speech_active = False

    def self_test(self) -> dict[str, Any]:
        return {
            "backend": self.backend_name,
            "fixtureOnly": True,
            "framing": "passed",
            "lifecycle": "passed",
            "modelInference": "not_run",
        }


class MoonshineBridgeBackend(SttBackend):
    """ctypes adapter for the native bridge built from the pinned Moonshine SDK."""

    backend_name = "moonshine-v0.1.5-native-cpu"
    fixture = False

    def __init__(
        self,
        bridge_path: Path,
        model_path: Path,
        *,
        model_arch: int = 5,
        update_interval_ms: int = 250,
        vad_threshold: float = 0.5,
    ) -> None:
        self._bridge_path = bridge_path.resolve(strict=False)
        self._model_path = model_path.resolve(strict=False)
        self._model_arch = model_arch
        self._update_interval_ms = update_interval_ms
        self._vad_threshold = vad_threshold
        self._library: ctypes.CDLL | None = None
        self._handle = ctypes.c_void_p()
        self._session_id: str | None = None
        self._lock = threading.Lock()

    def warm(self) -> None:
        if not self._bridge_path.is_file():
            raise ProtocolFault("bridge_unavailable", "native local STT bridge is not installed")
        if self._library is None:
            try:
                library = ctypes.WinDLL(str(self._bridge_path)) if hasattr(ctypes, "WinDLL") else ctypes.CDLL(str(self._bridge_path))
                self._configure_abi(library)
                self._library = library
            except ProtocolFault:
                raise
            except (AttributeError, OSError, TypeError, ValueError) as exc:
                raise ProtocolFault("bridge_unavailable", "native local STT bridge could not be loaded") from exc

    @staticmethod
    def _configure_abi(lib: ctypes.CDLL) -> None:
        lib.npc_stt_bridge_abi_version.argtypes = []
        lib.npc_stt_bridge_abi_version.restype = ctypes.c_int32
        abi_version = int(lib.npc_stt_bridge_abi_version())
        if abi_version != 1:
            raise ProtocolFault("bridge_abi_mismatch", "native local STT bridge ABI is incompatible")
        lib.npc_stt_create.argtypes = [
            ctypes.c_char_p,
            ctypes.c_int32,
            ctypes.c_int32,
            ctypes.c_float,
            ctypes.POINTER(ctypes.c_void_p),
        ]
        lib.npc_stt_create.restype = ctypes.c_int32
        lib.npc_stt_destroy.argtypes = [ctypes.c_void_p]
        lib.npc_stt_destroy.restype = None
        lib.npc_stt_start.argtypes = [ctypes.c_void_p, ctypes.c_int32]
        lib.npc_stt_start.restype = ctypes.c_int32
        lib.npc_stt_push_pcm16.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_int16),
            ctypes.c_uint64,
            ctypes.c_int32,
        ]
        lib.npc_stt_push_pcm16.restype = ctypes.c_int32
        lib.npc_stt_poll_json.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
        lib.npc_stt_poll_json.restype = ctypes.c_int32
        lib.npc_stt_stop_json.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
        lib.npc_stt_stop_json.restype = ctypes.c_int32
        lib.npc_stt_free_json.argtypes = [ctypes.c_void_p]
        lib.npc_stt_free_json.restype = None
        lib.npc_stt_cancel.argtypes = [ctypes.c_void_p]
        lib.npc_stt_cancel.restype = ctypes.c_int32
        lib.npc_stt_last_error.argtypes = [ctypes.c_void_p]
        lib.npc_stt_last_error.restype = ctypes.c_char_p

    def _check(self, result: int) -> None:
        if result == 0:
            return
        message = "native local STT bridge failed"
        if self._library is not None:
            try:
                raw = self._library.npc_stt_last_error(self._handle)
                if raw:
                    message = raw.decode("utf-8", errors="replace")[:512]
            except (AttributeError, OSError, TypeError, ValueError):
                pass
        # Moonshine v0.1.5 defines -4 as its transient BUSY error. Invalid
        # argument (-3) and all other native failures require correction or a
        # clean reload rather than blind retry.
        raise ProtocolFault("inference_failed", message, retryable=result == -4)

    def _call(self, name: str, *arguments: Any) -> Any:
        if self._library is None:
            raise ProtocolFault("bridge_unavailable", "native local STT bridge is not loaded")
        try:
            return getattr(self._library, name)(*arguments)
        except (AttributeError, OSError, TypeError, ValueError) as exc:
            raise ProtocolFault("bridge_call_failed", "native local STT bridge call failed") from exc

    def load(self) -> None:
        self.warm()
        if not self._model_path.is_dir():
            raise ProtocolFault("model_pack_invalid", "verified local STT model directory is unavailable")
        assert self._library is not None
        with self._lock:
            self._check(
                self._call(
                    "npc_stt_create",
                    str(self._model_path).encode("utf-8"),
                    self._model_arch,
                    self._update_interval_ms,
                    self._vad_threshold,
                    ctypes.byref(self._handle),
                )
            )

    def unload(self) -> None:
        with self._lock:
            if self._library is not None and self._handle:
                self._call("npc_stt_destroy", self._handle)
            self._handle = ctypes.c_void_p()
            self._session_id = None

    def start_session(self, session_id: str, word_timestamps: bool) -> None:
        if not self._library or not self._handle:
            raise ProtocolFault("model_not_loaded", "local STT model is not loaded")
        with self._lock:
            self._check(self._call("npc_stt_start", self._handle, 1 if word_timestamps else 0))
            self._session_id = session_id

    def _parse_native_json(self, pointer: ctypes.c_void_p, elapsed_ms: float) -> BackendDelta:
        if not pointer.value or self._library is None:
            return BackendDelta(inference_ms=elapsed_ms)
        try:
            try:
                raw = ctypes.string_at(pointer)
                if len(raw) > 4 * 1024 * 1024:
                    raise ValueError("native JSON is not bounded")
                payload = json.loads(raw.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as exc:
                raise ProtocolFault("bridge_output_invalid", "native local STT output is invalid") from exc
        finally:
            self._call("npc_stt_free_json", pointer)
        if not isinstance(payload, dict) or not isinstance(payload.get("lines", []), list):
            raise ProtocolFault("bridge_output_invalid", "native local STT output has an invalid shape")
        if len(payload.get("lines", [])) > 4096:
            raise ProtocolFault("bridge_output_invalid", "native local STT output has too many lines")
        lines: list[TranscriptLine] = []
        try:
            for item in payload.get("lines", []):
                if not isinstance(item, dict) or not isinstance(item.get("words", []), list):
                    raise ValueError("line is invalid")
                if len(item.get("words", [])) > 4096:
                    raise ValueError("too many words")
                text = item["text"]
                utterance_id = item["utteranceId"]
                if (
                    not isinstance(text, str)
                    or len(text.encode("utf-8")) > MAX_TEXT_BYTES
                    or not isinstance(utterance_id, str)
                    or len(utterance_id) > 128
                ):
                    raise ValueError("line text or ID is invalid")
                line_complete = item.get("complete")
                if not isinstance(line_complete, bool) or not isinstance(item.get("changed", True), bool):
                    raise ValueError("line flags are invalid")
                words_list: list[WordTiming] = []
                for word in item.get("words", []):
                    if not isinstance(word, dict) or not isinstance(word.get("text"), str):
                        raise ValueError("word is invalid")
                    start_ms = int(word["startMs"])
                    end_ms = int(word["endMs"])
                    if start_ms < 0 or end_ms < 0:
                        raise ValueError("word timing is invalid")
                    # Moonshine v0.1.5 can expose a word whose acoustic end has
                    # not caught up with its start, including on a line it marks
                    # complete. Represent that boundary as a zero-duration word;
                    # negative values and all line-level timing remain fail-closed.
                    if end_ms < start_ms:
                        end_ms = start_ms
                    confidence = float(word["confidence"]) if word.get("confidence") is not None else None
                    if confidence is not None and (not math.isfinite(confidence) or not 0.0 <= confidence <= 1.0):
                        raise ValueError("word confidence is invalid")
                    words_list.append(
                        WordTiming(
                            text=word["text"],
                            start_ms=start_ms,
                            end_ms=end_ms,
                            confidence=confidence,
                        )
                    )
                start_ms = int(item["startMs"])
                end_ms = int(item["endMs"])
                upstream_latency = (
                    int(item["upstreamLatencyMs"])
                    if item.get("upstreamLatencyMs") is not None
                    else None
                )
                if start_ms < 0 or end_ms < start_ms or (upstream_latency is not None and upstream_latency < 0):
                    raise ValueError("line timing is invalid")
                lines.append(
                    TranscriptLine(
                        utterance_id=utterance_id,
                        text=text,
                        start_ms=start_ms,
                        end_ms=end_ms,
                        complete=line_complete,
                        changed=item.get("changed", True),
                        upstream_latency_ms=upstream_latency,
                        words=tuple(words_list),
                    )
                )
            speech_started = payload.get("speechStarted", False)
            speech_ended = payload.get("speechEnded", False)
            analyzed_audio_ms = int(payload.get("analyzedAudioMs", 0))
            if not isinstance(speech_started, bool) or not isinstance(speech_ended, bool) or analyzed_audio_ms < 0:
                raise ValueError("delta metadata is invalid")
            return BackendDelta(
                lines=tuple(lines),
                speech_started=speech_started,
                speech_ended=speech_ended,
                inference_ms=elapsed_ms,
                analyzed_audio_ms=analyzed_audio_ms,
            )
        except (KeyError, TypeError, ValueError, OverflowError) as exc:
            raise ProtocolFault("bridge_output_invalid", "native local STT output has an invalid shape") from exc

    def add_pcm(self, frame: PcmFrame) -> BackendDelta:
        if self._session_id != frame.session_id:
            raise ProtocolFault("session_mismatch", "PCM frame does not belong to the active session")
        assert self._library is not None
        samples = (ctypes.c_int16 * frame.sample_count).from_buffer_copy(frame.pcm)
        started = time.perf_counter()
        with self._lock:
            self._check(self._call("npc_stt_push_pcm16", self._handle, samples, frame.sample_count, frame.sample_rate_hz))
            pointer = ctypes.c_void_p()
            self._check(self._call("npc_stt_poll_json", self._handle, ctypes.byref(pointer)))
        return self._parse_native_json(pointer, (time.perf_counter() - started) * 1000)

    def end_session(self, reason: str) -> BackendDelta:
        if self._session_id is None:
            raise ProtocolFault("invalid_state", "no local STT session is active")
        assert self._library is not None
        pointer = ctypes.c_void_p()
        started = time.perf_counter()
        with self._lock:
            self._check(self._call("npc_stt_stop_json", self._handle, ctypes.byref(pointer)))
            self._session_id = None
        return self._parse_native_json(pointer, (time.perf_counter() - started) * 1000)

    def cancel(self) -> None:
        if self._library is not None and self._handle:
            with self._lock:
                self._check(self._call("npc_stt_cancel", self._handle))
        self._session_id = None

    def self_test(self) -> dict[str, Any]:
        self.warm()
        assert self._library is not None
        return {
            "backend": self.backend_name,
            "bridgeLoaded": True,
            "modelInference": "requires_explicit_qualification_session",
        }
