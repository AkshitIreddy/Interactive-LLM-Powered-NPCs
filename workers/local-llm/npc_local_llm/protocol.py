"""Worker Control v1 adapter for the pinned local llama-server runtime."""

from __future__ import annotations

import json
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from .constants import (
    ALLOWED_OPERATIONS,
    ALLOWED_ROLES,
    MAX_CONTEXT_TOKENS,
    MAX_EVENTS_PER_REQUEST,
    MAX_FRAME_BYTES,
    MAX_MESSAGE_BYTES,
    MAX_MESSAGES,
    MAX_OUTPUT_TOKENS,
    MAX_RESPONSE_SCHEMA_BYTES,
    MAX_TEXT_BYTES,
    MODEL_ARTIFACT_SHA256,
    MODEL_ARTIFACT_SIZE,
    MODEL_ID,
    PROTOCOL_VERSION,
    RUNTIME_ABI,
)
from .digest import canonical_json_bytes, sha256_file
from .errors import LocalLlmError, invalid
from .manifest import ModelPack, RuntimeBundle
from .server import ChatCompletionRequest, CompletionTransport, LlamaServerSupervisor, ServerConfig

_ID_CHARS = frozenset("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-")


def _bounded_string(value: Any, name: str, maximum: int, *, allow_empty: bool = False) -> str:
    if not isinstance(value, str):
        raise invalid(f"{name} must be a string", field=name)
    size = len(value.encode("utf-8"))
    if (not allow_empty and size == 0) or size > maximum:
        raise invalid(f"{name} has an invalid length", field=name)
    return value


def _identifier(value: Any, name: str) -> str:
    result = _bounded_string(value, name, 128)
    if len(result) < 3 or any(character not in _ID_CHARS for character in result):
        raise invalid(f"{name} is invalid", field=name)
    return result


def _nonnegative_int(value: Any, name: str, maximum: int = 2**63 - 1) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value > maximum:
        raise invalid(f"{name} must be a bounded non-negative integer", field=name)
    return value


@dataclass(frozen=True, slots=True)
class Request:
    worker_instance_id: str
    request_id: str
    sequence: int
    generation: int
    deadline_unix_ms: int
    operation: str
    payload: dict[str, Any]

    @classmethod
    def parse(cls, value: Any) -> "Request":
        if not isinstance(value, dict):
            raise invalid("request root must be an object")
        required = {
            "protocol_version",
            "worker_instance_id",
            "request_id",
            "sequence",
            "generation",
            "deadline_unix_ms",
            "operation",
            "payload",
        }
        if set(value) != required:
            raise invalid("request envelope fields do not match Worker Control v1")
        if value["protocol_version"] != PROTOCOL_VERSION:
            raise LocalLlmError("unsupported_protocol", "worker protocol version is unsupported")
        operation = _bounded_string(value["operation"], "operation", 32)
        if operation not in ALLOWED_OPERATIONS:
            raise LocalLlmError("unsupported_operation", "worker operation is unsupported")
        payload = value["payload"]
        if not isinstance(payload, dict):
            raise invalid("payload must be an object", field="payload")
        result = cls(
            worker_instance_id=_identifier(value["worker_instance_id"], "worker_instance_id"),
            request_id=_identifier(value["request_id"], "request_id"),
            sequence=_nonnegative_int(value["sequence"], "sequence"),
            generation=_nonnegative_int(value["generation"], "generation"),
            deadline_unix_ms=_nonnegative_int(value["deadline_unix_ms"], "deadline_unix_ms"),
            operation=operation,
            payload=payload,
        )
        if result.sequence == 0:
            raise invalid("sequence must be positive", field="sequence")
        if result.deadline_unix_ms and int(time.time() * 1000) > result.deadline_unix_ms:
            raise LocalLlmError("deadline_exceeded", "worker request deadline has expired")
        return result


def parse_chat_request(payload: dict[str, Any]) -> ChatCompletionRequest:
    allowed = {"messages", "prompt", "max_tokens", "temperature", "top_p", "seed", "response_json_schema"}
    unknown = set(payload) - allowed
    if unknown:
        raise invalid("LLM request contains unsupported fields")
    raw_messages = payload.get("messages")
    if raw_messages is None:
        prompt = _bounded_string(payload.get("prompt"), "prompt", MAX_TEXT_BYTES)
        raw_messages = [{"role": "user", "content": prompt}]
    if not isinstance(raw_messages, list) or not 1 <= len(raw_messages) <= MAX_MESSAGES:
        raise invalid("messages has an invalid length", field="messages")
    messages: list[dict[str, str]] = []
    total_bytes = 0
    for index, raw_message in enumerate(raw_messages):
        if not isinstance(raw_message, dict) or set(raw_message) != {"role", "content"}:
            raise invalid("message fields are invalid", field=f"messages[{index}]")
        role = _bounded_string(raw_message["role"], "message.role", 16)
        if role not in ALLOWED_ROLES:
            raise invalid("message role is unsupported", field=f"messages[{index}].role")
        content = _bounded_string(raw_message["content"], "message.content", MAX_MESSAGE_BYTES)
        total_bytes += len(content.encode("utf-8"))
        if total_bytes > MAX_TEXT_BYTES:
            raise invalid("combined message content exceeds the request ceiling", field="messages")
        messages.append({"role": role, "content": content})
    max_tokens = payload.get("max_tokens", 512)
    if isinstance(max_tokens, bool) or not isinstance(max_tokens, int) or not 1 <= max_tokens <= MAX_OUTPUT_TOKENS:
        raise invalid("max_tokens is outside the approved range", field="max_tokens")
    temperature = payload.get("temperature", 0.7)
    top_p = payload.get("top_p", 0.95)
    for value, name, minimum, maximum in (
        (temperature, "temperature", 0.0, 2.0),
        (top_p, "top_p", 0.0, 1.0),
    ):
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not minimum <= float(value) <= maximum:
            raise invalid(f"{name} is outside the approved range", field=name)
    seed = payload.get("seed")
    if seed is not None:
        seed = _nonnegative_int(seed, "seed", 2**32 - 1)
    schema = payload.get("response_json_schema")
    if schema is not None:
        if not isinstance(schema, dict):
            raise invalid("response_json_schema must be an object", field="response_json_schema")
        encoded = canonical_json_bytes(schema)
        if len(encoded) > MAX_RESPONSE_SCHEMA_BYTES:
            raise invalid("response_json_schema exceeds the approved byte ceiling", field="response_json_schema")
        _validate_schema_shape(schema)
    return ChatCompletionRequest(
        messages=tuple(messages),
        max_tokens=max_tokens,
        temperature=float(temperature),
        top_p=float(top_p),
        seed=seed,
        response_json_schema=schema,
    )


def _validate_schema_shape(schema: dict[str, Any]) -> None:
    """Reject remote refs and pathological depth before llama.cpp grammar conversion."""

    nodes = 0

    def visit(value: Any, depth: int) -> None:
        nonlocal nodes
        nodes += 1
        if nodes > 2_048 or depth > 32:
            raise invalid("response_json_schema is too complex", field="response_json_schema")
        if isinstance(value, dict):
            for key, child in value.items():
                if key in {"$ref", "$dynamicRef"} and isinstance(child, str) and not child.startswith("#"):
                    raise invalid("remote JSON Schema references are not supported", field="response_json_schema")
                visit(child, depth + 1)
        elif isinstance(value, list):
            if len(value) > 256:
                raise invalid("response_json_schema array is too large", field="response_json_schema")
            for child in value:
                visit(child, depth + 1)
        elif not isinstance(value, (str, int, float, bool, type(None))):
            raise invalid("response_json_schema contains an unsupported value", field="response_json_schema")

    visit(schema, 0)


class WorkerController:
    def __init__(
        self,
        *,
        launch_nonce: str,
        worker_instance_id: str,
        model_pack: ModelPack,
        runtime_bundle: RuntimeBundle,
        emit: Callable[[dict[str, Any]], None],
        transport_factory: Callable[[ServerConfig], CompletionTransport] = LlamaServerSupervisor,
    ) -> None:
        self.launch_nonce = _bounded_string(launch_nonce, "launch_nonce", 512)
        self.worker_instance_id = _identifier(worker_instance_id, "worker_instance_id")
        self.model_pack = model_pack
        self.runtime_bundle = runtime_bundle
        self.emit = emit
        self.transport_factory = transport_factory
        self.lifecycle = "starting"
        self.generation = 0
        self.last_sequence = 0
        self.request_ids: set[str] = set()
        self.transport: CompletionTransport | None = None
        self.loaded_model_id: str | None = None
        self.loaded_lease_id: str | None = None
        self.jobs: dict[str, threading.Event] = {}
        self.event_indexes: dict[str, int] = {}
        self.lock = threading.RLock()
        self.counters = {"requests": 0, "inferences": 0, "completed": 0, "cancelled": 0, "errors": 0}

    def handle(self, raw: Any) -> None:
        request: Request | None = None
        try:
            request = Request.parse(raw)
            with self.lock:
                self._validate_envelope(request)
                self.counters["requests"] += 1
            getattr(self, f"_handle_{request.operation}")(request)
        except LocalLlmError as error:
            with self.lock:
                self.counters["errors"] += 1
            if request is not None:
                self._event(request, "error", terminal=True, error=error)

    def _validate_envelope(self, request: Request) -> None:
        if request.request_id in self.request_ids:
            raise LocalLlmError("duplicate_request", "request ID was already used")
        if request.sequence <= self.last_sequence:
            raise LocalLlmError("out_of_order_sequence", "request sequence did not increase")
        self.request_ids.add(request.request_id)
        if len(self.request_ids) > 8_192:
            raise LocalLlmError("request_history_exhausted", "worker request history ceiling was reached")
        self.last_sequence = request.sequence
        if self.lifecycle == "starting" and request.operation != "handshake":
            raise LocalLlmError("handshake_required", "handshake must be the first operation")
        if self.lifecycle != "starting" and request.worker_instance_id != self.worker_instance_id:
            raise LocalLlmError("instance_mismatch", "worker instance identity does not match")
        if request.operation == "cancel":
            if request.generation not in {self.generation, self.generation + 1}:
                raise LocalLlmError("generation_gap", "cancel may advance generation by exactly one")
        elif request.generation < self.generation:
            raise LocalLlmError("stale_generation", "request generation is stale")
        elif request.generation > self.generation:
            raise LocalLlmError("generation_gap", "only cancel may advance generation")

    def _event(
        self,
        request: Request,
        event: str,
        payload: dict[str, Any] | None = None,
        *,
        terminal: bool,
        error: LocalLlmError | None = None,
    ) -> None:
        with self.lock:
            index = self.event_indexes.get(request.request_id, 0)
            if index >= MAX_EVENTS_PER_REQUEST:
                return
            self.event_indexes[request.request_id] = index + 1
        envelope: dict[str, Any] = {
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": self.worker_instance_id,
            "request_id": request.request_id,
            "sequence": request.sequence,
            "generation": request.generation,
            "event_index": index,
            "event": event,
            "terminal": terminal,
            "payload": payload or {},
        }
        if error is not None:
            envelope["error"] = error.event_error()
        self.emit(envelope)

    def _handle_handshake(self, request: Request) -> None:
        if request.payload.get("launch_nonce") != self.launch_nonce:
            raise LocalLlmError("authentication_failed", "worker launch nonce is invalid")
        with self.lock:
            self.lifecycle = "cold"
        self._event(request, "completed", self._capabilities(), terminal=True)

    def _handle_capabilities(self, request: Request) -> None:
        self._event(request, "completed", self._capabilities(), terminal=True)

    def _capabilities(self) -> dict[str, Any]:
        return {
            "worker_id": self.worker_instance_id,
            "pack_id": MODEL_ID,
            "worker_kind": "llm",
            "engine": "llama.cpp",
            "runtime_abi": RUNTIME_ABI,
            "operations": sorted(ALLOWED_OPERATIONS),
            "input_modalities": ["text", "chat_messages"],
            "output_modalities": ["text", "tokens", "usage", "structured_json"],
            "compute_backends": ["cpu", "vulkan"],
            "streaming_output": True,
            "structured_output": True,
            "cancellation": True,
            "network_access": False,
            "development_stub": False,
            "api_first_default": True,
            "limits": {
                "max_frame_bytes": MAX_FRAME_BYTES,
                "max_text_bytes": MAX_TEXT_BYTES,
                "max_messages": MAX_MESSAGES,
                "max_output_tokens": MAX_OUTPUT_TOKENS,
                "max_context_tokens": MAX_CONTEXT_TOKENS,
            },
        }

    def _handle_health(self, request: Request) -> None:
        runtime = None
        with self.lock:
            transport = self.transport
            payload = {
                "status": "ok",
                "lifecycle": self.lifecycle,
                "generation": self.generation,
                "loaded_model_id": self.loaded_model_id,
                "loaded_lease_id": self.loaded_lease_id,
                "in_flight": len(self.jobs),
                "counters": dict(self.counters),
            }
        if transport is not None:
            runtime = transport.health()
        payload["runtime"] = runtime
        self._event(request, "completed", payload, terminal=True)

    def _handle_warm(self, request: Request) -> None:
        with self.lock:
            if self.lifecycle == "cold":
                self.lifecycle = "warm"
            lifecycle = self.lifecycle
        self._event(request, "completed", {"lifecycle": lifecycle}, terminal=True)

    def _handle_load(self, request: Request) -> None:
        payload = request.payload
        model_id = _identifier(payload.get("model_id"), "model_id")
        lease_id = _identifier(payload.get("lease_id"), "lease_id")
        if model_id != MODEL_ID or payload.get("verified_model_sha256") != MODEL_ARTIFACT_SHA256:
            raise LocalLlmError("invalid_model_lease", "model lease identity is not approved")
        model_path = Path(_bounded_string(payload.get("verified_model_path"), "verified_model_path", 2_048))
        executable = Path(_bounded_string(payload.get("runtime_executable_path"), "runtime_executable_path", 2_048))
        if not model_path.is_absolute() or not executable.is_absolute():
            raise LocalLlmError("invalid_model_lease", "model and runtime paths must be absolute")
        actual = sha256_file(model_path, expected_size=MODEL_ARTIFACT_SIZE)
        if actual != MODEL_ARTIFACT_SHA256:
            raise LocalLlmError("invalid_model_lease", "model artifact failed defense-in-depth verification")
        backend = _bounded_string(payload.get("runtime_backend"), "runtime_backend", 32)
        variant = self.runtime_bundle.variant(f"windows-x64-{backend}")
        if payload.get("runtime_abi") != RUNTIME_ABI or variant.backend != backend:
            raise LocalLlmError("runtime_abi_mismatch", "runtime lease is not approved")
        with self.lock:
            if self.jobs and (self.loaded_model_id != model_id or self.loaded_lease_id != lease_id):
                raise LocalLlmError("worker_busy", "cancel or drain inference before replacing a model", retryable=True)
            if self.loaded_model_id == model_id and self.loaded_lease_id == lease_id and self.transport is not None:
                self._event(request, "completed", {"lifecycle": "loaded", "already_loaded": True}, terminal=True)
                return
        config = ServerConfig(
            executable=executable,
            model_path=model_path,
            runtime_abi=RUNTIME_ABI,
            backend=backend,
            context_tokens=_optional_bounded_int(payload, "context_tokens", 8_192, 512, MAX_CONTEXT_TOKENS),
            cpu_threads=_optional_bounded_int(payload, "cpu_threads", 4, 1, 128),
            gpu_layers=_optional_bounded_int(payload, "gpu_layers", 0, 0, 256),
            process_memory_limit_bytes=_optional_bounded_int(
                payload, "process_memory_limit_bytes", None, 512 * 1_048_576, 128 * 1_073_741_824
            ),
        )
        transport = self.transport_factory(config)
        if not hasattr(transport, "start"):
            raise LocalLlmError("runtime_start_failed", "runtime transport is missing its start operation")
        getattr(transport, "start")()
        with self.lock:
            old = self.transport
            self.transport = transport
            self.loaded_model_id = model_id
            self.loaded_lease_id = lease_id
            self.lifecycle = "loaded"
        if old is not None and hasattr(old, "stop"):
            getattr(old, "stop")()
        self._event(request, "completed", {"lifecycle": "loaded", "already_loaded": False}, terminal=True)

    def _handle_infer(self, request: Request) -> None:
        chat = parse_chat_request(request.payload)
        with self.lock:
            if self.lifecycle != "loaded" or self.transport is None:
                raise LocalLlmError("model_not_loaded", "local LLM model is not loaded")
            if self.jobs:
                raise LocalLlmError("worker_busy", "local LLM supports one inference at a time", retryable=True)
            cancelled = threading.Event()
            self.jobs[request.request_id] = cancelled
            self.counters["inferences"] += 1
        self._event(request, "accepted", {"model_id": MODEL_ID}, terminal=False)
        threading.Thread(
            target=self._run_inference,
            args=(request, chat, cancelled),
            name=f"local-llm-{request.request_id}",
            daemon=True,
        ).start()

    def _run_inference(self, request: Request, chat: ChatCompletionRequest, cancelled: threading.Event) -> None:
        fragments: list[str] = []
        usage: dict[str, int] = {}
        finish_reason = "stop"
        try:
            assert self.transport is not None
            for delta in self.transport.stream_chat(chat, cancelled.is_set):
                if cancelled.is_set() or request.generation != self.generation:
                    return
                if delta.text:
                    fragments.append(delta.text)
                    if sum(len(item.encode("utf-8")) for item in fragments) > MAX_TEXT_BYTES:
                        raise LocalLlmError("output_too_large", "local LLM output exceeded the text ceiling")
                    self._event(
                        request,
                        "token",
                        {
                            "text": delta.text,
                            "content_kind": "structured_json_fragment" if chat.response_json_schema else "plain_text",
                        },
                        terminal=False,
                    )
                if delta.finish_reason and delta.finish_reason != "done":
                    finish_reason = delta.finish_reason
                if delta.usage:
                    usage.update(delta.usage)
            text = "".join(fragments)
            structured = None
            if chat.response_json_schema is not None:
                try:
                    structured = json.loads(text)
                except json.JSONDecodeError as error:
                    raise LocalLlmError("structured_output_invalid", "local LLM structured output is invalid") from error
            with self.lock:
                self.counters["completed"] += 1
            self._event(
                request,
                "llm_result",
                {
                    "model_id": MODEL_ID,
                    "text": text,
                    "structured_response": structured,
                    "finish_reason": finish_reason,
                    "usage": usage,
                },
                terminal=False,
            )
            self._event(request, "completed", {"model_id": MODEL_ID}, terminal=True)
        except LocalLlmError as error:
            if error.code == "cancelled" or cancelled.is_set():
                return
            self._event(request, "error", terminal=True, error=error)
        finally:
            with self.lock:
                self.jobs.pop(request.request_id, None)

    def _handle_cancel(self, request: Request) -> None:
        with self.lock:
            if request.generation == self.generation + 1:
                self.generation = request.generation
            jobs = list(self.jobs.values())
            for job in jobs:
                job.set()
            self.counters["cancelled"] += len(jobs)
            transport = self.transport
        drained = True if transport is None else transport.cancel_active()
        if not drained:
            with self.lock:
                self.transport = None
                self.loaded_model_id = None
                self.loaded_lease_id = None
                self.lifecycle = "warm"
        self._event(
            request,
            "completed",
            {"generation": self.generation, "cancelled_requests": len(jobs), "runtime_preserved": drained},
            terminal=True,
        )

    def _handle_unload(self, request: Request) -> None:
        with self.lock:
            for job in self.jobs.values():
                job.set()
            transport = self.transport
            self.transport = None
            self.loaded_model_id = None
            self.loaded_lease_id = None
            self.lifecycle = "warm"
        if transport is not None:
            transport.cancel_active()
            if hasattr(transport, "stop"):
                getattr(transport, "stop")()
        self._event(request, "completed", {"lifecycle": "warm"}, terminal=True)

    def _handle_shutdown(self, request: Request) -> None:
        with self.lock:
            for job in self.jobs.values():
                job.set()
            transport = self.transport
            self.transport = None
            self.loaded_model_id = None
            self.loaded_lease_id = None
            self.lifecycle = "stopped"
        if transport is not None:
            transport.cancel_active()
            if hasattr(transport, "stop"):
                getattr(transport, "stop")()
        self._event(request, "completed", {"lifecycle": "stopped"}, terminal=True)


def _optional_bounded_int(
    payload: dict[str, Any],
    name: str,
    default: int | None,
    minimum: int,
    maximum: int,
) -> int | None:
    value = payload.get(name, default)
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise invalid(f"{name} is outside the approved range", field=name)
    return value
