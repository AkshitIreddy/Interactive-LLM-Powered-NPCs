#!/usr/bin/env python3
"""Production local embedding worker entrypoint."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Any

from framing import EndOfStream, FrameWriter, FramingError, read_frame
from protocol import WorkerController


def install_network_denial() -> None:
    """The inference worker never downloads models or opens a socket."""

    def deny(event: str, _arguments: tuple[Any, ...]) -> None:
        if event == "socket.__new__" or event.startswith("socket.connect") or event.startswith("socket.getaddrinfo"):
            raise PermissionError("local embedding worker network access is disabled")

    sys.addaudithook(deny)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Interactive NPCs local BGE embedding worker")
    parser.add_argument("--launch-nonce", required=True)
    parser.add_argument("--worker-instance-id", required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.launch_nonce or len(args.launch_nonce.encode("utf-8")) > 256:
        return 2
    install_network_denial()
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
    if controller.scheduler is not None and not controller.scheduler.stop():
        # Do not race backend disposal against an uncooperative inference call.
        # The batch thread is daemonized, so process teardown is the hard
        # supervision boundary.
        return 6
    controller.backend.unload()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
