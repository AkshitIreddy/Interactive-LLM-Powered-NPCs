#!/usr/bin/env python3
"""Redacted two-turn WebSocket timing probes for qualified hosted TTS APIs.

Each profile opens one socket and synthesizes the same innocuous sentence twice.
Connection and provider-context setup are reported separately from generation
latency. Credential values and raw provider errors are never logged or saved.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import time
import uuid
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlencode

from websockets.exceptions import InvalidStatus
from websockets.sync.client import ClientConnection, connect

from provider_timing_alternatives import (
    DEFAULT_OUTPUT,
    FIXTURE_TEXT,
    MAX_RESPONSE_BYTES,
    audio_metrics,
    atomic_json,
    resolve_credentials,
    sanitize_transport_error,
    utc_now,
    write_new_bytes,
)


SAMPLE_RATE = 24_000
PLAYABLE_BYTES = SAMPLE_RATE * 2 // 50


@dataclass(frozen=True)
class WebSocketProfile:
    id: str
    provider: str
    model: str
    voice: str
    alignment: str


PROFILES = (
    WebSocketProfile("cartesia-sonic36-phonemes", "cartesia", "sonic-3.6", "Greg", "phoneme"),
    WebSocketProfile("deepgram-flux", "deepgram", "flux-miles-en", "Miles", "off"),
    WebSocketProfile("deepgram-aura2", "deepgram", "aura-2-arcas-en", "Arcas", "off"),
    WebSocketProfile("inworld-flash", "inworld", "inworld-tts-2-flash", "Dennis", "off"),
    WebSocketProfile("inworld-flash-word-async", "inworld", "inworld-tts-2-flash", "Dennis", "word-async"),
    WebSocketProfile("inworld-tts2", "inworld", "inworld-tts-2", "Dennis", "off"),
    WebSocketProfile("inworld-tts2-word-async", "inworld", "inworld-tts-2", "Dennis", "word-async"),
)


class TurnObservation:
    def __init__(self, ordinal: int, pcm_path: Path, timeout: float) -> None:
        self.ordinal = ordinal
        self.pcm_path = pcm_path
        self.started = time.perf_counter()
        self.deadline = self.started + timeout
        self.first_transport_at: float | None = None
        self.first_pcm_at: float | None = None
        self.first_playable_at: float | None = None
        self.pcm = bytearray()
        self.transport_events = 0
        self.first_transport_event_bytes: int | None = None
        self.smallest_transport_event_bytes: int | None = None
        self.largest_transport_event_bytes = 0
        self.binary_transport_events = 0
        self.json_transport_events = 0
        self.audio_events = 0
        self.smallest_audio_event_bytes: int | None = None
        self.largest_audio_event_bytes = 0
        self.event_types: Counter[str] = Counter()
        self.phoneme_count = 0
        self.viseme_count = 0
        self.alignment_events = 0
        self.wire_bytes = 0

    def remaining_timeout(self) -> float:
        remaining = self.deadline - time.perf_counter()
        if remaining <= 0:
            raise TimeoutError("turn-deadline-exceeded")
        return remaining

    def observe_transport(self, message: str | bytes) -> None:
        now = time.perf_counter()
        size = len(message.encode("utf-8")) if isinstance(message, str) else len(message)
        self.wire_bytes += size
        if self.wire_bytes > MAX_RESPONSE_BYTES:
            raise ValueError("response-too-large")
        self.transport_events += 1
        self.json_transport_events += int(isinstance(message, str))
        self.binary_transport_events += int(isinstance(message, bytes))
        if self.first_transport_at is None:
            self.first_transport_at = now
            self.first_transport_event_bytes = size
        self.smallest_transport_event_bytes = (
            size if self.smallest_transport_event_bytes is None else min(self.smallest_transport_event_bytes, size)
        )
        self.largest_transport_event_bytes = max(self.largest_transport_event_bytes, size)

    def observe_audio(self, audio: bytes) -> None:
        if len(audio) % 2:
            audio = audio[:-1]
        if not audio:
            return
        if len(self.pcm) + len(audio) > MAX_RESPONSE_BYTES:
            raise ValueError("decoded-audio-too-large")
        now = time.perf_counter()
        self.audio_events += 1
        self.smallest_audio_event_bytes = (
            len(audio) if self.smallest_audio_event_bytes is None else min(self.smallest_audio_event_bytes, len(audio))
        )
        self.largest_audio_event_bytes = max(self.largest_audio_event_bytes, len(audio))
        if self.first_pcm_at is None:
            self.first_pcm_at = now
        self.pcm.extend(audio)
        if self.first_playable_at is None and len(self.pcm) >= PLAYABLE_BYTES:
            self.first_playable_at = now

    def observe_alignment(self, value: Any) -> None:
        if isinstance(value, dict):
            phonemes = value.get("phoneme_timestamps")
            if isinstance(phonemes, dict) and isinstance(phonemes.get("phonemes"), list):
                self.phoneme_count += len(phonemes["phonemes"])
                self.alignment_events += 1
            details = value.get("phoneticDetails")
            if isinstance(details, list):
                for detail in details:
                    phones = detail.get("phones") if isinstance(detail, dict) else None
                    if not isinstance(phones, list):
                        continue
                    self.phoneme_count += len(phones)
                    self.viseme_count += sum(
                        isinstance(phone, dict) and isinstance(phone.get("visemeSymbol"), str) for phone in phones
                    )
                self.alignment_events += 1
            for child in value.values():
                self.observe_alignment(child)
        elif isinstance(value, list):
            for child in value:
                self.observe_alignment(child)

    def finish(self) -> dict[str, Any]:
        completed = time.perf_counter()
        metrics = audio_metrics(bytes(self.pcm), SAMPLE_RATE)
        usable = metrics["pcmBytes"] > 0 and not metrics["silent"]
        if usable:
            write_new_bytes(self.pcm_path, bytes(self.pcm))
        return {
            "ordinal": self.ordinal,
            "warmPersistentConnection": self.ordinal > 1,
            "status": "usable-audio" if usable else "empty-or-silent-audio",
            "requestToFirstTransportEventMs": self._elapsed(self.first_transport_at),
            "requestToFirstDecodedPcmMs": self._elapsed(self.first_pcm_at),
            "requestToFirst20msPcmMs": self._elapsed(self.first_playable_at),
            "requestToCompleteMs": round((completed - self.started) * 1000, 1),
            "transportEvents": self.transport_events,
            "firstTransportEventBytes": self.first_transport_event_bytes,
            "smallestTransportEventBytes": self.smallest_transport_event_bytes,
            "largestTransportEventBytes": self.largest_transport_event_bytes,
            "binaryTransportEvents": self.binary_transport_events,
            "jsonTransportEvents": self.json_transport_events,
            "audioEvents": self.audio_events,
            "smallestAudioEventBytes": self.smallest_audio_event_bytes,
            "largestAudioEventBytes": self.largest_audio_event_bytes,
            "eventTypes": dict(sorted(self.event_types.items())),
            "alignment": {
                "alignmentEvents": self.alignment_events,
                "phonemeCount": self.phoneme_count,
                "visemeCount": self.viseme_count,
            },
            "audio": metrics,
        }

    def _elapsed(self, event: float | None) -> float | None:
        return round((event - self.started) * 1000, 1) if event is not None else None


def decode_base64_audio(value: Any) -> bytes:
    if not isinstance(value, str) or len(value) < 4:
        return b""
    try:
        return base64.b64decode(value, validate=True)
    except (ValueError, base64.binascii.Error):
        return b""


def receive_json(socket: ClientConnection, timeout: float) -> tuple[str, dict[str, Any]]:
    message = socket.recv(timeout=timeout)
    if not isinstance(message, str):
        return "binary", {}
    value = json.loads(message)
    if not isinstance(value, dict):
        raise ValueError("unexpected-json-message")
    event_type = value.get("type")
    if isinstance(event_type, str):
        return event_type, value
    result = value.get("result")
    if isinstance(result, dict):
        for candidate in ("contextCreated", "audioChunk", "flushCompleted", "contextClosed"):
            if candidate in result:
                return candidate, value
    return "json", value


def connect_timed(uri: str, headers: dict[str, str], timeout: float) -> tuple[ClientConnection, float]:
    started = time.perf_counter()
    socket = connect(
        uri,
        additional_headers=headers,
        open_timeout=timeout,
        close_timeout=min(10.0, timeout),
        ping_interval=20,
        ping_timeout=20,
        max_size=4 * 1024 * 1024,
        compression=None,
    )
    return socket, round((time.perf_counter() - started) * 1000, 1)


def run_cartesia(profile: WebSocketProfile, key: str, output_dir: Path, timeout: float) -> dict[str, Any]:
    uri = "wss://api.cartesia.ai/tts/websocket?cartesia_version=2026-03-01"
    voice_id = os.environ.get("CARTESIA_VOICE_ID", "a0e99841-438c-4a64-b679-ae501e7d6091")
    socket, connection_ms = connect_timed(uri, {"X-API-Key": key}, timeout)
    runs: list[dict[str, Any]] = []
    try:
        for ordinal in (1, 2):
            context_id = f"qualification-{uuid.uuid4()}"
            observation = TurnObservation(ordinal, output_dir / f"{profile.id}-ws-{ordinal}.pcm", timeout)
            socket.send(
                json.dumps(
                    {
                        "model_id": profile.model,
                        "transcript": FIXTURE_TEXT,
                        "voice": {"mode": "id", "id": voice_id},
                        "language": "en",
                        "context_id": context_id,
                        "output_format": {"container": "raw", "encoding": "pcm_s16le", "sample_rate": SAMPLE_RATE},
                        "add_timestamps": True,
                        "add_phoneme_timestamps": True,
                        "continue": False,
                    },
                    separators=(",", ":"),
                )
            )
            while True:
                message = socket.recv(timeout=observation.remaining_timeout())
                observation.observe_transport(message)
                if not isinstance(message, str):
                    raise ValueError("unexpected-binary-cartesia-message")
                value = json.loads(message)
                if not isinstance(value, dict) or value.get("context_id") != context_id:
                    continue
                event_type = value.get("type") if isinstance(value.get("type"), str) else "json"
                observation.event_types[event_type] += 1
                if event_type == "error":
                    raise ValueError("provider-error")
                observation.observe_alignment(value)
                if event_type == "chunk":
                    observation.observe_audio(decode_base64_audio(value.get("data")))
                if event_type == "done" or value.get("done") is True:
                    break
            runs.append(observation.finish())
    finally:
        socket.close()
    return profile_report(profile, connection_ms, None, runs)


def run_deepgram(profile: WebSocketProfile, key: str, output_dir: Path, timeout: float) -> dict[str, Any]:
    version = "v2" if profile.id == "deepgram-flux" else "v1"
    query = urlencode({"model": profile.model, "encoding": "linear16", "sample_rate": SAMPLE_RATE, "mip_opt_out": "true"})
    uri = f"wss://api.deepgram.com/{version}/speak?{query}"
    socket, connection_ms = connect_timed(uri, {"Authorization": f"Token {key}"}, timeout)
    runs: list[dict[str, Any]] = []
    ready_ms: float | None = None
    setup_event_types: dict[str, int] = {}
    try:
        if version == "v2":
            ready_started = time.perf_counter()
            ready_deadline = ready_started + timeout
            message = socket.recv(timeout=max(0.001, ready_deadline - time.perf_counter()))
            if not isinstance(message, str):
                raise ValueError("unexpected-deepgram-ready-message")
            value = json.loads(message)
            if not isinstance(value, dict) or value.get("type") != "Connected":
                raise ValueError("deepgram-connected-event-missing")
            setup_event_types["Connected"] = 1
            ready_ms = round((time.perf_counter() - ready_started) * 1000, 1)
        for ordinal in (1, 2):
            observation = TurnObservation(ordinal, output_dir / f"{profile.id}-ws-{ordinal}.pcm", timeout)
            socket.send(json.dumps({"type": "Speak", "text": FIXTURE_TEXT}, separators=(",", ":")))
            socket.send('{"type":"Flush"}')
            while True:
                message = socket.recv(timeout=observation.remaining_timeout())
                observation.observe_transport(message)
                if isinstance(message, bytes):
                    observation.event_types["audio"] += 1
                    observation.observe_audio(message)
                    continue
                value = json.loads(message)
                event_type = value.get("type") if isinstance(value, dict) and isinstance(value.get("type"), str) else "json"
                observation.event_types[event_type] += 1
                if event_type in ("Error", "error"):
                    raise ValueError("provider-error")
                if version == "v2" and event_type == "SpeechMetadata":
                    break
                if version == "v1" and event_type == "Flushed":
                    break
            runs.append(observation.finish())
        socket.send('{"type":"Close"}')
    finally:
        socket.close()
    return profile_report(profile, connection_ms, ready_ms, runs, setup_event_types)


def run_inworld(profile: WebSocketProfile, key: str, output_dir: Path, timeout: float) -> dict[str, Any]:
    uri = "wss://api.inworld.ai/tts/v1/voice:streamBidirectional"
    socket, connection_ms = connect_timed(uri, {"Authorization": f"Basic {key}"}, timeout)
    context_id = f"qualification-{uuid.uuid4()}"
    create: dict[str, Any] = {
        "voiceId": profile.voice,
        "modelId": profile.model,
        "audioConfig": {"audioEncoding": "PCM", "sampleRateHertz": SAMPLE_RATE},
        "bufferCharThreshold": 100,
        "autoMode": True,
        "applyTextNormalization": "OFF",
    }
    if profile.alignment != "off":
        create.update({"timestampType": "WORD", "timestampTransportStrategy": "ASYNC"})
    context_started = time.perf_counter()
    context_deadline = context_started + timeout
    socket.send(json.dumps({"create": create, "contextId": context_id}, separators=(",", ":")))
    context_event_types: Counter[str] = Counter()
    try:
        while True:
            remaining = context_deadline - time.perf_counter()
            if remaining <= 0:
                raise TimeoutError("context-deadline-exceeded")
            event_type, value = receive_json(socket, remaining)
            context_event_types[event_type] += 1
            result = value.get("result") if isinstance(value, dict) else None
            if isinstance(result, dict):
                status = result.get("status")
                if isinstance(status, dict) and status.get("code") not in (None, 0):
                    raise ValueError("provider-error")
            if event_type == "contextCreated":
                break
        context_setup_ms = round((time.perf_counter() - context_started) * 1000, 1)
        runs: list[dict[str, Any]] = []
        for ordinal in (1, 2):
            observation = TurnObservation(ordinal, output_dir / f"{profile.id}-ws-{ordinal}.pcm", timeout)
            socket.send(
                json.dumps(
                    {"send_text": {"text": FIXTURE_TEXT, "flush_context": {}}, "contextId": context_id},
                    separators=(",", ":"),
                )
            )
            while True:
                message = socket.recv(timeout=observation.remaining_timeout())
                observation.observe_transport(message)
                if not isinstance(message, str):
                    raise ValueError("unexpected-binary-inworld-message")
                value = json.loads(message)
                if not isinstance(value, dict):
                    raise ValueError("unexpected-inworld-message")
                result = value.get("result")
                if not isinstance(result, dict) or result.get("contextId") != context_id:
                    continue
                status = result.get("status")
                if isinstance(status, dict) and status.get("code") not in (None, 0):
                    raise ValueError("provider-error")
                if "audioChunk" in result:
                    event_type = "audioChunk"
                    audio_chunk = result.get("audioChunk")
                    if isinstance(audio_chunk, dict):
                        observation.observe_alignment(audio_chunk)
                        observation.observe_audio(decode_base64_audio(audio_chunk.get("audioContent")))
                elif "flushCompleted" in result:
                    event_type = "flushCompleted"
                else:
                    event_type = "json"
                observation.event_types[event_type] += 1
                if event_type == "flushCompleted":
                    break
            runs.append(observation.finish())
        socket.send(json.dumps({"close_context": {}, "contextId": context_id}, separators=(",", ":")))
    finally:
        socket.close()
    return profile_report(profile, connection_ms, context_setup_ms, runs, dict(context_event_types))


def profile_report(
    profile: WebSocketProfile,
    connection_ms: float,
    context_setup_ms: float | None,
    runs: list[dict[str, Any]],
    setup_event_types: dict[str, int] | None = None,
) -> dict[str, Any]:
    return {
        "id": profile.id,
        "provider": profile.provider,
        "model": profile.model,
        "voice": profile.voice,
        "alignment": profile.alignment,
        "sampleRate": SAMPLE_RATE,
        "connectionSetupMs": connection_ms,
        "providerContextSetupMs": context_setup_ms,
        "setupEventTypes": setup_event_types or {},
        "runs": runs,
    }


RUNNERS: dict[str, Callable[[WebSocketProfile, str, Path, float], dict[str, Any]]] = {
    "cartesia": run_cartesia,
    "deepgram": run_deepgram,
    "inworld": run_inworld,
}


def profile_failure(profile: WebSocketProfile, error: Exception) -> dict[str, Any]:
    status = "websocket-handshake-rejected" if isinstance(error, InvalidStatus) else sanitize_transport_error(error)
    return {
        "id": profile.id,
        "provider": profile.provider,
        "model": profile.model,
        "voice": profile.voice,
        "alignment": profile.alignment,
        "sampleRate": SAMPLE_RATE,
        "status": status,
        "runs": [],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--report-file", type=Path, help="Immutable report path; defaults to a UTC-stamped file")
    parser.add_argument("--profiles", default="", help="Comma-separated profile IDs")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    if not 0 < args.timeout <= 120:
        parser.error("timeout must be greater than zero and at most 120 seconds")

    requested = {item.strip() for item in args.profiles.split(",") if item.strip()}
    known = {profile.id for profile in PROFILES}
    if requested - known:
        parser.error(f"unknown profiles={sorted(requested - known)}")
    selected = [profile for profile in PROFILES if not requested or profile.id in requested]
    credentials, sources = resolve_credentials(args.credentials)
    report: dict[str, Any] = {
        "schemaVersion": 1,
        "checkedAtUtc": utc_now(),
        "purpose": "bounded-live-hosted-stock-tts-persistent-websocket-qualification",
        "fixtureText": FIXTURE_TEXT,
        "containsCredentialValues": False,
        "measurementClock": "time.perf_counter",
        "connectionSetupExcludedFromTurnLatency": True,
        "credentialSources": sources,
        "profiles": [],
    }
    failures = 0
    for profile in selected:
        key = credentials.get(profile.provider)
        if not key:
            report["profiles"].append({"id": profile.id, "provider": profile.provider, "status": "missing-credential", "runs": []})
            failures += 1
            continue
        try:
            result = RUNNERS[profile.provider](profile, key, args.output_dir, args.timeout)
        except Exception as error:
            result = profile_failure(profile, error)
        if not result.get("runs") or any(run.get("status") != "usable-audio" for run in result.get("runs", [])):
            failures += 1
        report["profiles"].append(result)
    report_path = args.report_file or (
        args.output_dir / f"provider-websocket-warm-{report['checkedAtUtc'].replace(':', '').replace('-', '')}.json"
    )
    if report_path.exists():
        parser.error(f"report file already exists: {report_path}")
    report["reportFile"] = str(report_path)
    atomic_json(report_path, report)
    summary = {
        "report": str(report_path),
        "profiles": [
            {
                "id": profile["id"],
                "status": profile.get("status", "complete"),
                "connectionSetupMs": profile.get("connectionSetupMs"),
                "turns": [
                    {
                        "ordinal": run.get("ordinal"),
                        "status": run.get("status"),
                        "firstPcmMs": run.get("requestToFirstDecodedPcmMs"),
                        "first20msMs": run.get("requestToFirst20msPcmMs"),
                        "completeMs": run.get("requestToCompleteMs"),
                        "audioEvents": run.get("audioEvents"),
                    }
                    for run in profile.get("runs", [])
                ],
            }
            for profile in report["profiles"]
        ],
    }
    print(json.dumps(summary, indent=2))
    return 1 if args.strict and failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
