"""Strictly observational LM Studio control; never direct-admission evidence."""

from __future__ import annotations

import http.client
import json
import os
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Any, Iterator

from npc_local_llm.constants import MODEL_ARTIFACT_SHA256, MODEL_ARTIFACT_SIZE
from npc_local_llm.digest import canonical_json_bytes, sha256_file
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.measurements import nvidia_smi_snapshot
from npc_local_llm.sse import CompletionDelta, SseDecoder, parse_openai_event
from npc_local_llm.windows_job import CREATE_NO_WINDOW

from .harness import _gpu_used, _wait_for_vram_return, require_explicit_gpu_grant


def load_control(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_bytes())
        if value["schema"] != "npc.local-llm.observational-control/v1":
            raise ValueError("schema")
        if value["classification"] != "installed_third_party_observational_control_not_redistributable_not_admission_evidence":
            raise ValueError("classification")
        if value["engine"]["engine_version"] != "2.31.2" or value["engine"]["llama_cpp_release"] != "b10662":
            raise ValueError("engine")
        if value["engine"]["cuda_label"] != "12.8":
            raise ValueError("CUDA label")
        if value["limits"]["samples"] != 10 or value["limits"]["p99_or_admission_claims_allowed"] is not False:
            raise ValueError("sample policy")
        if value["model_access"]["mode"] != "temporary_symbolic_link_only" or value["model_access"]["copy_model"] is not False:
            raise ValueError("model access")
        return value
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise LocalLlmError("invalid_lmstudio_control", "LM Studio control trust record is invalid") from error


def verify_control(control: dict[str, Any]) -> dict[str, Any]:
    if os.name != "nt":
        raise LocalLlmError("unsupported_platform", "LM Studio control requires Windows Python")
    cli = _expand(control["cli"]["path_template"])
    if sha256_file(cli, expected_size=control["cli"]["size_bytes"]) != control["cli"]["sha256"]:
        raise LocalLlmError("lmstudio_identity_mismatch", "LM Studio CLI does not match the control record")
    engine = _expand(control["engine"]["directory_template"])
    vendor = _expand(control["vendor_runtime"]["directory_template"])
    _verify_files(engine, control["engine"]["files"])
    _verify_files(vendor, control["vendor_runtime"]["files"])
    for filename, field in (
        ("backend-manifest.json", "backend_manifest_sha256"),
        ("display-data.json", "display_data_sha256"),
        ("engine-protocol-server-artifacts.json", "protocol_artifacts_sha256"),
    ):
        if sha256_file(engine / filename) != control["engine"][field]:
            raise LocalLlmError("lmstudio_identity_mismatch", "LM Studio engine manifest failed verification")
    runtime = _lms(cli, ["runtime", "ls"])
    if control["engine"]["alias"] not in runtime.stdout:
        raise LocalLlmError("lmstudio_runtime_mismatch", "pinned LM Studio runtime is not installed")
    return {
        "cli_sha256": control["cli"]["sha256"],
        "engine": control["engine"]["alias"],
        "cuda_label": control["engine"]["cuda_label"],
        "verified_files": len(control["engine"]["files"]) + len(control["vendor_runtime"]["files"]),
    }


def build_load_command(cli: Path, model_key: str, identifier: str) -> list[str]:
    return [
        str(cli),
        "load",
        model_key,
        "--gpu",
        "max",
        "--context-length",
        "8192",
        "--parallel",
        "1",
        "--identifier",
        identifier,
        "--yes",
    ]


def run_observational_control(
    *,
    control_path: Path,
    fixture_path: Path,
    model_path: Path,
    gpu_lock_file: Path | None,
    execute_model_benchmark: bool,
    vram_return_tolerance_mib: int = 128,
    vram_return_timeout_seconds: int = 30,
) -> dict[str, Any]:
    require_explicit_gpu_grant(gpu_lock_file, execute_model_benchmark)
    control = load_control(control_path)
    identity = verify_control(control)
    if sha256_file(model_path, expected_size=MODEL_ARTIFACT_SIZE) != MODEL_ARTIFACT_SHA256:
        raise LocalLlmError("model_identity_mismatch", "LM Studio control model does not match the frozen GGUF")
    try:
        fixture = json.loads(fixture_path.read_bytes())
    except (OSError, json.JSONDecodeError) as error:
        raise LocalLlmError("invalid_self_test", "LM Studio control fixture is invalid") from error
    cli = _expand(control["cli"]["path_template"])
    if json.loads(_lms(cli, ["ps", "--json"]).stdout) != []:
        raise LocalLlmError("lmstudio_in_use", "LM Studio already has a loaded model; the control will not disturb it")
    status = _lms(cli, ["server", "status"], check=False)
    if "not running" not in (status.stdout + status.stderr).casefold():
        raise LocalLlmError("lmstudio_in_use", "LM Studio server is already active; isolated control attribution is unavailable")
    expected_link = _expand(control["model_access"]["dry_run_destination_template"], strict=False)
    link_created = False
    if not expected_link.exists() and not expected_link.is_symlink():
        imported = _lms(
            cli,
            [
                "import",
                str(model_path),
                "--symbolic-link",
                "--user-repo",
                "interactive-npcs/qwen3-4b-instruct-2507",
                "--yes",
            ],
            check=False,
        )
        if imported.returncode != 0:
            return {
                "schema": "npc.local-llm.lmstudio-control-result/v1",
                "classification": "observational_only_no_p99_no_admission",
                "available": False,
                "reason": "Windows did not permit the required no-copy symbolic link",
                "identity": identity,
            }
        link_created = True
        deadline = time.monotonic() + 5.0
        while not expected_link.is_symlink() and time.monotonic() < deadline:
            time.sleep(0.05)
    if not expected_link.is_symlink() or expected_link.resolve(strict=True) != model_path.resolve(strict=True):
        if link_created and expected_link.is_symlink():
            expected_link.unlink()
        raise LocalLlmError("lmstudio_model_link_mismatch", "LM Studio model entry is not the exact temporary symbolic link")
    port = _candidate_port()
    identifier = "npc-qwen3-control"
    model_key = "interactive-npcs/qwen3-4b-instruct-2507"
    observations: list[dict[str, Any]] = []
    server_started = False
    try:
        baseline_snapshot = nvidia_smi_snapshot("lmstudio_control_before")
        baseline = _gpu_used(baseline_snapshot)
        _lms(cli, ["server", "start", "--port", str(port), "--bind", "127.0.0.1"])
        server_started = True
        transport = _LmStudioTransport(port)
        for sample in range(1, 11):
            load_started = time.perf_counter()
            _run_hidden(build_load_command(cli, model_key, identifier))
            load_millis = (time.perf_counter() - load_started) * 1000.0
            transport.health()
            structured = transport.structured_probe(identifier, fixture)
            cancelled = transport.cancellation_probe(identifier)
            recovery = transport.structured_probe(identifier, fixture)
            _lms(cli, ["unload", identifier])
            after_unload = _wait_for_vram_return(
                baseline,
                tolerance_mib=vram_return_tolerance_mib,
                timeout_seconds=vram_return_timeout_seconds,
                label="lmstudio_control_after_unload",
            )
            reload_started = time.perf_counter()
            _run_hidden(build_load_command(cli, model_key, identifier))
            reload_millis = (time.perf_counter() - reload_started) * 1000.0
            reload_recovery = transport.structured_probe(identifier, fixture)
            _lms(cli, ["unload", identifier])
            after_reload_unload = _wait_for_vram_return(
                baseline,
                tolerance_mib=vram_return_tolerance_mib,
                timeout_seconds=vram_return_timeout_seconds,
                label="lmstudio_control_after_reload_unload",
            )
            observations.append(
                {
                    "sample": sample,
                    "load_millis": load_millis,
                    "reload_millis": reload_millis,
                    "ttft_millis": structured["ttft_millis"],
                    "operation_millis": structured["total_millis"],
                    "tokens_per_second": structured["tokens_per_second"],
                    "structured": structured["valid"] and recovery["valid"] and reload_recovery["valid"],
                    "cancel_recovery": cancelled and recovery["valid"],
                    "after_unload": after_unload,
                    "after_reload_unload": after_reload_unload,
                }
            )
    finally:
        _lms(cli, ["unload", identifier], check=False)
        if server_started:
            _lms(cli, ["server", "stop"], check=False)
        if link_created and expected_link.is_symlink():
            expected_link.unlink()
    after = _wait_for_vram_return(
        baseline,
        tolerance_mib=vram_return_tolerance_mib,
        timeout_seconds=vram_return_timeout_seconds,
        label="lmstudio_control_after_cleanup",
    )
    return {
        "schema": "npc.local-llm.lmstudio-control-result/v1",
        "classification": "observational_only_no_p99_no_admission",
        "available": True,
        "identity": identity,
        "initial_gpu": baseline_snapshot,
        "final_gpu": after,
        "sample_count": len(observations),
        "raw_observations": observations,
        "p99": None,
    }


class _LmStudioTransport:
    def __init__(self, port: int) -> None:
        self.port = port
        self._active: http.client.HTTPResponse | None = None
        self._lock = threading.Lock()

    def health(self) -> None:
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5.0)
        try:
            connection.request("GET", "/v1/models", headers={"Accept": "application/json"})
            response = connection.getresponse()
            body = response.read(1_048_577)
            if response.status != 200 or len(body) > 1_048_576 or not isinstance(json.loads(body), dict):
                raise LocalLlmError("lmstudio_unhealthy", "LM Studio local server health check failed")
        finally:
            connection.close()

    def stream(self, body: dict[str, Any], cancelled) -> Iterator[CompletionDelta]:  # type: ignore[no-untyped-def]
        encoded = canonical_json_bytes(body)
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=120.0)
        try:
            connection.request(
                "POST",
                "/v1/chat/completions",
                body=encoded,
                headers={"Content-Type": "application/json", "Accept": "text/event-stream"},
            )
            response = connection.getresponse()
            with self._lock:
                self._active = response
            if response.status != 200:
                response.read(8192)
                raise LocalLlmError("lmstudio_inference_failed", "LM Studio rejected the control request")
            decoder = SseDecoder()
            while True:
                if cancelled():
                    response.close()
                    raise LocalLlmError("cancelled", "LM Studio control request was cancelled")
                line = response.readline(1_048_577)
                if not line:
                    break
                for raw in decoder.feed(line):
                    event = parse_openai_event(raw)
                    if event is not None:
                        yield event
            decoder.finish()
        finally:
            with self._lock:
                self._active = None
            connection.close()

    def cancel(self) -> None:
        with self._lock:
            if self._active is not None:
                self._active.close()

    def structured_probe(self, identifier: str, fixture: dict[str, Any]) -> dict[str, Any]:
        body = {
            "model": identifier,
            "messages": fixture["messages"],
            "stream": True,
            "stream_options": {"include_usage": True},
            "max_tokens": fixture["sampling"]["max_tokens"],
            "temperature": fixture["sampling"]["temperature"],
            "top_p": fixture["sampling"]["top_p"],
            "seed": fixture["sampling"]["seed"],
            "response_format": {
                "type": "json_schema",
                "json_schema": {"name": "npc_response", "strict": True, "schema": fixture["response_json_schema"]},
            },
        }
        started = time.perf_counter()
        first: float | None = None
        parts: list[str] = []
        usage: dict[str, int] = {}
        for delta in self.stream(body, lambda: False):
            if delta.text:
                first = first or time.perf_counter()
                parts.append(delta.text)
            if delta.usage:
                usage.update(delta.usage)
        finished = time.perf_counter()
        if first is None:
            raise LocalLlmError("lmstudio_probe_failed", "LM Studio structured control emitted no output")
        try:
            valid = json.loads("".join(parts)) == fixture["expected_canonical_output"]
        except json.JSONDecodeError as error:
            raise LocalLlmError("lmstudio_probe_failed", "LM Studio structured control emitted invalid JSON") from error
        if not valid:
            raise LocalLlmError("lmstudio_probe_failed", "LM Studio structured control did not match the fixture")
        total = (finished - started) * 1000.0
        ttft = (first - started) * 1000.0
        tokens = usage.get("completion_tokens")
        throughput = tokens / ((total - ttft) / 1000.0) if tokens and total > ttft else None
        return {"valid": True, "total_millis": total, "ttft_millis": ttft, "tokens_per_second": throughput}

    def cancellation_probe(self, identifier: str) -> bool:
        event = threading.Event()
        result = {"cancelled": False}
        body = {
            "model": identifier,
            "messages": [{"role": "user", "content": "Write a long numbered list of short neutral words."}],
            "stream": True,
            "max_tokens": 512,
            "temperature": 0.0,
            "top_p": 1.0,
            "seed": 424242,
        }

        def generate() -> None:
            try:
                for delta in self.stream(body, event.is_set):
                    if delta.text:
                        event.set()
            except (LocalLlmError, OSError, http.client.HTTPException):
                result["cancelled"] = event.is_set()

        thread = threading.Thread(target=generate, name="lmstudio-control-cancel", daemon=True)
        thread.start()
        thread.join(timeout=20.0)
        if thread.is_alive():
            event.set()
            self.cancel()
        thread.join(timeout=5.0)
        return not thread.is_alive() and (result["cancelled"] or event.is_set())


def _expand(template: str, *, strict: bool = True) -> Path:
    return Path(os.path.expandvars(template)).resolve(strict=strict)


def _verify_files(root: Path, files: list[dict[str, Any]]) -> None:
    for item in files:
        path = root / item["path"]
        if sha256_file(path, expected_size=item["size_bytes"]) != item["sha256"]:
            raise LocalLlmError("lmstudio_identity_mismatch", "LM Studio runtime file failed verification")


def _candidate_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def _lms(cli: Path, arguments: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return _run_hidden([str(cli), *arguments], check=check)


def _run_hidden(command: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            command,
            check=check,
            capture_output=True,
            text=True,
            timeout=240.0,
            creationflags=CREATE_NO_WINDOW,
            shell=False,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise LocalLlmError("lmstudio_command_failed", "LM Studio control command failed") from error
