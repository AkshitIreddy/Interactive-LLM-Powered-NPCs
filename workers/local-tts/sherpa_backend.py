"""ctypes adapter for the pinned sherpa-onnx 1.13.6 C ABI."""

from __future__ import annotations

import ctypes
import math
import os
import threading
from pathlib import Path

from backend import AudioCallback, BackendIdentity, SynthesisSummary


class SherpaBackendError(RuntimeError):
    pass


class _Vits(ctypes.Structure):
    _fields_ = [
        ("model", ctypes.c_char_p),
        ("lexicon", ctypes.c_char_p),
        ("tokens", ctypes.c_char_p),
        ("data_dir", ctypes.c_char_p),
        ("noise_scale", ctypes.c_float),
        ("noise_scale_w", ctypes.c_float),
        ("length_scale", ctypes.c_float),
        ("dict_dir", ctypes.c_char_p),
    ]


class _Matcha(ctypes.Structure):
    _fields_ = [
        ("acoustic_model", ctypes.c_char_p),
        ("vocoder", ctypes.c_char_p),
        ("lexicon", ctypes.c_char_p),
        ("tokens", ctypes.c_char_p),
        ("data_dir", ctypes.c_char_p),
        ("noise_scale", ctypes.c_float),
        ("length_scale", ctypes.c_float),
        ("dict_dir", ctypes.c_char_p),
    ]


class _Kokoro(ctypes.Structure):
    _fields_ = [
        ("model", ctypes.c_char_p),
        ("voices", ctypes.c_char_p),
        ("tokens", ctypes.c_char_p),
        ("data_dir", ctypes.c_char_p),
        ("length_scale", ctypes.c_float),
        ("dict_dir", ctypes.c_char_p),
        ("lexicon", ctypes.c_char_p),
        ("lang", ctypes.c_char_p),
    ]


class _Kitten(ctypes.Structure):
    _fields_ = [
        ("model", ctypes.c_char_p),
        ("voices", ctypes.c_char_p),
        ("tokens", ctypes.c_char_p),
        ("data_dir", ctypes.c_char_p),
        ("length_scale", ctypes.c_float),
    ]


class _Zipvoice(ctypes.Structure):
    _fields_ = [
        ("tokens", ctypes.c_char_p),
        ("encoder", ctypes.c_char_p),
        ("decoder", ctypes.c_char_p),
        ("vocoder", ctypes.c_char_p),
        ("data_dir", ctypes.c_char_p),
        ("lexicon", ctypes.c_char_p),
        ("feat_scale", ctypes.c_float),
        ("t_shift", ctypes.c_float),
        ("target_rms", ctypes.c_float),
        ("guidance_scale", ctypes.c_float),
    ]


class _Pocket(ctypes.Structure):
    _fields_ = [
        ("lm_flow", ctypes.c_char_p),
        ("lm_main", ctypes.c_char_p),
        ("encoder", ctypes.c_char_p),
        ("decoder", ctypes.c_char_p),
        ("text_conditioner", ctypes.c_char_p),
        ("vocab_json", ctypes.c_char_p),
        ("token_scores_json", ctypes.c_char_p),
        ("voice_embedding_cache_capacity", ctypes.c_int32),
    ]


class _Supertonic(ctypes.Structure):
    _fields_ = [
        ("duration_predictor", ctypes.c_char_p),
        ("text_encoder", ctypes.c_char_p),
        ("vector_estimator", ctypes.c_char_p),
        ("vocoder", ctypes.c_char_p),
        ("tts_json", ctypes.c_char_p),
        ("unicode_indexer", ctypes.c_char_p),
        ("voice_style", ctypes.c_char_p),
    ]


class _ModelConfig(ctypes.Structure):
    _fields_ = [
        ("vits", _Vits),
        ("num_threads", ctypes.c_int32),
        ("debug", ctypes.c_int32),
        ("provider", ctypes.c_char_p),
        ("matcha", _Matcha),
        ("kokoro", _Kokoro),
        ("kitten", _Kitten),
        ("zipvoice", _Zipvoice),
        ("pocket", _Pocket),
        ("supertonic", _Supertonic),
    ]


class _TtsConfig(ctypes.Structure):
    _fields_ = [
        ("model", _ModelConfig),
        ("rule_fsts", ctypes.c_char_p),
        ("max_num_sentences", ctypes.c_int32),
        ("rule_fars", ctypes.c_char_p),
        ("silence_scale", ctypes.c_float),
    ]


class _GenerationConfig(ctypes.Structure):
    _fields_ = [
        ("silence_scale", ctypes.c_float),
        ("speed", ctypes.c_float),
        ("sid", ctypes.c_int32),
        ("reference_audio", ctypes.POINTER(ctypes.c_float)),
        ("reference_audio_len", ctypes.c_int32),
        ("reference_sample_rate", ctypes.c_int32),
        ("reference_text", ctypes.c_char_p),
        ("num_steps", ctypes.c_int32),
        ("extra", ctypes.c_char_p),
    ]


class _GeneratedAudio(ctypes.Structure):
    _fields_ = [
        ("samples", ctypes.POINTER(ctypes.c_float)),
        ("n", ctypes.c_int32),
        ("sample_rate", ctypes.c_int32),
    ]


_ProgressCallback = ctypes.CFUNCTYPE(
    ctypes.c_int32,
    ctypes.POINTER(ctypes.c_float),
    ctypes.c_int32,
    ctypes.c_float,
    ctypes.c_void_p,
)


class SherpaOnnxBackend:
    production = True

    def __init__(
        self,
        *,
        expected_version: str,
        expected_git_sha: str,
        expected_onnxruntime_version: str,
    ) -> None:
        self.expected_version = expected_version
        self.expected_git_sha = expected_git_sha
        self.expected_onnxruntime_version = expected_onnxruntime_version
        self.library: ctypes.CDLL | None = None
        self.engine: int | None = None
        self.identity: BackendIdentity | None = None
        self._dll_directory: object | None = None
        self._path_bytes: list[bytes] = []
        self._callback_ref: _ProgressCallback | None = None

    @staticmethod
    def _decode(pointer: bytes | None, field: str) -> str:
        if not pointer:
            raise SherpaBackendError(f"sherpa-onnx did not report {field}")
        try:
            return pointer.decode("utf-8")
        except UnicodeDecodeError as error:
            raise SherpaBackendError(f"sherpa-onnx reported invalid {field}") from error

    def _bind(self, library: ctypes.CDLL) -> None:
        library.SherpaOnnxGetVersionStr.argtypes = []
        library.SherpaOnnxGetVersionStr.restype = ctypes.c_char_p
        library.SherpaOnnxGetGitSha1.argtypes = []
        library.SherpaOnnxGetGitSha1.restype = ctypes.c_char_p
        library.SherpaOnnxGetOnnxruntimeVersionStr.argtypes = []
        library.SherpaOnnxGetOnnxruntimeVersionStr.restype = ctypes.c_char_p
        library.SherpaOnnxCreateOfflineTts.argtypes = [ctypes.POINTER(_TtsConfig)]
        library.SherpaOnnxCreateOfflineTts.restype = ctypes.c_void_p
        library.SherpaOnnxDestroyOfflineTts.argtypes = [ctypes.c_void_p]
        library.SherpaOnnxDestroyOfflineTts.restype = None
        library.SherpaOnnxOfflineTtsSampleRate.argtypes = [ctypes.c_void_p]
        library.SherpaOnnxOfflineTtsSampleRate.restype = ctypes.c_int32
        library.SherpaOnnxOfflineTtsNumSpeakers.argtypes = [ctypes.c_void_p]
        library.SherpaOnnxOfflineTtsNumSpeakers.restype = ctypes.c_int32
        library.SherpaOnnxOfflineTtsGenerateWithConfig.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.POINTER(_GenerationConfig),
            _ProgressCallback,
            ctypes.c_void_p,
        ]
        library.SherpaOnnxOfflineTtsGenerateWithConfig.restype = ctypes.POINTER(
            _GeneratedAudio
        )
        library.SherpaOnnxDestroyOfflineTtsGeneratedAudio.argtypes = [
            ctypes.POINTER(_GeneratedAudio)
        ]
        library.SherpaOnnxDestroyOfflineTtsGeneratedAudio.restype = None

    def _path(self, value: Path) -> bytes:
        resolved = str(value.resolve(strict=True)).encode("utf-8")
        self._path_bytes.append(resolved)
        return resolved

    def load(self, pack_root: Path, *, num_threads: int) -> BackendIdentity:
        if os.name != "nt":
            raise SherpaBackendError("the pinned runtime is Windows x64 only")
        if self.engine is not None:
            assert self.identity is not None
            return self.identity
        if not 1 <= num_threads <= 8:
            raise SherpaBackendError("num_threads must be between 1 and 8")
        root = pack_root.resolve(strict=True)
        runtime_dir = root / "runtime" / "lib"
        model_dir = root / "model"
        dll = runtime_dir / "sherpa-onnx-c-api.dll"
        for required in (
            dll,
            runtime_dir / "onnxruntime.dll",
            model_dir / "model.int8.onnx",
            model_dir / "voices.bin",
            model_dir / "tokens.txt",
            model_dir / "lexicon-us-en.txt",
            model_dir / "espeak-ng-data" / "phondata",
        ):
            if not required.is_file() or required.is_symlink():
                raise SherpaBackendError("verified runtime/model file is absent")
        if hasattr(os, "add_dll_directory"):
            self._dll_directory = os.add_dll_directory(str(runtime_dir))
        try:
            library = ctypes.CDLL(str(dll))
        except OSError as error:
            raise SherpaBackendError("sherpa-onnx runtime could not be loaded") from error
        self._bind(library)
        version = self._decode(library.SherpaOnnxGetVersionStr(), "version")
        git_sha = self._decode(library.SherpaOnnxGetGitSha1(), "Git SHA")
        onnxruntime_version = self._decode(
            library.SherpaOnnxGetOnnxruntimeVersionStr(),
            "ONNX Runtime version",
        )
        if version.lstrip("v") != self.expected_version:
            raise SherpaBackendError("sherpa-onnx runtime version mismatch")
        if not self.expected_git_sha.startswith(git_sha) and not git_sha.startswith(
            self.expected_git_sha
        ):
            raise SherpaBackendError("sherpa-onnx runtime Git SHA mismatch")
        if onnxruntime_version != self.expected_onnxruntime_version:
            raise SherpaBackendError("ONNX Runtime version mismatch")

        self._path_bytes.clear()
        config = _TtsConfig()
        config.model.kokoro.model = self._path(model_dir / "model.int8.onnx")
        config.model.kokoro.voices = self._path(model_dir / "voices.bin")
        config.model.kokoro.tokens = self._path(model_dir / "tokens.txt")
        config.model.kokoro.data_dir = self._path(model_dir / "espeak-ng-data")
        config.model.kokoro.length_scale = 1.0
        config.model.kokoro.lexicon = self._path(model_dir / "lexicon-us-en.txt")
        config.model.num_threads = num_threads
        config.model.debug = 0
        config.model.provider = b"cpu"
        config.max_num_sentences = 1
        config.silence_scale = 0.2
        engine = library.SherpaOnnxCreateOfflineTts(ctypes.byref(config))
        if not engine:
            raise SherpaBackendError("sherpa-onnx rejected the verified Kokoro configuration")
        sample_rate = int(library.SherpaOnnxOfflineTtsSampleRate(engine))
        speakers = int(library.SherpaOnnxOfflineTtsNumSpeakers(engine))
        if sample_rate != 24000 or speakers < 53:
            library.SherpaOnnxDestroyOfflineTts(engine)
            raise SherpaBackendError("sherpa-onnx model metadata does not match the pack")
        self.library = library
        self.engine = engine
        self.identity = BackendIdentity(
            runtime_version=version.lstrip("v"),
            runtime_git_sha=git_sha,
            onnxruntime_version=onnxruntime_version,
            sample_rate_hz=sample_rate,
            speaker_count=speakers,
        )
        return self.identity

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
        if self.library is None or self.engine is None or self.identity is None:
            raise SherpaBackendError("sherpa-onnx backend is not loaded")
        if not 0 <= speaker_id < self.identity.speaker_count:
            raise SherpaBackendError("speaker ID is outside the loaded model")
        if not math.isfinite(speed) or not 0.5 <= speed <= 2.0:
            raise SherpaBackendError("speed is outside the safe range")
        text_bytes = text.encode("utf-8")
        generated = 0
        callbacks = 0

        def progress(
            samples: ctypes.POINTER(ctypes.c_float),
            count: int,
            fraction: float,
            _argument: int,
        ) -> int:
            nonlocal generated, callbacks
            if cancel.is_set() or count < 0 or count > 10_000_000:
                return 0
            values = [float(samples[index]) for index in range(count)]
            callbacks += 1
            generated += count
            try:
                keep_going = callback(values, float(fraction))
            except Exception:
                cancel.set()
                return 0
            return 1 if keep_going and not cancel.is_set() else 0

        callback_ref = _ProgressCallback(progress)
        self._callback_ref = callback_ref
        generation = _GenerationConfig()
        generation.silence_scale = silence_scale
        generation.speed = speed
        generation.sid = speaker_id
        audio = self.library.SherpaOnnxOfflineTtsGenerateWithConfig(
            self.engine,
            text_bytes,
            ctypes.byref(generation),
            callback_ref,
            None,
        )
        try:
            if cancel.is_set():
                return SynthesisSummary(generated, callbacks, True)
            if not audio:
                raise SherpaBackendError("sherpa-onnx synthesis returned no audio")
            result = audio.contents
            if result.sample_rate != self.identity.sample_rate_hz or result.n < 0:
                raise SherpaBackendError("sherpa-onnx returned invalid audio metadata")
            if callbacks == 0 and result.n:
                values = [float(result.samples[index]) for index in range(result.n)]
                callbacks = 1
                generated = result.n
                if not callback(values, 1.0):
                    cancel.set()
                    return SynthesisSummary(generated, callbacks, True)
            return SynthesisSummary(generated, callbacks, False)
        finally:
            if audio:
                self.library.SherpaOnnxDestroyOfflineTtsGeneratedAudio(audio)
            self._callback_ref = None

    def unload(self) -> None:
        if self.library is not None and self.engine is not None:
            self.library.SherpaOnnxDestroyOfflineTts(self.engine)
        self.engine = None
        self.identity = None
        self.library = None
        self._path_bytes.clear()
        directory = self._dll_directory
        self._dll_directory = None
        close = getattr(directory, "close", None)
        if close is not None:
            close()
