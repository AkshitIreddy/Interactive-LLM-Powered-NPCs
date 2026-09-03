"""Backend interface and deterministic non-production test backend."""

from __future__ import annotations

import math
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Protocol


@dataclass(frozen=True, slots=True)
class BackendIdentity:
    runtime_version: str
    runtime_git_sha: str
    onnxruntime_version: str
    sample_rate_hz: int
    speaker_count: int


@dataclass(frozen=True, slots=True)
class SynthesisSummary:
    generated_frames: int
    callbacks: int
    cancelled: bool


AudioCallback = Callable[[list[float], float], bool]


class TtsBackend(Protocol):
    def load(self, pack_root: Path, *, num_threads: int) -> BackendIdentity: ...
    def synthesize(
        self,
        text: str,
        *,
        speaker_id: int,
        speed: float,
        silence_scale: float,
        callback: AudioCallback,
        cancel: threading.Event,
    ) -> SynthesisSummary: ...
    def unload(self) -> None: ...


class FixtureBackend:
    """Deterministic protocol fixture; never selected by the worker CLI."""

    production = False

    def __init__(
        self,
        *,
        callback_delay_seconds: float = 0.0,
        callbacks: int = 4,
        frames_per_callback: int = 480,
    ) -> None:
        self.callback_delay_seconds = callback_delay_seconds
        self.callback_count = callbacks
        self.frames_per_callback = frames_per_callback
        self.loaded = False

    def load(self, pack_root: Path, *, num_threads: int) -> BackendIdentity:
        del pack_root, num_threads
        self.loaded = True
        return BackendIdentity(
            runtime_version="1.13.6",
            runtime_git_sha="1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911",
            onnxruntime_version="1.27.1",
            sample_rate_hz=24000,
            speaker_count=53,
        )

    def synthesize(
        self,
        text: str,
        *,
        speaker_id: int,
        speed: float,
        silence_scale: float,
        callback: AudioCallback,
        cancel: threading.Event,
    ) -> SynthesisSummary:
        del text, speed, silence_scale
        if not self.loaded:
            raise RuntimeError("fixture backend is not loaded")
        generated = 0
        callbacks = 0
        frequency = 180.0 + speaker_id * 3.0
        for index in range(self.callback_count):
            if cancel.is_set():
                return SynthesisSummary(generated, callbacks, True)
            samples = [
                math.sin(
                    2.0
                    * math.pi
                    * frequency
                    * (generated + sample_index)
                    / 24000.0
                )
                * 0.15
                for sample_index in range(self.frames_per_callback)
            ]
            if self.callback_delay_seconds:
                time.sleep(self.callback_delay_seconds)
            callbacks += 1
            generated += len(samples)
            if not callback(samples, (index + 1) / self.callback_count):
                return SynthesisSummary(generated, callbacks, True)
        return SynthesisSummary(generated, callbacks, False)

    def unload(self) -> None:
        self.loaded = False
