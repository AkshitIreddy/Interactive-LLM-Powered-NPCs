#!/usr/bin/env python3
"""Hidden-process entrypoint for the optional local identity worker."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Any

from framing import EndOfStream, FrameWriter, FramingError, read_frame
from protocol import WorkerController


def deny_network() -> None:
    def audit(event: str, _arguments: tuple[Any, ...]) -> None:
        if event == "socket.__new__" or event.startswith(("socket.connect", "socket.getaddrinfo", "socket.bind")):
            raise PermissionError("identity worker network access is disabled")
    sys.addaudithook(audit)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Interactive NPCs local identity observation worker")
    parser.add_argument("--launch-nonce", required=True)
    parser.add_argument("--worker-instance-id", required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.launch_nonce or len(args.launch_nonce.encode("utf-8")) > 256 or not args.worker_instance_id:
        return 2
    deny_network()
    controller = WorkerController(
        FrameWriter(sys.stdout.buffer),
        launch_nonce=args.launch_nonce,
        worker_instance_id=args.worker_instance_id,
        manifest_path=args.manifest,
    )
    while not controller.should_exit.is_set():
        try:
            frame = read_frame(sys.stdin.buffer)
        except EndOfStream:
            break
        except FramingError as exc:
            controller.protocol_frame_error(str(exc))
            return 2
        controller.handle(frame)
    if controller.scheduler is not None:
        controller.scheduler.stop()
    controller.backend.unload()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

