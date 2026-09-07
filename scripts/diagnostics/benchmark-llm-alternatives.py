#!/usr/bin/env python3
"""Bounded, secret-safe streaming qualification for hosted NPC LLM routes.

The script reads provider credentials from the user-supplied key file, keeps all
model text in memory, and writes only timing, shape, usage, and boolean semantic
checks. Each run requires a new output directory and refuses to overwrite it.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import statistics
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

import httpx


MAX_PROVIDER_EVENT_BYTES = 256 * 1024
MAX_PROVIDER_STREAM_BYTES = 2 * 1024 * 1024
MAX_RESPONSE_TEXT_BYTES = 16 * 1024

SCHEMA: dict[str, Any] = {
    "type": "object",
    "additionalProperties": False,
    "required": ["schema_version", "spoken_response"],
    "properties": {
        # A one-value enum is semantically equivalent to const here and is part
        # of Gemini's documented structured-output JSON Schema subset.
        "schema_version": {"type": "string", "enum": ["npc_response.v1"]},
        "spoken_response": {
            "type": "object",
            "additionalProperties": False,
            "required": ["text"],
            # Keep the provider-facing schema to the portable intersection.
            # Cohere explicitly rejects minLength/maxLength; the runtime still
            # enforces non-empty text and its byte cap after generation.
            "properties": {"text": {"type": "string"}},
        },
    },
}

SYSTEM = (
    "You are Mara Vale, the harbor watch officer at North Beacon. Your duty is to warn "
    "the player about unsafe tide gates. Stay in character, never mention being an AI or "
    "a model, and always include the name Mara Vale in the spoken sentence. Return only "
    "JSON matching the supplied schema. Keep spoken_response.text to one short, natural "
    "sentence for speech synthesis. Do not add stage directions, Markdown, or actions."
)

PROMPTS = (
    ("identity", "Introduce yourself and tell the player your duty at North Beacon."),
    ("purpose", "The player approaches the unstable north tide gate. Warn them why it is unsafe."),
)

PROVIDER_SOURCES = {
    "groq": (
        "https://console.groq.com/docs/rate-limits",
        "https://console.groq.com/docs/deprecations",
    ),
    "mistral": (
        "https://docs.mistral.ai/models",
        "https://docs.mistral.ai/studio/conversations/structured-output",
    ),
    "gemini": (
        "https://ai.google.dev/gemini-api/docs/models",
        "https://ai.google.dev/gemini-api/docs/pricing",
    ),
    "openrouter": (
        "https://openrouter.ai/docs/faq",
        "https://openrouter.ai/docs/guides/routing/model-variants/free",
    ),
    "cloudflare": (
        "https://developers.cloudflare.com/workers-ai/platform/pricing/",
        "https://developers.cloudflare.com/workers-ai/models/",
    ),
}


@dataclass(frozen=True)
class Target:
    provider: str
    model: str
    endpoint: str
    protocol: str
    key_label: str
    samples: int


TARGETS = (
    Target("groq", "openai/gpt-oss-20b", "https://api.groq.com/openai/v1/chat/completions", "chat", "groq", 2),
    Target("groq", "qwen/qwen3.6-27b", "https://api.groq.com/openai/v1/chat/completions", "chat", "groq", 2),
    Target("mistral", "ministral-3b-2512", "https://api.mistral.ai/v1/chat/completions", "chat", "mistral", 2),
    Target("mistral", "ministral-8b-2512", "https://api.mistral.ai/v1/chat/completions", "chat", "mistral", 2),
    Target("gemini", "gemini-3.1-flash-lite", "https://generativelanguage.googleapis.com/v1beta", "gemini", "googlegemini", 2),
    Target("gemini", "gemini-3.5-flash-lite", "https://generativelanguage.googleapis.com/v1beta", "gemini", "googlegemini", 2),
    Target("openrouter", "liquid/lfm-2.5-2.6b:free", "https://openrouter.ai/api/v1/chat/completions", "chat", "openrouter", 1),
    Target("cloudflare", "@cf/meta/llama-3.1-8b-instruct-fast", "https://api.cloudflare.com/client/v4/accounts", "cloudflare", "cloudfare_worker_api", 1),
    Target("cohere", "command-a-plus-05-2026", "https://api.cohere.com/v2/chat", "cohere", "cohere", 1),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--keys-file", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--providers", nargs="*", default=[])
    parser.add_argument("--models", nargs="*", default=[])
    parser.add_argument("--timeout-seconds", type=float, default=45.0)
    parser.add_argument("--sample-limit", type=int, default=None)
    return parser.parse_args()


def read_keys(path: Path) -> dict[str, str]:
    wanted = {target.key_label for target in TARGETS} | {"cloudfare_worker_account_id"}
    keys: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8", errors="strict").splitlines():
        line = raw.strip()
        if not line or line.startswith(("#", ";", "//")):
            continue
        positions = [pos for pos in (line.find("="), line.find(":")) if pos >= 0]
        if not positions:
            continue
        split = min(positions)
        label = line[:split].strip().strip("\"'`").lower().replace(" ", "_").replace("-", "_")
        if label not in wanted:
            continue
        value = line[split + 1 :].strip().strip("\"'")
        if len(value) >= 8 and not any(ord(char) < 32 for char in value):
            keys[label] = value
    return keys


def chat_body(target: Target, prompt: str) -> dict[str, Any]:
    body: dict[str, Any] = {
        "model": target.model,
        "stream": True,
        "max_tokens": 160,
        "temperature": 0,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}],
    }
    if target.provider == "mistral":
        body["response_format"] = {
            "type": "json_schema",
            "json_schema": {"name": "npc_response", "strict": True, "schema": SCHEMA},
        }
    else:
        body["response_format"] = {
            "type": "json_schema",
            "json_schema": {"name": "npc_response", "strict": True, "schema": SCHEMA},
        }
        body["stream_options"] = {"include_usage": True}
    if target.provider == "groq" and target.model.startswith("qwen/"):
        # Groq's official Qwen guidance requires non-thinking mode for
        # low-latency dialogue. Without this, a 160-token NPC budget can be
        # exhausted entirely by reasoning deltas before any speakable content.
        body["reasoning_effort"] = "none"
        body["reasoning_format"] = "hidden"
    if target.provider == "openrouter":
        # The explicit :free model plus parameter gating prevents a paid or
        # schema-ignoring fallback during this single quota-preserving probe.
        body["provider"] = {"allow_fallbacks": False, "require_parameters": True}
    return body


def cohere_body(target: Target, prompt: str) -> dict[str, Any]:
    return {
        "model": target.model,
        "stream": True,
        "max_tokens": 160,
        "temperature": 0,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}],
        "response_format": {"type": "json_object", "schema": SCHEMA},
        "thinking": {"type": "disabled"},
    }


def gemini_body(prompt: str) -> dict[str, Any]:
    # This intentionally mirrors the repository adapter, including store=false,
    # so the receipt qualifies the integrated wire shape rather than a friendlier
    # one-off request.
    return {
        "contents": [{"role": "user", "parts": [{"text": prompt}]}],
        "systemInstruction": {"parts": [{"text": SYSTEM}]},
        "generationConfig": {
            "maxOutputTokens": 160,
            "temperature": 0,
            "responseMimeType": "application/json",
            "responseJsonSchema": SCHEMA,
        },
        "store": False,
    }


def iter_sse(response: httpx.Response) -> Iterable[str]:
    data: list[str] = []
    for line in response.iter_lines():
        if line == "":
            if data:
                yield "\n".join(data)
                data.clear()
            continue
        if line.startswith("data:"):
            data.append(line[5:].lstrip())
    if data:
        yield "\n".join(data)


def delta_from_event(protocol: str, event: dict[str, Any]) -> str:
    if protocol == "cloudflare":
        return event.get("response", "") if isinstance(event.get("response"), str) else ""
    if protocol == "gemini":
        parts = (((event.get("candidates") or [{}])[0].get("content") or {}).get("parts") or [])
        return "".join(part.get("text", "") for part in parts if isinstance(part, dict))
    if protocol == "cohere":
        if event.get("type") != "content-delta":
            return ""
        return ((((event.get("delta") or {}).get("message") or {}).get("content") or {}).get("text") or "")
    choices = event.get("choices") or []
    if not choices:
        return ""
    delta = choices[0].get("delta") or {}
    content = delta.get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "".join(item.get("text", "") for item in content if isinstance(item, dict))
    return ""


def usage_from_event(protocol: str, event: dict[str, Any]) -> dict[str, int] | None:
    if protocol == "gemini":
        raw = event.get("usageMetadata")
        if isinstance(raw, dict):
            return {
                "input_tokens": int(raw.get("promptTokenCount") or 0),
                "output_tokens": int(raw.get("candidatesTokenCount") or raw.get("thoughtsTokenCount") or 0),
                "total_tokens": int(raw.get("totalTokenCount") or 0),
            }
        return None
    if protocol == "cohere":
        raw = ((event.get("delta") or {}).get("usage") or event.get("usage"))
        if not isinstance(raw, dict):
            return None
        billed = raw.get("billed_units") or raw
        return {
            "input_tokens": int(billed.get("input_tokens") or 0),
            "output_tokens": int(billed.get("output_tokens") or 0),
            "total_tokens": int(billed.get("input_tokens") or 0) + int(billed.get("output_tokens") or 0),
        }
    raw = event.get("usage")
    if isinstance(raw, dict):
        return {
            "input_tokens": int(raw.get("prompt_tokens") or 0),
            "output_tokens": int(raw.get("completion_tokens") or 0),
            "total_tokens": int(raw.get("total_tokens") or 0),
        }
    return None


def partial_spoken_text(raw: str) -> str:
    match = re.search(r'"spoken_response"\s*:\s*\{.*?"text"\s*:\s*"', raw, re.S)
    if not match:
        return ""
    chars: list[str] = []
    escaped = False
    unicode_digits = ""
    for char in raw[match.end() :]:
        if unicode_digits:
            unicode_digits += char
            if len(unicode_digits) == 5:
                try:
                    chars.append(chr(int(unicode_digits[1:], 16)))
                except ValueError:
                    return "".join(chars)
                unicode_digits = ""
            continue
        if escaped:
            escaped = False
            if char == "u":
                unicode_digits = "u"
            else:
                chars.append({"n": "\n", "r": "\r", "t": "\t"}.get(char, char))
            continue
        if char == "\\":
            escaped = True
        elif char == '"':
            break
        else:
            chars.append(char)
    return "".join(chars)


def has_usable_clause(text: str) -> bool:
    return len(text.strip()) >= 12 and re.search(r"[.!?;](?:\s|$)", text) is not None


def validate_envelope(raw: str, prompt_id: str) -> dict[str, Any]:
    result = {
        "json_valid": False,
        "strict_runtime_shape": False,
        "identity_anchor_present": False,
        "purpose_anchor_present": False,
        "one_or_two_sentences": False,
        "no_model_meta": False,
        "no_stage_directions_or_markdown": False,
        "spoken_bytes": 0,
        "top_level_field_count": 0,
        "top_level_unknown_field_count": 0,
        "has_schema_version_field": False,
        "has_spoken_response_field": False,
        "schema_version_exact": False,
        "spoken_response_is_object": False,
        "spoken_response_field_count": 0,
        "spoken_response_has_text_field": False,
        "response_sha256": hashlib.sha256(raw.encode("utf-8")).hexdigest(),
    }
    try:
        value = json.loads(raw)
    except (json.JSONDecodeError, UnicodeError):
        return result
    result["json_valid"] = True
    if not isinstance(value, dict):
        return result
    result["top_level_field_count"] = len(value)
    result["top_level_unknown_field_count"] = len(set(value) - {"schema_version", "spoken_response"})
    result["has_schema_version_field"] = "schema_version" in value
    result["has_spoken_response_field"] = "spoken_response" in value
    result["schema_version_exact"] = value.get("schema_version") == "npc_response.v1"
    if set(value) != {"schema_version", "spoken_response"}:
        return result
    spoken = value.get("spoken_response")
    result["spoken_response_is_object"] = isinstance(spoken, dict)
    if isinstance(spoken, dict):
        result["spoken_response_field_count"] = len(spoken)
        result["spoken_response_has_text_field"] = "text" in spoken
    if value.get("schema_version") != "npc_response.v1" or not isinstance(spoken, dict) or set(spoken) != {"text"}:
        return result
    text = spoken.get("text")
    if not isinstance(text, str) or not text.strip() or len(text.encode("utf-8")) > 16384:
        return result
    if any(ord(char) < 32 and char not in "\n\r\t" for char in text):
        return result
    result["strict_runtime_shape"] = True
    lower = text.lower()
    result["spoken_bytes"] = len(text.encode("utf-8"))
    result["identity_anchor_present"] = "mara vale" in lower
    purpose_terms = ("watch", "officer", "duty", "warn", "guard") if prompt_id == "identity" else ("tide", "gate", "unsafe", "unstable", "danger")
    result["purpose_anchor_present"] = any(term in lower for term in purpose_terms)
    result["one_or_two_sentences"] = 1 <= len(re.findall(r"[.!?](?:\s|$)", text.strip())) <= 2
    result["no_model_meta"] = not any(term in lower for term in ("language model", "as an ai", "openai", "google", "gemini", "groq", "mistral", "cohere"))
    result["no_stage_directions_or_markdown"] = not any(char in text for char in ("*", "[", "]", "#", "`"))
    return result


def sanitized_rate_headers(headers: httpx.Headers) -> dict[str, str]:
    allowed = (
        "x-ratelimit-limit-requests", "x-ratelimit-remaining-requests", "x-ratelimit-reset-requests",
        "x-ratelimit-limit-tokens", "x-ratelimit-remaining-tokens", "x-ratelimit-reset-tokens",
        "ratelimit-limit", "ratelimit-remaining", "ratelimit-reset",
    )
    return {name: headers[name][:96] for name in allowed if name in headers}


def execute(
    client: httpx.Client,
    target: Target,
    key: str,
    prompt_id: str,
    prompt: str,
    cloudflare_account_id: str | None = None,
) -> dict[str, Any]:
    started = time.perf_counter()
    first_delta_ms: int | None = None
    first_clause_ms: int | None = None
    headers_ms: int | None = None
    text = ""
    usage: dict[str, int] | None = None
    event_count = 0
    stream_bytes = 0
    status: int | None = None
    rate_headers: dict[str, str] = {}
    error: str | None = None
    if target.protocol == "cloudflare":
        if cloudflare_account_id is None:
            raise ValueError("Cloudflare account ID is required")
        url = f"{target.endpoint}/{cloudflare_account_id}/ai/run/{target.model}"
        headers = {"authorization": f"Bearer {key}", "content-type": "application/json"}
        # Cloudflare's official JSON mode does not support streaming. This one
        # quota-preserving compatibility call therefore streams a prompt-enforced
        # envelope and checks it locally after completion.
        body = {
            "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}],
            "stream": True,
            "max_tokens": 160,
            "temperature": 0,
        }
    elif target.protocol == "gemini":
        url = f"{target.endpoint}/models/{target.model}:streamGenerateContent?alt=sse"
        headers = {"x-goog-api-key": key, "content-type": "application/json"}
        body = gemini_body(prompt)
    else:
        url = target.endpoint
        headers = {"authorization": f"Bearer {key}", "content-type": "application/json"}
        if target.protocol == "cohere":
            headers["x-client-name"] = "interactive-npcs-diagnostic"
            body = cohere_body(target, prompt)
        else:
            body = chat_body(target, prompt)
    try:
        with client.stream("POST", url, headers=headers, json=body) as response:
            headers_ms = round((time.perf_counter() - started) * 1000)
            status = response.status_code
            rate_headers = sanitized_rate_headers(response.headers)
            if status != 200:
                error = f"http_{status}"
            else:
                for data in iter_sse(response):
                    if data == "[DONE]":
                        continue
                    event_bytes = len(data.encode("utf-8"))
                    stream_bytes += event_bytes
                    if event_bytes > MAX_PROVIDER_EVENT_BYTES or stream_bytes > MAX_PROVIDER_STREAM_BYTES:
                        error = "provider_stream_too_large"
                        break
                    try:
                        event = json.loads(data)
                    except json.JSONDecodeError:
                        error = "malformed_sse_json"
                        continue
                    event_count += 1
                    candidate_usage = usage_from_event(target.protocol, event)
                    if candidate_usage:
                        usage = candidate_usage
                    delta = delta_from_event(target.protocol, event)
                    if not delta:
                        continue
                    if len(text.encode("utf-8")) + len(delta.encode("utf-8")) > MAX_RESPONSE_TEXT_BYTES:
                        error = "response_text_too_large"
                        break
                    now_ms = round((time.perf_counter() - started) * 1000)
                    if first_delta_ms is None:
                        first_delta_ms = now_ms
                    text += delta
                    if first_clause_ms is None and has_usable_clause(partial_spoken_text(text)):
                        first_clause_ms = now_ms
    except (httpx.TimeoutException, httpx.NetworkError, httpx.ProtocolError) as exc:
        error = type(exc).__name__
    total_ms = round((time.perf_counter() - started) * 1000)
    validation = validate_envelope(text, prompt_id)
    return {
        "provider": target.provider,
        "model": target.model,
        "prompt": prompt_id,
        "http_status": status,
        "error_category": error,
        "response_headers_ms": headers_ms,
        "first_text_delta_ms": first_delta_ms,
        "first_heuristic_spoken_clause_ms": first_clause_ms,
        # StructuredV1 currently withholds all ready sentences until finish.
        "current_runtime_safe_spoken_ms": total_ms if validation["strict_runtime_shape"] else None,
        "complete_ms": total_ms,
        "sse_event_count": event_count,
        "usage": usage,
        "rate_limit_headers": rate_headers,
        "provider_tuning": (
            "groq_qwen_non_thinking"
            if target.provider == "groq" and target.model.startswith("qwen/")
            else "cohere_non_thinking"
            if target.provider == "cohere"
            else "none"
        ),
        "validation": validation,
    }


def median(values: Iterable[int | None]) -> float | None:
    kept = [value for value in values if value is not None]
    return round(statistics.median(kept), 1) if kept else None


def main() -> int:
    args = parse_args()
    known_providers = {target.provider for target in TARGETS}
    known_models = {target.model for target in TARGETS}
    unknown_providers = sorted(set(args.providers) - known_providers)
    unknown_models = sorted(set(args.models) - known_models)
    if unknown_providers or unknown_models:
        raise SystemExit(
            f"unknown providers={unknown_providers} models={unknown_models}"
        )
    if not 0 < args.timeout_seconds <= 120:
        raise SystemExit("timeout-seconds must be greater than zero and at most 120")
    if args.sample_limit is not None and not 1 <= args.sample_limit <= 2:
        raise SystemExit("sample-limit must be 1 or 2")
    if not args.keys_file.is_file():
        raise SystemExit("keys file is unavailable")
    if args.output_dir.exists():
        raise SystemExit("output directory already exists; choose a new immutable run path")
    selected = [
        target
        for target in TARGETS
        if (not args.providers or target.provider in set(args.providers))
        and (not args.models or target.model in set(args.models))
    ]
    if not selected:
        raise SystemExit("provider/model filters selected no routes")
    args.output_dir.mkdir(parents=True)
    keys = read_keys(args.keys_file)
    receipts: list[dict[str, Any]] = []
    availability = {target.key_label: target.key_label in keys for target in selected}
    if any(target.provider == "cloudflare" for target in selected):
        availability["cloudfare_worker_account_id"] = "cloudfare_worker_account_id" in keys
    for target in selected:
        key = keys.get(target.key_label)
        if key is None:
            receipts.append({"provider": target.provider, "model": target.model, "skipped": "credential_label_unavailable"})
            continue
        if target.provider == "cloudflare" and "cloudfare_worker_account_id" not in keys:
            receipts.append({"provider": target.provider, "model": target.model, "skipped": "account_id_label_unavailable"})
            continue
        limits = httpx.Limits(max_connections=1, max_keepalive_connections=1, keepalive_expiry=30.0)
        timeout = httpx.Timeout(args.timeout_seconds, connect=min(args.timeout_seconds, 15.0))
        # Keep the diagnostic dependency-free: the workspace Python does not
        # necessarily include httpx's optional h2 package. HTTP/1.1 still lets
        # sample 2 prove persistent-connection reuse on these SSE endpoints.
        with httpx.Client(http2=False, limits=limits, timeout=timeout) as client:
            sample_count = min(target.samples, args.sample_limit) if args.sample_limit is not None else target.samples
            for sample in range(sample_count):
                prompt_id, prompt = PROMPTS[sample % len(PROMPTS)]
                receipt = execute(
                    client,
                    target,
                    key,
                    prompt_id,
                    prompt,
                    keys.get("cloudfare_worker_account_id"),
                )
                receipt["sample"] = sample + 1
                receipt["connection_condition"] = "cold_client" if sample == 0 else "reused_client"
                receipts.append(receipt)
                print(
                    f"{target.provider}/{target.model} sample={sample + 1} "
                    f"status={receipt.get('http_status')} headers_ms={receipt.get('response_headers_ms')} "
                    f"delta_ms={receipt.get('first_text_delta_ms')} complete_ms={receipt.get('complete_ms')} "
                    f"strict={receipt.get('validation', {}).get('strict_runtime_shape', False)}",
                    flush=True,
                )
    groups: list[dict[str, Any]] = []
    for target in selected:
        rows = [row for row in receipts if row.get("provider") == target.provider and row.get("model") == target.model and "complete_ms" in row]
        groups.append({
            "provider": target.provider,
            "model": target.model,
            "samples": len(rows),
            "successful_http": sum(row.get("http_status") == 200 for row in rows),
            "strict_runtime_shape": sum(row.get("validation", {}).get("strict_runtime_shape", False) for row in rows),
            "identity_and_purpose": sum(
                row.get("validation", {}).get("identity_anchor_present", False)
                and row.get("validation", {}).get("purpose_anchor_present", False)
                for row in rows
            ),
            "median_headers_ms": median(row.get("response_headers_ms") for row in rows),
            "median_first_delta_ms": median(row.get("first_text_delta_ms") for row in rows),
            "median_first_heuristic_clause_ms": median(row.get("first_heuristic_spoken_clause_ms") for row in rows),
            "median_complete_ms": median(row.get("complete_ms") for row in rows),
        })
    document = {
        "schema_version": 1,
        "receipt_type": "sanitized_hosted_llm_alternatives_streaming_qualification",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "raw_provider_text_persisted": False,
        "credentials_persisted": False,
        "scope": "tiny synthetic NPC structured-output generations; no game, GUI, audio, or old-code execution",
        "method": {
            "connection": "one persistent HTTP/1.1 client per model; sample 1 cold client and sample 2 reuses it",
            "response_headers_ms": "wall clock from request start until response headers; includes DNS/TCP/TLS on cold client and provider queueing",
            "first_text_delta_ms": "wall clock until first non-empty model text delta",
            "first_heuristic_spoken_clause_ms": "wall clock until streamed spoken_response text first contains sentence/clause punctuation",
            "current_runtime_safe_spoken_ms": "completion time because StructuredV1 currently releases no ready sentences before finish",
            "network_timeout": "per connect, read, write, and pool inactivity operation; provider stream bytes and event size are separately capped",
        },
        "credential_labels_available": availability,
        "cloudflare": {
            "tested": any(row.get("provider") == "cloudflare" and "complete_ms" in row for row in receipts),
            "structured_streaming_limit": "official JSON mode does not support streaming; compatibility run uses prompt-enforced JSON and local strict validation",
        },
        "official_sources": PROVIDER_SOURCES,
        "summary": groups,
        "receipts": receipts,
    }
    output = args.output_dir / "qualification.json"
    serialized = json.dumps(document, indent=2, sort_keys=True) + "\n"
    if any(value and value in serialized for value in keys.values()):
        raise RuntimeError("credential_or_account_identifier_redaction_invariant_failed")
    with output.open("x", encoding="utf-8") as stream:
        stream.write(serialized)
    print(f"wrote={output}")
    return 0


if __name__ == "__main__":
    try:
        exit_code = main()
    except Exception as error:
        print(json.dumps({"status": "qualification_failed", "error_category": type(error).__name__}))
        exit_code = 1
    sys.exit(exit_code)
