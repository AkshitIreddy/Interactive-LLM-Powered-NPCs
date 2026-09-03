"""Production local-LLM worker entrypoint (weights are never bundled)."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from .constants import PROTOCOL_VERSION
from .errors import LocalLlmError
from .framing import FrameIO
from .manifest import ModelPack, RuntimeBundle
from .protocol import WorkerController


def serve(
    *,
    manifest_path: Path,
    runtime_bundle_path: Path,
    launch_nonce: str,
    worker_instance_id: str,
) -> int:
    model_pack = ModelPack.load(manifest_path)
    runtime_bundle = RuntimeBundle.load(runtime_bundle_path)
    frames = FrameIO(sys.stdin.buffer, sys.stdout.buffer)
    controller = WorkerController(
        launch_nonce=launch_nonce,
        worker_instance_id=worker_instance_id,
        model_pack=model_pack,
        runtime_bundle=runtime_bundle,
        emit=frames.write,
    )
    while controller.lifecycle != "stopped":
        try:
            request = frames.read()
        except LocalLlmError as error:
            frames.write(
                {
                    "protocol_version": PROTOCOL_VERSION,
                    "worker_instance_id": worker_instance_id,
                    "request_id": "protocol-error",
                    "sequence": 0,
                    "generation": controller.generation,
                    "event_index": 0,
                    "event": "error",
                    "terminal": True,
                    "payload": {},
                    "error": error.event_error(),
                }
            )
            return 2
        if request is None:
            break
        controller.handle(request)
    if controller.transport is not None and hasattr(controller.transport, "stop"):
        getattr(controller.transport, "stop")()
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Pinned Qwen3 local LLM Worker Control v1 adapter")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--runtime-bundle", type=Path, required=True)
    parser.add_argument("--worker-instance-id", default="worker-qwen3-4b-local")
    options = parser.parse_args(argv)
    launch_nonce = os.environ.get("NPC_WORKER_LAUNCH_NONCE")
    if launch_nonce is None or not launch_nonce:
        print("NPC_WORKER_LAUNCH_NONCE is required", file=sys.stderr)
        return 2
    try:
        return serve(
            manifest_path=options.manifest.resolve(strict=True),
            runtime_bundle_path=options.runtime_bundle.resolve(strict=True),
            launch_nonce=launch_nonce,
            worker_instance_id=options.worker_instance_id,
        )
    except LocalLlmError as error:
        print(f"local LLM worker failed: {error.code}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
