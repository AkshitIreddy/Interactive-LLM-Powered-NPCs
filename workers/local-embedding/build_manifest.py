#!/usr/bin/env python3
"""Emit the final schema-shaped BGE manifest after gated hashes exist."""

from __future__ import annotations

import argparse
import json
import os
import tempfile
from pathlib import Path

from manifest_builder import build_manifest


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build the immutable BGE local-embedding manifest")
    parser.add_argument("--tokenizer-sha256", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--planning-resident-ram-bytes", type=int)
    parser.add_argument("--planning-load-millis", type=int)
    parser.add_argument("--planning-hardware")
    return parser.parse_args(argv)


def _write_atomic(path: Path, value: dict[str, object]) -> None:
    path = path.resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".part", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        Path(temporary).replace(path)
    except Exception:
        try:
            Path(temporary).unlink()
        except FileNotFoundError:
            pass
        raise


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        manifest = build_manifest(
            tokenizer_sha256=args.tokenizer_sha256,
            planning_resident_ram_bytes=args.planning_resident_ram_bytes,
            planning_load_millis=args.planning_load_millis,
            planning_hardware=args.planning_hardware,
        )
        _write_atomic(args.out, manifest)
        return 0
    except (OSError, ValueError) as exc:
        print(json.dumps({"ok": False, "code": "manifest_build_failed", "message": str(exc)}))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
