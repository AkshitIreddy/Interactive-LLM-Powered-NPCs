#!/usr/bin/env python3
"""Measure NVIDIA Magpie SynthesizeOnline with NVIDIA's official Riva client.

The output is a sanitized JSON timing receipt. It never stores provider text, PCM,
credential values, response metadata values, or exception details.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path
import statistics
import threading
import time
from typing import Any

import grpc


AUTHORITY = "grpc.nvcf.nvidia.com:443"
FUNCTION_ID = "877104f7-e885-42b9-8de8-f6e4c6303969"
METHOD = "/nvidia.riva.tts.RivaSpeechSynthesis/SynthesizeOnline"
VOICE = "Magpie-Multilingual.EN-US.Aria"
LANGUAGE = "en-US"
TEXT = "The north beacon is ready."
SAMPLE_RATE_HZ = 22_050


def load_riva_dependencies() -> None:
    global AudioEncoding, RequestId, RivaSpeechSynthesisStub, SynthesizeSpeechRequest, riva_client
    try:
        import riva.client as loaded_client
        from riva.client.proto.riva_audio_pb2 import AudioEncoding as LoadedAudioEncoding
        from riva.client.proto.riva_common_pb2 import RequestId as LoadedRequestId
        from riva.client.proto.riva_tts_pb2 import (
            SynthesizeSpeechRequest as LoadedSynthesizeSpeechRequest,
        )
        from riva.client.proto.riva_tts_pb2_grpc import (
            RivaSpeechSynthesisStub as LoadedRivaSpeechSynthesisStub,
        )
    except ModuleNotFoundError as error:
        raise RuntimeError("nvidia-riva-client dependency is unavailable") from error
    riva_client = loaded_client
    AudioEncoding = LoadedAudioEncoding
    RequestId = LoadedRequestId
    SynthesizeSpeechRequest = LoadedSynthesizeSpeechRequest
    RivaSpeechSynthesisStub = LoadedRivaSpeechSynthesisStub


def read_nvidia_key(path: Path) -> str:
    contents = path.read_text(encoding="utf-8-sig")
    nvidia_label_seen = False
    for raw_line in contents.splitlines():
        line = raw_line.strip()
        if not line or line.startswith(("#", ";", "//")):
            nvidia_label_seen = False
            continue
        lowered = line.casefold()
        if "nvidia" in lowered or "nim" in lowered:
            nvidia_label_seen = True
            for separator in (":", "="):
                if separator in line:
                    candidate = line.split(separator, 1)[1].strip().strip("\"'")
                    if len(candidate) >= 16 and not any(char.isspace() for char in candidate):
                        return candidate
            continue
        if nvidia_label_seen:
            candidate = line.strip("\"'")
            if len(candidate) >= 16 and not any(char.isspace() for char in candidate):
                return candidate
            nvidia_label_seen = False
    raise RuntimeError("nvidia_credential_not_found")


def percentile(values: list[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round((len(ordered) - 1) * fraction)))
    return round(ordered[index], 1)


def sanitized_error(error: grpc.RpcError) -> dict[str, object]:
    try:
        code = error.code().name
    except Exception:
        code = "UNKNOWN"
    return {"ok": False, "grpcStatus": code}


def measure(service: Any, run_index: int) -> dict[str, object]:
    started = time.perf_counter()
    responses = service.synthesize_online(
        text=TEXT,
        voice_name=VOICE,
        language_code=LANGUAGE,
        encoding=AudioEncoding.LINEAR_PCM,
        sample_rate_hz=SAMPLE_RATE_HZ,
    )
    call_created_ms = (time.perf_counter() - started) * 1000

    response_header_ms = None
    first_message_ms = None
    first_pcm_ms = None
    message_count = 0
    empty_message_count = 0
    pcm_bytes = 0
    pcm_hash = hashlib.sha256()
    cancel_timer = threading.Timer(60, responses.cancel)
    cancel_timer.daemon = True
    cancel_timer.start()
    try:
        # `_MultiThreadedRendezvous` is the public iterator returned by NVIDIA's
        # official wrapper and exposes gRPC initial metadata timing.
        responses.initial_metadata()
        response_header_ms = (time.perf_counter() - started) * 1000
        for response in responses:
            elapsed_ms = (time.perf_counter() - started) * 1000
            message_count += 1
            if first_message_ms is None:
                first_message_ms = elapsed_ms
            audio = bytes(response.audio)
            if not audio:
                empty_message_count += 1
                continue
            if first_pcm_ms is None:
                first_pcm_ms = elapsed_ms
            pcm_hash.update(audio)
            pcm_bytes += len(audio)
        total_ms = (time.perf_counter() - started) * 1000
        status = responses.code().name
        return {
            "ok": status == "OK" and pcm_bytes > 0,
            "runIndex": run_index,
            "callCreatedMs": round(call_created_ms, 1),
            "responseHeadersMs": round(response_header_ms, 1),
            "firstGrpcMessageMs": None if first_message_ms is None else round(first_message_ms, 1),
            "firstNonEmptyPcmMs": None if first_pcm_ms is None else round(first_pcm_ms, 1),
            "totalMs": round(total_ms, 1),
            "grpcStatus": status,
            "messageCount": message_count,
            "emptyMessageCount": empty_message_count,
            "pcmBytes": pcm_bytes,
            "pcmSha256": pcm_hash.hexdigest(),
        }
    except grpc.RpcError as error:
        result = sanitized_error(error)
        result.update(
            {
                "runIndex": run_index,
                "callCreatedMs": round(call_created_ms, 1),
                "responseHeadersMs": None
                if response_header_ms is None
                else round(response_header_ms, 1),
                "firstGrpcMessageMs": None
                if first_message_ms is None
                else round(first_message_ms, 1),
                "firstNonEmptyPcmMs": None if first_pcm_ms is None else round(first_pcm_ms, 1),
                "elapsedMs": round((time.perf_counter() - started) * 1000, 1),
                "messageCount": message_count,
                "emptyMessageCount": empty_message_count,
                "pcmBytes": pcm_bytes,
            }
        )
        return result
    finally:
        cancel_timer.cancel()


def measure_direct_variant(
    stub: Any,
    metadata: list[tuple[str, str]],
    name: str,
    request_id_present: bool,
    grpc_timeout_present: bool,
) -> dict[str, object]:
    request = SynthesizeSpeechRequest(
        text=TEXT,
        language_code=LANGUAGE,
        encoding=AudioEncoding.LINEAR_PCM,
        sample_rate_hz=SAMPLE_RATE_HZ,
        voice_name=VOICE,
        custom_dictionary="",
    )
    if request_id_present:
        request.id.CopyFrom(RequestId(value=f"official-ab:{name}:0:0"))

    started = time.perf_counter()
    responses = stub.SynthesizeOnline(
        request,
        metadata=metadata,
        timeout=60 if grpc_timeout_present else None,
    )
    call_created_ms = (time.perf_counter() - started) * 1000
    cancel_timer = None
    if not grpc_timeout_present:
        cancel_timer = threading.Timer(60, responses.cancel)
        cancel_timer.daemon = True
        cancel_timer.start()

    response_header_ms = None
    first_message_ms = None
    first_pcm_ms = None
    message_count = 0
    empty_message_count = 0
    pcm_bytes = 0
    initial_metadata_keys: list[str] = []
    upstream_service_time_ms = None
    try:
        initial_metadata = responses.initial_metadata()
        response_header_ms = (time.perf_counter() - started) * 1000
        initial_metadata_keys = sorted({item.key for item in initial_metadata})
        for item in initial_metadata:
            if item.key == "x-envoy-upstream-service-time" and str(item.value).isdigit():
                upstream_service_time_ms = int(item.value)
        for response in responses:
            elapsed_ms = (time.perf_counter() - started) * 1000
            message_count += 1
            if first_message_ms is None:
                first_message_ms = elapsed_ms
            audio = bytes(response.audio)
            if not audio:
                empty_message_count += 1
                continue
            if first_pcm_ms is None:
                first_pcm_ms = elapsed_ms
            pcm_bytes += len(audio)
        status = responses.code().name
        return {
            "name": name,
            "ok": status == "OK" and pcm_bytes > 0,
            "singleExplicitMetadataLayer": True,
            "requestIdPresent": request_id_present,
            "grpcTimeoutPresent": grpc_timeout_present,
            "callCreatedMs": round(call_created_ms, 1),
            "responseHeadersMs": round(response_header_ms, 1),
            "firstGrpcMessageMs": None if first_message_ms is None else round(first_message_ms, 1),
            "firstNonEmptyPcmMs": None if first_pcm_ms is None else round(first_pcm_ms, 1),
            "totalMs": round((time.perf_counter() - started) * 1000, 1),
            "grpcStatus": status,
            "messageCount": message_count,
            "emptyMessageCount": empty_message_count,
            "pcmBytes": pcm_bytes,
            "initialMetadataKeys": initial_metadata_keys,
            "upstreamServiceTimeMs": upstream_service_time_ms,
        }
    except grpc.RpcError as error:
        result = sanitized_error(error)
        result.update(
            {
                "name": name,
                "singleExplicitMetadataLayer": True,
                "requestIdPresent": request_id_present,
                "grpcTimeoutPresent": grpc_timeout_present,
                "callCreatedMs": round(call_created_ms, 1),
                "responseHeadersMs": None
                if response_header_ms is None
                else round(response_header_ms, 1),
                "firstGrpcMessageMs": None
                if first_message_ms is None
                else round(first_message_ms, 1),
                "firstNonEmptyPcmMs": None if first_pcm_ms is None else round(first_pcm_ms, 1),
                "elapsedMs": round((time.perf_counter() - started) * 1000, 1),
                "messageCount": message_count,
                "emptyMessageCount": empty_message_count,
                "pcmBytes": pcm_bytes,
                "initialMetadataKeys": initial_metadata_keys,
                "upstreamServiceTimeMs": upstream_service_time_ms,
            }
        )
        return result
    finally:
        if cancel_timer is not None:
            cancel_timer.cancel()


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=3, choices=range(1, 4), metavar="1..3")
    parser.add_argument("--wire-variants", action="store_true")
    args = parser.parse_args()
    if args.output.exists():
        raise RuntimeError("refusing_to_overwrite_metrics")
    if not args.credentials.is_file():
        raise RuntimeError("credential_file_missing")
    load_riva_dependencies()

    credential = read_nvidia_key(args.credentials)
    metadata = [
        ["function-id", FUNCTION_ID],
        ["authorization", f"Bearer {credential}"],
    ]
    auth_started = time.perf_counter()
    auth = riva_client.Auth(use_ssl=True, uri=AUTHORITY, metadata_args=metadata)
    service = riva_client.SpeechSynthesisService(auth)
    channel_ready_started = time.perf_counter()
    grpc.channel_ready_future(auth.channel).result(timeout=20)
    channel_ready_ms = (time.perf_counter() - channel_ready_started) * 1000
    client_setup_ms = (time.perf_counter() - auth_started) * 1000

    runs = [measure(service, index) for index in range(args.runs)]
    variants: list[dict[str, object]] = []
    if args.wire_variants:
        raw_channel = grpc.secure_channel(AUTHORITY, grpc.ssl_channel_credentials())
        try:
            grpc.channel_ready_future(raw_channel).result(timeout=20)
            stub = RivaSpeechSynthesisStub(raw_channel)
            explicit_metadata = [
                ("function-id", FUNCTION_ID),
                ("authorization", f"Bearer {credential}"),
            ]
            variants = [
                measure_direct_variant(stub, explicit_metadata, "no-id-no-timeout", False, False),
                measure_direct_variant(stub, explicit_metadata, "id-no-timeout", True, False),
                measure_direct_variant(stub, explicit_metadata, "no-id-timeout-60s", False, True),
                measure_direct_variant(stub, explicit_metadata, "id-timeout-60s", True, True),
            ]
        finally:
            raw_channel.close()
    first_pcm_values = [
        float(run["firstNonEmptyPcmMs"])
        for run in runs
        if run.get("firstNonEmptyPcmMs") is not None
    ]
    receipt: dict[str, object] = {
        "schemaVersion": 1,
        "test": "minimal-official-riva-synthesize-online-channel-reuse",
        "officialClientPackage": "nvidia-riva-client",
        "officialClientVersion": importlib.metadata.version("nvidia-riva-client"),
        "authority": AUTHORITY,
        "functionId": FUNCTION_ID,
        "method": METHOD,
        "voice": VOICE,
        "languageCode": LANGUAGE,
        "sampleRateHz": SAMPLE_RATE_HZ,
        "encoding": "LINEAR_PCM",
        "textSha256": hashlib.sha256(TEXT.encode("utf-8")).hexdigest(),
        "textCharacters": len(TEXT),
        "textWords": len(TEXT.split()),
        "requestIdPresent": False,
        "zeroShotDataPresent": False,
        "customDictionaryPresent": False,
        "channelReused": True,
        "clientSetupMs": round(client_setup_ms, 1),
        "channelReadyMs": round(channel_ready_ms, 1),
        "runs": runs,
        "wireVariants": variants,
        "summary": {
            "successfulRuns": sum(bool(run.get("ok")) for run in runs),
            "firstPcmMedianMs": round(statistics.median(first_pcm_values), 1)
            if first_pcm_values
            else None,
            "firstPcmMinMs": min(first_pcm_values) if first_pcm_values else None,
            "firstPcmMaxMs": max(first_pcm_values) if first_pcm_values else None,
            "firstPcmP95ObservedMs": percentile(first_pcm_values, 0.95),
        },
        "containsCredentialValues": False,
        "containsProviderResponseText": False,
        "audioPersisted": False,
        "playbackAttempted": False,
    }
    serialized = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if credential in serialized:
        raise RuntimeError("credential_redaction_invariant_failed")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as stream:
        stream.write(serialized)
    all_results = runs + variants
    print(json.dumps({"ok": all(bool(run.get("ok")) for run in all_results), "output": str(args.output)}))
    return 0 if all(bool(run.get("ok")) for run in all_results) else 1


if __name__ == "__main__":
    try:
        exit_code = main()
    except Exception as error:
        print(json.dumps({"ok": False, "errorCategory": type(error).__name__}))
        exit_code = 1
    raise SystemExit(exit_code)
