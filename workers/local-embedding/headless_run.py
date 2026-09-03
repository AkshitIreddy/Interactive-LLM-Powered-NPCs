#!/usr/bin/env python3
"""Run one Windows qualification command with no visible console window."""

from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stdout", type=Path, required=True)
    parser.add_argument("--stderr", type=Path, required=True)
    parser.add_argument("--env", action="append", default=[])
    parser.add_argument("command", nargs=argparse.REMAINDER)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        raise SystemExit("headless command is required")
    environment = os.environ.copy()
    for assignment in args.env:
        if "=" not in assignment:
            raise SystemExit("--env must be NAME=VALUE")
        name, value = assignment.split("=", 1)
        if not name or "\x00" in name or "\x00" in value:
            raise SystemExit("--env contains an invalid name or value")
        environment[name] = value
    args.stdout.parent.mkdir(parents=True, exist_ok=True)
    args.stderr.parent.mkdir(parents=True, exist_ok=True)
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    if os.name != "nt" or creationflags == 0:
        raise SystemExit("CREATE_NO_WINDOW is unavailable; refusing Windows qualification launch")
    with args.stdout.open("wb") as stdout, args.stderr.open("wb") as stderr:
        completed = subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
            stdout=stdout,
            stderr=stderr,
            check=False,
            creationflags=creationflags,
            close_fds=True,
            env=environment,
        )
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
