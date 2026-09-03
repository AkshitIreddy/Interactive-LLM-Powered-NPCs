#!/usr/bin/env python3
"""Gated real Windows CPU qualification for the pinned Kokoro pack.

There is deliberately no download or repair path in this command.  It accepts
only a previously installed, fully verified revision.  The report is a
content-free, tamper-evident review artifact; it is not a signed
QualifiedResourceEnvelopeV1 and never authorizes admission.
"""

from __future__ import annotations

import argparse
import contextlib
import ctypes
import hashlib
import json
import math
import os
import platform
import secrets
import shutil
import subprocess
import tempfile
import threading
import time
import wave
from dataclasses import asdict, dataclass
from pathlib import Path

from manifest import PackManifest, load_manifest
from measurement import Observation, build_unsigned_report
from pack_manager import PackLifecycleError, PackManager, sha256_file
from pcm_transport import floats_to_pcm16
from sherpa_backend import SherpaOnnxBackend
from voices import STOCK_VOICES, public_voice_records

REPORT_SCHEMA = "npc.local-tts.windows-cpu-qualification/v1"
SUITE_REVISION = "kokoro-sherpa-windows-cpu/1"
EXPLICIT_GRANT = "local-tts-kokoro-real-windows-cpu"
MINIMUM_SAMPLES = 20
MAXIMUM_SAMPLES = 100
PLAYABLE_FRAMES = 480
CREATE_NO_WINDOW = 0x08000000

TEXT_PROFILES = {
    "short": "Lantern ready.",
    "medium": (
        "The village watch reports clear roads beyond the eastern gate, "
        "but the bridge keeper still advises caution after sunset."
    ),
    "long": (
        "Before the caravan leaves, the quartermaster checks every crate, "
        "records the seals in a weathered ledger, and asks the scouts to "
        "confirm that the northern road is clear. The innkeeper offers warm "
        "bread for the journey, while the blacksmith tightens the final "
        "buckles on the horses' harnesses. At dawn the bell rings twice, the "
        "gate opens, and the travelers begin their careful climb toward the "
        "mountain pass."
    ),
}


class QualificationError(RuntimeError):
    pass


class NoDownload:
    def fetch(self, *_args: object, **_kwargs: object) -> None:
        raise QualificationError("qualification has no download path")


@dataclass(frozen=True, slots=True)
class ProcessSnapshot:
    monotonic_ns: int
    cpu_time_ns: int
    rss_bytes: int


@dataclass(frozen=True, slots=True)
class AudioEvidence:
    frames: int
    callbacks: int
    first_callback_frames: int
    first_playable_chunk_millis: float
    total_synthesis_millis: float
    audio_duration_millis: float
    realtime_factor: float
    clipped_samples: int
    peak_absolute_sample: float
    rms: float
    pcm_sha256: str
    pcm: bytes


class _ProcessMemoryCounters(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
        ("PrivateUsage", ctypes.c_size_t),
    ]


def process_snapshot() -> ProcessSnapshot:
    if os.name != "nt":
        raise QualificationError("real Kokoro qualification is Windows-only")
    counters = _ProcessMemoryCounters()
    counters.cb = ctypes.sizeof(counters)
    process = ctypes.windll.kernel32.GetCurrentProcess()
    ok = ctypes.windll.psapi.GetProcessMemoryInfo(
        process,
        ctypes.byref(counters),
        counters.cb,
    )
    if not ok or counters.WorkingSetSize <= 0:
        raise QualificationError("target-process RSS telemetry is unavailable")
    return ProcessSnapshot(
        monotonic_ns=time.monotonic_ns(),
        cpu_time_ns=time.process_time_ns(),
        rss_bytes=int(counters.WorkingSetSize),
    )


class PeakRssSampler:
    def __init__(self, interval_seconds: float = 0.005) -> None:
        self.interval_seconds = interval_seconds
        self.stop = threading.Event()
        self.values: list[int] = []
        self.thread: threading.Thread | None = None

    def __enter__(self) -> "PeakRssSampler":
        self.values.append(process_snapshot().rss_bytes)

        def sample() -> None:
            while not self.stop.wait(self.interval_seconds):
                with contextlib.suppress(Exception):
                    self.values.append(process_snapshot().rss_bytes)

        self.thread = threading.Thread(
            target=sample,
            name="npc-local-tts-rss-sampler",
            daemon=True,
        )
        self.thread.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.stop.set()
        if self.thread is not None:
            self.thread.join(timeout=1.0)
        self.values.append(process_snapshot().rss_bytes)

    @property
    def peak(self) -> int:
        if not self.values:
            raise QualificationError("RSS sampler produced no observations")
        return max(self.values)


def _nvidia_smi() -> Path | None:
    found = shutil.which("nvidia-smi")
    if found:
        return Path(found)
    system_root = Path(os.environ.get("SystemRoot", r"C:\Windows"))
    candidate = system_root / "System32" / "nvidia-smi.exe"
    return candidate if candidate.is_file() else None


def target_process_vram_probe() -> dict[str, object]:
    """Measure NVIDIA compute allocation for this exact qualification PID."""

    executable = _nvidia_smi()
    if executable is None:
        return {
            "available": False,
            "target_pid": os.getpid(),
            "target_process_vram_bytes": None,
            "reason": "nvidia-smi is unavailable; zero target-process VRAM is unproven",
        }
    command = [
        str(executable),
        "--query-compute-apps=pid,used_gpu_memory",
        "--format=csv,noheader,nounits",
    ]
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
        creationflags=CREATE_NO_WINDOW,
    )
    if completed.returncode != 0:
        return {
            "available": False,
            "target_pid": os.getpid(),
            "target_process_vram_bytes": None,
            "reason": "nvidia-smi compute-process query failed",
        }
    target_mebibytes = 0
    target_seen = False
    for line in completed.stdout.splitlines():
        fields = [field.strip() for field in line.split(",")]
        if len(fields) != 2:
            continue
        try:
            pid = int(fields[0])
            mebibytes = int(fields[1])
        except ValueError:
            continue
        if pid == os.getpid():
            target_seen = True
            target_mebibytes += max(0, mebibytes)
    return {
        "available": True,
        "target_pid": os.getpid(),
        "target_process_seen": target_seen,
        "target_process_vram_bytes": target_mebibytes * 1024 * 1024,
        "source": "nvidia-smi query-compute-apps for exact target PID",
    }


def require_zero_target_vram() -> dict[str, object]:
    evidence = target_process_vram_probe()
    if (
        evidence.get("available") is not True
        or evidence.get("target_process_vram_bytes") != 0
    ):
        raise QualificationError(
            "CPU qualification could not prove zero target-process NVIDIA VRAM"
        )
    return evidence


def _profile_for(index: int) -> tuple[str, str]:
    names = ("short", "medium", "long")
    name = names[index % len(names)]
    return name, TEXT_PROFILES[name]


def _voice_for(index: int):
    # Walk the whole typed inventory before repeating; the minimum 20 samples
    # still covers substantially more than the required three voices.
    return STOCK_VOICES[index % len(STOCK_VOICES)]


def synthesize_evidence(
    backend: SherpaOnnxBackend,
    *,
    text: str,
    speaker_id: int,
    cancel: threading.Event | None = None,
) -> AudioEvidence:
    cancel = cancel or threading.Event()
    started = time.perf_counter()
    first_at: float | None = None
    first_frames = 0
    callbacks = 0
    frames = 0
    clipped = 0
    square_sum = 0.0
    peak = 0.0
    pcm = bytearray()

    def receive(samples: list[float], _progress: float) -> bool:
        nonlocal first_at, first_frames, callbacks, frames
        nonlocal clipped, square_sum, peak
        if first_at is None and samples:
            first_at = time.perf_counter()
            first_frames = len(samples)
        callbacks += 1
        frames += len(samples)
        for sample in samples:
            if not math.isfinite(sample):
                raise QualificationError("synthesis emitted a non-finite sample")
            square_sum += sample * sample
            peak = max(peak, abs(sample))
        encoded, clipped_now = floats_to_pcm16(samples)
        clipped += clipped_now
        pcm.extend(encoded)
        return not cancel.is_set()

    summary = backend.synthesize(
        text,
        speaker_id=speaker_id,
        speed=1.0,
        silence_scale=0.2,
        callback=receive,
        cancel=cancel,
    )
    finished = time.perf_counter()
    if summary.cancelled or cancel.is_set():
        raise QualificationError("ordinary synthesis was unexpectedly cancelled")
    if summary.generated_frames != frames or frames <= 0 or callbacks <= 0:
        raise QualificationError("streamed and backend frame counts disagree")
    if first_at is None or first_frames < PLAYABLE_FRAMES:
        raise QualificationError("first callback did not contain a playable PCM chunk")
    if clipped:
        raise QualificationError("synthesis clipped during audio qualification")
    rms = math.sqrt(square_sum / frames)
    if rms <= 1e-5 or peak <= 1e-5:
        raise QualificationError("synthesis output is effectively silent")
    total_millis = (finished - started) * 1000.0
    audio_millis = frames * 1000.0 / 24_000.0
    encoded = bytes(pcm)
    if len(encoded) != frames * 2:
        raise QualificationError("PCM byte length does not match the frame count")
    return AudioEvidence(
        frames=frames,
        callbacks=callbacks,
        first_callback_frames=first_frames,
        first_playable_chunk_millis=(first_at - started) * 1000.0,
        total_synthesis_millis=total_millis,
        audio_duration_millis=audio_millis,
        realtime_factor=total_millis / audio_millis,
        clipped_samples=clipped,
        peak_absolute_sample=peak,
        rms=rms,
        pcm_sha256=hashlib.sha256(encoded).hexdigest(),
        pcm=encoded,
    )


def write_review_wav(path: Path, evidence: AudioEvidence) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{secrets.token_hex(4)}.part")
    try:
        with wave.open(str(temporary), "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(24_000)
            output.writeframes(evidence.pcm)
        temporary.replace(path)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def _timed_load(
    backend: SherpaOnnxBackend,
    pack_root: Path,
    cpu_threads: int,
) -> tuple[float, float, int]:
    before = process_snapshot()
    with PeakRssSampler() as sampler:
        backend.load(pack_root, num_threads=cpu_threads)
    after = process_snapshot()
    return (
        (after.monotonic_ns - before.monotonic_ns) / 1_000_000.0,
        (after.cpu_time_ns - before.cpu_time_ns) / 1_000_000.0,
        sampler.peak,
    )


def cancellation_probe(
    backend: SherpaOnnxBackend,
    *,
    attempts: int = 5,
) -> dict[str, object]:
    long_text = " ".join([TEXT_PROFILES["long"]] * 8)
    for attempt in range(1, attempts + 1):
        cancel = threading.Event()
        callback_seen = threading.Event()
        outcome: list[object] = []

        def receive(_samples: list[float], _progress: float) -> bool:
            callback_seen.set()
            return not cancel.is_set()

        def run() -> None:
            try:
                outcome.append(
                    backend.synthesize(
                        long_text,
                        speaker_id=3,
                        speed=1.0,
                        silence_scale=0.2,
                        callback=receive,
                        cancel=cancel,
                    )
                )
            except BaseException as error:
                outcome.append(error)

        thread = threading.Thread(
            target=run,
            name="npc-local-tts-cancel-probe",
            daemon=True,
        )
        thread.start()
        if not callback_seen.wait(15.0):
            thread.join(1.0)
            if thread.is_alive():
                raise QualificationError("cancel probe emitted no callback and remained hung")
            continue
        started = time.perf_counter()
        cancel.set()
        thread.join(5.0)
        barrier_millis = (time.perf_counter() - started) * 1000.0
        if thread.is_alive():
            raise QualificationError("native cancellation exceeded five seconds")
        if len(outcome) != 1:
            raise QualificationError("cancel probe produced an invalid terminal outcome")
        result = outcome[0]
        if isinstance(result, BaseException):
            raise QualificationError("native cancellation raised unexpectedly") from result
        if getattr(result, "cancelled", False) is not True:
            # The call can win the race after its first callback. Retry rather
            # than mislabel a completed utterance as cancellation evidence.
            continue
        return {
            "attempts": attempt,
            "callback_observed": True,
            "cancelled": True,
            "drain_barrier_millis": barrier_millis,
        }
    raise QualificationError("synthesis completed before cancellation could be observed")


def artifact_snapshot(
    manager: PackManager,
    manifest: PackManifest,
) -> dict[str, object]:
    record = manager.verify(manifest)
    inventory = record.get("inventory")
    if not isinstance(inventory, list):
        raise QualificationError("verified installation inventory is unavailable")
    critical_files = []
    for item in manifest.critical_files:
        size, digest = sha256_file(
            manager.target(manifest) / item.path,
            item.size_bytes,
        )
        critical_files.append(
            {
                "path": item.path,
                "size_bytes": size,
                "sha256": digest,
            }
        )
    return {
        "manifest_sha256": manifest.canonical_sha256,
        "installation_record_sha256": record.get("record_sha256"),
        "installed_bytes": sum(
            int(row["size_bytes"])
            for row in inventory
            if isinstance(row, dict) and isinstance(row.get("size_bytes"), int)
        ),
        "inventory_files": len(inventory),
        "critical_files": critical_files,
    }


def _public_audio_row(evidence: AudioEvidence) -> dict[str, object]:
    row = asdict(evidence)
    row.pop("pcm")
    return row


def _atomic_json(path: Path, value: dict[str, object]) -> None:
    path = path.resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    if len(encoded) > 4 * 1024 * 1024:
        raise QualificationError("qualification report exceeded four MiB")
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".part",
        dir=path.parent,
    )
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        Path(temporary_name).replace(path)
    except Exception:
        Path(temporary_name).unlink(missing_ok=True)
        raise


def qualification_plan(manifest: PackManifest, samples: int) -> dict[str, object]:
    return {
        "schema": "npc.local-tts.windows-cpu-qualification-plan/v1",
        "identity": {
            "pack_id": manifest.pack_id,
            "revision": manifest.revision,
            "manifest_sha256": manifest.canonical_sha256,
        },
        "samples": samples,
        "warmups": "configured at execution",
        "profiles": sorted(TEXT_PROFILES),
        "typed_voice_inventory": public_voice_records(),
        "real_actions": [
            "verify installed inventory before and after",
            "measure fresh load and same-process reload separately",
            "stream and structurally verify PCM",
            "measure first playable chunk and full realtime factor",
            "measure target-process RSS and CPU",
            "prove zero exact-target NVIDIA compute VRAM",
            "cancel and drain native callback",
            "unload after every load and at final teardown",
            "write three WAV files for human listening review",
        ],
        "downloads": False,
        "qualified_resource_envelope_created": False,
        "admission_allowed": False,
    }


def qualify(args: argparse.Namespace) -> dict[str, object]:
    if os.name != "nt":
        raise QualificationError("real Kokoro qualification is Windows-only")
    if not args.acknowledge_real_model_run:
        raise QualificationError(
            "real qualification requires --acknowledge-real-model-run"
        )
    if args.coordination_grant != EXPLICIT_GRANT:
        raise QualificationError("the parent TTS lane grant token is absent")
    if not MINIMUM_SAMPLES <= args.samples <= MAXIMUM_SAMPLES:
        raise QualificationError("samples must be between 20 and 100")
    if not 1 <= args.warmups <= 10:
        raise QualificationError("warmups must be between 1 and 10")
    if not 1 <= args.cpu_threads <= 8:
        raise QualificationError("cpu-threads must be between 1 and 8")
    lock_path = args.gpu_lock_file.resolve(strict=True)
    if lock_path.read_text(encoding="utf-8").strip().lower() != "yes":
        raise QualificationError("shared AI-model lock is not granted")

    manifest = load_manifest(args.manifest)
    pack_root = args.pack_root.resolve(strict=True)
    expected_root = (
        pack_root.parents[1] / manifest.pack_id / manifest.revision
    ).resolve(strict=True)
    if pack_root != expected_root:
        raise QualificationError("pack root does not match the manifest identity")
    manager = PackManager(pack_root.parents[1], NoDownload())
    artifacts_before = artifact_snapshot(manager, manifest)
    vram_probes = [require_zero_target_vram()]

    loads: list[Observation] = []
    reloads: list[Observation] = []
    syntheses: list[Observation] = []
    load_rows: list[dict[str, object]] = []
    synthesis_rows: list[dict[str, object]] = []
    unload_millis: list[float] = []

    total_cycles = args.warmups + args.samples
    for index in range(total_cycles):
        backend = SherpaOnnxBackend(
            expected_version=manifest.runtime_version,
            expected_git_sha=manifest.runtime_git_sha,
            expected_onnxruntime_version=manifest.onnxruntime_version,
        )
        try:
            load_ms, load_cpu_ms, load_rss = _timed_load(
                backend,
                pack_root,
                args.cpu_threads,
            )
            vram_probes.append(require_zero_target_vram())
            unload_started = time.perf_counter()
            backend.unload()
            first_unload_ms = (time.perf_counter() - unload_started) * 1000.0

            reload_ms, reload_cpu_ms, reload_rss = _timed_load(
                backend,
                pack_root,
                args.cpu_threads,
            )
            vram_probes.append(require_zero_target_vram())
            if index >= args.warmups:
                loads.append(Observation("load", load_ms, load_rss))
                reloads.append(Observation("reload", reload_ms, reload_rss))
                load_rows.append(
                    {
                        "sample": index - args.warmups,
                        "load_millis": load_ms,
                        "load_cpu_millis": load_cpu_ms,
                        "load_peak_rss_bytes": load_rss,
                        "reload_millis": reload_ms,
                        "reload_cpu_millis": reload_cpu_ms,
                        "reload_peak_rss_bytes": reload_rss,
                    }
                )
                unload_millis.append(first_unload_ms)
        finally:
            unload_started = time.perf_counter()
            backend.unload()
            if index >= args.warmups:
                unload_millis.append(
                    (time.perf_counter() - unload_started) * 1000.0
                )

    backend = SherpaOnnxBackend(
        expected_version=manifest.runtime_version,
        expected_git_sha=manifest.runtime_git_sha,
        expected_onnxruntime_version=manifest.onnxruntime_version,
    )
    review_wavs: list[dict[str, object]] = []
    self_test_audio: AudioEvidence | None = None
    try:
        backend.load(pack_root, num_threads=args.cpu_threads)
        identity = backend.identity
        if identity is None or identity.speaker_count < 53:
            raise QualificationError("loaded speaker inventory is incomplete")
        self_test_path = (
            manifest.path.parents[2]
            / manifest.raw["self_test"]["input_fixture"]
        ).resolve(strict=True)
        self_test_bytes = self_test_path.read_bytes()
        if (
            hashlib.sha256(self_test_bytes).hexdigest()
            != manifest.raw["self_test"]["input_fixture_sha256"]
        ):
            raise QualificationError("self-test fixture digest changed")
        self_test_audio = synthesize_evidence(
            backend,
            text=self_test_bytes.decode("utf-8").rstrip("\r\n"),
            speaker_id=3,
        )
        for index in range(total_cycles):
            profile, text = _profile_for(index)
            voice = _voice_for(index)
            before = process_snapshot()
            with PeakRssSampler() as sampler:
                evidence = synthesize_evidence(
                    backend,
                    text=text,
                    speaker_id=voice.speaker_id,
                )
            after = process_snapshot()
            vram_probes.append(require_zero_target_vram())
            if index < args.warmups:
                continue
            sample_index = index - args.warmups
            syntheses.append(
                Observation(
                    "synthesis",
                    evidence.total_synthesis_millis,
                    sampler.peak,
                    voice_id=voice.voice_id,
                    text_profile=profile,
                    first_pcm_millis=evidence.first_playable_chunk_millis,
                    audio_duration_millis=evidence.audio_duration_millis,
                )
            )
            synthesis_rows.append(
                {
                    "sample": sample_index,
                    "voice_id": voice.voice_id,
                    "speaker_id": voice.speaker_id,
                    "text_profile": profile,
                    "text_sha256": hashlib.sha256(
                        text.encode("utf-8")
                    ).hexdigest(),
                    "cpu_millis": (
                        after.cpu_time_ns - before.cpu_time_ns
                    )
                    / 1_000_000.0,
                    "peak_rss_bytes": sampler.peak,
                    **_public_audio_row(evidence),
                }
            )
            if len(review_wavs) < 3 and profile not in {
                row["text_profile"] for row in review_wavs
            }:
                filename = f"{profile}-{voice.voice_id}.wav"
                output = args.audio_review_dir.resolve() / filename
                write_review_wav(output, evidence)
                review_wavs.append(
                    {
                        "text_profile": profile,
                        "voice_id": voice.voice_id,
                        "path": str(output),
                        "wav_sha256": hashlib.sha256(
                            output.read_bytes()
                        ).hexdigest(),
                    }
                )
        cancel = cancellation_probe(backend)
        cancel_observation = Observation(
            "cancel",
            float(cancel["drain_barrier_millis"]),
            process_snapshot().rss_bytes,
        )
    finally:
        backend.unload()

    artifacts_after = artifact_snapshot(manager, manifest)
    if artifacts_before != artifacts_after:
        raise QualificationError("installed artifacts changed during qualification")
    if lock_path.read_text(encoding="utf-8").strip().lower() != "yes":
        raise QualificationError("shared AI-model lock changed during qualification")
    observations: list[Observation] = [
        *loads,
        *reloads,
        *syntheses,
        cancel_observation,
    ]
    projection = build_unsigned_report(
        pack_id=manifest.pack_id,
        revision=manifest.revision,
        manifest_sha256=manifest.canonical_sha256,
        runtime_revision=(
            f"sherpa-onnx/{manifest.runtime_version}+"
            f"{manifest.runtime_git_sha}+onnxruntime/"
            f"{manifest.onnxruntime_version}"
        ),
        observations=observations,
    )
    now = int(time.time())
    return {
        "schema": REPORT_SCHEMA,
        "suite_revision": SUITE_REVISION,
        "report_id": f"kokoro-cpu-{now}-{secrets.token_hex(6)}",
        "measured_unix_seconds": now,
        "identity": {
            "pack_id": manifest.pack_id,
            "revision": manifest.revision,
            "manifest_sha256": manifest.canonical_sha256,
        },
        "runtime": {
            "sherpa_onnx": manifest.runtime_version,
            "sherpa_git_sha": manifest.runtime_git_sha,
            "onnxruntime": manifest.onnxruntime_version,
            "backend": "cpu",
            "cpu_threads": args.cpu_threads,
            "python": platform.python_version(),
            "platform": platform.platform(),
        },
        "typed_voice_inventory": {
            "count": len(STOCK_VOICES),
            "voices": public_voice_records(),
            "canonical_sha256": hashlib.sha256(
                json.dumps(
                    public_voice_records(),
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode("utf-8")
            ).hexdigest(),
        },
        "samples": {
            "warmups": args.warmups,
            "measured_loads": len(loads),
            "measured_reloads": len(reloads),
            "measured_syntheses": len(syntheses),
            "load_rows": load_rows,
            "synthesis_rows": synthesis_rows,
            "unload_millis": unload_millis,
        },
        "cancellation": cancel,
        "self_test": {
            "passed": self_test_audio is not None,
            "voice_id": "af_heart",
            "text_fixture_sha256": manifest.raw["self_test"][
                "input_fixture_sha256"
            ],
            "audio": (
                _public_audio_row(self_test_audio)
                if self_test_audio is not None
                else None
            ),
        },
        "target_process_vram_probes": vram_probes,
        "audio_review": {
            "human_listening_required": True,
            "accepted": False,
            "files": review_wavs,
        },
        "artifact_verification": {
            "before": artifacts_before,
            "after": artifacts_after,
        },
        "unsigned_resource_projection": projection,
        "sample_content_sha256": hashlib.sha256(
            json.dumps(
                {
                    "load_rows": load_rows,
                    "synthesis_rows": synthesis_rows,
                    "cancel": cancel,
                    "vram": vram_probes,
                },
                sort_keys=True,
                separators=(",", ":"),
            ).encode("utf-8")
        ).hexdigest(),
        "admission": {
            "qualified_resource_envelope_created": False,
            "admission_allowed": False,
            "measurement_signature": None,
            "reason": (
                "This worker report is unsigned. Model Manager must bind the "
                "current device fingerprint, validity window, monotonic "
                "sequence, game reserve and trusted signatures after review."
            ),
        },
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--pack-root", type=Path)
    parser.add_argument("--gpu-lock-file", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--audio-review-dir", type=Path)
    parser.add_argument("--samples", type=int, default=24)
    parser.add_argument("--warmups", type=int, default=3)
    parser.add_argument("--cpu-threads", type=int, default=2)
    parser.add_argument("--coordination-grant")
    parser.add_argument("--acknowledge-real-model-run", action="store_true")
    parser.add_argument("--plan", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        manifest = load_manifest(args.manifest)
        if args.plan:
            print(json.dumps(qualification_plan(manifest, args.samples), indent=2))
            return 0
        missing = [
            name
            for name in (
                "pack_root",
                "gpu_lock_file",
                "out",
                "audio_review_dir",
            )
            if getattr(args, name) is None
        ]
        if missing:
            raise QualificationError(
                f"real run is missing required arguments: {', '.join(missing)}"
            )
        report = qualify(args)
        _atomic_json(args.out, report)
        print(
            json.dumps(
                {
                    "ok": True,
                    "report": str(args.out.resolve()),
                    "report_sha256": hashlib.sha256(
                        args.out.read_bytes()
                    ).hexdigest(),
                    "qualified_resource_envelope_created": False,
                    "admission_allowed": False,
                }
            )
        )
        return 0
    except (QualificationError, PackLifecycleError, OSError, ValueError) as error:
        print(
            json.dumps(
                {
                    "ok": False,
                    "error": {
                        "code": "qualification_blocked_or_failed",
                        "message": str(error),
                    },
                }
            )
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
