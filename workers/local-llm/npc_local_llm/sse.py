"""Bounded OpenAI-compatible SSE decoder for llama-server."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any, Iterable

from .constants import MAX_SSE_EVENT_BYTES, MAX_TEXT_BYTES
from .errors import LocalLlmError


@dataclass(frozen=True, slots=True)
class CompletionDelta:
    text: str = ""
    finish_reason: str | None = None
    usage: dict[str, int] | None = None


class SseDecoder:
    def __init__(self) -> None:
        self._buffer = bytearray()

    def feed(self, chunk: bytes) -> list[bytes]:
        self._buffer.extend(chunk)
        if len(self._buffer) > MAX_SSE_EVENT_BYTES:
            raise LocalLlmError("oversized_sse_event", "llama-server emitted an oversized SSE event")
        events: list[bytes] = []
        while True:
            marker = self._buffer.find(b"\n\n")
            windows_marker = self._buffer.find(b"\r\n\r\n")
            if windows_marker >= 0 and (marker < 0 or windows_marker < marker):
                marker = windows_marker
                delimiter = 4
            else:
                delimiter = 2
            if marker < 0:
                break
            events.append(bytes(self._buffer[:marker]))
            del self._buffer[: marker + delimiter]
        return events

    def finish(self) -> None:
        if self._buffer.strip():
            raise LocalLlmError("truncated_sse", "llama-server SSE stream ended mid-event")


def parse_openai_event(raw: bytes) -> CompletionDelta | None:
    data_lines: list[bytes] = []
    for line in raw.replace(b"\r\n", b"\n").split(b"\n"):
        if line.startswith(b"data:"):
            data_lines.append(line[5:].lstrip())
    if not data_lines:
        return None
    payload = b"\n".join(data_lines)
    if payload == b"[DONE]":
        return CompletionDelta(finish_reason="done")
    try:
        value = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise LocalLlmError("invalid_sse_json", "llama-server emitted invalid SSE JSON") from error
    if not isinstance(value, dict):
        raise LocalLlmError("invalid_sse_json", "llama-server emitted an invalid SSE object")
    if "error" in value:
        raise LocalLlmError("inference_failed", "llama-server rejected the generation request", retryable=True)
    choices = value.get("choices", [])
    text = ""
    finish_reason = None
    if choices:
        if not isinstance(choices, list) or not isinstance(choices[0], dict):
            raise LocalLlmError("invalid_sse_json", "llama-server emitted invalid choices")
        choice = choices[0]
        delta = choice.get("delta", {})
        if isinstance(delta, dict):
            content = delta.get("content")
            if content is not None:
                if not isinstance(content, str) or len(content.encode("utf-8")) > MAX_TEXT_BYTES:
                    raise LocalLlmError("invalid_sse_json", "llama-server emitted invalid text content")
                text = content
        finish = choice.get("finish_reason")
        if finish is not None:
            if not isinstance(finish, str) or len(finish) > 64:
                raise LocalLlmError("invalid_sse_json", "llama-server emitted an invalid finish reason")
            finish_reason = finish
    usage = _parse_usage(value.get("usage"))
    return CompletionDelta(text=text, finish_reason=finish_reason, usage=usage)


def _parse_usage(value: Any) -> dict[str, int] | None:
    if value is None:
        return None
    if not isinstance(value, dict):
        raise LocalLlmError("invalid_sse_json", "llama-server emitted invalid usage metadata")
    result: dict[str, int] = {}
    for source, destination in (
        ("prompt_tokens", "prompt_tokens"),
        ("completion_tokens", "completion_tokens"),
        ("total_tokens", "total_tokens"),
    ):
        item = value.get(source)
        if item is not None:
            if isinstance(item, bool) or not isinstance(item, int) or item < 0:
                raise LocalLlmError("invalid_sse_json", "llama-server emitted invalid usage metadata")
            result[destination] = item
    return result or None


def decode_chunks(chunks: Iterable[bytes]) -> list[CompletionDelta]:
    decoder = SseDecoder()
    output: list[CompletionDelta] = []
    for chunk in chunks:
        for event in decoder.feed(chunk):
            parsed = parse_openai_event(event)
            if parsed is not None:
                output.append(parsed)
    decoder.finish()
    return output
