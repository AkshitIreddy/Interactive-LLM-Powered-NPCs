"""Streaming digests and canonical JSON helpers."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
from typing import Any, BinaryIO, Callable

from .errors import LocalLlmError

CHUNK_BYTES = 4 * 1_048_576


def canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_stream(
    stream: BinaryIO,
    *,
    maximum_bytes: int | None = None,
    cancelled: Callable[[], bool] | None = None,
) -> tuple[str, int]:
    digest = hashlib.sha256()
    total = 0
    while True:
        if cancelled is not None and cancelled():
            raise LocalLlmError("cancelled", "operation was cancelled")
        block = stream.read(CHUNK_BYTES)
        if not block:
            break
        total += len(block)
        if maximum_bytes is not None and total > maximum_bytes:
            raise LocalLlmError("size_mismatch", "artifact exceeded its declared byte length")
        digest.update(block)
    return digest.hexdigest(), total


def sha256_file(
    path: Path,
    *,
    expected_size: int | None = None,
    cancelled: Callable[[], bool] | None = None,
) -> str:
    try:
        stat = path.stat(follow_symlinks=False)
    except OSError as error:
        raise LocalLlmError("artifact_unavailable", "verified artifact is unavailable") from error
    if not stat or not path.is_file() or path.is_symlink():
        raise LocalLlmError("unsafe_artifact", "artifact must be a regular non-link file")
    if expected_size is not None and stat.st_size != expected_size:
        raise LocalLlmError("size_mismatch", "artifact byte length does not match the manifest")
    with path.open("rb", buffering=0) as stream:
        digest, total = sha256_stream(stream, maximum_bytes=expected_size, cancelled=cancelled)
    if expected_size is not None and total != expected_size:
        raise LocalLlmError("size_mismatch", "artifact byte length does not match the manifest")
    return digest


def fsync_parent(path: Path) -> None:
    if os.name == "nt":
        return
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
