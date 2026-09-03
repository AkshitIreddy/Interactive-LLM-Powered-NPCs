#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import struct
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.backend import FixtureMoonshineBackend, MoonshineBridgeBackend
from npc_local_stt.contract import PROTOCOL_VERSION, PcmFrame, ProtocolFault
from npc_local_stt.framing import read_control_frame, write_control_frame
from npc_local_stt.ingress import PcmFrameStore, PcmReaderThread, open_inherited_pcm_stream
from npc_local_stt.runtime import WorkerRuntime


def _request(instance: str, sequence: int, operation: str, payload: dict[str, Any], generation: int = 0) -> dict[str, Any]:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "workerInstanceId": instance,
        "requestId": f"self-test-{sequence}",
        "sequence": sequence,
        "generation": generation,
        "deadlineUnixMs": int(time.time() * 1000) + 10_000,
        "operation": operation,
        "payload": payload,
    }


def fixture_self_test() -> dict[str, Any]:
    instance = "fixture-worker-1"
    nonce = "fixture-launch-nonce"
    backend = FixtureMoonshineBackend("Can you hear me clearly")
    runtime = WorkerRuntime(worker_instance_id=instance, launch_nonce=nonce, backend=backend)
    operations: list[list[dict[str, Any]]] = []
    operations.append(
        runtime.handle(
            _request(
                instance,
                1,
                "handshake",
                {
                    "launchNonce": nonce,
                    "supervisorPid": 1,
                    "supervisorExecutableSha256": "0" * 64,
                },
            )
        )
    )
    operations.append(runtime.handle(_request(instance, 2, "warm", {})))
    operations.append(
        runtime.handle(
            _request(
                instance,
                3,
                "load",
                {
                    "packId": "fixture.moonshine",
                    "revision": "fixture-v1",
                    "modelLeaseId": "lease-fixture-1",
                },
            )
        )
    )
    operations.append(
        runtime.handle(
            _request(
                instance,
                4,
                "session_start",
                {
                    "sessionId": "session-1",
                    "inputSource": "supervisor_microphone_pcm",
                    "turnMode": "push_to_talk",
                    "sampleRateHz": 16000,
                    "channels": 1,
                    "sampleFormat": "pcm_s16le",
                    "wordTimestamps": True,
                },
            )
        )
    )
    samples = [int(math.sin(i * 2.0 * math.pi * 220.0 / 16000.0) * 9000) for i in range(4800)]
    pcm = struct.pack(f"<{len(samples)}h", *samples)
    frame = PcmFrame.parse(
        {
            "protocolVersion": PROTOCOL_VERSION,
            "workerInstanceId": instance,
            "sessionId": "session-1",
            "generation": 0,
            "chunkId": "chunk-0",
            "chunkIndex": 0,
            "sampleRateHz": 16000,
            "channels": 1,
            "sampleFormat": "pcm_s16le",
            "sampleCount": len(samples),
            "capturedAtQpc": 1000,
            "qpcFrequencyHz": 10000000,
        },
        pcm,
    )
    runtime.feed_pcm_for_test(frame)
    operations.append(runtime.handle(_request(instance, 5, "pcm_commit", {"sessionId": "session-1", "chunkId": "chunk-0"})))
    operations.append(
        runtime.handle(
            _request(
                instance,
                6,
                "session_end",
                {"sessionId": "session-1", "reason": "ptt_key_up"},
            )
        )
    )
    events = [event for batch in operations for event in batch]
    errors = [event for event in events if event["event"] == "error"]
    finals = [event for event in events if event["event"] == "final_transcript"]
    partials = [event for event in events if event["event"] == "partial_transcript"]
    if errors or not finals or not partials:
        raise RuntimeError("fixture self-test did not produce the required event flow")
    return {
        "protocol": PROTOCOL_VERSION,
        "passed": True,
        "fixtureOnly": True,
        "partialEvents": len(partials),
        "finalEvents": len(finals),
        "modelDownload": "not_run",
        "modelInference": "not_run",
    }


def _build_runtime(fixture: bool) -> tuple[WorkerRuntime, PcmReaderThread | None]:
    instance = os.environ.get("NPC_STT_WORKER_INSTANCE_ID", "")
    nonce = os.environ.get("NPC_STT_LAUNCH_NONCE", "")
    if not instance or not nonce:
        raise ProtocolFault("invalid_launch", "supervisor instance identity and launch nonce are required")
    store = PcmFrameStore()
    reader: PcmReaderThread | None = None
    pcm_stream = open_inherited_pcm_stream()
    if pcm_stream is not None:
        reader = PcmReaderThread(pcm_stream, store)
        reader.start()
    if fixture:
        backend = FixtureMoonshineBackend()
    else:
        bridge = os.environ.get("NPC_STT_BRIDGE_PATH", "")
        model = os.environ.get("NPC_STT_MODEL_PATH", "")
        if not bridge or not model:
            raise ProtocolFault("invalid_launch", "verified bridge and model locations are required")
        backend = MoonshineBridgeBackend(Path(bridge), Path(model))
    return WorkerRuntime(worker_instance_id=instance, launch_nonce=nonce, backend=backend, pcm_store=store), reader


def run_worker(fixture: bool) -> int:
    runtime, _reader = _build_runtime(fixture)
    while not runtime.stopped:
        try:
            raw = read_control_frame(sys.stdin.buffer)
            if raw is None:
                break
            for event in runtime.handle(raw):
                write_control_frame(sys.stdout.buffer, event)
        except ProtocolFault as fault:
            fallback = {
                "protocolVersion": PROTOCOL_VERSION,
                "workerInstanceId": runtime.worker_instance_id,
                "requestId": "invalid-frame",
                "sequence": 0,
                "generation": 0,
                "eventIndex": 0,
                "event": "error",
                "terminal": True,
                "payload": {},
                "error": {
                    "code": fault.code,
                    "message": fault.safe_message,
                    "retryable": fault.retryable,
                    "details": fault.details,
                },
            }
            write_control_frame(sys.stdout.buffer, fallback)
            if fault.code == "invalid_frame":
                return 2
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Supervised optional local STT worker")
    parser.add_argument("--fixture", action="store_true", help="use deterministic fixture backend")
    parser.add_argument("--self-test-fixture", action="store_true", help="run protocol fixture self-test")
    args = parser.parse_args()
    if args.self_test_fixture:
        print(json.dumps(fixture_self_test(), indent=2, sort_keys=True))
        return 0
    return run_worker(args.fixture)


if __name__ == "__main__":
    raise SystemExit(main())
