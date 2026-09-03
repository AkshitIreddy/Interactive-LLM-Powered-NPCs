#!/usr/bin/env python3
"""Gated real CPU qualification for the immutable BGE pack.

This command has no download path.  It operates only on a pack that the Model
Manager already installed and verified.  Running it requires an explicit
acknowledgement plus the user-owned AI-model coordination lock.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import math
import os
import platform
import secrets
import tempfile
import threading
import time
from pathlib import Path
from typing import Sequence

from backend import (
    PINNED_NUMPY_VERSION,
    PINNED_ONNXRUNTIME_VERSION,
    PINNED_TOKENIZERS_VERSION,
    Cancelled,
    OnnxBgeBackend,
)
from gpu_lock import GpuLockBusy, ai_model_lock, externally_owned_ai_model_lock
from model_spec import (
    DIMENSIONS,
    MODEL_ID,
    PACK_ID,
    PACK_REVISION,
    QUERY_INSTRUCTION,
    SOURCE_REVISION,
    PackSpec,
    normalize_vector,
    sha256_file,
)
from pack_manager import PackLifecycle
from telemetry import Telemetry, percentile

REPORT_SCHEMA = "npc.embedding-qualification-report/v1"
SUITE_REVISION = "bge-small-en-v1.5-windows-cpu-v1"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Qualify the pinned local BGE embedding pack")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--install-root", type=Path, required=True)
    parser.add_argument("--gpu-lock-file", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=24)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--cpu-threads", type=int, default=2)
    parser.add_argument("--acknowledge-real-model-run", action="store_true")
    parser.add_argument("--external-lock-owned-by-root", action="store_true")
    return parser.parse_args(argv)


def _p99_int(values: Sequence[float]) -> int:
    value = percentile(values, 0.99)
    if value is None:
        raise RuntimeError("qualification produced no measurements")
    return max(1, math.ceil(value))


def _atomic_json(path: Path, value: dict[str, object]) -> None:
    path = path.resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    if len(encoded) > 1024 * 1024:
        raise RuntimeError("qualification report exceeded 1 MiB")
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".part", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        Path(temporary).replace(path)
    except Exception:
        with contextlib.suppress(FileNotFoundError):
            Path(temporary).unlink()
        raise


def _artifact_snapshot(lifecycle: PackLifecycle) -> list[dict[str, object]]:
    values = []
    for artifact in lifecycle.manifest.artifacts:
        path = lifecycle.target.joinpath(*artifact.destination.parts)
        digest, size = sha256_file(path, expected_size=artifact.size_bytes)
        values.append({"id": artifact.artifact_id, "size_bytes": size, "sha256": digest})
    return values


def _texts(batch_size: int, iteration: int) -> list[str]:
    # Fixed public fixtures only. Text is never copied into the report.
    fixtures = (
        "A blacksmith repairs a sword in the village forge.",
        "The village smith fixes a damaged blade.",
        "Saturn has an extensive ring system.",
        "The ranger returned to the northern watchtower before dawn.",
        "A merchant recorded the caravan debt in a weathered ledger.",
        "The old bridge is unsafe after three days of heavy rain.",
        "Ask the court archivist about the treaty signed at Red Harbor.",
        "The healer stores winter herbs beside the eastern window.",
    )
    return [fixtures[(iteration + index) % len(fixtures)] for index in range(batch_size)]


def _cancel_probe(backend: OnnxBgeBackend, *, attempts: int = 5) -> dict[str, object]:
    """Prove an active ONNX call reaches a no-output cancellation barrier."""

    if attempts < 1:
        raise ValueError("cancellation attempts must be positive")
    text = " ".join(["The archivist compares the complete village ledger before answering."] * 96)
    for attempt in range(1, attempts + 1):
        cancelled = threading.Event()
        outputs: list[tuple[list[tuple[float, ...]], int]] = []
        errors: list[tuple[BaseException, int]] = []

        def run() -> None:
            try:
                result = backend.infer([text] * 32, cancelled)
                outputs.append((result, time.monotonic_ns()))
            except BaseException as exc:  # captured for the qualification thread boundary
                errors.append((exc, time.monotonic_ns()))

        worker = threading.Thread(target=run, name="npc-embedding-cancel-qualification", daemon=True)
        worker.start()
        if not backend.wait_until_active(1.0):
            worker.join(2.0)
            if worker.is_alive():
                raise RuntimeError("cancellation probe could not observe or stop the backend call")
            continue
        barrier_started = time.monotonic_ns()
        cancelled.set()
        backend.cancel_active()
        worker.join(2.0)
        barrier_millis = (time.monotonic_ns() - barrier_started) / 1_000_000.0
        if worker.is_alive():
            raise RuntimeError("embedding cancellation exceeded the 2 second worker barrier")
        if outputs and outputs[0][1] < barrier_started:
            # The call won the observation race and completed before the cancel
            # request existed. It is neither a pass nor a late-output failure.
            continue
        if outputs:
            raise RuntimeError("embedding backend emitted tensor output after active cancellation")
        if len(errors) != 1 or not isinstance(errors[0][0], Cancelled):
            raise RuntimeError("embedding backend did not report the reviewed cancellation outcome")
        return {
            "attempts": attempt,
            "active_run_observed": True,
            "cancelled_without_output": True,
            "barrier_millis": barrier_millis,
        }
    raise RuntimeError("embedding inference completed before an active cancellation could be qualified")


def _cosine(left: Sequence[float], right: Sequence[float]) -> float:
    if len(left) != DIMENSIONS or len(right) != DIMENSIONS:
        raise RuntimeError("quality probe received a dimension mismatch")
    value = math.fsum(float(a) * float(b) for a, b in zip(left, right, strict=True))
    if not math.isfinite(value) or not -1.0001 <= value <= 1.0001:
        raise RuntimeError("quality probe produced an invalid cosine score")
    return value


def _quality_probe(backend: OnnxBgeBackend) -> dict[str, object]:
    passages = [
        "The village blacksmith repairs broken swords and damaged armor.",
        "The healer keeps medicinal winter herbs beside the eastern window.",
        "The court archivist preserves the treaty signed at Red Harbor.",
        "The old stone bridge is unsafe after several days of heavy rain.",
        "The ranger returned to the northern watchtower before dawn.",
        "The merchant recorded the caravan debt in a weathered ledger.",
        "Saturn is a gas giant with an extensive ring system.",
        "Bread dough rises when yeast ferments sugars and releases gas.",
    ]
    queries = [
        "Who can fix my broken sword?",
        "Where are the medicinal herbs stored?",
        "Who should I ask about the Red Harbor treaty?",
        "Is the bridge safe after the rain?",
        "Where did the ranger go before sunrise?",
        "Where was the caravan debt written down?",
    ]
    expected = list(range(len(queries)))
    passage_vectors = backend.infer(passages, threading.Event())
    query_vectors = backend.infer([QUERY_INSTRUCTION + query for query in queries], threading.Event())
    rows = [[_cosine(query, passage) for passage in passage_vectors] for query in query_vectors]
    rankings = [sorted(range(len(row)), key=lambda index: row[index], reverse=True) for row in rows]
    top1 = sum(ranking[0] == target for ranking, target in zip(rankings, expected, strict=True))
    top3 = sum(target in ranking[:3] for ranking, target in zip(rankings, expected, strict=True))
    margins = []
    for row, target in zip(rows, expected, strict=True):
        margins.append(row[target] - max(score for index, score in enumerate(row) if index != target))
    if top1 < 5 or top3 != len(queries) or min(margins) <= -0.02 or sum(margins) / len(margins) <= 0.05:
        raise RuntimeError("embedding top-k semantic quality gate failed")
    return {
        "contract_version": "npc.embedding-quality-report/v1",
        "queries": len(queries),
        "passages": len(passages),
        "top1_correct": top1,
        "top3_correct": top3,
        "top1_accuracy": top1 / len(queries),
        "top3_accuracy": top3 / len(queries),
        "minimum_expected_margin": min(margins),
        "mean_expected_margin": sum(margins) / len(margins),
        "cosine_minimum": min(min(row) for row in rows),
        "cosine_maximum": max(max(row) for row in rows),
        "query_instruction_applied": True,
    }


def qualify(args: argparse.Namespace) -> dict[str, object]:
    if not args.acknowledge_real_model_run:
        raise RuntimeError("real qualification requires --acknowledge-real-model-run")
    if args.samples < 20 or args.samples > 200:
        raise RuntimeError("samples must be in 20..=200")
    if args.warmups < 1 or args.warmups > 20:
        raise RuntimeError("warmups must be in 1..=20")
    if args.cpu_threads < 1 or args.cpu_threads > 8:
        raise RuntimeError("cpu-threads must be in 1..=8")

    pack = PackSpec.load(args.manifest)
    lifecycle = PackLifecycle(args.install_root, pack)
    verification = lifecycle.verify()
    if not verification.healthy:
        raise RuntimeError("installed pack is missing or corrupt; verify/repair before qualification")
    artifacts_before = _artifact_snapshot(lifecycle)
    load_ms: list[float] = []
    reload_ms: list[float] = []
    unload_ms: list[float] = []
    load_peak_rss: list[int] = []
    operation_ms: list[float] = []
    mixed_operation_ms: list[float] = []
    operation_cpu_ms: list[float] = []
    operation_peak_rss: list[int] = []
    batch_sizes: list[int] = []
    self_test: dict[str, object] | None = None
    quality_test: dict[str, object] | None = None
    cancellation_tests: list[dict[str, object]] = []

    lock_context = externally_owned_ai_model_lock(args.gpu_lock_file) if args.external_lock_owned_by_root else ai_model_lock(args.gpu_lock_file)
    with lock_context:
        backend: OnnxBgeBackend | None = None
        try:
            # A load sample uses a new backend object; the immediately following
            # unload->load on that same object is a reload sample.  Keeping the
            # distributions separate prevents the governor from treating the
            # reload budget as an alias for activation load.  OS page-cache state
            # is intentionally not manipulated by this worker: the outer trusted
            # harness records machine state and signs the reviewed distribution.
            for index in range(args.warmups + args.samples):
                sample_backend = OnnxBgeBackend()
                try:
                    started = Telemetry.snapshot()
                    sample_backend.load(lifecycle.target, pack, cpu_threads=args.cpu_threads)
                    ended = Telemetry.snapshot()
                    if index >= args.warmups:
                        load_ms.append((ended.monotonic_ns - started.monotonic_ns) / 1_000_000.0)
                        if ended.rss_bytes is not None:
                            load_peak_rss.append(ended.rss_bytes)
                    unload_started = Telemetry.snapshot()
                    sample_backend.unload()
                    unload_ended = Telemetry.snapshot()
                    if index >= args.warmups:
                        unload_ms.append((unload_ended.monotonic_ns - unload_started.monotonic_ns) / 1_000_000.0)

                    reload_started = Telemetry.snapshot()
                    sample_backend.load(lifecycle.target, pack, cpu_threads=args.cpu_threads)
                    reload_ended = Telemetry.snapshot()
                    if index >= args.warmups:
                        reload_ms.append((reload_ended.monotonic_ns - reload_started.monotonic_ns) / 1_000_000.0)
                        if reload_ended.rss_bytes is not None:
                            load_peak_rss.append(reload_ended.rss_bytes)
                finally:
                    final_unload_started = Telemetry.snapshot()
                    sample_backend.unload()
                    final_unload_ended = Telemetry.snapshot()
                    if index >= args.warmups:
                        unload_ms.append((final_unload_ended.monotonic_ns - final_unload_started.monotonic_ns) / 1_000_000.0)

            backend = OnnxBgeBackend()
            backend.load(lifecycle.target, pack, cpu_threads=args.cpu_threads)
            self_test = backend.self_test()
            quality_test = _quality_probe(backend)
            for index in range(args.warmups + args.samples):
                batch_size = 1
                cancelled = threading.Event()
                started = Telemetry.snapshot()
                vectors = backend.infer(_texts(batch_size, index), cancelled)
                ended = Telemetry.snapshot()
                if len(vectors) != batch_size:
                    raise RuntimeError("backend returned an incorrect result count")
                for vector in vectors:
                    normalize_vector(vector, DIMENSIONS)
                if index >= args.warmups:
                    operation_ms.append((ended.monotonic_ns - started.monotonic_ns) / 1_000_000.0)
                    operation_cpu_ms.append((ended.cpu_time_ns - started.cpu_time_ns) / 1_000_000.0)
                    if ended.rss_bytes is not None:
                        operation_peak_rss.append(ended.rss_bytes)
                    batch_sizes.append(batch_size)
            # Supplemental throughput shapes do not feed the governor's p99
            # operation field, which is deliberately batch-1 latency.
            for index in range(args.warmups + args.samples):
                batch_size = (4, 16, 32)[index % 3]
                started = Telemetry.snapshot()
                vectors = backend.infer(_texts(batch_size, index), threading.Event())
                ended = Telemetry.snapshot()
                if len(vectors) != batch_size:
                    raise RuntimeError("backend returned an incorrect throughput result count")
                if index >= args.warmups:
                    batch_sizes.append(batch_size)
                    # Kept only as raw supplemental samples below.
                    mixed_operation_ms.append((ended.monotonic_ns - started.monotonic_ns) / 1_000_000.0)
            for _ in range(args.samples):
                cancellation_tests.append(_cancel_probe(backend))
        finally:
            if backend is not None:
                backend.unload()

    artifacts_after = _artifact_snapshot(lifecycle)
    if artifacts_before != artifacts_after:
        raise RuntimeError("model artifacts changed during qualification")
    if not operation_peak_rss or not load_peak_rss:
        raise RuntimeError("process RAM telemetry is unavailable on this platform")
    resident_ram = min(operation_peak_rss)
    now = int(time.time())
    return {
        "schema": REPORT_SCHEMA,
        "report_id": f"bge-small-cpu-{now}-{secrets.token_hex(6)}",
        "suite_revision": SUITE_REVISION,
        "measured_unix_seconds": now,
        "provenance": "real_local_onnxruntime_cpu",
        "identity": {"pack_id": PACK_ID, "revision": PACK_REVISION},
        "manifest_sha256": pack.manifest_sha256,
        "source": {"model_id": MODEL_ID, "source_revision": SOURCE_REVISION},
        "runtime": {
            "python": platform.python_version(),
            "onnxruntime": PINNED_ONNXRUNTIME_VERSION,
            "numpy": PINNED_NUMPY_VERSION,
            "tokenizers": PINNED_TOKENIZERS_VERSION,
            "backend": "cpu",
            "cpu_threads": args.cpu_threads,
        },
        "hardware": {
            "platform": platform.platform(),
            "logical_cpu_count": os.cpu_count(),
            "device_fingerprint_sha256": None,
            "device_fingerprint_owner": "system-telemetry integration",
        },
        "samples": {
            "warmups": args.warmups,
            "measured_loads": len(load_ms),
            "measured_reloads": len(reload_ms),
            "measured_operations": len(operation_ms),
            "measured_unloads": len(unload_ms),
            "measured_cancellation_barriers": len(cancellation_tests),
            "batch_sizes": batch_sizes,
        },
        "placement": {
            "residency_mode": "cpu_resident",
            "resident_ram_bytes": resident_ram,
            "p99_total_ram_bytes": _p99_int([float(value) for value in operation_peak_rss + load_peak_rss]),
            "resident_vram_bytes": 0,
            "p99_workspace_vram_bytes": 0,
            "p99_load_millis": _p99_int(load_ms),
            "p99_reload_millis": _p99_int(reload_ms),
            "p99_operation_millis": _p99_int(operation_ms),
        },
        "supplemental": {
            "p99_unload_millis": _p99_int(unload_ms),
            "p99_cancel_millis": _p99_int([float(value["barrier_millis"]) for value in cancellation_tests]),
        },
        "distributions": {
            "load_millis": {"p50": percentile(load_ms, 0.50), "p95": percentile(load_ms, 0.95), "p99": percentile(load_ms, 0.99)},
            "reload_millis": {"p50": percentile(reload_ms, 0.50), "p95": percentile(reload_ms, 0.95), "p99": percentile(reload_ms, 0.99)},
            "unload_millis": {"p50": percentile(unload_ms, 0.50), "p95": percentile(unload_ms, 0.95), "p99": percentile(unload_ms, 0.99)},
            "operation_millis": {"p50": percentile(operation_ms, 0.50), "p95": percentile(operation_ms, 0.95), "p99": percentile(operation_ms, 0.99)},
            "operation_cpu_millis": {"p50": percentile(operation_cpu_ms, 0.50), "p95": percentile(operation_cpu_ms, 0.95), "p99": percentile(operation_cpu_ms, 0.99)},
        },
        "raw_samples": {
            "load_millis": load_ms,
            "reload_millis": reload_ms,
            "unload_millis": unload_ms,
            "operation_millis": operation_ms,
            "mixed_operation_millis": mixed_operation_ms,
            "operation_cpu_millis": operation_cpu_ms,
            "load_rss_bytes": load_peak_rss,
            "operation_rss_bytes": operation_peak_rss,
            "batch_sizes": batch_sizes,
        },
        "self_test": self_test,
        "quality_test": quality_test,
        "cancellation_tests": cancellation_tests,
        "artifact_verification": {"before": artifacts_before, "after": artifacts_after},
        "activation": {
            "qualified_resource_envelope_created": False,
            "reason": "Resource governor must bind system telemetry fingerprint and sign the reviewed report.",
            "outer_probe_required": [
                "device_fingerprint_sha256",
                "game_and_desktop_vram_pressure",
                "game_frame_time_baseline_and_active",
                "measurement_signature"
            ],
        },
    }


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        report = qualify(args)
        _atomic_json(args.out, report)
        print(json.dumps({"ok": True, "report": str(args.out.resolve()), "report_sha256": hashlib.sha256(args.out.read_bytes()).hexdigest()}))
        return 0
    except GpuLockBusy as exc:
        print(json.dumps({"ok": False, "code": "ai_model_lock_busy", "message": str(exc)}))
        return 5
    except Exception as exc:
        print(json.dumps({"ok": False, "code": "qualification_failed", "message": str(exc)}))
        return 4


if __name__ == "__main__":
    raise SystemExit(main())
