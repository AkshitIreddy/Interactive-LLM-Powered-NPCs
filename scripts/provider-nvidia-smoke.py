#!/usr/bin/env python3
"""Bounded NVIDIA NIM smoke using only synthetic, non-sensitive content.

The ignored credential file is read locally. The key, response text, vectors,
request IDs, and account identifiers are never printed or persisted.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import re
import struct
import time
import urllib.error
import urllib.request
import wave
from pathlib import Path


MAXIMUM_RESPONSE = 4 * 1024 * 1024
PREFERRED_STOCK_EN_US_VOICES = ("Jason", "Leo", "Ray")


def read_nvidia_key(path: Path) -> str:
    current = False
    for raw in path.read_text(encoding="utf-8-sig").splitlines():
        line = raw.strip()
        if not line:
            continue
        lowered = line.lower()
        if "nvidia" in lowered or "nim" in lowered:
            current = True
            match = re.search(r"[:=]\s*([^\s].*)$", line)
            if match:
                value = match.group(1).strip().strip("\"'")
                if len(value) >= 16:
                    return value
            continue
        if current:
            value = line.strip("\"'")
            if len(value) >= 16:
                return value
            current = False
    raise RuntimeError("NVIDIA NIM credential label was not found")


def call_json(
    url: str,
    key: str,
    *,
    method: str = "GET",
    payload: dict | None = None,
    timeout: float = 30.0,
) -> tuple[int, dict, dict[str, str]]:
    body = None if payload is None else json.dumps(payload, separators=(",", ":")).encode()
    request = urllib.request.Request(
        url,
        data=body,
        method=method,
        headers={
            "Authorization": f"Bearer {key}",
            "Accept": "application/json",
            "User-Agent": "Interactive-NPCs-2.0-NVIDIA-local-smoke",
            **({"Content-Type": "application/json"} if body else {}),
        },
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read(MAXIMUM_RESPONSE + 1)
        if len(raw) > MAXIMUM_RESPONSE:
            raise RuntimeError("response_too_large")
        decoded = json.loads(raw)
        if not isinstance(decoded, dict):
            raise RuntimeError("unexpected_response_shape")
        headers = {name.lower(): value for name, value in response.headers.items()}
        return response.status, decoded, headers


def call_bytes(
    url: str,
    key: str,
    *,
    method: str,
    body: bytes | None = None,
    content_type: str | None = None,
    timeout: float = 45.0,
) -> tuple[int, bytes, dict[str, str]]:
    headers = {
        "Authorization": f"Bearer {key}",
        "Accept": "application/json, audio/wav, audio/x-wav, application/octet-stream",
        "User-Agent": "Interactive-NPCs-2.0-NVIDIA-local-smoke",
    }
    if content_type:
        headers["Content-Type"] = content_type
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read(16 * 1024 * 1024 + 1)
        if len(raw) > 16 * 1024 * 1024:
            raise RuntimeError("response_too_large")
        return (
            response.status,
            raw,
            {name.lower(): value for name, value in response.headers.items()},
        )


def sanitized_error(error: Exception) -> dict:
    if isinstance(error, urllib.error.HTTPError):
        return {
            "ok": False,
            "httpStatus": error.code,
            "status": {
                401: "invalid_credential",
                402: "payment_or_entitlement_required",
                403: "forbidden_or_scope_limited",
                429: "rate_limited",
            }.get(error.code, "provider_http_error"),
        }
    if isinstance(error, (urllib.error.URLError, TimeoutError)):
        return {"ok": False, "httpStatus": None, "status": "network_or_timeout"}
    return {"ok": False, "httpStatus": None, "status": "invalid_response"}


def model_discovery(key: str) -> tuple[dict, list[str]]:
    started = time.perf_counter()
    try:
        status, payload, headers = call_json(
            "https://integrate.api.nvidia.com/v1/models", key, timeout=20
        )
        records = payload.get("data")
        ids = (
            sorted(
                item["id"]
                for item in records
                if isinstance(item, dict) and isinstance(item.get("id"), str)
            )
            if isinstance(records, list)
            else []
        )
        return (
            {
                "ok": status == 200 and bool(ids),
                "httpStatus": status,
                "modelRecordsVisible": len(ids),
                "latencyMs": round((time.perf_counter() - started) * 1000, 1),
                "rateLimitHeadersPresent": any(
                    name.startswith("x-ratelimit") for name in headers
                ),
            },
            ids,
        )
    except Exception as error:  # sanitized below; no provider body is retained
        return sanitized_error(error), []


def chat_smoke(key: str, discovered: list[str]) -> dict:
    preferred = [
        "nvidia/nemotron-3.5-lightning-30b-a3b",
        "nvidia/nemotron-3-super-120b-a12b",
        "nvidia/nemotron-nano-3-30b-a3b",
        "nvidia/nemotron-3-nano-30b-a3b",
        "meta/llama-3.1-8b-instruct",
        "nvidia/nvidia-nemotron-nano-9b-v2",
    ]
    model = next((candidate for candidate in preferred if candidate in discovered), None)
    if model is None:
        model = next(
            (
                candidate
                for candidate in discovered
                if candidate.startswith(("nvidia/", "meta/", "mistralai/"))
                and any(token in candidate for token in ("instruct", "lightning", "super", "nano"))
                and not any(token in candidate for token in ("embed", "rerank", "guard", "safety", "parse", "vision"))
            ),
            None,
        )
    if model is None:
        return {"ok": False, "status": "no_compatible_chat_model"}
    started = time.perf_counter()
    try:
        status, payload, _ = call_json(
            "https://integrate.api.nvidia.com/v1/chat/completions",
            key,
            method="POST",
            payload={
                "model": model,
                "messages": [{"role": "user", "content": "Reply with exactly: READY"}],
                "max_tokens": 8,
                "temperature": 0,
                "stream": False,
            },
            timeout=45,
        )
        choices = payload.get("choices")
        text = ""
        if isinstance(choices, list) and choices and isinstance(choices[0], dict):
            message = choices[0].get("message")
            if isinstance(message, dict) and isinstance(message.get("content"), str):
                text = message["content"]
        return {
            "ok": status == 200 and bool(text),
            "httpStatus": status,
            "model": model,
            "exactReady": text.strip().upper() == "READY",
            "responseCharacters": len(text),
            "latencyMs": round((time.perf_counter() - started) * 1000, 1),
        }
    except Exception as error:
        return {"model": model, **sanitized_error(error)}


def embedding_smoke(key: str, discovered: list[str]) -> dict:
    started = time.perf_counter()
    preferred = [
        "nvidia/nemotron-3-embed-1b",
        "nvidia/embed-qa-4",
        "nvidia/llama-3.2-nv-embedqa-1b-v1",
    ]
    model = next((candidate for candidate in preferred if candidate in discovered), preferred[0])
    try:
        status, payload, _ = call_json(
            "https://integrate.api.nvidia.com/v1/embeddings",
            key,
            method="POST",
            payload={
                "model": model,
                "input": ["Eclipse Harbor lighthouse fixture"],
                "input_type": "query",
                "encoding_format": "float",
                "truncate": "NONE",
            },
            timeout=45,
        )
        data = payload.get("data")
        vector = None
        if isinstance(data, list) and len(data) == 1 and isinstance(data[0], dict):
            vector = data[0].get("embedding")
        valid = (
            isinstance(vector, list)
            and len(vector) > 0
            and all(isinstance(value, (int, float)) and math.isfinite(value) for value in vector)
        )
        return {
            "ok": status == 200 and valid,
            "httpStatus": status,
            "model": model,
            "dimensions": len(vector) if isinstance(vector, list) else 0,
            "latencyMs": round((time.perf_counter() - started) * 1000, 1),
        }
    except Exception as error:
        return {"model": model, **sanitized_error(error)}


def rerank_smoke(key: str) -> dict:
    started = time.perf_counter()
    candidates = [
        (
            "nvidia/llama-3.2-nv-rerankqa-1b-v1",
            "https://ai.api.nvidia.com/v1/retrieval/nvidia/llama-3_2-nv-rerankqa-1b-v1/reranking",
        ),
        (
            "nvidia/nv-rerankqa-mistral-4b-v3",
            "https://ai.api.nvidia.com/v1/retrieval/nvidia/reranking",
        ),
    ]
    last: dict = {"ok": False, "status": "no_current_rerank_endpoint"}
    for model, url in candidates:
        try:
            status, payload, _ = call_json(
                url,
                key,
                method="POST",
                payload={
                    "model": model,
                    "query": {"text": "Which beacon is ready?"},
                    "passages": [
                        {"text": "The north beacon is ready."},
                        {"text": "The eastern lock is closed."},
                    ],
                },
                timeout=45,
            )
            rankings = payload.get("rankings")
            if not isinstance(rankings, list):
                rankings = payload.get("results")
            return {
                "ok": status == 200 and isinstance(rankings, list) and len(rankings) == 2,
                "httpStatus": status,
                "model": model,
                "rankedPassages": len(rankings) if isinstance(rankings, list) else 0,
                "latencyMs": round((time.perf_counter() - started) * 1000, 1),
            }
        except Exception as error:
            last = {"model": model, **sanitized_error(error)}
            if last.get("httpStatus") not in (404, 410):
                break
    return last


def multipart(fields: dict[str, str]) -> tuple[bytes, str]:
    boundary = "npc2-nvidia-synthetic-smoke"
    chunks: list[bytes] = []
    for name, value in fields.items():
        chunks.extend(
            [
                f"--{boundary}\r\n".encode(),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode(),
                value.encode(),
                b"\r\n",
            ]
        )
    chunks.append(f"--{boundary}--\r\n".encode())
    return b"".join(chunks), f"multipart/form-data; boundary={boundary}"


def multipart_audio(
    fields: dict[str, str], file_field: str, filename: str, audio: bytes
) -> tuple[bytes, str]:
    boundary = "npc2-nvidia-synthetic-audio-smoke"
    chunks: list[bytes] = []
    for name, value in fields.items():
        chunks.extend(
            [
                f"--{boundary}\r\n".encode(),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode(),
                value.encode(),
                b"\r\n",
            ]
        )
    chunks.extend(
        [
            f"--{boundary}\r\n".encode(),
            (
                f'Content-Disposition: form-data; name="{file_field}"; '
                f'filename="{filename}"\r\n'
            ).encode(),
            b"Content-Type: audio/wav\r\n\r\n",
            audio,
            b"\r\n",
            f"--{boundary}--\r\n".encode(),
        ]
    )
    return b"".join(chunks), f"multipart/form-data; boundary={boundary}"


def measure_wav(path: Path) -> dict:
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        sample_rate = stream.getframerate()
        width = stream.getsampwidth()
        frames = stream.getnframes()
        raw = stream.readframes(frames)
    if channels != 1 or width != 2:
        raise RuntimeError("unexpected_audio_shape")
    samples = struct.unpack(f"<{frames}h", raw) if frames else ()
    peak_sample = max((abs(value) for value in samples), default=0)
    peak = peak_sample / 32768
    rms = math.sqrt(sum(value * value for value in samples) / max(1, len(samples))) / 32768
    clipped = sum(abs(value) >= 32767 for value in samples)
    max_delta = max(
        (abs(right - left) for left, right in zip(samples, samples[1:])), default=0
    ) / 32768
    first_window = samples[: max(1, sample_rate // 1000)]
    tail_window = samples[-max(1, sample_rate // 50) :]
    onset = (
        max((abs(value) for value in first_window), default=0) / peak_sample
        if peak_sample
        else 0
    )
    tail_rms = (
        math.sqrt(sum(value * value for value in tail_window) / max(1, len(tail_window)))
        / 32768
    )
    return {
        "channels": channels,
        "sampleRate": sample_rate,
        "durationSeconds": round(frames / sample_rate, 3),
        "peak": round(peak, 6),
        "rms": round(rms, 6),
        "clippedSamples": clipped,
        "maxAdjacentDelta": round(max_delta, 6),
        "onsetRatio1ms": round(onset, 6),
        "tailRms20ms": round(tail_rms, 6),
        "silent": rms < 0.0001,
    }


def is_cloning_voice_identifier(voice: str) -> bool:
    normalized = re.sub(r"[^A-Z0-9]", "", voice.upper())
    return "ZEROSHOT" in normalized or "CLON" in normalized


def stock_en_us_voices(candidates: list[str]) -> list[str]:
    return sorted(
        {
            voice
            for voice in candidates
            if "EN-US" in voice.upper() and not is_cloning_voice_identifier(voice)
        },
        key=lambda voice: (voice.casefold(), voice),
    )


def select_stock_en_us_voice(
    candidates: list[str], requested_voice: str | None = None
) -> str:
    if requested_voice is not None:
        if is_cloning_voice_identifier(requested_voice):
            raise ValueError("tts_voice_cloning_not_allowed")
        if requested_voice not in candidates:
            raise ValueError("tts_voice_not_discovered_stock_en_us")
        return requested_voice

    for preferred_name in PREFERRED_STOCK_EN_US_VOICES:
        suffix = f".{preferred_name}".casefold()
        preferred = next(
            (voice for voice in candidates if voice.casefold().endswith(suffix)), None
        )
        if preferred is not None:
            return preferred
    if not candidates:
        raise ValueError("no_stock_english_voice")
    return candidates[0]


def magpie_tts_smoke(
    key: str, output: Path, requested_voice: str | None = None
) -> dict:
    base = "https://877104f7-e885-42b9-8de8-f6e4c6303969.invocation.api.nvcf.nvidia.com"
    started = time.perf_counter()
    try:
        status, raw, _ = call_bytes(
            f"{base}/v1/audio/list_voices", key, method="GET", timeout=30
        )
        payload = json.loads(raw)
        candidates: list[str] = []
        values = payload.get("voices") if isinstance(payload, dict) else payload
        if isinstance(payload, dict) and not isinstance(values, list):
            nested = []
            for group in payload.values():
                if isinstance(group, dict) and isinstance(group.get("voices"), list):
                    nested.extend(group["voices"])
            values = nested
        if isinstance(values, list):
            for item in values:
                if isinstance(item, str):
                    candidates.append(item)
                elif isinstance(item, dict):
                    value = item.get("name") or item.get("voice") or item.get("voice_id")
                    if isinstance(value, str):
                        candidates.append(value)
        stock = stock_en_us_voices(candidates)
        if status != 200:
            return {
                "ok": False,
                "httpStatus": status,
                "status": "no_stock_english_voice",
                "voiceRecordsVisible": len(candidates),
            }
        try:
            voice = select_stock_en_us_voice(stock, requested_voice)
        except ValueError as error:
            return {
                "ok": False,
                "httpStatus": status,
                "status": str(error),
                "voiceRecordsVisible": len(candidates),
                "stockEnglishVoices": len(stock),
            }
        body, content_type = multipart(
            {
                "text": "The north beacon is ready.",
                "language": "en-US",
                "voice": voice,
                "encoding": "LINEAR_PCM",
                "sample_rate_hz": "22050",
            }
        )
        synth_status, audio, response_headers = call_bytes(
            f"{base}/v1/audio/synthesize",
            key,
            method="POST",
            body=body,
            content_type=content_type,
            timeout=60,
        )
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(audio)
        metrics = measure_wav(output)
        return {
            "ok": synth_status == 200
            and not metrics["silent"]
            and metrics["clippedSamples"] == 0,
            "httpStatus": synth_status,
            "voice": voice,
            "voiceRecordsVisible": len(candidates),
            "stockEnglishVoices": len(stock),
            "audioBytes": len(audio),
            "contentType": response_headers.get("content-type", "unknown").split(";", 1)[0],
            "latencyMs": round((time.perf_counter() - started) * 1000, 1),
            "audioMetrics": metrics,
            "voiceCloningUsed": False,
        }
    except Exception as error:
        return sanitized_error(error)


def nemotron_asr_smoke(key: str, audio: Path) -> dict:
    if not audio.is_file():
        return {"ok": False, "status": "tts_fixture_unavailable"}
    function_id = "bb0837de-8c7b-481f-9ec8-ef5663e9c1fa"
    url = (
        f"https://{function_id}.invocation.api.nvcf.nvidia.com"
        "/v1/audio/transcriptions"
    )
    body, content_type = multipart_audio(
        {"language": "en-US", "response_format": "json"},
        "file",
        "eclipse-harbor-fixture.wav",
        audio.read_bytes(),
    )
    started = time.perf_counter()
    try:
        status, raw, _ = call_bytes(
            url,
            key,
            method="POST",
            body=body,
            content_type=content_type,
            timeout=60,
        )
        payload = json.loads(raw)
        text = payload.get("text", "") if isinstance(payload, dict) else ""
        normalized = re.sub(r"[^a-z]", "", text.lower())
        expected = re.sub(r"[^a-z]", "", "The north beacon is ready.".lower())
        return {
            "ok": status == 200 and bool(text),
            "httpStatus": status,
            "model": "nemotron-asr-streaming",
            "textMatchesFixture": normalized == expected,
            "responseCharacters": len(text),
            "latencyMs": round((time.perf_counter() - started) * 1000, 1),
        }
    except Exception as error:
        return sanitized_error(error)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--credentials", type=Path, default=Path(".secrets/Commonly used Keys.txt")
    )
    parser.add_argument(
        "--out", type=Path, default=Path("artifacts/provider-smoke/nvidia-nim.json")
    )
    parser.add_argument(
        "--tts-voice",
        help=(
            "Exact discovered stock EN-US Magpie voice identifier. "
            "ZeroShot and cloning identifiers are rejected."
        ),
    )
    return parser.parse_args(argv)


def main() -> None:
    args = parse_args()
    key = read_nvidia_key(args.credentials)
    discovery, models = model_discovery(key)
    tts_path = args.out.parent / "nvidia-magpie-fixture.wav"
    tts_result = magpie_tts_smoke(key, tts_path, args.tts_voice)
    report = {
        "schemaVersion": 1,
        "checkedAtUtc": dt.datetime.now(dt.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "purpose": "local-synthetic-nvidia-nim-smoke",
        "containsCredentialValues": False,
        "containsProviderResponseTextOrVectors": False,
        "discovery": discovery,
        "chat": chat_smoke(key, models),
        "embedding": embedding_smoke(key, models),
        "rerank": rerank_smoke(key),
        "tts": tts_result,
        "asr": nemotron_asr_smoke(key, tts_path) if tts_result.get("ok") else {
            "ok": False,
            "status": "tts_fixture_unavailable",
        },
        "notes": [
            "Synthetic fixture content only; this is not a benchmark.",
            "A successful trial response is not evidence of permanent unlimited access or production entitlement.",
        ],
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    args.out.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    if not discovery.get("ok") or not report["chat"].get("ok"):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
