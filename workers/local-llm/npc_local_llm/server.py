"""Supervised, loopback-only llama-server process and streaming client."""

from __future__ import annotations

import http.client
import json
import os
import secrets
import socket
import subprocess
import tempfile
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterator, Protocol

from .constants import MAX_CONTEXT_TOKENS, MAX_HTTP_ERROR_BYTES, MODEL_ID, RUNTIME_ABI
from .digest import canonical_json_bytes
from .errors import LocalLlmError
from .sse import CompletionDelta, SseDecoder, parse_openai_event
from .windows_job import CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, WindowsJob


@dataclass(frozen=True, slots=True)
class ServerConfig:
    executable: Path
    model_path: Path
    runtime_abi: str
    backend: str
    context_tokens: int = 8_192
    cpu_threads: int = 4
    cpu_threads_batch: int | None = None
    gpu_layers: int = 0
    batch_tokens: int = 2_048
    ubatch_tokens: int = 512
    cache_type_k: str = "f16"
    cache_type_v: str = "f16"
    flash_attention: str = "auto"
    cache_prompt: bool = True
    startup_timeout_seconds: float = 120.0
    request_timeout_seconds: float = 60.0
    process_memory_limit_bytes: int | None = None

    def validate(self) -> None:
        if os.name != "nt":
            raise LocalLlmError("unsupported_platform", "production local LLM runtime requires Windows")
        if self.runtime_abi != RUNTIME_ABI:
            raise LocalLlmError("runtime_abi_mismatch", "runtime ABI is not approved")
        if self.backend not in {"cpu", "vulkan", "cuda"}:
            raise LocalLlmError("unsupported_runtime_backend", "runtime backend is not approved")
        for path, name in ((self.executable, "runtime executable"), (self.model_path, "model")):
            if not path.is_absolute() or not path.is_file() or path.is_symlink():
                raise LocalLlmError("unsafe_runtime_path", f"{name} path is not an absolute regular file")
        if self.executable.name.casefold() != "llama-server.exe":
            raise LocalLlmError("runtime_entrypoint_mismatch", "runtime executable name is not approved")
        if not 512 <= self.context_tokens <= MAX_CONTEXT_TOKENS:
            raise LocalLlmError("invalid_runtime_config", "context-token limit is outside the approved range")
        if not 1 <= self.cpu_threads <= 128:
            raise LocalLlmError("invalid_runtime_config", "CPU thread count is outside the approved range")
        if self.cpu_threads_batch is not None and not 1 <= self.cpu_threads_batch <= 128:
            raise LocalLlmError("invalid_runtime_config", "batch CPU thread count is outside the approved range")
        if not 0 <= self.gpu_layers <= 256 or (self.backend == "cpu" and self.gpu_layers != 0):
            raise LocalLlmError("invalid_runtime_config", "GPU layer count is incompatible with the backend")
        if not 1 <= self.ubatch_tokens <= self.batch_tokens <= 8_192:
            raise LocalLlmError("invalid_runtime_config", "batch and ubatch sizes are outside the approved range")
        if self.cache_type_k not in {"f16", "q8_0"} or self.cache_type_v not in {"f16", "q8_0"}:
            raise LocalLlmError("invalid_runtime_config", "KV cache type is outside the benchmark contract")
        if self.flash_attention not in {"on", "off", "auto"}:
            raise LocalLlmError("invalid_runtime_config", "Flash Attention mode is outside the benchmark contract")


@dataclass(frozen=True, slots=True)
class ChatCompletionRequest:
    messages: tuple[dict[str, str], ...]
    max_tokens: int
    temperature: float
    top_p: float
    seed: int | None
    response_json_schema: dict[str, Any] | None


class CompletionTransport(Protocol):
    def stream_chat(
        self,
        request: ChatCompletionRequest,
        cancelled: Callable[[], bool],
    ) -> Iterator[CompletionDelta]: ...

    def health(self) -> dict[str, Any]: ...

    def cancel_active(self, grace_seconds: float = 1.0) -> bool: ...


class _BoundedPipeTail:
    def __init__(self, stream) -> None:  # type: ignore[no-untyped-def]
        self.stream = stream
        self.tail: deque[bytes] = deque(maxlen=32)
        self.thread = threading.Thread(target=self._drain, name="llama-server-log-drain", daemon=True)
        self.thread.start()

    def _drain(self) -> None:
        try:
            while True:
                line = self.stream.readline(4_096)
                if not line:
                    return
                self.tail.append(line[:4_096])
        except OSError:
            return


class LlamaServerSupervisor(CompletionTransport):
    def __init__(self, config: ServerConfig) -> None:
        config.validate()
        self.config = config
        self.process: subprocess.Popen[bytes] | None = None
        self.job: WindowsJob | None = None
        self.port: int | None = None
        self.api_key: str | None = None
        self._key_file: Path | None = None
        self._active_response: http.client.HTTPResponse | None = None
        self._active_lock = threading.Lock()
        self._active_done = threading.Event()
        self._active_done.set()
        self._started_monotonic: float | None = None

    def start(self) -> dict[str, Any]:
        if self.process is not None and self.process.poll() is None:
            return self.health()
        self.port = _reserve_candidate_port()
        self.api_key = secrets.token_urlsafe(32)
        self._key_file = _write_api_key_file(self.api_key)
        command = build_server_command(self.config, self.port, self._key_file)
        flags = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        self.job = WindowsJob(process_memory_limit_bytes=self.config.process_memory_limit_bytes)
        try:
            self.process = subprocess.Popen(
                command,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                cwd=self.config.executable.parent,
                env=_minimal_runtime_environment(self.config.executable.parent),
                creationflags=flags,
                close_fds=True,
                shell=False,
            )
            self.job.assign_pid_handle(int(self.process._handle))  # type: ignore[attr-defined]
            if self.process.stdout is not None:
                _BoundedPipeTail(self.process.stdout)
            if self.process.stderr is not None:
                _BoundedPipeTail(self.process.stderr)
            self._started_monotonic = time.monotonic()
            deadline = self._started_monotonic + self.config.startup_timeout_seconds
            last_error: Exception | None = None
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    raise LocalLlmError("runtime_start_failed", "llama-server exited before becoming ready")
                try:
                    status = self.health()
                    if status.get("status") == "ok":
                        self._key_file.unlink(missing_ok=True)
                        self._key_file = None
                        return status
                except LocalLlmError as error:
                    last_error = error
                time.sleep(0.1)
            raise LocalLlmError("runtime_start_timeout", "llama-server did not become ready in time", retryable=True) from last_error
        except Exception:
            self.stop()
            raise

    def health(self) -> dict[str, Any]:
        if self.process is None or self.process.poll() is not None or self.port is None or self.api_key is None:
            raise LocalLlmError("runtime_unavailable", "llama-server is not running", retryable=True)
        public = self._json_request("GET", "/health", authenticated=False, timeout=2.0)
        models = self._json_request("GET", "/v1/models", authenticated=True, timeout=2.0)
        if not isinstance(public, dict) or public.get("status") != "ok":
            raise LocalLlmError("runtime_unhealthy", "llama-server health response is invalid", retryable=True)
        if not isinstance(models, dict) or not isinstance(models.get("data"), list):
            raise LocalLlmError("runtime_identity_failed", "authenticated llama-server identity check failed")
        return {
            "status": "ok",
            "pid": self.process.pid,
            "backend": self.config.backend,
            "runtime_abi": self.config.runtime_abi,
            "model_id": MODEL_ID,
            "uptime_millis": int((time.monotonic() - (self._started_monotonic or time.monotonic())) * 1000),
        }

    def stream_chat(
        self,
        request: ChatCompletionRequest,
        cancelled: Callable[[], bool],
    ) -> Iterator[CompletionDelta]:
        if self.process is None or self.process.poll() is not None or self.port is None or self.api_key is None:
            raise LocalLlmError("runtime_unavailable", "llama-server is not running", retryable=True)
        body: dict[str, Any] = {
            "model": MODEL_ID,
            "messages": list(request.messages),
            "stream": True,
            "stream_options": {"include_usage": True},
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "top_p": request.top_p,
            "cache_prompt": self.config.cache_prompt,
        }
        if request.seed is not None:
            body["seed"] = request.seed
        if request.response_json_schema is not None:
            body["response_format"] = {
                "type": "json_schema",
                "json_schema": {
                    "name": "npc_response",
                    "strict": True,
                    "schema": request.response_json_schema,
                },
            }
        encoded = canonical_json_bytes(body)
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=self.config.request_timeout_seconds)
        self._active_done.clear()
        try:
            connection.request(
                "POST",
                "/v1/chat/completions",
                body=encoded,
                headers={
                    "Authorization": f"Bearer {self.api_key}",
                    "Content-Type": "application/json",
                    "Accept": "text/event-stream",
                    "Content-Length": str(len(encoded)),
                },
            )
            response = connection.getresponse()
            with self._active_lock:
                self._active_response = response
            if response.status != 200:
                response.read(MAX_HTTP_ERROR_BYTES)
                raise LocalLlmError("inference_failed", "llama-server rejected the generation request", retryable=response.status >= 500)
            content_type = response.getheader("Content-Type", "").lower()
            if not content_type.startswith("text/event-stream"):
                raise LocalLlmError("invalid_stream", "llama-server response is not an SSE stream")
            decoder = SseDecoder()
            event_count = 0
            while True:
                if cancelled():
                    response.close()
                    raise LocalLlmError("cancelled", "generation was cancelled")
                # `HTTPResponse.read(N)` may wait to fill N bytes on a chunked
                # response and hide first-token latency. SSE is line-oriented;
                # read one bounded line so each llama-server flush reaches the
                # Worker Control token stream immediately.
                chunk = response.readline(1_048_577)
                if not chunk:
                    break
                if len(chunk) > 1_048_576:
                    raise LocalLlmError("oversized_sse_event", "llama-server emitted an oversized SSE line")
                for raw in decoder.feed(chunk):
                    event_count += 1
                    if event_count > 4_096:
                        raise LocalLlmError("event_limit_exceeded", "llama-server exceeded the event ceiling")
                    event = parse_openai_event(raw)
                    if event is not None:
                        yield event
            decoder.finish()
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            if cancelled():
                raise LocalLlmError("cancelled", "generation was cancelled") from error
            raise LocalLlmError("runtime_transport", "llama-server transport failed", retryable=True) from error
        finally:
            with self._active_lock:
                self._active_response = None
            self._active_done.set()
            connection.close()

    def cancel_active(self, grace_seconds: float = 1.0) -> bool:
        with self._active_lock:
            response = self._active_response
            if response is not None:
                response.close()
        drained = self._active_done.wait(max(0.0, min(grace_seconds, 5.0)))
        if not drained:
            self.stop()
        return drained

    def stop(self) -> None:
        with self._active_lock:
            if self._active_response is not None:
                self._active_response.close()
                self._active_response = None
        process = self.process
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                if self.job is not None:
                    self.job.terminate()
                process.kill()
                process.wait(timeout=2.0)
        if self.job is not None:
            self.job.close()
        if self._key_file is not None:
            self._key_file.unlink(missing_ok=True)
        self.process = None
        self.job = None
        self.port = None
        self.api_key = None
        self._key_file = None
        self._started_monotonic = None
        self._active_done.set()

    def _json_request(self, method: str, path: str, *, authenticated: bool, timeout: float) -> Any:
        assert self.port is not None
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        headers = {"Accept": "application/json"}
        if authenticated:
            assert self.api_key is not None
            headers["Authorization"] = f"Bearer {self.api_key}"
        try:
            connection.request(method, path, headers=headers)
            response = connection.getresponse()
            body = response.read(MAX_HTTP_ERROR_BYTES + 1)
            if response.status != 200 or len(body) > MAX_HTTP_ERROR_BYTES:
                raise LocalLlmError("runtime_unhealthy", "llama-server health endpoint failed", retryable=True)
            return json.loads(body)
        except (OSError, http.client.HTTPException, UnicodeDecodeError, json.JSONDecodeError) as error:
            raise LocalLlmError("runtime_unhealthy", "llama-server health endpoint failed", retryable=True) from error
        finally:
            connection.close()


def _reserve_candidate_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def build_server_command(config: ServerConfig, port: int, key_file: Path) -> list[str]:
    """Return the fixed b10689 argv; benchmark tests compare it across backends."""
    return [
        str(config.executable),
        "--model",
        str(config.model_path),
        "--host",
        "127.0.0.1",
        "--port",
        str(port),
        "--api-key-file",
        str(key_file),
        "--offline",
        "--no-webui",
        "--parallel",
        "1",
        "--ctx-size",
        str(config.context_tokens),
        "--threads",
        str(config.cpu_threads),
        "--threads-batch",
        str(config.cpu_threads_batch or config.cpu_threads),
        "--threads-http",
        "2",
        "--batch-size",
        str(config.batch_tokens),
        "--ubatch-size",
        str(config.ubatch_tokens),
        "--cache-type-k",
        config.cache_type_k,
        "--cache-type-v",
        config.cache_type_v,
        "--flash-attn",
        config.flash_attention,
        "--gpu-layers",
        str(config.gpu_layers),
        "--jinja",
        "--reasoning-format",
        "none",
        "--log-disable",
    ]


def _write_api_key_file(api_key: str) -> Path:
    descriptor, raw_path = tempfile.mkstemp(prefix="npc-llama-key-", suffix=".txt")
    path = Path(raw_path)
    try:
        os.write(descriptor, api_key.encode("ascii") + b"\n")
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return path


def _minimal_runtime_environment(runtime_dir: Path) -> dict[str, str]:
    system_root = os.environ.get("SystemRoot", r"C:\Windows")
    environment = {
        "SystemRoot": system_root,
        "WINDIR": system_root,
        "PATH": os.pathsep.join((str(runtime_dir), str(Path(system_root) / "System32"))),
        "LLAMA_ARG_OFFLINE": "1",
    }
    for name in ("TEMP", "TMP"):
        value = os.environ.get(name)
        if value:
            environment[name] = value
    return environment
