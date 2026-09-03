"""Repository AI-model coordination lock.

The user-owned coordination file is authoritative even for a CPU model run.
Every real qualification must transition exact `no -> yes -> no` while holding
an advisory file lock.  Fixture tests use a temporary file.
"""

from __future__ import annotations

import contextlib
import os
from pathlib import Path
from typing import BinaryIO, Iterator


class GpuLockBusy(RuntimeError):
    pass


@contextlib.contextmanager
def externally_owned_ai_model_lock(path: Path) -> Iterator[None]:
    """Verify root's already-active exclusive lane without changing its file."""

    if not path.is_file() or path.is_symlink():
        raise GpuLockBusy("AI-model coordination file is missing or unsafe")
    before = path.read_bytes()
    if before.strip().lower() != b"yes":
        raise GpuLockBusy("root-owned AI-model lane is not active")
    yield
    after = path.read_bytes()
    if after != before:
        raise GpuLockBusy("root-owned AI-model coordination state changed during qualification")


def _lock(handle: BinaryIO) -> None:
    if os.name == "nt":
        import msvcrt

        handle.seek(0)
        try:
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        except OSError as exc:
            raise GpuLockBusy("AI-model coordination file is locked") from exc
    else:
        import fcntl

        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as exc:
            raise GpuLockBusy("AI-model coordination file is locked") from exc


def _unlock(handle: BinaryIO) -> None:
    if os.name == "nt":
        import msvcrt

        handle.seek(0)
        with contextlib.suppress(OSError):
            msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
    else:
        import fcntl

        with contextlib.suppress(OSError):
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def _write_state(handle: BinaryIO, state: bytes) -> None:
    handle.seek(0)
    handle.truncate(0)
    handle.write(state)
    handle.flush()
    os.fsync(handle.fileno())


@contextlib.contextmanager
def ai_model_lock(path: Path) -> Iterator[None]:
    if not path.is_file() or path.is_symlink():
        raise GpuLockBusy("AI-model coordination file is missing or unsafe")
    with path.open("r+b", buffering=0) as handle:
        _lock(handle)
        armed = False
        try:
            handle.seek(0)
            current = handle.read(16).strip().lower()
            if current != b"no":
                raise GpuLockBusy("another AI-model lane currently owns the coordination file")
            _write_state(handle, b"yes")
            armed = True
            yield
        finally:
            if armed:
                _write_state(handle, b"no")
            _unlock(handle)
