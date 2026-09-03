#!/usr/bin/env python3
from __future__ import annotations

import argparse
import gc
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time
import uuid
import wave
from typing import Any


ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.backend import BackendDelta, MoonshineBridgeBackend, TranscriptLine
from npc_local_stt.contract import PROTOCOL_VERSION, PcmFrame
from npc_local_stt.metrics import process_rss_bytes
from npc_local_stt.pack_lifecycle import sha256_file


def atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    encoded = json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    with temporary.open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        raise ValueError("cannot calculate a percentile without samples")
    ordered = sorted(values)
    rank = (len(ordered) - 1) * fraction
    lower = int(rank)
    upper = min(len(ordered) - 1, lower + 1)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (rank - lower)


def metric_summary(values: list[float]) -> dict[str, Any]:
    return {
        "count": len(values),
        "minimum": min(values),
        "p50": percentile(values, 0.50),
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "maximum": max(values),
        "mean": statistics.fmean(values),
    }


def run_command(arguments: list[str], timeout: int = 20) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )


def nvidia_snapshot() -> dict[str, Any]:
    executable = Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32" / "nvidia-smi.exe"
    if not executable.is_file():
        return {"available": False, "source": "nvidia-smi.exe not installed"}
    gpu = run_command(
        [
            str(executable),
            "--query-gpu=index,uuid,name,driver_version,memory.total,memory.used",
            "--format=csv,noheader,nounits",
        ]
    )
    apps = run_command(
        [
            str(executable),
            "--query-compute-apps=pid,process_name,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ]
    )
    gpus: list[dict[str, Any]] = []
    if gpu.returncode == 0:
        for line in gpu.stdout.splitlines():
            fields = [field.strip() for field in line.split(",")]
            if len(fields) >= 6:
                gpus.append(
                    {
                        "index": int(fields[0]),
                        "uuid": fields[1],
                        "name": fields[2],
                        "driverVersion": fields[3],
                        "totalMiB": int(fields[4]),
                        "usedMiB": int(fields[5]),
                    }
                )
    processes: list[dict[str, Any]] = []
    if apps.returncode == 0:
        for line in apps.stdout.splitlines():
            fields = [field.strip() for field in line.split(",")]
            if len(fields) >= 3 and fields[0].isdigit():
                memory = None if fields[2].upper() in {"N/A", "[N/A]"} else int(fields[2])
                processes.append({"pid": int(fields[0]), "name": fields[1], "usedMiB": memory})
    own = [entry for entry in processes if entry["pid"] == os.getpid()]
    return {
        "available": gpu.returncode == 0,
        "source": "nvidia-smi query-gpu and query-compute-apps",
        "gpuQueryReturnCode": gpu.returncode,
        "processQueryReturnCode": apps.returncode,
        "gpus": gpus,
        "computeProcesses": processes,
        "ownProcessPresent": bool(own),
        "ownProcessUsedMiB": sum(entry["usedMiB"] or 0 for entry in own),
    }


def frame(session_id: str, chunk_index: int, sample_rate: int, pcm: bytes) -> PcmFrame:
    sample_count = len(pcm) // 2
    return PcmFrame.parse(
        {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": "moonshine-real-qualification",
            "sessionId": session_id,
            "generation": 0,
            "chunkId": f"{session_id}-chunk-{chunk_index}",
            "chunkIndex": chunk_index,
            "sampleRateHz": sample_rate,
            "channels": 1,
            "sampleFormat": "pcm_s16le",
            "sampleCount": sample_count,
            "capturedAtQpc": chunk_index * sample_count,
            "qpcFrequencyHz": sample_rate,
        },
        pcm,
    )


def collect_delta(
    delta: BackendDelta,
    lines: dict[str, TranscriptLine],
    counters: dict[str, int],
) -> None:
    counters["vadStarted"] += int(delta.speech_started)
    counters["vadEnded"] += int(delta.speech_ended)
    counters["partialEvents"] += sum(not line.complete for line in delta.lines)
    counters["finalEvents"] += sum(line.complete for line in delta.lines)
    for line in delta.lines:
        lines[line.utterance_id] = line


def validate_transcript(lines: list[TranscriptLine]) -> dict[str, Any]:
    completed = sorted((line for line in lines if line.complete), key=lambda line: (line.start_ms, line.end_ms))
    if not completed:
        raise RuntimeError("Moonshine produced no completed transcript lines")
    transcript = " ".join(line.text.strip() for line in completed if line.text.strip()).strip()
    lowered = transcript.lower()
    if "try again" not in lowered or "fail better" not in lowered:
        raise RuntimeError("Moonshine transcript failed the original Beckett fixture semantic check")
    words = [word for line in completed for word in line.words]
    if len(words) < 6:
        raise RuntimeError("Moonshine word timestamp output is incomplete")
    for line in completed:
        if line.start_ms < 0 or line.end_ms < line.start_ms:
            raise RuntimeError("Moonshine line timestamps are invalid")
        for word in line.words:
            if word.start_ms < 0 or word.end_ms < word.start_ms:
                raise RuntimeError("Moonshine word timestamps are invalid")
    structured = {
        "schema": "npc.local-stt-self-test-transcript/v1",
        "text": transcript,
        "textSha256": hashlib.sha256(transcript.encode("utf-8")).hexdigest(),
        "lineCount": len(completed),
        "wordCount": len(words),
        "lines": [line.as_dict() for line in completed],
    }
    return structured


def load_wave(path: Path) -> tuple[int, list[bytes], int]:
    with wave.open(str(path), "rb") as stream:
        if stream.getnchannels() != 1 or stream.getsampwidth() != 2 or stream.getcomptype() != "NONE":
            raise RuntimeError("qualification WAV must be uncompressed mono PCM16")
        sample_rate = stream.getframerate()
        total_frames = stream.getnframes()
        raw = stream.readframes(total_frames)
    frames_per_chunk = sample_rate // 5
    chunk_bytes = frames_per_chunk * 2
    chunks = [raw[offset : offset + chunk_bytes] for offset in range(0, len(raw), chunk_bytes)]
    return sample_rate, chunks, total_frames


def one_cycle(
    backend: MoonshineBridgeBackend,
    cycle: int,
    sample_rate: int,
    chunks: list[bytes],
    total_frames: int,
    *,
    pace_audio: bool,
) -> tuple[dict[str, Any], dict[str, Any]]:
    rss_before = process_rss_bytes()
    wall_started = time.perf_counter()
    cpu_started = time.process_time()
    load_started = time.perf_counter()
    backend.load()
    load_ms = (time.perf_counter() - load_started) * 1000
    rss_loaded = process_rss_bytes()
    gpu_loaded = nvidia_snapshot()

    session_id = f"real-{cycle:02d}"
    backend.start_session(session_id, True)
    audio_started = time.perf_counter()
    pacing_deadline = audio_started
    inference_ms = 0.0
    first_partial_ms: float | None = None
    first_final_ms: float | None = None
    first_vad_end_ms: float | None = None
    lines: dict[str, TranscriptLine] = {}
    counters = {"vadStarted": 0, "vadEnded": 0, "partialEvents": 0, "finalEvents": 0}
    peak_rss = max(value for value in (rss_before, rss_loaded) if value is not None)
    for index, pcm in enumerate(chunks):
        if pace_audio and index:
            pacing_deadline += len(chunks[index - 1]) / 2 / sample_rate
            delay = pacing_deadline - time.perf_counter()
            if delay > 0:
                time.sleep(delay)
        delta = backend.add_pcm(frame(session_id, index, sample_rate, pcm))
        now = time.perf_counter()
        inference_ms += delta.inference_ms
        if first_partial_ms is None and any(not line.complete for line in delta.lines):
            first_partial_ms = (now - audio_started) * 1000
        if first_final_ms is None and any(line.complete for line in delta.lines):
            first_final_ms = (now - audio_started) * 1000
        if first_vad_end_ms is None and delta.speech_ended:
            first_vad_end_ms = (now - audio_started) * 1000
        collect_delta(delta, lines, counters)
        current_rss = process_rss_bytes()
        if current_rss is not None:
            peak_rss = max(peak_rss, current_rss)

    end_started = time.perf_counter()
    final_delta = backend.end_session("ptt_key_up")
    final_after_end_ms = (time.perf_counter() - end_started) * 1000
    inference_ms += final_delta.inference_ms
    collect_delta(final_delta, lines, counters)
    structured = validate_transcript(list(lines.values()))
    if first_partial_ms is None or first_final_ms is None or first_vad_end_ms is None:
        raise RuntimeError("Moonshine did not emit required partial, final, and VAD events")

    cancel_session = f"cancel-{cycle:02d}"
    backend.start_session(cancel_session, False)
    backend.add_pcm(frame(cancel_session, 0, sample_rate, chunks[0]))
    cancel_started = time.perf_counter()
    backend.cancel()
    cancel_ms = (time.perf_counter() - cancel_started) * 1000
    gpu_peak = nvidia_snapshot()
    unload_started = time.perf_counter()
    backend.unload()
    unload_ms = (time.perf_counter() - unload_started) * 1000
    gc.collect()
    rss_unloaded = process_rss_bytes()
    gpu_unloaded = nvidia_snapshot()
    wall_ms = (time.perf_counter() - wall_started) * 1000
    cpu_ms = (time.process_time() - cpu_started) * 1000
    audio_ms = total_frames * 1000 / sample_rate
    sample = {
        "cycle": cycle,
        "classification": "cold_load" if cycle == 0 else "reload",
        "loadMs": load_ms,
        "unloadMs": unload_ms,
        "inferenceMs": inference_ms,
        "audioMs": audio_ms,
        "realTimeFactor": inference_ms / audio_ms,
        "firstPartialAfterFirstPcmMs": first_partial_ms,
        "firstFinalAfterFirstPcmMs": first_final_ms,
        "firstVadEndAfterFirstPcmMs": first_vad_end_ms,
        "finalAfterPttEndMs": final_after_end_ms,
        "cancelAckMs": cancel_ms,
        "wallMs": wall_ms,
        "cpuMs": cpu_ms,
        "averageCpuPercentOneCore": cpu_ms / wall_ms * 100,
        "rssBeforeBytes": rss_before,
        "rssLoadedBytes": rss_loaded,
        "peakRssBytes": peak_rss,
        "rssUnloadedBytes": rss_unloaded,
        "events": counters,
        "transcriptSha256": structured["textSha256"],
        "lineCount": structured["lineCount"],
        "wordCount": structured["wordCount"],
        "gpuLoaded": gpu_loaded,
        "gpuPeak": gpu_peak,
        "gpuUnloaded": gpu_unloaded,
    }
    return sample, structured


def summarize(samples: list[dict[str, Any]]) -> dict[str, Any]:
    fields = {
        "loadMs": [float(sample["loadMs"]) for sample in samples],
        "reloadMs": [float(sample["loadMs"]) for sample in samples if sample["classification"] == "reload"],
        "unloadMs": [float(sample["unloadMs"]) for sample in samples],
        "inferenceMs": [float(sample["inferenceMs"]) for sample in samples],
        "realTimeFactor": [float(sample["realTimeFactor"]) for sample in samples],
        "firstPartialAfterFirstPcmMs": [float(sample["firstPartialAfterFirstPcmMs"]) for sample in samples],
        "firstFinalAfterFirstPcmMs": [float(sample["firstFinalAfterFirstPcmMs"]) for sample in samples],
        "firstVadEndAfterFirstPcmMs": [float(sample["firstVadEndAfterFirstPcmMs"]) for sample in samples],
        "finalAfterPttEndMs": [float(sample["finalAfterPttEndMs"]) for sample in samples],
        "cancelAckMs": [float(sample["cancelAckMs"]) for sample in samples],
        "rssLoadedBytes": [float(sample["rssLoadedBytes"]) for sample in samples],
        "peakRssBytes": [float(sample["peakRssBytes"]) for sample in samples],
        "averageCpuPercentOneCore": [float(sample["averageCpuPercentOneCore"]) for sample in samples],
    }
    gpu_process_values = [
        float(snapshot["ownProcessUsedMiB"])
        for sample in samples
        for snapshot in (sample["gpuLoaded"], sample["gpuPeak"], sample["gpuUnloaded"])
        if snapshot.get("available") and snapshot.get("ownProcessUsedMiB") is not None
    ]
    return {
        "metrics": {name: metric_summary(values) for name, values in fields.items()},
        "modelProcessVramMiB": metric_summary(gpu_process_values),
        "allCyclesHavePartialFinalVadAndCancel": all(
            sample["events"]["partialEvents"] > 0
            and sample["events"]["finalEvents"] > 0
            and sample["events"]["vadEnded"] > 0
            and sample["cancelAckMs"] >= 0
            for sample in samples
        ),
        "uniqueTranscriptHashes": sorted({sample["transcriptSha256"] for sample in samples}),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Real Moonshine local-STT qualification")
    parser.add_argument("--bridge", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--wav", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--gpu-lock", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cycles", type=int, default=21)
    parser.add_argument("--pace-audio", action="store_true")
    parser.add_argument("--authorized", choices=["YES"], required=True)
    args = parser.parse_args()
    if args.cycles < 21 or args.cycles > 32:
        raise SystemExit("cycles must be between 21 and 32 (one cold load plus at least 20 reloads)")
    if args.gpu_lock.read_text(encoding="utf-8").strip().lower() != "yes":
        raise SystemExit("GPU/model lane lock is not yes; qualification refused")
    for path, label in ((args.bridge, "bridge"), (args.model, "model"), (args.wav, "WAV"), (args.manifest, "manifest")):
        if not path.exists():
            raise SystemExit(f"{label} path does not exist: {path}")
    args.output.mkdir(parents=True, exist_ok=True)
    sample_rate, chunks, total_frames = load_wave(args.wav)
    if sample_rate != 16000:
        raise SystemExit("the pinned Beckett fixture must be 16 kHz")
    baseline_gpu = nvidia_snapshot()
    evidence: dict[str, Any] = {
        "schema": "npc.local-stt-qualification-evidence/v1",
        "qualificationId": str(uuid.uuid4()),
        "admissionEligible": False,
        "admissionBlock": "Unsigned local measurement; catalog trust and whole-loadout admission remain external gates.",
        "pack": {
            "packId": "npc.stt.moonshine-v2-medium-streaming-en.win-x64",
            "revision": "0.1.5-moonshine.234f60f",
            "manifestRawSha256": sha256_file(args.manifest),
            "artifactSha256": "a56bcd27765fefa4ab9a9b219cbb26e6de7d48b6536c88b92b6d24ffa7eb4c25",
            "bridgeSha256": sha256_file(args.bridge),
            "fixtureSha256": sha256_file(args.wav),
        },
        "benchmark": {
            "suiteRevision": "moonshine-real-2026.08.30.v1",
            "cycles": args.cycles,
            "pacedAudio": args.pace_audio,
            "audioSampleRateHz": sample_rate,
            "audioFrames": total_frames,
            "audioDurationMs": total_frames * 1000 / sample_rate,
            "chunkDurationMs": 200,
            "startedUnixMs": int(time.time() * 1000),
        },
        "hardware": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "logicalCpuCount": os.cpu_count(),
            "python": sys.version,
            "nvidiaBaseline": baseline_gpu,
        },
        "samples": [],
    }
    fingerprint_payload = json.dumps(evidence["hardware"], sort_keys=True, separators=(",", ":")).encode("utf-8")
    evidence["hardware"]["deviceFingerprintSha256"] = hashlib.sha256(fingerprint_payload).hexdigest()
    backend = MoonshineBridgeBackend(args.bridge, args.model)
    transcripts: list[dict[str, Any]] = []
    try:
        backend.warm()
        bridge_self_test = backend.self_test()
        evidence["bridgeSelfTest"] = bridge_self_test
        for cycle in range(args.cycles):
            sample, transcript = one_cycle(
                backend,
                cycle,
                sample_rate,
                chunks,
                total_frames,
                pace_audio=args.pace_audio,
            )
            evidence["samples"].append(sample)
            transcripts.append(transcript)
            evidence["benchmark"]["completedCycles"] = cycle + 1
            atomic_json(args.output / "qualification-progress.json", evidence)
            print(
                f"cycle {cycle + 1}/{args.cycles}: load={sample['loadMs']:.1f}ms "
                f"rtf={sample['realTimeFactor']:.3f} partial={sample['firstPartialAfterFirstPcmMs']:.1f}ms "
                f"finalEnd={sample['finalAfterPttEndMs']:.1f}ms cancel={sample['cancelAckMs']:.1f}ms",
                flush=True,
            )
    finally:
        try:
            backend.cancel()
        except Exception:
            pass
        try:
            backend.unload()
        except Exception:
            pass
        gc.collect()
    evidence["benchmark"]["finishedUnixMs"] = int(time.time() * 1000)
    evidence["summary"] = summarize(evidence["samples"])
    evidence["terminal"] = {
        "backendUnloaded": True,
        "processPid": os.getpid(),
        "rssBytes": process_rss_bytes(),
        "nvidia": nvidia_snapshot(),
    }
    if len(evidence["samples"]) < 21 or len([sample for sample in evidence["samples"] if sample["classification"] == "reload"]) < 20:
        raise RuntimeError("qualification did not produce the required sample counts")
    if not evidence["summary"]["allCyclesHavePartialFinalVadAndCancel"]:
        raise RuntimeError("one or more qualification cycles missed a required event or cancellation")
    canonical_transcript = transcripts[0]
    if any(transcript["textSha256"] != canonical_transcript["textSha256"] for transcript in transcripts):
        canonical_transcript["crossRunTranscriptHashes"] = sorted({transcript["textSha256"] for transcript in transcripts})
    atomic_json(args.output / "self-test-transcript.json", canonical_transcript)
    evidence["transcriptEvidence"] = {
        "path": "self-test-transcript.json",
        "sha256": sha256_file(args.output / "self-test-transcript.json"),
        "diagnosticsContainTranscriptText": False,
    }
    atomic_json(args.output / "qualification-evidence.json", evidence)
    (args.output / "qualification-progress.json").unlink(missing_ok=True)
    print(json.dumps({"passed": True, "output": str(args.output), "summary": evidence["summary"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
