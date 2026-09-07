#!/usr/bin/env python3
"""Live, redacted cancellation and same-socket reuse checks for TTS fallbacks."""

from __future__ import annotations

import argparse
import json
import os
import time
import uuid
from pathlib import Path
from typing import Any

from websockets.sync.client import ClientConnection, connect

from provider_timing_alternatives import (
    DEFAULT_OUTPUT,
    FIXTURE_TEXT,
    atomic_json,
    resolve_credentials,
    sanitize_transport_error,
    utc_now,
)
from provider_websocket_warm import SAMPLE_RATE, decode_base64_audio


def ws_connect(uri: str, headers: dict[str, str], timeout: float) -> tuple[ClientConnection, float]:
    started = time.perf_counter()
    socket = connect(
        uri,
        additional_headers=headers,
        open_timeout=timeout,
        close_timeout=min(timeout, 10.0),
        ping_interval=20,
        ping_timeout=20,
        max_size=4 * 1024 * 1024,
        compression=None,
    )
    return socket, round((time.perf_counter() - started) * 1000, 1)


def remaining_timeout(deadline: float) -> float:
    remaining = deadline - time.perf_counter()
    if remaining <= 0:
        raise TimeoutError("operation-deadline-exceeded")
    return remaining


def receive_dict(socket: ClientConnection, deadline: float) -> dict[str, Any]:
    message = socket.recv(timeout=remaining_timeout(deadline))
    if not isinstance(message, str):
        raise ValueError("unexpected-binary-message")
    value = json.loads(message)
    if not isinstance(value, dict):
        raise ValueError("unexpected-json-message")
    return value


def cartesia_generation(socket: ClientConnection, context_id: str, voice_id: str) -> None:
    socket.send(
        json.dumps(
            {
                "model_id": "sonic-3.6",
                "transcript": FIXTURE_TEXT,
                "voice": {"mode": "id", "id": voice_id},
                "language": "en",
                "context_id": context_id,
                "output_format": {"container": "raw", "encoding": "pcm_s16le", "sample_rate": SAMPLE_RATE},
                "add_timestamps": False,
                "add_phoneme_timestamps": False,
                "continue": False,
            },
            separators=(",", ":"),
        )
    )


def probe_cartesia(key: str, timeout: float) -> dict[str, Any]:
    socket, connection_ms = ws_connect(
        "wss://api.cartesia.ai/tts/websocket?cartesia_version=2026-03-01",
        {"X-API-Key": key},
        timeout,
    )
    voice_id = os.environ.get("CARTESIA_VOICE_ID", "a0e99841-438c-4a64-b679-ae501e7d6091")
    try:
        context_id = f"cancel-check-{uuid.uuid4()}"
        started = time.perf_counter()
        cancel_deadline = started + timeout
        cartesia_generation(socket, context_id, voice_id)
        pre_cancel_bytes = 0
        post_cancel_bytes = 0
        cancel_sent_at: float | None = None
        done_at: float | None = None
        messages_after_cancel = 0
        while True:
            value = receive_dict(socket, cancel_deadline)
            if value.get("context_id") != context_id:
                continue
            if value.get("type") == "error":
                raise ValueError("provider-error")
            if value.get("type") == "chunk":
                audio = decode_base64_audio(value.get("data"))
                if cancel_sent_at is None:
                    pre_cancel_bytes += len(audio)
                    if audio:
                        cancel_sent_at = time.perf_counter()
                        socket.send(json.dumps({"context_id": context_id, "cancel": True}, separators=(",", ":")))
                else:
                    post_cancel_bytes += len(audio)
                    messages_after_cancel += 1
            elif cancel_sent_at is not None:
                messages_after_cancel += 1
            if value.get("type") == "done" or value.get("done") is True:
                done_at = time.perf_counter()
                break

        reuse_id = f"reuse-check-{uuid.uuid4()}"
        reuse_started = time.perf_counter()
        reuse_deadline = reuse_started + timeout
        cartesia_generation(socket, reuse_id, voice_id)
        reuse_pcm = 0
        reuse_first_audio: float | None = None
        while True:
            value = receive_dict(socket, reuse_deadline)
            if value.get("context_id") != reuse_id:
                continue
            if value.get("type") == "error":
                raise ValueError("provider-error")
            if value.get("type") == "chunk":
                audio = decode_base64_audio(value.get("data"))
                if audio and reuse_first_audio is None:
                    reuse_first_audio = time.perf_counter()
                reuse_pcm += len(audio)
            if value.get("type") == "done" or value.get("done") is True:
                break
        reuse_done = time.perf_counter()
        return {
            "status": "complete",
            "credentialTarget": "api.cartesia.ai",
            "endpoint": "wss://api.cartesia.ai/tts/websocket",
            "model": "sonic-3.6",
            "voice": "Greg",
            "connectionSetupMs": connection_ms,
            "activeCancel": {
                "cancelSentAfterFirstAudio": cancel_sent_at is not None,
                "firstAudioMs": round((cancel_sent_at - started) * 1000, 1) if cancel_sent_at else None,
                "pcmBytesBeforeCancel": pre_cancel_bytes,
                "pcmBytesAfterCancel": post_cancel_bytes,
                "messagesAfterCancel": messages_after_cancel,
                "terminalDoneReceived": done_at is not None,
                "cancelToDoneMs": round((done_at - cancel_sent_at) * 1000, 1) if cancel_sent_at and done_at else None,
                "observedSemantics": "active-generation-continued-to-done" if post_cancel_bytes else "no-post-cancel-audio-observed",
            },
            "newContextSameSocket": {
                "usableAudio": reuse_pcm > 0,
                "pcmBytes": reuse_pcm,
                "firstAudioMs": round((reuse_first_audio - reuse_started) * 1000, 1) if reuse_first_audio else None,
                "completeMs": round((reuse_done - reuse_started) * 1000, 1),
            },
        }
    finally:
        socket.close()


def inworld_create(socket: ClientConnection, context_id: str, timeout: float) -> float:
    started = time.perf_counter()
    deadline = started + timeout
    socket.send(
        json.dumps(
            {
                "create": {
                    "voiceId": "Dennis",
                    "modelId": "inworld-tts-2-flash",
                    "audioConfig": {"audioEncoding": "PCM", "sampleRateHertz": SAMPLE_RATE},
                    "bufferCharThreshold": 100,
                    "autoMode": True,
                    "applyTextNormalization": "OFF",
                    "timestampType": "WORD",
                    "timestampTransportStrategy": "ASYNC",
                },
                "contextId": context_id,
            },
            separators=(",", ":"),
        )
    )
    while True:
        value = receive_dict(socket, deadline)
        result = value.get("result")
        if isinstance(result, dict) and result.get("contextId") == context_id and "contextCreated" in result:
            return round((time.perf_counter() - started) * 1000, 1)


def inworld_send(socket: ClientConnection, context_id: str) -> None:
    socket.send(
        json.dumps(
            {"send_text": {"text": FIXTURE_TEXT, "flush_context": {}}, "contextId": context_id},
            separators=(",", ":"),
        )
    )


def probe_inworld(key: str, timeout: float) -> dict[str, Any]:
    socket, connection_ms = ws_connect(
        "wss://api.inworld.ai/tts/v1/voice:streamBidirectional",
        {"Authorization": f"Basic {key}"},
        timeout,
    )
    try:
        context_id = f"close-check-{uuid.uuid4()}"
        context_ms = inworld_create(socket, context_id, timeout)
        started = time.perf_counter()
        close_deadline = started + timeout
        inworld_send(socket, context_id)
        pre_close_bytes = 0
        post_close_bytes = 0
        close_sent_at: float | None = None
        closed_at: float | None = None
        saw_flush_completed = False
        messages_after_close = 0
        while True:
            value = receive_dict(socket, close_deadline)
            result = value.get("result")
            if not isinstance(result, dict) or result.get("contextId") != context_id:
                continue
            status = result.get("status")
            if isinstance(status, dict) and status.get("code") not in (None, 0):
                raise ValueError("provider-error")
            audio_chunk = result.get("audioChunk")
            if isinstance(audio_chunk, dict):
                audio = decode_base64_audio(audio_chunk.get("audioContent"))
                if close_sent_at is None:
                    pre_close_bytes += len(audio)
                    if audio:
                        close_sent_at = time.perf_counter()
                        socket.send(json.dumps({"close_context": {}, "contextId": context_id}, separators=(",", ":")))
                else:
                    post_close_bytes += len(audio)
                    messages_after_close += 1
            elif close_sent_at is not None:
                messages_after_close += 1
            saw_flush_completed = saw_flush_completed or "flushCompleted" in result
            if "contextClosed" in result:
                closed_at = time.perf_counter()
                break

        reuse_id = f"reuse-check-{uuid.uuid4()}"
        reuse_context_ms = inworld_create(socket, reuse_id, timeout)
        reuse_started = time.perf_counter()
        reuse_deadline = reuse_started + timeout
        inworld_send(socket, reuse_id)
        reuse_pcm = 0
        reuse_first_audio: float | None = None
        while True:
            value = receive_dict(socket, reuse_deadline)
            result = value.get("result")
            if not isinstance(result, dict) or result.get("contextId") != reuse_id:
                continue
            audio_chunk = result.get("audioChunk")
            if isinstance(audio_chunk, dict):
                audio = decode_base64_audio(audio_chunk.get("audioContent"))
                if audio and reuse_first_audio is None:
                    reuse_first_audio = time.perf_counter()
                reuse_pcm += len(audio)
            if "flushCompleted" in result:
                break
        reuse_done = time.perf_counter()
        socket.send(json.dumps({"close_context": {}, "contextId": reuse_id}, separators=(",", ":")))
        return {
            "status": "complete",
            "credentialTarget": "api.inworld.ai",
            "endpoint": "wss://api.inworld.ai/tts/v1/voice:streamBidirectional",
            "model": "inworld-tts-2-flash",
            "voice": "Dennis",
            "connectionSetupMs": connection_ms,
            "firstContextSetupMs": context_ms,
            "closeDuringActiveGeneration": {
                "closeSentAfterFirstAudio": close_sent_at is not None,
                "firstAudioMs": round((close_sent_at - started) * 1000, 1) if close_sent_at else None,
                "pcmBytesBeforeClose": pre_close_bytes,
                "pcmBytesAfterClose": post_close_bytes,
                "messagesAfterClose": messages_after_close,
                "flushCompletedReceived": saw_flush_completed,
                "contextClosedReceived": closed_at is not None,
                "closeToContextClosedMs": round((closed_at - close_sent_at) * 1000, 1) if close_sent_at and closed_at else None,
                "observedSemantics": "close-flushed-active-generation-before-context-closed" if post_close_bytes else "no-post-close-audio-observed",
            },
            "newContextSameSocket": {
                "contextSetupMs": reuse_context_ms,
                "usableAudio": reuse_pcm > 0,
                "pcmBytes": reuse_pcm,
                "firstAudioMs": round((reuse_first_audio - reuse_started) * 1000, 1) if reuse_first_audio else None,
                "completeMs": round((reuse_done - reuse_started) * 1000, 1),
            },
        }
    finally:
        socket.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--report-file", type=Path)
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()

    if not 0 < args.timeout <= 120:
        parser.error("timeout must be greater than zero and at most 120 seconds")

    credentials, sources = resolve_credentials(args.credentials)
    if "cartesia" not in credentials or "inworld" not in credentials:
        parser.error("Cartesia and Inworld credentials are required")
    checked = utc_now()
    report_path = args.report_file or args.output_dir / f"provider-cancellation-reuse-{checked.replace(':', '').replace('-', '')}.json"
    if report_path.exists():
        parser.error(f"report file already exists: {report_path}")
    try:
        cartesia = probe_cartesia(credentials["cartesia"], args.timeout)
    except Exception as error:
        cartesia = {"status": sanitize_transport_error(error)}
    try:
        inworld = probe_inworld(credentials["inworld"], args.timeout)
    except Exception as error:
        inworld = {"status": sanitize_transport_error(error)}
    report = {
        "schemaVersion": 1,
        "checkedAtUtc": checked,
        "purpose": "bounded-live-cancellation-and-same-socket-reuse-qualification",
        "fixtureText": FIXTURE_TEXT,
        "containsCredentialValues": False,
        "credentialSources": {provider: sources[provider] for provider in ("cartesia", "inworld")},
        "reportFile": str(report_path),
        "cartesia": cartesia,
        "inworld": inworld,
    }
    atomic_json(report_path, report)
    print(
        json.dumps(
            {
                "report": str(report_path),
                "cartesia": report["cartesia"],
                "inworld": report["inworld"],
            },
            indent=2,
        )
    )
    return 0 if cartesia.get("status") == "complete" and inworld.get("status") == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
