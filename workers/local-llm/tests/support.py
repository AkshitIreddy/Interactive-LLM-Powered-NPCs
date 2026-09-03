from __future__ import annotations

import hashlib
import io
import json
import tempfile
import time
from pathlib import Path, PurePosixPath
from typing import Iterator

from npc_local_llm.manifest import Artifact, ModelPack, RuntimeBundle
from npc_local_llm.server import ChatCompletionRequest, CompletionDelta, ServerConfig


ROOT = Path(__file__).resolve().parents[3]
MANIFEST = ROOT / "packaging/model-packs/qwen3-4b-instruct-2507-q4-k-m.json"
RUNTIME_BUNDLE = ROOT / "workers/local-llm/runtime-bundle.b10689.json"


def artifact(artifact_id: str, payload: bytes, destination: str) -> Artifact:
    return Artifact(
        artifact_id=artifact_id,
        kind="file",
        source_urls=(f"https://huggingface.co/test/repo/resolve/{'a' * 40}/{artifact_id}",),
        size_bytes=len(payload),
        sha256=hashlib.sha256(payload).hexdigest(),
        destination=PurePosixPath(destination),
    )


def fixture_pack(payloads: dict[str, bytes]) -> ModelPack:
    artifacts = tuple(artifact(key, value, f"files/{key}.bin") for key, value in payloads.items())
    return ModelPack(
        path=MANIFEST,
        pack_id="fixture-pack",
        revision="fixture-revision",
        runtime="fixture",
        runtime_abi="fixture-abi",
        minimum_runtime_revision="fixture",
        artifacts=artifacts,
        self_test={},
    )


class FakeTransport:
    def __init__(self, config: ServerConfig | None = None, *, structured: bool = False, hold: bool = False) -> None:
        self.config = config
        self.structured = structured
        self.hold = hold
        self.started = False
        self.cancelled = False

    def start(self):
        self.started = True
        return self.health()

    def stop(self):
        self.started = False

    def health(self):
        return {"status": "ok", "fixture": True}

    def cancel_active(self, grace_seconds: float = 1.0):
        self.cancelled = True
        return True

    def stream_chat(self, request: ChatCompletionRequest, cancelled) -> Iterator[CompletionDelta]:
        if self.structured:
            for value in ('{"schema_version":', '"npc_response.v1",', '"spoken_response":{"text":"Ready."}}'):
                if cancelled():
                    return
                yield CompletionDelta(text=value)
            yield CompletionDelta(finish_reason="stop", usage={"completion_tokens": 8})
            return
        yield CompletionDelta(text="Ready")
        if self.hold:
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline and not cancelled():
                time.sleep(0.01)
            if cancelled():
                return
        yield CompletionDelta(text=".", finish_reason="stop", usage={"completion_tokens": 2})
