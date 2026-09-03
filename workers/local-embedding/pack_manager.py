"""Transactional, checksum-first lifecycle for the optional embedding pack.

This module is intentionally independent of the application's Model Manager.
It is the reviewed implementation seam for later integration: production calls
provide an exact install root and a schema-validated manifest, while tests inject
a byte source and never contact a network.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import secrets
import shutil
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO, Callable, Iterator

from model_spec import ArtifactSpec, PACK_ID, PACK_REVISION, PackSpec, SpecError, sha256_file

RECEIPT_SCHEMA = "npc.local-model-install-receipt/v1"
MAX_REDIRECTS = 4
CHUNK_BYTES = 1024 * 1024


class LifecycleError(RuntimeError):
    def __init__(self, code: str, message: str, *, details: dict[str, object] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.details = details or {}


@dataclass(frozen=True, slots=True)
class VerificationIssue:
    artifact_id: str
    code: str
    expected: object
    actual: object


@dataclass(frozen=True, slots=True)
class VerificationReport:
    installed: bool
    healthy: bool
    receipt_valid: bool
    issues: tuple[VerificationIssue, ...]
    checked_bytes: int


@dataclass(frozen=True, slots=True)
class LifecycleResult:
    action: str
    target: Path
    changed: bool
    report: VerificationReport
    recovery_path: Path | None = None


Fetch = Callable[[ArtifactSpec, BinaryIO], tuple[str, int]]


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[no-untyped-def]
        return None


def https_fetch(artifact: ArtifactSpec, destination: BinaryIO) -> tuple[str, int]:
    """Fetch exactly one artifact with HTTPS-only, bounded manual redirects.

    The caller owns the staging file.  Data never enters the final install tree
    until both declared length and SHA-256 have been verified.
    """

    context = ssl.create_default_context()
    opener = urllib.request.build_opener(
        _NoRedirect,
        urllib.request.HTTPSHandler(context=context),
    )
    last_error: Exception | None = None
    for source in artifact.source_urls:
        url = source
        for _redirect in range(MAX_REDIRECTS + 1):
            if not url.startswith("https://"):
                raise LifecycleError("insecure_redirect", "model artifacts may only use HTTPS")
            request = urllib.request.Request(
                url,
                method="GET",
                headers={
                    "Accept": "application/octet-stream",
                    "Accept-Encoding": "identity",
                    "User-Agent": "InteractiveNPCs-ModelManager/2 local-pack",
                },
            )
            try:
                response = opener.open(request, timeout=30)
            except urllib.error.HTTPError as exc:
                if exc.code in {301, 302, 303, 307, 308}:
                    location = exc.headers.get("Location")
                    if not location:
                        last_error = exc
                        break
                    url = urllib.parse.urljoin(url, location)
                    continue
                last_error = exc
                break
            except (OSError, urllib.error.URLError) as exc:
                last_error = exc
                break
            with response:
                declared = response.headers.get("Content-Length")
                if declared is not None:
                    try:
                        parsed_length = int(declared)
                    except ValueError as exc:
                        raise LifecycleError("invalid_content_length", "artifact server returned an invalid length") from exc
                    if parsed_length != artifact.size_bytes:
                        raise LifecycleError(
                            "size_mismatch",
                            "artifact response length differs from the signed manifest",
                            details={"artifact_id": artifact.artifact_id},
                        )
                digest = hashlib.sha256()
                size = 0
                while chunk := response.read(CHUNK_BYTES):
                    size += len(chunk)
                    if size > artifact.size_bytes:
                        raise LifecycleError(
                            "size_mismatch",
                            "artifact exceeded the signed size bound",
                            details={"artifact_id": artifact.artifact_id},
                        )
                    digest.update(chunk)
                    destination.write(chunk)
                return digest.hexdigest(), size
        destination.seek(0)
        destination.truncate(0)
    raise LifecycleError(
        "download_failed",
        "all immutable artifact sources failed",
        details={"artifact_id": artifact.artifact_id, "error_type": type(last_error).__name__},
    )


class PackLifecycle:
    def __init__(self, install_root: Path, manifest: PackSpec, *, fetch: Fetch = https_fetch) -> None:
        self.root = install_root.resolve()
        self.manifest = manifest
        self.fetch = fetch
        self.target = self._inside(self.root / PACK_ID / PACK_REVISION)
        self.receipt_path = self.target / "install-receipt.v1.json"

    def _inside(self, path: Path) -> Path:
        resolved = path.resolve(strict=False)
        try:
            resolved.relative_to(self.root)
        except ValueError as exc:
            raise LifecycleError("path_escape", "model pack path escaped its explicit install root") from exc
        return resolved

    def _artifact_path(self, base: Path, artifact: ArtifactSpec) -> Path:
        base = base.resolve(strict=False)
        candidate = base.joinpath(*artifact.destination.parts)
        current = base
        for part in artifact.destination.parts:
            current = current / part
            if current.is_symlink():
                raise LifecycleError("unsafe_destination", "artifact path contains a symlink")
        resolved = candidate.resolve(strict=False)
        try:
            resolved.relative_to(base.resolve(strict=False))
        except ValueError as exc:
            raise LifecycleError("path_escape", "artifact destination escaped its pack directory") from exc
        return candidate

    @contextlib.contextmanager
    def _exclusive_lock(self) -> Iterator[None]:
        self.root.mkdir(parents=True, exist_ok=True)
        lock = self._inside(self.root / f".{PACK_ID}.{PACK_REVISION}.lifecycle.lock")
        try:
            descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError as exc:
            raise LifecycleError("lifecycle_busy", "another pack lifecycle operation owns the exact lock") from exc
        try:
            os.write(descriptor, json.dumps({"pid": os.getpid(), "started_unix_ms": int(time.time() * 1000)}).encode("ascii"))
            os.fsync(descriptor)
            yield
        finally:
            os.close(descriptor)
            with contextlib.suppress(FileNotFoundError):
                lock.unlink()

    def verify(self, target: Path | None = None) -> VerificationReport:
        base = self.target if target is None else self._inside(target)
        issues: list[VerificationIssue] = []
        checked = 0
        if not base.is_dir() or base.is_symlink():
            return VerificationReport(False, False, False, (VerificationIssue("pack", "missing", "directory", "absent"),), 0)
        for artifact in self.manifest.artifacts:
            try:
                path = self._artifact_path(base, artifact)
            except LifecycleError as exc:
                issues.append(VerificationIssue(artifact.artifact_id, exc.code, artifact.destination.as_posix(), "unsafe"))
                continue
            if not path.is_file() or path.is_symlink():
                issues.append(VerificationIssue(artifact.artifact_id, "missing", artifact.destination.as_posix(), "absent"))
                continue
            try:
                digest, size = sha256_file(path, expected_size=artifact.size_bytes)
            except (OSError, SpecError) as exc:
                issues.append(VerificationIssue(artifact.artifact_id, "unreadable", artifact.sha256, type(exc).__name__))
                continue
            checked += size
            if size != artifact.size_bytes:
                issues.append(VerificationIssue(artifact.artifact_id, "size_mismatch", artifact.size_bytes, size))
            if digest != artifact.sha256:
                issues.append(VerificationIssue(artifact.artifact_id, "sha256_mismatch", artifact.sha256, digest))
        receipt_valid = self._verify_receipt(base)
        if not receipt_valid:
            issues.append(VerificationIssue("receipt", "invalid", self.manifest.manifest_sha256, "missing_or_mismatched"))
        return VerificationReport(True, not issues, receipt_valid, tuple(issues), checked)

    def _verify_receipt(self, base: Path) -> bool:
        path = base / "install-receipt.v1.json"
        if not path.is_file() or path.is_symlink():
            return False
        try:
            raw = json.loads(path.read_bytes())
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            return False
        if not isinstance(raw, dict):
            return False
        expected_artifacts = [
            {
                "id": artifact.artifact_id,
                "destination": artifact.destination.as_posix(),
                "size_bytes": artifact.size_bytes,
                "sha256": artifact.sha256,
            }
            for artifact in self.manifest.artifacts
        ]
        return (
            raw.get("schema") == RECEIPT_SCHEMA
            and raw.get("pack_id") == PACK_ID
            and raw.get("revision") == PACK_REVISION
            and raw.get("manifest_sha256") == self.manifest.manifest_sha256
            and raw.get("artifacts") == expected_artifacts
            and isinstance(raw.get("installed_unix_ms"), int)
        )

    def _write_receipt(self, staging: Path) -> None:
        receipt = {
            "schema": RECEIPT_SCHEMA,
            "pack_id": PACK_ID,
            "revision": PACK_REVISION,
            "manifest_sha256": self.manifest.manifest_sha256,
            "installed_unix_ms": int(time.time() * 1000),
            "artifacts": [
                {
                    "id": artifact.artifact_id,
                    "destination": artifact.destination.as_posix(),
                    "size_bytes": artifact.size_bytes,
                    "sha256": artifact.sha256,
                }
                for artifact in self.manifest.artifacts
            ],
        }
        encoded = (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode("utf-8")
        path = staging / "install-receipt.v1.json"
        with path.open("xb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())

    def _populate_staging(self, staging: Path) -> None:
        for artifact in self.manifest.artifacts:
            destination = self._artifact_path(staging, artifact)
            destination.parent.mkdir(parents=True, exist_ok=True)
            if destination.parent.is_symlink():
                raise LifecycleError("unsafe_destination", "artifact parent cannot be a symlink")
            part = destination.with_name(destination.name + ".part")
            with part.open("xb") as handle:
                digest, size = self.fetch(artifact, handle)
                handle.flush()
                os.fsync(handle.fileno())
            if size != artifact.size_bytes or digest != artifact.sha256:
                with contextlib.suppress(FileNotFoundError):
                    part.unlink()
                raise LifecycleError(
                    "verification_failed",
                    "downloaded artifact did not match its signed size and SHA-256",
                    details={"artifact_id": artifact.artifact_id},
                )
            part.replace(destination)
        self._write_receipt(staging)
        report = self.verify(staging)
        if not report.healthy:
            raise LifecycleError("verification_failed", "staged model pack failed full verification")

    def install(self) -> LifecycleResult:
        with self._exclusive_lock():
            current = self.verify()
            if current.healthy:
                return LifecycleResult("install", self.target, False, current)
            if current.installed:
                raise LifecycleError("repair_required", "an unhealthy pack already exists; use repair")
            parent = self.target.parent
            parent.mkdir(parents=True, exist_ok=True)
            staging = self._inside(parent / f".{PACK_REVISION}.staging-{secrets.token_hex(8)}")
            try:
                staging.mkdir(mode=0o700)
                self._populate_staging(staging)
                staging.replace(self.target)
            except Exception:
                shutil.rmtree(staging, ignore_errors=True)
                raise
            report = self.verify()
            if not report.healthy:
                raise LifecycleError("commit_verification_failed", "installed pack failed post-commit verification")
            return LifecycleResult("install", self.target, True, report)

    def repair(self) -> LifecycleResult:
        with self._exclusive_lock():
            current = self.verify()
            if current.healthy:
                return LifecycleResult("repair", self.target, False, current)
            parent = self.target.parent
            parent.mkdir(parents=True, exist_ok=True)
            staging = self._inside(parent / f".{PACK_REVISION}.repair-{secrets.token_hex(8)}")
            backup: Path | None = None
            try:
                staging.mkdir(mode=0o700)
                self._populate_staging(staging)
                if self.target.exists():
                    backup = self._inside(parent / f".{PACK_REVISION}.replaced-{int(time.time() * 1000)}-{secrets.token_hex(4)}")
                    self.target.replace(backup)
                try:
                    staging.replace(self.target)
                except Exception:
                    if backup is not None and backup.exists() and not self.target.exists():
                        backup.replace(self.target)
                    raise
            except Exception:
                shutil.rmtree(staging, ignore_errors=True)
                raise
            report = self.verify()
            if not report.healthy:
                raise LifecycleError("commit_verification_failed", "repaired pack failed post-commit verification")
            return LifecycleResult("repair", self.target, True, report, backup)

    def remove(self) -> LifecycleResult:
        with self._exclusive_lock():
            current = self.verify()
            if not self.target.exists():
                return LifecycleResult("remove", self.target, False, current)
            trash_root = self._inside(self.root / ".trash")
            trash_root.mkdir(parents=True, exist_ok=True)
            recovery = self._inside(
                trash_root / f"{PACK_ID}-{PACK_REVISION}-{int(time.time() * 1000)}-{secrets.token_hex(4)}"
            )
            self.target.replace(recovery)
            after = self.verify()
            return LifecycleResult("remove", self.target, True, after, recovery)
