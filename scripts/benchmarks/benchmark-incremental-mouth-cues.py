#!/usr/bin/env python3
"""Measure immutable mouth cues from bounded Rhubarb prefix/window passes.

This is a standalone, trace-based replay.  It does not claim that Rhubarb is a
streaming recognizer or that the product currently invokes it incrementally.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
from decimal import Decimal, ROUND_HALF_UP
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import threading
import time
from typing import Iterable, Sequence
import wave


SCHEMA = "npc-incremental-mouth-cue-benchmark-v1"
RHUBARB_SHAPES = frozenset("XABCDEFGH")


@dataclass(frozen=True)
class Cue:
    start_frame: int
    end_frame: int
    shape: str


@dataclass(frozen=True)
class RecognitionRun:
    prefix_end_frame: int
    window_start_frame: int
    elapsed_ms: float
    virtual_start_ms: float
    virtual_finish_ms: float
    committed_until_frame: int
    accepted_cue_count: int


class IncrementalCueError(RuntimeError):
    pass


class ImmutableCueLedger:
    """Append-only sample-clock cue ledger for one cancellation generation."""

    def __init__(self, generation: int) -> None:
        if generation < 1:
            raise ValueError("generation must be positive")
        self.generation = generation
        self.cancelled = False
        self.committed_until_frame = 0
        self.cues: list[Cue] = []

    def cancel(self, generation: int) -> None:
        if generation == self.generation:
            self.cancelled = True

    def accept(
        self,
        generation: int,
        recognized: Sequence[Cue],
        watermark_frame: int,
    ) -> bool:
        if generation != self.generation or self.cancelled:
            return False
        if watermark_frame < self.committed_until_frame:
            raise IncrementalCueError("commit watermark moved backwards")
        if watermark_frame == self.committed_until_frame:
            return True
        validate_contiguous_cues(recognized)
        cursor = self.committed_until_frame
        projected: list[Cue] = []
        for cue in recognized:
            start = max(cue.start_frame, cursor)
            end = min(cue.end_frame, watermark_frame)
            if end <= start:
                continue
            if start != cursor:
                raise IncrementalCueError("recognition window does not cover commit cursor")
            projected.append(Cue(start, end, cue.shape))
            cursor = end
            if cursor == watermark_frame:
                break
        if cursor != watermark_frame:
            raise IncrementalCueError("recognition result does not cover commit watermark")
        for cue in projected:
            if self.cues and self.cues[-1].shape == cue.shape:
                previous = self.cues[-1]
                self.cues[-1] = Cue(previous.start_frame, cue.end_frame, cue.shape)
            else:
                self.cues.append(cue)
        self.committed_until_frame = watermark_frame
        return True


def validate_contiguous_cues(cues: Sequence[Cue]) -> None:
    if not cues:
        raise IncrementalCueError("recognizer returned no cues")
    cursor = cues[0].start_frame
    for cue in cues:
        if cue.shape not in RHUBARB_SHAPES:
            raise IncrementalCueError("recognizer returned an unknown mouth shape")
        if cue.start_frame != cursor or cue.end_frame <= cue.start_frame:
            raise IncrementalCueError("recognizer cues are not contiguous")
        cursor = cue.end_frame


def decimal_seconds_to_frames(value: object, sample_rate: int) -> int:
    return int(
        (Decimal(str(value)) * sample_rate).to_integral_value(rounding=ROUND_HALF_UP)
    )


def parse_rhubarb_json(
    path: Path,
    expected_audio: Path,
    sample_rate: int,
    window_frames: int,
    frame_offset: int,
) -> list[Cue]:
    document = json.loads(path.read_text(encoding="utf-8-sig"))
    metadata = document.get("metadata", {})
    if Path(metadata.get("soundFile", "")).resolve() != expected_audio.resolve():
        raise IncrementalCueError("recognizer output references different audio")
    duration_frames = decimal_seconds_to_frames(metadata.get("duration"), sample_rate)
    # Rhubarb serializes centiseconds, so only its final sub-centisecond tail may
    # be extended to the exact PCM sample count.
    if not 0 <= window_frames - duration_frames < sample_rate / 100:
        raise IncrementalCueError("recognizer duration does not match PCM window")
    result: list[Cue] = []
    for raw in document.get("mouthCues", []):
        shape = raw.get("value")
        if shape not in RHUBARB_SHAPES:
            raise IncrementalCueError("recognizer returned an unknown mouth shape")
        start = decimal_seconds_to_frames(raw.get("start"), sample_rate)
        end = decimal_seconds_to_frames(raw.get("end"), sample_rate)
        result.append(Cue(frame_offset + start, frame_offset + end, shape))
    validate_contiguous_cues(result)
    expected_start = frame_offset
    expected_end = frame_offset + window_frames
    if result[0].start_frame != expected_start:
        raise IncrementalCueError("recognizer omitted the start of the PCM window")
    if not 0 <= expected_end - result[-1].end_frame < sample_rate / 100:
        raise IncrementalCueError("recognizer omitted more than its rounded PCM tail")
    last = result[-1]
    result[-1] = Cue(last.start_frame, expected_end, last.shape)
    validate_contiguous_cues(result)
    return result


def run_process_cancellable(
    command: Sequence[str],
    timeout_seconds: float,
    cancelled: threading.Event,
    poll_seconds: float = 0.01,
) -> tuple[bytes, bytes, float]:
    if cancelled.is_set():
        raise IncrementalCueError("recognition cancelled before process start")
    started = time.perf_counter()
    process = subprocess.Popen(
        list(command), stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    try:
        while process.poll() is None:
            if cancelled.wait(poll_seconds):
                process.kill()
                process.communicate()
                raise IncrementalCueError("recognition cancelled")
            if time.perf_counter() - started > timeout_seconds:
                process.kill()
                process.communicate()
                raise IncrementalCueError("recognition timed out")
        stdout, stderr = process.communicate()
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate()
    elapsed_ms = (time.perf_counter() - started) * 1000
    if process.returncode != 0:
        raise IncrementalCueError(
            f"recognizer exited with status {process.returncode}; stderr omitted"
        )
    return stdout, stderr, elapsed_ms


def write_pcm_window(
    destination: Path,
    params: wave._wave_params,
    pcm: bytes,
    start_frame: int,
    end_frame: int,
) -> None:
    frame_bytes = params.nchannels * params.sampwidth
    with wave.open(str(destination), "wb") as output:
        output.setparams(params)
        output.writeframes(pcm[start_frame * frame_bytes : end_frame * frame_bytes])


def recognize(
    rhubarb: Path,
    params: wave._wave_params,
    pcm: bytes,
    start_frame: int,
    end_frame: int,
    threads: int,
    timeout_seconds: float,
    cancelled: threading.Event,
    temporary_root: Path,
    sequence: int,
) -> tuple[list[Cue], float]:
    audio = temporary_root / f"window-{sequence:03d}.wav"
    output = temporary_root / f"window-{sequence:03d}.json"
    write_pcm_window(audio, params, pcm, start_frame, end_frame)
    command = [
        str(rhubarb),
        "-r",
        "phonetic",
        "-f",
        "json",
        "--threads",
        str(threads),
        "-q",
        "-o",
        str(output),
        str(audio),
    ]
    _, _, elapsed_ms = run_process_cancellable(command, timeout_seconds, cancelled)
    cues = parse_rhubarb_json(
        output,
        audio,
        params.framerate,
        end_frame - start_frame,
        start_frame,
    )
    return cues, elapsed_ms


def prefix_endpoints(
    total_frames: int, sample_rate: int, initial_ms: int, cadence_ms: int
) -> list[int]:
    initial = min(total_frames, max(1, round(initial_ms * sample_rate / 1000)))
    step = max(1, round(cadence_ms * sample_rate / 1000))
    result = list(range(initial, total_frames, step))
    if not result or result[-1] != total_frames:
        result.append(total_frames)
    return result


def disagreement_frames(left: Sequence[Cue], right: Sequence[Cue]) -> int:
    validate_contiguous_cues(left)
    validate_contiguous_cues(right)
    if left[0].start_frame != right[0].start_frame or left[-1].end_frame != right[-1].end_frame:
        raise IncrementalCueError("cue timelines cover different sample ranges")
    i = j = disagreed = 0
    cursor = left[0].start_frame
    while i < len(left) and j < len(right):
        end = min(left[i].end_frame, right[j].end_frame)
        if left[i].shape != right[j].shape:
            disagreed += end - cursor
        cursor = end
        if cursor == left[i].end_frame:
            i += 1
        if cursor == right[j].end_frame:
            j += 1
    return disagreed


def safe_playback_start_ms(runs: Sequence[RecognitionRun], sample_rate: int) -> float:
    if not runs:
        raise IncrementalCueError("no recognition runs")
    required = runs[0].virtual_finish_ms
    # Before the next result arrives, playback can consume only the horizon
    # committed by the preceding result.
    for previous, following in zip(runs, runs[1:]):
        horizon_ms = previous.committed_until_frame * 1000 / sample_rate
        required = max(required, following.virtual_finish_ms - horizon_ms)
    return required


def benchmark_strategy(
    strategy: str,
    rhubarb: Path,
    params: wave._wave_params,
    pcm: bytes,
    total_frames: int,
    baseline: Sequence[Cue],
    initial_ms: int,
    cadence_ms: int,
    window_ms: int,
    holdback_ms: int,
    threads: int,
    timeout_seconds: float,
    cancelled: threading.Event,
    temporary_root: Path,
) -> dict[str, object]:
    sample_rate = params.framerate
    endpoints = prefix_endpoints(total_frames, sample_rate, initial_ms, cadence_ms)
    window_frames = round(window_ms * sample_rate / 1000)
    holdback_frames = round(holdback_ms * sample_rate / 1000)
    ledger = ImmutableCueLedger(generation=1)
    runs: list[RecognitionRun] = []
    virtual_available_ms = 0.0
    index = 0
    while True:
        prefix_end = endpoints[index]
        arrival_ms = prefix_end * 1000 / sample_rate
        virtual_start_ms = max(arrival_ms, virtual_available_ms)
        if strategy == "growing-prefix":
            window_start = 0
        elif strategy == "rolling-window":
            window_start = max(0, prefix_end - window_frames)
        else:
            raise ValueError(f"unknown strategy: {strategy}")
        cues, elapsed_ms = recognize(
            rhubarb,
            params,
            pcm,
            window_start,
            prefix_end,
            threads,
            timeout_seconds,
            cancelled,
            temporary_root / strategy,
            len(runs),
        )
        virtual_finish_ms = virtual_start_ms + elapsed_ms
        final = prefix_end == total_frames
        watermark = total_frames if final else max(0, prefix_end - holdback_frames)
        before = len(ledger.cues)
        if not ledger.accept(1, cues, watermark):
            raise IncrementalCueError("current generation result was rejected")
        runs.append(
            RecognitionRun(
                prefix_end,
                window_start,
                round(elapsed_ms, 3),
                round(virtual_start_ms, 3),
                round(virtual_finish_ms, 3),
                ledger.committed_until_frame,
                len(ledger.cues) - before,
            )
        )
        virtual_available_ms = virtual_finish_ms
        if final:
            break
        next_index = index + 1
        while (
            next_index + 1 < len(endpoints)
            and endpoints[next_index + 1] * 1000 / sample_rate <= virtual_available_ms
        ):
            next_index += 1
        index = next_index
    validate_contiguous_cues(ledger.cues)
    if ledger.cues[0].start_frame != 0 or ledger.cues[-1].end_frame != total_frames:
        raise IncrementalCueError("incremental ledger does not cover exact PCM sample clock")
    disagreed = disagreement_frames(ledger.cues, baseline)
    analyzed_frames = sum(run.prefix_end_frame - run.window_start_frame for run in runs)
    processing_ms = sum(run.elapsed_ms for run in runs)
    return {
        "strategy": strategy,
        "recognition_runs": len(runs),
        "first_stable_cue_ready_ms_from_first_pcm": runs[0].virtual_finish_ms,
        "minimum_no_underrun_playback_delay_ms": round(
            safe_playback_start_ms(runs, sample_rate), 3
        ),
        "last_cue_ready_ms_from_first_pcm": runs[-1].virtual_finish_ms,
        "total_recognizer_process_ms": round(processing_ms, 3),
        "analyzed_audio_seconds": round(analyzed_frames / sample_rate, 6),
        "aggregate_processing_rtf": round(
            processing_ms / 1000 / (analyzed_frames / sample_rate), 6
        ),
        "incremental_cue_count": len(ledger.cues),
        "full_pass_disagreement_frames": disagreed,
        "full_pass_disagreement_percent": round(disagreed * 100 / total_frames, 4),
        "exact_sample_clock_coverage": True,
        "past_intervals_revised": 0,
        "runs": [asdict(run) for run in runs],
        "cues": [asdict(cue) for cue in ledger.cues],
    }


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audio", required=True, type=Path)
    parser.add_argument("--rhubarb", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument(
        "--strategies",
        nargs="+",
        choices=("rolling-window", "growing-prefix"),
        default=("rolling-window",),
    )
    parser.add_argument("--initial-ms", type=int, default=500)
    parser.add_argument("--cadence-ms", type=int, default=250)
    parser.add_argument("--window-ms", type=int, default=2500)
    parser.add_argument("--holdback-ms", type=int, default=250)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--timeout-seconds", type=float, default=10.0)
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args(argv)
    if min(args.initial_ms, args.cadence_ms, args.window_ms) <= 0:
        parser.error("prefix, cadence, and window durations must be positive")
    if args.holdback_ms < 0 or args.holdback_ms >= args.window_ms:
        parser.error("holdback must be non-negative and shorter than the window")
    if not 1 <= args.threads <= 16:
        parser.error("threads must be between 1 and 16")
    if args.timeout_seconds <= 0:
        parser.error("timeout must be positive")
    return args


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.audio.is_file() or not args.rhubarb.is_file():
        raise IncrementalCueError("audio and Rhubarb executable must exist")
    if args.out.exists() and not args.overwrite:
        raise IncrementalCueError("output exists; pass --overwrite to replace it")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(args.audio), "rb") as source:
        params = source.getparams()
        if params.comptype != "NONE" or params.sampwidth != 2 or params.nchannels not in (1, 2):
            raise IncrementalCueError("input must be uncompressed PCM16 mono or stereo")
        total_frames = source.getnframes()
        pcm = source.readframes(total_frames)
    cancelled = threading.Event()
    with tempfile.TemporaryDirectory(
        prefix="incremental-mouth-cues-", dir=str(args.out.parent)
    ) as directory:
        temporary_root = Path(directory)
        for strategy in args.strategies:
            (temporary_root / strategy).mkdir()
        baseline_root = temporary_root / "full-pass"
        baseline_root.mkdir()
        baseline, baseline_ms = recognize(
            args.rhubarb,
            params,
            pcm,
            0,
            total_frames,
            args.threads,
            args.timeout_seconds,
            cancelled,
            baseline_root,
            0,
        )
        results = [
            benchmark_strategy(
                strategy,
                args.rhubarb,
                params,
                pcm,
                total_frames,
                baseline,
                args.initial_ms,
                args.cadence_ms,
                args.window_ms,
                args.holdback_ms,
                args.threads,
                args.timeout_seconds,
                cancelled,
                temporary_root,
            )
            for strategy in args.strategies
        ]
    version = subprocess.run(
        [str(args.rhubarb), "--version"], capture_output=True, text=True, timeout=5
    ).stdout.strip()
    report = {
        "schema": SCHEMA,
        "classification": {
            "execution": "headless CPU standalone",
            "replay": "trace-based paced-arrival simulation",
            "product_streaming_integration": False,
            "perceptual_quality_ground_truth": False,
        },
        "audio": {
            "path": str(args.audio.resolve()),
            "sha256": file_sha256(args.audio),
            "sample_rate": params.framerate,
            "channels": params.nchannels,
            "sample_width_bytes": params.sampwidth,
            "frames": total_frames,
            "duration_seconds": total_frames / params.framerate,
        },
        "recognizer": {
            "path": str(args.rhubarb.resolve()),
            "sha256": file_sha256(args.rhubarb),
            "version": version,
            "mode": "phonetic",
            "threads": args.threads,
            "full_pass_ms": round(baseline_ms, 3),
            "full_pass_cue_count": len(baseline),
        },
        "configuration": {
            "initial_prefix_ms": args.initial_ms,
            "cadence_ms": args.cadence_ms,
            "rolling_window_ms": args.window_ms,
            "commit_holdback_ms": args.holdback_ms,
            "process_timeout_seconds": args.timeout_seconds,
            "generation": 1,
        },
        "safety_contract": {
            "commit_clock": "integer PCM sample frames",
            "already_committed_intervals_are_immutable": True,
            "stale_generation_results_are_rejected": True,
            "cancelled_generation_results_are_rejected": True,
            "recognizer_process_is_killed_on_cancel_or_timeout": True,
        },
        "strategies": results,
    }
    args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "out": str(args.out),
                "strategies": [
                    {
                        key: result[key]
                        for key in (
                            "strategy",
                            "recognition_runs",
                            "first_stable_cue_ready_ms_from_first_pcm",
                            "minimum_no_underrun_playback_delay_ms",
                            "total_recognizer_process_ms",
                            "full_pass_disagreement_percent",
                        )
                    }
                    for result in results
                ],
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
