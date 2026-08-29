#!/usr/bin/env python3
"""Minimal Worker Control v1 stdio client for the llama.cpp fixture."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

WORKERS = Path(__file__).resolve().parents[1]
STUBS = WORKERS / "stubs"
sys.path.insert(0, str(STUBS))

from framing import FrameWriter, read_frame  # noqa: E402


def request(sequence: int, operation: str, payload: dict, generation: int = 0) -> dict:
    return {
        "protocol_version": "1.0",
        "worker_instance_id": "" if operation == "handshake" else "worker-llamacpp-stub",
        "request_id": f"example-{sequence}",
        "sequence": sequence,
        "generation": generation,
        "deadline_unix_ms": 0,
        "operation": operation,
        "payload": payload,
    }


def exchange(writer: FrameWriter, process: subprocess.Popen[bytes], message: dict) -> list[dict]:
    writer.write(message)
    events = []
    while True:
        event = read_frame(process.stdout)
        events.append(event)
        if event["request_id"] == message["request_id"] and event["terminal"]:
            return events


def main() -> int:
    process = subprocess.Popen(
        [
            sys.executable,
            str(STUBS / "worker.py"),
            "--descriptor",
            str(WORKERS / "packs" / "llamacpp.stub-pack.json"),
            "--launch-nonce",
            "example-nonce",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert process.stdin is not None and process.stdout is not None
    writer = FrameWriter(process.stdin)
    messages = [
        request(1, "handshake", {"launch_nonce": "example-nonce", "supervisor": "example-client"}),
        request(2, "warm", {}),
        request(3, "load", {"model_id": "fixture.llm.echo-v1", "lease_id": "example-lease"}),
        request(4, "infer", {"prompt": "Is anyone there?", "max_tokens": 32}),
        request(5, "shutdown", {}),
    ]
    try:
        for message in messages:
            for event in exchange(writer, process, message):
                print(json.dumps(event, indent=2, ensure_ascii=False))
    finally:
        process.stdin.close()
        process.wait(timeout=5)
    return process.returncode or 0


if __name__ == "__main__":
    raise SystemExit(main())
