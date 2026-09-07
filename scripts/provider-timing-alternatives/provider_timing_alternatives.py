#!/usr/bin/env python3
"""Bounded, redacted live timing probes for hosted stock TTS services.

Credentials are read from environment variables or a user-supplied labeled key
file. They are sent only to the matching provider's official API host and are
never printed, persisted, placed in URLs, or included in exceptions.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import hashlib
import http.client
import json
import math
import os
import re
import ssl
import struct
import sys
import threading
import time
import socket
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Protocol
from urllib.parse import urlencode


FIXTURE_TEXT = "The harbor lantern is ready for tonight's test."
MAX_RESPONSE_BYTES = 16 * 1024 * 1024
DEFAULT_OUTPUT = Path(r"E:\temp\InteractiveNPCs\provider-live-2026-09-05\alternatives")

PROVIDER_ALIASES: dict[str, tuple[str, ...]] = {
    "openai": ("openai", "open ai"),
    "gemini": ("gemini", "google ai", "google api"),
    "groq": ("groq",),
    "cartesia": ("cartesia",),
    "deepgram": ("deepgram",),
    "inworld": ("inworld",),
    "elevenlabs": ("elevenlabs", "eleven labs", "elevenlab"),
}

PROVIDER_ENV = {
    "openai": "OPENAI_API_KEY",
    "gemini": "GEMINI_API_KEY",
    "groq": "GROQ_API_KEY",
    "cartesia": "CARTESIA_API_KEY",
    "deepgram": "DEEPGRAM_API_KEY",
    "inworld": "INWORLD_API_KEY",
    "elevenlabs": "ELEVENLABS_API_KEY",
}


@dataclass(frozen=True)
class Profile:
    id: str
    provider: str
    model: str
    voice: str
    transport: str
    sample_rate: int = 24_000
    alignment: str = "off"


PROFILES = (
    Profile("openai-gpt4o-mini", "openai", "gpt-4o-mini-tts", "cedar", "http-chunked-pcm"),
    Profile("openai-tts1", "openai", "tts-1", "onyx", "http-chunked-pcm"),
    Profile("gemini-31-flash", "gemini", "gemini-3.1-flash-tts-preview", "Charon", "sse-base64-pcm"),
    Profile("groq-orpheus", "groq", "canopylabs/orpheus-v1-english", "troy", "http-wav"),
    Profile("cartesia-sonic36", "cartesia", "sonic-3.6", "Greg", "http-chunked-pcm"),
    Profile("deepgram-flux", "deepgram", "flux-miles-en", "Miles", "http-chunked-pcm"),
    Profile("deepgram-aura2", "deepgram", "aura-2-arcas-en", "Arcas", "http-chunked-pcm"),
    Profile("inworld-flash", "inworld", "inworld-tts-2-flash", "Dennis", "json-stream-base64-pcm"),
    Profile(
        "inworld-flash-word-async",
        "inworld",
        "inworld-tts-2-flash",
        "Dennis",
        "json-stream-base64-pcm",
        alignment="word-async",
    ),
    Profile("inworld-tts2", "inworld", "inworld-tts-2", "Dennis", "json-stream-base64-pcm"),
    Profile(
        "inworld-tts2-word-async",
        "inworld",
        "inworld-tts-2",
        "Dennis",
        "json-stream-base64-pcm",
        alignment="word-async",
    ),
    Profile("elevenlabs-flash", "elevenlabs", "eleven_flash_v2_5", "George", "http-chunked-pcm"),
)


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def read_labeled_credentials(path: Path | None) -> dict[str, str]:
    """Read provider-labeled values without returning labels or source lines."""
    result: dict[str, str] = {}
    if path is None or not path.is_file():
        return result
    current: str | None = None
    for raw in path.read_text(encoding="utf-8-sig").splitlines():
        line = raw.strip()
        if not line or line.startswith(("#", ";", "//")):
            current = None
            continue
        lowered = line.lower()
        provider = next(
            (
                name
                for name, aliases in PROVIDER_ALIASES.items()
                if any(re.search(rf"(?<![a-z0-9]){re.escape(alias)}(?![a-z0-9])", lowered) for alias in aliases)
            ),
            None,
        )
        if provider:
            current = provider
            match = re.search(r"[:=]\s*([^\s].*)$", line)
            if match:
                candidate = match.group(1).strip().strip("\"'")
                if len(candidate) >= 8:
                    result.setdefault(provider, candidate)
                    current = None
            continue
        if current:
            candidate = line.strip("\"'")
            if len(candidate) >= 8:
                result.setdefault(current, candidate)
            current = None
    return result


def resolve_credentials(path: Path | None) -> tuple[dict[str, str], dict[str, str]]:
    file_values = read_labeled_credentials(path)
    values: dict[str, str] = {}
    sources: dict[str, str] = {}
    for provider, env_name in PROVIDER_ENV.items():
        env_value = os.environ.get(env_name, "").strip()
        if env_value:
            values[provider] = env_value
            sources[provider] = "environment"
        elif provider in file_values:
            values[provider] = file_values[provider]
            sources[provider] = "labeled-key-file"
        else:
            sources[provider] = "missing"
    return values, sources


class Decoder(Protocol):
    metadata: dict[str, Any]

    def feed(self, chunk: bytes) -> bytes: ...

    def finish(self) -> bytes: ...


class RawPcmDecoder:
    def __init__(self) -> None:
        self.metadata: dict[str, Any] = {}
        self._pending = b""

    def feed(self, chunk: bytes) -> bytes:
        combined = self._pending + chunk
        usable = len(combined) - (len(combined) % 2)
        self._pending = combined[usable:]
        return combined[:usable]

    def finish(self) -> bytes:
        if self._pending:
            raise ValueError("odd_length_pcm")
        return b""


class WavPcmDecoder:
    """Incrementally extracts PCM from an ordinary RIFF/WAVE stream."""

    def __init__(self) -> None:
        self.metadata: dict[str, Any] = {}
        self._buffer = bytearray()
        self._data_offset: int | None = None
        self._emitted = 0

    def _locate_data(self) -> None:
        if self._data_offset is not None or len(self._buffer) < 12:
            return
        if self._buffer[:4] != b"RIFF" or self._buffer[8:12] != b"WAVE":
            raise ValueError("not_pcm_wave")
        cursor = 12
        while cursor + 8 <= len(self._buffer):
            chunk_id = bytes(self._buffer[cursor : cursor + 4])
            chunk_size = struct.unpack_from("<I", self._buffer, cursor + 4)[0]
            body = cursor + 8
            if chunk_id == b"fmt " and len(self._buffer) >= body + min(chunk_size, 16) and chunk_size >= 16:
                fmt, channels, rate, _, _, bits = struct.unpack_from("<HHIIHH", self._buffer, body)
                if fmt != 1 or channels != 1 or bits != 16:
                    raise ValueError("unexpected_wave_format")
                self.metadata.update({"sampleRate": rate, "channels": channels, "bitsPerSample": bits})
            if chunk_id == b"data":
                self._data_offset = body
                return
            if len(self._buffer) < body + chunk_size:
                return
            cursor = body + chunk_size + (chunk_size % 2)

    def feed(self, chunk: bytes) -> bytes:
        self._buffer.extend(chunk)
        self._locate_data()
        if self._data_offset is None:
            return b""
        available = len(self._buffer) - self._data_offset
        usable = available - (available % 2)
        if usable <= self._emitted:
            return b""
        start = self._data_offset + self._emitted
        end = self._data_offset + usable
        self._emitted = usable
        return bytes(self._buffer[start:end])

    def finish(self) -> bytes:
        self._locate_data()
        if self._data_offset is None:
            raise ValueError("wave_data_chunk_missing")
        return b""


class JsonAudioDecoder:
    """Decodes SSE, NDJSON, or concatenated JSON base64 audio envelopes."""

    def __init__(self) -> None:
        self.metadata: dict[str, Any] = {
            "alignmentEvents": 0,
            "phonemeCount": 0,
            "visemeCount": 0,
            "audioEvents": 0,
            "smallestAudioEventBytes": None,
            "largestAudioEventBytes": 0,
        }
        self._text = ""
        self._json_decoder = json.JSONDecoder()

    def _observe_alignment(self, value: Any) -> None:
        if isinstance(value, dict):
            if "phoneme_timestamps" in value and isinstance(value["phoneme_timestamps"], dict):
                phones = value["phoneme_timestamps"].get("phonemes")
                if isinstance(phones, list):
                    self.metadata["phonemeCount"] += len(phones)
                    self.metadata["alignmentEvents"] += 1
            if "phoneticDetails" in value and isinstance(value["phoneticDetails"], list):
                for item in value["phoneticDetails"]:
                    if not isinstance(item, dict):
                        continue
                    phones = item.get("phones")
                    if not isinstance(phones, list):
                        continue
                    self.metadata["phonemeCount"] += len(phones)
                    self.metadata["visemeCount"] += sum(
                        1 for phone in phones if isinstance(phone, dict) and isinstance(phone.get("visemeSymbol"), str)
                    )
                self.metadata["alignmentEvents"] += 1
            for child in value.values():
                self._observe_alignment(child)
        elif isinstance(value, list):
            for child in value:
                self._observe_alignment(child)

    @staticmethod
    def _decode_b64(value: Any) -> bytes:
        if not isinstance(value, str) or len(value) < 4:
            return b""
        try:
            decoded = base64.b64decode(value, validate=True)
        except (ValueError, base64.binascii.Error):
            return b""
        return decoded[: len(decoded) - (len(decoded) % 2)]

    def _extract_audio(self, value: Any) -> bytes:
        if not isinstance(value, dict):
            return b""
        event_type = value.get("type") or value.get("event_type")
        if event_type == "chunk":
            return self._decode_b64(value.get("data"))
        delta = value.get("delta")
        if isinstance(delta, dict) and delta.get("type") == "audio":
            return self._decode_b64(delta.get("data"))
        for key in ("audioContent", "audio_content"):
            if key in value:
                return self._decode_b64(value.get(key))
        for key in ("output_audio", "outputAudio", "result", "interaction"):
            child = value.get(key)
            if isinstance(child, dict):
                if key in ("output_audio", "outputAudio"):
                    decoded = self._decode_b64(child.get("data"))
                    if decoded:
                        return decoded
                decoded = self._extract_audio(child)
                if decoded:
                    return decoded
        return b""

    def _accept(self, value: Any) -> bytes:
        self._observe_alignment(value)
        audio = self._extract_audio(value)
        if audio:
            self.metadata["audioEvents"] += 1
            smallest = self.metadata["smallestAudioEventBytes"]
            self.metadata["smallestAudioEventBytes"] = len(audio) if smallest is None else min(smallest, len(audio))
            self.metadata["largestAudioEventBytes"] = max(self.metadata["largestAudioEventBytes"], len(audio))
        return audio

    def _drain(self, final: bool = False) -> bytes:
        output = bytearray()
        while True:
            self._text = self._text.lstrip("\r\n \t")
            if not self._text:
                break
            if self._text.startswith("data:"):
                separator = self._text.find("\n\n")
                cr_separator = self._text.find("\r\n\r\n")
                if cr_separator >= 0 and (separator < 0 or cr_separator < separator):
                    separator, separator_size = cr_separator, 4
                else:
                    separator_size = 2
                if separator < 0:
                    if not final:
                        break
                    frame, self._text = self._text[5:].strip(), ""
                else:
                    frame = self._text[5:separator].strip()
                    self._text = self._text[separator + separator_size :]
                if frame and frame != "[DONE]":
                    try:
                        output.extend(self._accept(json.loads(frame)))
                    except json.JSONDecodeError:
                        self.metadata["malformedEvents"] = self.metadata.get("malformedEvents", 0) + 1
                continue
            try:
                value, end = self._json_decoder.raw_decode(self._text)
            except json.JSONDecodeError:
                newline = self._text.find("\n")
                if newline >= 0:
                    candidate = self._text[:newline].strip()
                    self._text = self._text[newline + 1 :]
                    if candidate:
                        try:
                            output.extend(self._accept(json.loads(candidate)))
                        except json.JSONDecodeError:
                            self.metadata["malformedEvents"] = self.metadata.get("malformedEvents", 0) + 1
                    continue
                if final and self._text.strip():
                    self.metadata["malformedEvents"] = self.metadata.get("malformedEvents", 0) + 1
                    self._text = ""
                break
            output.extend(self._accept(value))
            self._text = self._text[end:]
        return bytes(output)

    def feed(self, chunk: bytes) -> bytes:
        self._text += chunk.decode("utf-8", errors="strict")
        return self._drain()

    def finish(self) -> bytes:
        return self._drain(final=True)


@dataclass
class RequestSpec:
    host: str
    path: str
    headers: dict[str, str]
    body: bytes
    decoder: Decoder


def json_body(value: dict[str, Any]) -> bytes:
    return json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def request_spec(profile: Profile, key: str) -> RequestSpec:
    common = {"Content-Type": "application/json", "User-Agent": "Interactive-NPCs-2.0-TTS-qualification"}
    if profile.provider == "openai":
        return RequestSpec(
            "api.openai.com",
            "/v1/audio/speech",
            {**common, "Authorization": f"Bearer {key}", "Accept": "application/octet-stream"},
            json_body(
                {
                    "model": profile.model,
                    "voice": profile.voice,
                    "input": FIXTURE_TEXT,
                    "response_format": "pcm",
                    "stream_format": "audio",
                }
            ),
            RawPcmDecoder(),
        )
    if profile.provider == "gemini":
        return RequestSpec(
            "generativelanguage.googleapis.com",
            "/v1beta/interactions?alt=sse",
            {**common, "x-goog-api-key": key, "Accept": "text/event-stream"},
            json_body(
                {
                    "model": profile.model,
                    "input": FIXTURE_TEXT,
                    "response_format": {"type": "audio"},
                    "generation_config": {"speech_config": [{"voice": profile.voice}]},
                    "stream": True,
                }
            ),
            JsonAudioDecoder(),
        )
    if profile.provider == "groq":
        return RequestSpec(
            "api.groq.com",
            "/openai/v1/audio/speech",
            {**common, "Authorization": f"Bearer {key}", "Accept": "audio/wav"},
            json_body({"model": profile.model, "voice": profile.voice, "input": FIXTURE_TEXT, "response_format": "wav"}),
            WavPcmDecoder(),
        )
    if profile.provider == "cartesia":
        voice_id = os.environ.get("CARTESIA_VOICE_ID", "a0e99841-438c-4a64-b679-ae501e7d6091")
        return RequestSpec(
            "api.cartesia.ai",
            "/tts/bytes",
            {
                **common,
                "X-API-Key": key,
                "Cartesia-Version": "2026-03-01",
                "Accept": "application/octet-stream",
            },
            json_body(
                {
                    "model_id": profile.model,
                    "transcript": FIXTURE_TEXT,
                    "voice": {"id": voice_id},
                    "language": "en",
                    "output_format": {"container": "raw", "encoding": "pcm_s16le", "sample_rate": profile.sample_rate},
                    "generation_config": {"speed": 1.0, "volume": 1.0},
                }
            ),
            RawPcmDecoder(),
        )
    if profile.provider == "deepgram":
        version = "v2" if profile.id == "deepgram-flux" else "v1"
        query = urlencode(
            {
                "model": profile.model,
                "encoding": "linear16",
                "sample_rate": profile.sample_rate,
                "container": "none",
                "mip_opt_out": "true",
            }
        )
        return RequestSpec(
            "api.deepgram.com",
            f"/{version}/speak?{query}",
            {**common, "Authorization": f"Token {key}", "Accept": "application/octet-stream"},
            json_body({"text": FIXTURE_TEXT}),
            RawPcmDecoder(),
        )
    if profile.provider == "inworld":
        payload: dict[str, Any] = {
            "text": FIXTURE_TEXT,
            "voiceId": profile.voice,
            "modelId": profile.model,
            "audioConfig": {"audioEncoding": "PCM", "sampleRateHertz": profile.sample_rate},
            "applyTextNormalization": "OFF",
        }
        if profile.alignment != "off":
            payload.update({"timestampType": "WORD", "timestampTransportStrategy": "ASYNC"})
        return RequestSpec(
            "api.inworld.ai",
            "/tts/v1/voice:stream",
            {**common, "Authorization": f"Basic {key}", "Accept": "application/json"},
            json_body(payload),
            JsonAudioDecoder(),
        )
    if profile.provider == "elevenlabs":
        query = urlencode({"output_format": "pcm_24000"})
        return RequestSpec(
            "api.elevenlabs.io",
            f"/v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb/stream?{query}",
            {**common, "xi-api-key": key, "Accept": "application/octet-stream"},
            json_body({"text": FIXTURE_TEXT, "model_id": profile.model}),
            RawPcmDecoder(),
        )
    raise ValueError("unsupported_provider")


def classify_http_status(status: int) -> str:
    if status in (401, 403):
        return "credential-or-scope-rejected"
    if status == 404:
        return "model-or-route-unavailable"
    if status == 408:
        return "provider-timeout"
    if status == 429:
        return "quota-or-rate-limit"
    if status >= 500:
        return "provider-server-error"
    return "provider-http-error"


def audio_metrics(pcm: bytes, sample_rate: int) -> dict[str, Any]:
    pcm = pcm[: len(pcm) - (len(pcm) % 2)]
    count = len(pcm) // 2
    if not count:
        return {
            "pcmBytes": 0,
            "durationSeconds": 0.0,
            "rms": 0.0,
            "peak": 0.0,
            "clippedSamples": 0,
            "silent": True,
            "sha256": None,
        }
    sum_squares = 0
    peak = 0
    clipped = 0
    for (sample,) in struct.iter_unpack("<h", pcm):
        absolute = abs(sample)
        peak = max(peak, absolute)
        sum_squares += sample * sample
        clipped += int(absolute >= 32767)
    return {
        "pcmBytes": len(pcm),
        "durationSeconds": round(count / sample_rate, 4),
        "rms": round(math.sqrt(sum_squares / count) / 32768.0, 7),
        "peak": round(peak / 32768.0, 7),
        "clippedSamples": clipped,
        "silent": math.sqrt(sum_squares / count) / 32768.0 < 0.0001,
        "sha256": hashlib.sha256(pcm).hexdigest(),
    }


def sanitize_transport_error(error: BaseException) -> str:
    if isinstance(error, FileExistsError):
        return "artifact-already-exists"
    if isinstance(error, TimeoutError):
        return "network-timeout"
    if isinstance(error, ssl.SSLError):
        return "tls-error"
    if isinstance(error, (ConnectionError, OSError, http.client.HTTPException)):
        return "network-or-protocol-error"
    if isinstance(error, (ValueError, UnicodeError, json.JSONDecodeError)):
        return "invalid-audio-response"
    return "unexpected-local-error"


def run_once(
    profile: Profile,
    key: str,
    connection: http.client.HTTPSConnection,
    output_dir: Path,
    ordinal: int,
    timeout: float,
) -> dict[str, Any]:
    spec = request_spec(profile, key)
    started = time.perf_counter()
    deadline = started + timeout
    first_body_at: float | None = None
    first_pcm_at: float | None = None
    first_playable_at: float | None = None
    headers_at: float | None = None
    pcm = bytearray()
    wire_bytes = 0
    transport_reads = 0
    first_transport_read_bytes: int | None = None
    smallest_transport_read_bytes: int | None = None
    largest_transport_read_bytes = 0
    decoded_audio_fragments = 0
    smallest_decoded_audio_fragment_bytes: int | None = None
    largest_decoded_audio_fragment_bytes = 0
    response_status: int | None = None
    deadline_expired = threading.Event()

    def abort_at_deadline() -> None:
        deadline_expired.set()
        active_socket = connection.sock
        if active_socket is not None:
            try:
                active_socket.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        connection.close()

    watchdog = threading.Timer(timeout, abort_at_deadline)
    watchdog.daemon = True
    watchdog.start()

    def apply_remaining_socket_deadline() -> None:
        remaining = deadline - time.perf_counter()
        if remaining <= 0:
            raise TimeoutError("operation-deadline-exceeded")
        connection.timeout = remaining
        if connection.sock is not None:
            connection.sock.settimeout(remaining)

    try:
        apply_remaining_socket_deadline()
        connection.request("POST", spec.path, body=spec.body, headers=spec.headers)
        apply_remaining_socket_deadline()
        response = connection.getresponse()
        headers_at = time.perf_counter()
        response_status = response.status
        if not 200 <= response.status < 300:
            response.close()
            return {
                "ordinal": ordinal,
                "warmConnection": ordinal > 1,
                "status": classify_http_status(response.status),
                "httpStatus": response.status,
                "requestToHeadersMs": round((headers_at - started) * 1000, 1),
                "requestToFirstBodyByteMs": None,
                "requestToFirstDecodedPcmMs": None,
                "requestToFirst20msPcmMs": None,
                "requestToCompleteMs": round((time.perf_counter() - started) * 1000, 1),
            }
        while True:
            if time.perf_counter() > deadline:
                raise TimeoutError("operation-deadline-exceeded")
            apply_remaining_socket_deadline()
            # read1 returns the next transport chunk without waiting to fill the
            # requested size, preserving time-to-first-body/audio evidence.
            chunk = response.read1(4096)
            if not chunk:
                break
            now = time.perf_counter()
            wire_bytes += len(chunk)
            transport_reads += 1
            if first_transport_read_bytes is None:
                first_transport_read_bytes = len(chunk)
            smallest_transport_read_bytes = (
                len(chunk) if smallest_transport_read_bytes is None else min(smallest_transport_read_bytes, len(chunk))
            )
            largest_transport_read_bytes = max(largest_transport_read_bytes, len(chunk))
            if wire_bytes > MAX_RESPONSE_BYTES:
                raise ValueError("response_too_large")
            if first_body_at is None:
                first_body_at = now
            decoded = spec.decoder.feed(chunk)
            if decoded:
                decoded_audio_fragments += 1
                smallest_decoded_audio_fragment_bytes = (
                    len(decoded)
                    if smallest_decoded_audio_fragment_bytes is None
                    else min(smallest_decoded_audio_fragment_bytes, len(decoded))
                )
                largest_decoded_audio_fragment_bytes = max(largest_decoded_audio_fragment_bytes, len(decoded))
                if first_pcm_at is None:
                    first_pcm_at = now
                pcm.extend(decoded)
                if first_playable_at is None and len(pcm) >= profile.sample_rate * 2 // 50:
                    first_playable_at = now
        tail = spec.decoder.finish()
        if tail:
            decoded_audio_fragments += 1
            smallest_decoded_audio_fragment_bytes = (
                len(tail)
                if smallest_decoded_audio_fragment_bytes is None
                else min(smallest_decoded_audio_fragment_bytes, len(tail))
            )
            largest_decoded_audio_fragment_bytes = max(largest_decoded_audio_fragment_bytes, len(tail))
            if first_pcm_at is None:
                first_pcm_at = time.perf_counter()
            pcm.extend(tail)
        completed = time.perf_counter()
        metrics = audio_metrics(bytes(pcm), spec.decoder.metadata.get("sampleRate", profile.sample_rate))
        usable = not metrics["silent"] and metrics["pcmBytes"] > 0
        if usable:
            write_new_bytes(output_dir / f"{profile.id}-{ordinal}.pcm", bytes(pcm))
        return {
            "ordinal": ordinal,
            "warmConnection": ordinal > 1,
            "status": "usable-audio" if usable else "empty-or-silent-audio",
            "httpStatus": response_status,
            "requestToHeadersMs": round((headers_at - started) * 1000, 1) if headers_at else None,
            "requestToFirstBodyByteMs": round((first_body_at - started) * 1000, 1) if first_body_at else None,
            "requestToFirstDecodedPcmMs": round((first_pcm_at - started) * 1000, 1) if first_pcm_at else None,
            "requestToFirst20msPcmMs": round((first_playable_at - started) * 1000, 1) if first_playable_at else None,
            "requestToCompleteMs": round((completed - started) * 1000, 1),
            "wireBytes": wire_bytes,
            "transportReads": transport_reads,
            "firstTransportReadBytes": first_transport_read_bytes,
            "smallestTransportReadBytes": smallest_transport_read_bytes,
            "largestTransportReadBytes": largest_transport_read_bytes,
            "decodedAudioFragments": decoded_audio_fragments,
            "smallestDecodedAudioFragmentBytes": smallest_decoded_audio_fragment_bytes,
            "largestDecodedAudioFragmentBytes": largest_decoded_audio_fragment_bytes,
            "audio": metrics,
            "alignment": spec.decoder.metadata,
        }
    except Exception as error:
        return {
            "ordinal": ordinal,
            "warmConnection": ordinal > 1,
            "status": (
                "operation-deadline-exceeded"
                if deadline_expired.is_set()
                else sanitize_transport_error(error)
            ),
            "httpStatus": response_status,
            "requestToHeadersMs": round((headers_at - started) * 1000, 1) if headers_at else None,
            "requestToFirstBodyByteMs": round((first_body_at - started) * 1000, 1) if first_body_at else None,
            "requestToFirstDecodedPcmMs": round((first_pcm_at - started) * 1000, 1) if first_pcm_at else None,
            "requestToFirst20msPcmMs": round((first_playable_at - started) * 1000, 1) if first_playable_at else None,
            "requestToCompleteMs": round((time.perf_counter() - started) * 1000, 1),
        }
    finally:
        watchdog.cancel()


def run_profile(profile: Profile, key: str, output_dir: Path, repeat: int, timeout: float) -> dict[str, Any]:
    runs: list[dict[str, Any]] = []
    connection = http.client.HTTPSConnection(request_spec(profile, key).host, timeout=timeout, context=ssl.create_default_context())
    try:
        for ordinal in range(1, repeat + 1):
            runs.append(run_once(profile, key, connection, output_dir, ordinal, timeout))
            if runs[-1]["status"] != "usable-audio":
                break
    finally:
        connection.close()
    return {
        "id": profile.id,
        "provider": profile.provider,
        "model": profile.model,
        "voice": profile.voice,
        "transport": profile.transport,
        "sampleRate": profile.sample_rate,
        "alignment": profile.alignment,
        "runs": runs,
    }


def safe_json_get(host: str, path: str, headers: dict[str, str], timeout: float) -> tuple[int, dict[str, Any]]:
    connection = http.client.HTTPSConnection(host, timeout=timeout, context=ssl.create_default_context())
    try:
        connection.request("GET", path, headers={**headers, "Accept": "application/json", "User-Agent": "Interactive-NPCs-2.0-account-probe"})
        response = connection.getresponse()
        raw = response.read(2 * 1024 * 1024 + 1)
        if len(raw) > 2 * 1024 * 1024:
            raise ValueError("response_too_large")
        if response.status != 200:
            return response.status, {}
        value = json.loads(raw)
        return response.status, value if isinstance(value, dict) else {}
    finally:
        connection.close()


def probe_elevenlabs_subscription(key: str, timeout: float) -> dict[str, Any]:
    """A non-generating account quota probe; no names, IDs, invoices, or keys survive."""
    started = time.perf_counter()
    try:
        status, payload = safe_json_get(
            "api.elevenlabs.io",
            "/v1/user/subscription",
            {"xi-api-key": key},
            timeout,
        )
        if status != 200:
            return {"status": classify_http_status(status), "httpStatus": status}
        used = payload.get("character_count") if isinstance(payload.get("character_count"), int) else None
        limit = payload.get("character_limit") if isinstance(payload.get("character_limit"), int) else None
        remaining = max(0, limit - used) if used is not None and limit is not None else None
        reset = payload.get("next_character_count_reset_unix")
        reset_utc = None
        if isinstance(reset, int):
            reset_utc = dt.datetime.fromtimestamp(reset, tz=dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
        return {
            "status": "ok",
            "httpStatus": status,
            "latencyMs": round((time.perf_counter() - started) * 1000, 1),
            "tier": payload.get("tier") if isinstance(payload.get("tier"), str) else "unknown",
            "accountStatus": payload.get("status") if isinstance(payload.get("status"), str) else "unknown",
            "accountCharactersUsed": used,
            "accountCharacterLimit": limit,
            "accountCharactersRemaining": remaining,
            "accountQuotaExhausted": remaining == 0 if remaining is not None else None,
            "usageBasedExtensionEnabled": bool(payload.get("can_extend_character_limit", False)),
            "nextAccountResetUtc": reset_utc,
            "perKeyQuotaInspectable": False,
            "quotaScopeConclusion": (
                "account-quota-exhausted" if remaining == 0 else "account-has-quota; key-level cap remains unobservable"
            ),
        }
    except BaseException as error:
        return {"status": sanitize_transport_error(error), "httpStatus": None}


def select_profiles(profile_ids: set[str], providers: set[str]) -> Iterable[Profile]:
    for profile in PROFILES:
        if profile_ids and profile.id not in profile_ids:
            continue
        if providers and profile.provider not in providers:
            continue
        yield profile


def write_new_bytes(path: Path, value: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(value)
        stream.flush()
        os.fsync(stream.fileno())


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    serialized = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    write_new_bytes(path, serialized)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--credentials", type=Path, help="Provider-labeled local key file")
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--report-file",
        type=Path,
        help="Immutable report path; defaults to a UTC-stamped file under output-dir",
    )
    parser.add_argument("--providers", default="", help="Comma-separated provider IDs")
    parser.add_argument("--profiles", default="", help="Comma-separated profile IDs")
    parser.add_argument("--repeat", type=int, choices=(1, 2), default=1, help="Use 2 only to separate cold from steady connection timing")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--probe-accounts", action="store_true", help="Run non-generating account probes where supported")
    parser.add_argument("--strict", action="store_true", help="Fail if any selected profile is skipped or unusable")
    args = parser.parse_args()

    if not 0 < args.timeout <= 120:
        parser.error("timeout must be greater than zero and at most 120 seconds")

    profile_ids = {item.strip() for item in args.profiles.split(",") if item.strip()}
    providers = {item.strip() for item in args.providers.split(",") if item.strip()}
    known_profiles = {profile.id for profile in PROFILES}
    known_providers = set(PROVIDER_ALIASES)
    unknown_profiles = sorted(profile_ids - known_profiles)
    unknown_providers = sorted(providers - known_providers)
    if unknown_profiles or unknown_providers:
        parser.error(f"unknown profiles={unknown_profiles} providers={unknown_providers}")

    credentials, sources = resolve_credentials(args.credentials)
    selected = list(select_profiles(profile_ids, providers))
    report: dict[str, Any] = {
        "schemaVersion": 1,
        "checkedAtUtc": utc_now(),
        "purpose": "bounded-live-hosted-stock-tts-qualification",
        "fixtureText": FIXTURE_TEXT,
        "containsCredentialValues": False,
        "measurementClock": "time.perf_counter",
        "credentialSources": sources,
        "requestedRepeatCount": args.repeat,
        "profiles": [],
        "accountProbes": {},
    }

    if args.dry_run:
        report["profiles"] = [
            {
                "id": profile.id,
                "provider": profile.provider,
                "credentialPresent": profile.provider in credentials,
                "model": profile.model,
                "voice": profile.voice,
                "transport": profile.transport,
                "alignment": profile.alignment,
            }
            for profile in selected
        ]
    else:
        for profile in selected:
            if profile.provider not in credentials:
                report["profiles"].append(
                    {
                        "id": profile.id,
                        "provider": profile.provider,
                        "model": profile.model,
                        "voice": profile.voice,
                        "transport": profile.transport,
                        "alignment": profile.alignment,
                        "runs": [{"status": "skipped-missing-credential"}],
                    }
                )
                continue
            report["profiles"].append(run_profile(profile, credentials[profile.provider], args.output_dir, args.repeat, args.timeout))

    if args.probe_accounts and "elevenlabs" in credentials:
        report["accountProbes"]["elevenlabs"] = probe_elevenlabs_subscription(credentials["elevenlabs"], args.timeout)

    report_path = args.report_file or (
        args.output_dir / f"provider-timing-alternatives-{report['checkedAtUtc'].replace(':', '').replace('-', '')}.json"
    )
    if report_path.exists():
        parser.error(f"report file already exists: {report_path}")
    report["reportFile"] = str(report_path)
    atomic_json(report_path, report)
    print(json.dumps(report, indent=2, sort_keys=True))

    if args.strict:
        for profile in report["profiles"]:
            runs = profile.get("runs", [])
            if not runs or any(run.get("status") != "usable-audio" for run in runs):
                return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
