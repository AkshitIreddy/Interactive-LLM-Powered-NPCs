"""Authorized Windows-only direct Vulkan/CUDA qualification harness.

Importing or planning this module cannot launch a model. `run_benchmark` also
requires both an explicit execution flag and an independently owned lock file.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

from npc_local_llm.constants import MODEL_ARTIFACT_SHA256, MODEL_ARTIFACT_SIZE, RUNTIME_ABI
from npc_local_llm.digest import canonical_json_bytes, sha256_file
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.measurements import nvidia_smi_snapshot, windows_process_memory
from npc_local_llm.protocol import parse_chat_request
from npc_local_llm.server import LlamaServerSupervisor, ServerConfig, build_server_command
from npc_local_llm.windows_job import CREATE_NO_WINDOW

from .config import BenchmarkContract
from .schedule import abba_schedule, measured_distribution, warmup_schedule


def require_explicit_gpu_grant(lock_path: Path | None, execute_model_benchmark: bool) -> None:
    if not execute_model_benchmark:
        raise LocalLlmError("benchmark_execution_not_authorized", "model benchmark requires the explicit execution flag")
    if lock_path is None:
        raise LocalLlmError("gpu_lock_required", "model benchmark requires the GPU coordination lock")
    try:
        state = lock_path.read_text(encoding="utf-8").strip().lower()
    except OSError as error:
        raise LocalLlmError("gpu_lock_required", "GPU coordination lock could not be read") from error
    if state != "yes":
        raise LocalLlmError("gpu_lock_required", "GPU coordination lock is not authorized")


def direct_backend_configs(
    contract: BenchmarkContract,
    model_path: Path,
    vulkan_executable: Path,
    cuda_executable: Path,
) -> dict[str, ServerConfig]:
    controls = contract.controls
    shared = {
        "model_path": model_path,
        "runtime_abi": RUNTIME_ABI,
        "context_tokens": controls.context_tokens,
        "cpu_threads": controls.cpu_threads,
        "cpu_threads_batch": controls.cpu_threads_batch,
        "gpu_layers": controls.gpu_layers,
        "batch_tokens": controls.batch_tokens,
        "ubatch_tokens": controls.ubatch_tokens,
        "cache_type_k": controls.cache_type_k,
        "cache_type_v": controls.cache_type_v,
        "flash_attention": controls.flash_attention,
        "cache_prompt": controls.cache_prompt,
        "startup_timeout_seconds": 180.0,
        "request_timeout_seconds": 120.0,
    }
    return {
        "vulkan": ServerConfig(executable=vulkan_executable, backend="vulkan", **shared),
        "cuda": ServerConfig(executable=cuda_executable, backend="cuda", **shared),
    }


def comparable_argv(configs: dict[str, ServerConfig]) -> dict[str, list[str]]:
    """Return fixed argv with path/port/key locations normalized for equality."""
    values: dict[str, list[str]] = {}
    for backend, config in configs.items():
        command = build_server_command(config, 49152, Path(r"C:\benchmark\key.txt"))
        command[0] = "<llama-server.exe>"
        command[2] = "<model.gguf>"
        values[backend] = command
    if values["vulkan"] != values["cuda"]:
        raise LocalLlmError("benchmark_controls_differ", "direct backend runtime arguments are not identical")
    return values


def preflight(
    contract: BenchmarkContract,
    model_path: Path,
    vulkan_executable: Path,
    cuda_executable: Path,
) -> tuple[dict[str, ServerConfig], dict[str, Any]]:
    if os.name != "nt":
        raise LocalLlmError("unsupported_platform", "direct local LLM benchmark must run in Windows Python")
    _assert_no_llama_server_processes()
    if sha256_file(model_path, expected_size=MODEL_ARTIFACT_SIZE) != MODEL_ARTIFACT_SHA256:
        raise LocalLlmError("model_identity_mismatch", "benchmark model does not match the frozen GGUF")
    configs = direct_backend_configs(contract, model_path, vulkan_executable, cuda_executable)
    for config in configs.values():
        config.validate()
    argv = comparable_argv(configs)
    return configs, {
        "model_sha256": MODEL_ARTIFACT_SHA256,
        "model_size_bytes": MODEL_ARTIFACT_SIZE,
        "contract_sha256": sha256_file(contract.path),
        "fixture_sha256": sha256_file(contract.fixture_path),
        "normalized_direct_argv": argv["vulkan"],
    }


def run_benchmark(
    *,
    contract: BenchmarkContract,
    model_path: Path,
    vulkan_executable: Path,
    cuda_executable: Path,
    gpu_lock_file: Path | None,
    execute_model_benchmark: bool,
    runtime_identity: dict[str, Any],
) -> dict[str, Any]:
    require_explicit_gpu_grant(gpu_lock_file, execute_model_benchmark)
    configs, identity = preflight(contract, model_path, vulkan_executable, cuda_executable)
    fixture = json.loads(contract.fixture_path.read_bytes())
    measured: dict[str, list[dict[str, Any]]] = {"vulkan": [], "cuda": []}
    warmups: list[dict[str, Any]] = []
    initial_gpu = nvidia_smi_snapshot("benchmark_before")
    baseline = _gpu_used(initial_gpu)
    try:
        for backend in warmup_schedule(contract.warmups_per_backend):
            result = _run_cycle(backend, configs[backend], fixture, contract, baseline)
            warmups.append({"backend": backend, "passed": result["checks"]["all_passed"]})
        for sequence, backend in enumerate(abba_schedule(contract.measured_samples_per_backend), start=1):
            result = _run_cycle(backend, configs[backend], fixture, contract, baseline)
            result["sequence"] = sequence
            measured[backend].append(result)
    finally:
        final_gpu = _wait_for_vram_return(
            baseline,
            tolerance_mib=contract.vram_return_tolerance_mib,
            timeout_seconds=contract.vram_return_timeout_seconds,
            label="benchmark_after_cleanup",
        )
        _assert_no_llama_server_processes()
    summaries = {backend: _summarize_backend(samples) for backend, samples in measured.items()}
    report: dict[str, Any] = {
        "schema": "npc.local-llm.backend-ab-result/v1",
        "provenance": "measured_local_review",
        "admissible_for_resource_governor": False,
        "non_admission_reasons": [
            "unsigned local benchmark",
            "not yet wrapped by QualifiedResourceEnvelopeV1",
            "synthetic game-load matrix is reported separately",
        ],
        "measured_unix_ms": int(time.time() * 1000),
        "identity": identity,
        "runtime_identity": runtime_identity,
        "schedule": {
            "warmup": list(warmup_schedule(contract.warmups_per_backend)),
            "measured": list(abba_schedule(contract.measured_samples_per_backend)),
            "measured_samples_per_backend": contract.measured_samples_per_backend,
        },
        "initial_gpu": initial_gpu,
        "final_gpu": final_gpu,
        "warmups": warmups,
        "raw_samples": measured,
        "summaries": summaries,
        "structured_output_passed": all(sample["checks"]["structured"] for values in measured.values() for sample in values),
        "cancellation_recovery_passed": all(sample["checks"]["cancel_recovery"] for values in measured.values() for sample in values),
        "reload_passed": all(sample["checks"]["reload"] for values in measured.values() for sample in values),
        "vram_return_passed": all(sample["checks"]["vram_return"] for values in measured.values() for sample in values),
        "game_matrix": {
            "status": "not_run_by_direct_backend_lane",
            "classification": "bounded_synthetic_host_load_not_real_game_frame_impact",
            "run_separately_with": "game-matrix subcommand",
            "p99_reporting_allowed": False,
        },
    }
    report["report_sha256"] = hashlib.sha256(canonical_json_bytes(report)).hexdigest()
    return report


def _run_cycle(
    backend: str,
    config: ServerConfig,
    fixture: dict[str, Any],
    contract: BenchmarkContract,
    baseline_gpu_mib: dict[str, int],
) -> dict[str, Any]:
    server = LlamaServerSupervisor(config)
    load_started = time.perf_counter()
    server.start()
    load_millis = (time.perf_counter() - load_started) * 1000.0
    try:
        assert server.process is not None
        loaded_gpu = nvidia_smi_snapshot(f"{backend}_loaded")
        memory = windows_process_memory(server.process.pid)
        primary = _structured_probe(server, fixture)
        cancelled = _cancellation_probe(server)
        recovery = _structured_probe(server, fixture)
    finally:
        server.stop()
    after_unload = _wait_for_vram_return(
        baseline_gpu_mib,
        tolerance_mib=contract.vram_return_tolerance_mib,
        timeout_seconds=contract.vram_return_timeout_seconds,
        label=f"{backend}_after_unload",
    )
    reloaded = LlamaServerSupervisor(config)
    reload_started = time.perf_counter()
    reloaded.start()
    reload_millis = (time.perf_counter() - reload_started) * 1000.0
    try:
        reload_recovery = _structured_probe(reloaded, fixture)
    finally:
        reloaded.stop()
    after_reload_unload = _wait_for_vram_return(
        baseline_gpu_mib,
        tolerance_mib=contract.vram_return_tolerance_mib,
        timeout_seconds=contract.vram_return_timeout_seconds,
        label=f"{backend}_after_reload_unload",
    )
    checks = {
        "structured": primary["valid"] and recovery["valid"] and reload_recovery["valid"],
        "cancel_recovery": cancelled and recovery["valid"],
        "reload": reload_recovery["valid"],
        "vram_return": _within_vram_tolerance(after_unload, baseline_gpu_mib, contract.vram_return_tolerance_mib)
        and _within_vram_tolerance(after_reload_unload, baseline_gpu_mib, contract.vram_return_tolerance_mib),
    }
    checks["all_passed"] = all(checks.values())
    if not checks["all_passed"]:
        raise LocalLlmError("benchmark_cycle_failed", "direct backend benchmark lifecycle check failed")
    return {
        "backend": backend,
        "load_millis": load_millis,
        "reload_millis": reload_millis,
        "operation_millis": primary["total_millis"],
        "ttft_millis": primary["ttft_millis"],
        "tokens_per_second": primary["tokens_per_second"],
        "output_tokens": primary["output_tokens"],
        "inter_token_millis": primary["inter_token_millis"],
        "process_memory": memory,
        "gpu": {
            "baseline_used_mib": baseline_gpu_mib,
            "loaded": loaded_gpu,
            "loaded_delta_mib": {
                key: _gpu_used(loaded_gpu)[key] - baseline_gpu_mib[key] for key in baseline_gpu_mib
            },
            "after_unload": after_unload,
            "after_reload_unload": after_reload_unload,
        },
        "checks": checks,
    }


def _structured_probe(server: LlamaServerSupervisor, fixture: dict[str, Any]) -> dict[str, Any]:
    request = parse_chat_request(
        {
            "messages": fixture["messages"],
            "max_tokens": fixture["sampling"]["max_tokens"],
            "temperature": fixture["sampling"]["temperature"],
            "top_p": fixture["sampling"]["top_p"],
            "seed": fixture["sampling"]["seed"],
            "response_json_schema": fixture["response_json_schema"],
        }
    )
    started = time.perf_counter()
    first_token: float | None = None
    times: list[float] = []
    parts: list[str] = []
    usage: dict[str, int] = {}
    for delta in server.stream_chat(request, lambda: False):
        now = time.perf_counter()
        if delta.text:
            first_token = first_token or now
            times.append(now)
            parts.append(delta.text)
        if delta.usage:
            usage.update(delta.usage)
    finished = time.perf_counter()
    if first_token is None:
        raise LocalLlmError("benchmark_probe_failed", "structured probe emitted no output")
    try:
        actual = json.loads("".join(parts))
    except json.JSONDecodeError as error:
        raise LocalLlmError("benchmark_probe_failed", "structured probe emitted invalid JSON") from error
    valid = actual == fixture["expected_canonical_output"]
    if not valid:
        raise LocalLlmError("benchmark_probe_failed", "structured probe output did not match the fixture")
    total_millis = (finished - started) * 1000.0
    ttft_millis = (first_token - started) * 1000.0
    output_tokens = usage.get("completion_tokens")
    tokens_per_second = None
    if output_tokens is not None and output_tokens > 0 and total_millis > ttft_millis:
        tokens_per_second = output_tokens / ((total_millis - ttft_millis) / 1000.0)
    return {
        "valid": True,
        "total_millis": total_millis,
        "ttft_millis": ttft_millis,
        "output_tokens": output_tokens,
        "tokens_per_second": tokens_per_second,
        "inter_token_millis": [(right - left) * 1000.0 for left, right in zip(times, times[1:])],
    }


def _cancellation_probe(server: LlamaServerSupervisor) -> bool:
    cancelled = threading.Event()
    outcome = {"cancelled": False}

    def generate() -> None:
        request = parse_chat_request(
            {
                "prompt": "Write a long numbered list of short neutral words.",
                "max_tokens": 512,
                "temperature": 0.0,
                "top_p": 1.0,
                "seed": 424242,
            }
        )
        try:
            for delta in server.stream_chat(request, cancelled.is_set):
                if delta.text:
                    cancelled.set()
        except LocalLlmError as error:
            outcome["cancelled"] = error.code == "cancelled"

    thread = threading.Thread(target=generate, name="local-llm-cancel-probe", daemon=True)
    thread.start()
    thread.join(timeout=20.0)
    if thread.is_alive():
        cancelled.set()
    preserved = server.cancel_active(grace_seconds=2.0)
    thread.join(timeout=5.0)
    return not thread.is_alive() and preserved and (outcome["cancelled"] or cancelled.is_set())


def _gpu_used(snapshot: dict[str, Any]) -> dict[str, int]:
    return {str(device["uuid"]): int(device["memory_used_mib"]) for device in snapshot["devices"]}


def _within_vram_tolerance(snapshot: dict[str, Any], baseline: dict[str, int], tolerance_mib: int) -> bool:
    current = _gpu_used(snapshot)
    return set(current) == set(baseline) and all(current[key] <= baseline[key] + tolerance_mib for key in baseline)


def _wait_for_vram_return(
    baseline: dict[str, int], *, tolerance_mib: int, timeout_seconds: int, label: str
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    last = nvidia_smi_snapshot(label)
    while not _within_vram_tolerance(last, baseline, tolerance_mib):
        if time.monotonic() >= deadline:
            raise LocalLlmError("vram_not_released", "GPU memory did not return to the bounded baseline")
        time.sleep(0.5)
        last = nvidia_smi_snapshot(label)
    return last


def _summarize_backend(samples: list[dict[str, Any]]) -> dict[str, Any]:
    if len(samples) != 20:
        raise LocalLlmError("insufficient_benchmark_samples", "backend does not have exactly 20 measured samples")
    tokens = [float(item["tokens_per_second"]) for item in samples if item["tokens_per_second"] is not None]
    if len(tokens) != 20:
        raise LocalLlmError("benchmark_usage_missing", "backend did not return token usage for every sample")
    inter_token = [float(value) for item in samples for value in item["inter_token_millis"]]
    return {
        "load_millis": measured_distribution(item["load_millis"] for item in samples),
        "reload_millis": measured_distribution(item["reload_millis"] for item in samples),
        "operation_millis": measured_distribution(item["operation_millis"] for item in samples),
        "ttft_millis": measured_distribution(item["ttft_millis"] for item in samples),
        "tokens_per_second": measured_distribution(tokens),
        "inter_token_millis": measured_distribution(inter_token),
        "working_set_bytes": measured_distribution(item["process_memory"]["working_set_bytes"] for item in samples),
        "private_bytes": measured_distribution(item["process_memory"]["private_bytes"] for item in samples),
        "gpu_resident_delta_mib": measured_distribution(
            max(item["gpu"]["loaded_delta_mib"].values()) for item in samples
        ),
        "peak_gpu_used_mib": max(
            max(device["memory_used_mib"] for device in item["gpu"]["loaded"]["devices"]) for item in samples
        ),
        "peak_working_set_bytes": max(item["process_memory"]["peak_working_set_bytes"] for item in samples),
        "peak_private_bytes": max(item["process_memory"]["private_bytes"] for item in samples),
    }


def launch_synthetic_game(repo_root: Path, state: dict[str, Any], exit_after_seconds: int = 900) -> tuple[int, dict[str, Any]]:
    """Launch only the project-owned fixture; return exact child PID and evidence."""
    if os.name != "nt":
        raise LocalLlmError("unsupported_platform", "synthetic game matrix requires Windows")
    if state["id"] == "baseline":
        return 0, {"state": "baseline", "fixture_started": False}
    metadata = repo_root / "out" / "evidence" / f"local-llm-game-{state['id']}.json"
    command = [
        "powershell.exe",
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        str(repo_root / "scripts" / "synthetic-game-replay.ps1"),
        "-MetadataPath",
        str(metadata),
        "-Width",
        str(state["width"]),
        "-Height",
        str(state["height"]),
        "-FramesPerSecond",
        str(state["frames_per_second"]),
        "-ExitAfterSeconds",
        str(exit_after_seconds),
        "-Mute",
        "-PlaceOnSecondMonitor",
    ]
    completed = subprocess.run(
        command,
        check=True,
        capture_output=True,
        text=True,
        timeout=60.0,
        creationflags=CREATE_NO_WINDOW,
    )
    try:
        evidence = json.loads(completed.stdout)
        pid = int(evidence["pid"])
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise LocalLlmError("synthetic_game_failed", "synthetic game returned invalid launch evidence") from error
    return pid, evidence


def stop_synthetic_game(pid: int) -> None:
    if pid <= 0:
        return
    subprocess.run(
        ["taskkill.exe", "/PID", str(pid), "/T", "/F"],
        check=False,
        capture_output=True,
        timeout=15.0,
        creationflags=CREATE_NO_WINDOW,
    )


def run_game_matrix(
    *,
    contract: BenchmarkContract,
    repo_root: Path,
    model_path: Path,
    vulkan_executable: Path,
    cuda_executable: Path,
    gpu_lock_file: Path | None,
    execute_model_benchmark: bool,
    allow_synthetic_game_gui: bool,
) -> dict[str, Any]:
    """Run the separately classified 5x backend/state synthetic load matrix."""
    require_explicit_gpu_grant(gpu_lock_file, execute_model_benchmark)
    if not allow_synthetic_game_gui:
        raise LocalLlmError(
            "synthetic_game_gui_not_authorized",
            "game matrix requires separate coordination before opening the task-owned synthetic GUI",
        )
    configs, identity = preflight(contract, model_path, vulkan_executable, cuda_executable)
    fixture = json.loads(contract.fixture_path.read_bytes())
    baseline_snapshot = nvidia_smi_snapshot("game_matrix_before")
    baseline = _gpu_used(baseline_snapshot)
    observations: list[dict[str, Any]] = []
    states = contract.raw["game_matrix"]["states"]
    repeats = int(contract.raw["game_matrix"]["repeats_per_backend_per_state"])
    try:
        for state in states:
            fixture_pid = 0
            fixture_evidence: dict[str, Any] = {}
            try:
                fixture_pid, fixture_evidence = launch_synthetic_game(repo_root, state)
                for repeat in range(repeats):
                    order = ("vulkan", "cuda") if repeat % 2 == 0 else ("cuda", "vulkan")
                    for backend in order:
                        cycle = _run_cycle(backend, configs[backend], fixture, contract, baseline)
                        observations.append(
                            {
                                "state": state["id"],
                                "repeat": repeat + 1,
                                "backend": backend,
                                "load_millis": cycle["load_millis"],
                                "reload_millis": cycle["reload_millis"],
                                "operation_millis": cycle["operation_millis"],
                                "ttft_millis": cycle["ttft_millis"],
                                "tokens_per_second": cycle["tokens_per_second"],
                                "process_memory": cycle["process_memory"],
                                "checks": cycle["checks"],
                                "fixture": fixture_evidence,
                            }
                        )
            finally:
                stop_synthetic_game(fixture_pid)
    finally:
        after = _wait_for_vram_return(
            baseline,
            tolerance_mib=contract.vram_return_tolerance_mib,
            timeout_seconds=contract.vram_return_timeout_seconds,
            label="game_matrix_after_cleanup",
        )
        _assert_no_llama_server_processes()
    expected = len(states) * repeats * 2
    if len(observations) != expected:
        raise LocalLlmError("game_matrix_incomplete", "synthetic game-load matrix did not complete every observation")
    return {
        "schema": "npc.local-llm.synthetic-game-matrix-result/v1",
        "classification": "bounded_synthetic_host_load_not_real_game_frame_impact",
        "admissible_for_resource_governor": False,
        "p99_reporting_allowed": False,
        "reason": contract.raw["game_matrix"]["reason"],
        "identity": identity,
        "initial_gpu": baseline_snapshot,
        "final_gpu": after,
        "observation_count": len(observations),
        "raw_observations": observations,
    }


def _assert_no_llama_server_processes() -> None:
    completed = subprocess.run(
        ["tasklist.exe", "/FI", "IMAGENAME eq llama-server.exe", "/FO", "CSV", "/NH"],
        check=True,
        capture_output=True,
        text=True,
        timeout=10.0,
        creationflags=CREATE_NO_WINDOW,
    )
    if "llama-server.exe" in completed.stdout.casefold():
        raise LocalLlmError(
            "runtime_process_conflict",
            "an existing llama-server process prevents isolated benchmark attribution",
        )
