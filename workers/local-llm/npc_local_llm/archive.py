"""Fail-closed ZIP extraction for pinned llama.cpp runtime releases."""

from __future__ import annotations

import os
import shutil
import stat
import zipfile
from pathlib import Path

from .constants import (
    ABSOLUTE_MAX_ARCHIVE_EXPANDED_BYTES,
    MAX_ARCHIVE_EXPANDED_BYTES,
    MAX_ARCHIVE_EXPANSION_RATIO,
    MAX_ARCHIVE_MEMBERS,
)
from .errors import LocalLlmError
from .manifest import safe_relative_path

COPY_CHUNK_BYTES = 4 * 1_048_576


def secure_extract_zip(
    archive_path: Path,
    destination: Path,
    *,
    maximum_expanded_bytes: int = MAX_ARCHIVE_EXPANDED_BYTES,
    expected_member_count: int | None = None,
    expected_expanded_bytes: int | None = None,
) -> tuple[int, int]:
    if not 1 <= maximum_expanded_bytes <= ABSOLUTE_MAX_ARCHIVE_EXPANDED_BYTES:
        raise LocalLlmError("unsafe_archive_policy", "runtime archive expansion policy is outside the hard ceiling")
    if expected_member_count is not None and not 1 <= expected_member_count <= MAX_ARCHIVE_MEMBERS:
        raise LocalLlmError("unsafe_archive_policy", "expected runtime member count is outside the hard ceiling")
    if expected_expanded_bytes is not None and not 1 <= expected_expanded_bytes <= maximum_expanded_bytes:
        raise LocalLlmError("unsafe_archive_policy", "expected runtime size is outside the approved ceiling")
    destination.mkdir(parents=True, exist_ok=False)
    with zipfile.ZipFile(archive_path, "r") as archive:
        members = archive.infolist()
        if not 1 <= len(members) <= MAX_ARCHIVE_MEMBERS:
            raise LocalLlmError("unsafe_archive", "runtime archive member count is outside the approved range")
        total_expanded = 0
        seen: set[str] = set()
        validated: list[tuple[zipfile.ZipInfo, Path]] = []
        for member in members:
            relative = safe_relative_path(member.filename.rstrip("/"), "archive_member")
            folded = str(relative).casefold()
            if folded in seen:
                raise LocalLlmError("unsafe_archive", "runtime archive contains a case-folding collision")
            seen.add(folded)
            if member.flag_bits & 0x1:
                raise LocalLlmError("unsafe_archive", "encrypted runtime archives are not supported")
            unix_mode = (member.external_attr >> 16) & 0xFFFF
            kind = stat.S_IFMT(unix_mode)
            if kind not in {0, stat.S_IFREG, stat.S_IFDIR}:
                raise LocalLlmError("unsafe_archive", "runtime archive contains a link or special file")
            if member.file_size < 0 or member.compress_size < 0:
                raise LocalLlmError("unsafe_archive", "runtime archive contains invalid sizes")
            total_expanded += member.file_size
            if total_expanded > maximum_expanded_bytes:
                raise LocalLlmError("unsafe_archive", "runtime archive exceeds its expansion ceiling")
            if member.file_size > 0 and member.compress_size == 0:
                raise LocalLlmError("unsafe_archive", "runtime archive member has an invalid compression ratio")
            if member.compress_size and member.file_size > member.compress_size * MAX_ARCHIVE_EXPANSION_RATIO:
                raise LocalLlmError("unsafe_archive", "runtime archive member exceeds its compression-ratio ceiling")
            validated.append((member, destination.joinpath(*relative.parts)))
        if expected_member_count is not None and len(members) != expected_member_count:
            raise LocalLlmError("archive_identity_mismatch", "runtime archive member count does not match its trust record")
        if expected_expanded_bytes is not None and total_expanded != expected_expanded_bytes:
            raise LocalLlmError("archive_identity_mismatch", "runtime archive expanded size does not match its trust record")
        for member, target in validated:
            if member.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o700 if target.suffix == ".exe" else 0o600)
            try:
                with archive.open(member, "r") as source, os.fdopen(descriptor, "wb", buffering=0) as output:
                    descriptor = -1
                    shutil.copyfileobj(source, output, length=COPY_CHUNK_BYTES)
                    output.flush()
                    os.fsync(output.fileno())
            finally:
                if descriptor >= 0:
                    os.close(descriptor)
        return len(members), total_expanded
