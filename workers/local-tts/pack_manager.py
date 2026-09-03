#!/usr/bin/env python3
"""Explicit, hash-pinned lifecycle for an optional local TTS pack.

This module is intentionally independent from the product Model Manager. The
integration handoff must replace the CLI manifest-digest trust anchor with the
product signed-catalog trust before exposing installation in Release builds.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import ssl
import sys
import tarfile
import tempfile
import time
import unicodedata
import urllib.request
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Protocol
from urllib.parse import urlparse

from manifest import Artifact, ManifestError, PackManifest, canonical_json_bytes, load_manifest

DOWNLOAD_CHUNK_BYTES = 1024 * 1024
MAX_INSTALL_INVENTORY_FILES = 25_000
MAX_EXTRACTED_BYTES = 1_000_000_000
MAX_ARCHIVE_MEMBERS = 20_000
INSTALL_RECORD = "installation-record.v1.json"


class PackLifecycleError(RuntimeError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class Downloader(Protocol):
    def fetch(self, artifact: Artifact, destination: Path, allowed_hosts: frozenset[str]) -> None: ...


class HttpsDownloader:
    """Bounded HTTPS downloader with post-redirect host validation."""

    def __init__(self, timeout_seconds: float = 30.0) -> None:
        self.timeout_seconds = timeout_seconds
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPSHandler(context=ssl.create_default_context())
        )

    def fetch(self, artifact: Artifact, destination: Path, allowed_hosts: frozenset[str]) -> None:
        request = urllib.request.Request(
            artifact.url,
            headers={"User-Agent": "Interactive-NPCs-Local-TTS-Pack/1"},
            method="GET",
        )
        digest = hashlib.sha256()
        written = 0
        try:
            with self.opener.open(request, timeout=self.timeout_seconds) as response:
                final = urlparse(response.geturl())
                if final.scheme != "https" or (final.hostname or "").lower() not in allowed_hosts:
                    raise PackLifecycleError(
                        "untrusted_redirect",
                        "download redirected outside the pinned host allowlist",
                    )
                content_length = response.headers.get("Content-Length")
                if content_length is not None:
                    try:
                        advertised = int(content_length)
                    except ValueError as error:
                        raise PackLifecycleError(
                            "invalid_content_length",
                            "artifact response has an invalid Content-Length",
                        ) from error
                    if advertised != artifact.size_bytes:
                        raise PackLifecycleError(
                            "artifact_size_mismatch",
                            "artifact response size differs from the signed manifest",
                        )
                with destination.open("xb") as output:
                    while True:
                        chunk = response.read(DOWNLOAD_CHUNK_BYTES)
                        if not chunk:
                            break
                        written += len(chunk)
                        if written > artifact.size_bytes:
                            raise PackLifecycleError(
                                "artifact_too_large",
                                "artifact exceeded its pinned byte size",
                            )
                        digest.update(chunk)
                        output.write(chunk)
                    output.flush()
                    os.fsync(output.fileno())
        except PackLifecycleError:
            destination.unlink(missing_ok=True)
            raise
        except Exception as error:
            destination.unlink(missing_ok=True)
            raise PackLifecycleError("download_failed", "artifact download failed") from error
        if written != artifact.size_bytes:
            destination.unlink(missing_ok=True)
            raise PackLifecycleError(
                "artifact_size_mismatch",
                "downloaded artifact has the wrong byte size",
            )
        if digest.hexdigest() != artifact.sha256:
            destination.unlink(missing_ok=True)
            raise PackLifecycleError(
                "artifact_digest_mismatch",
                "downloaded artifact failed SHA-256 verification",
            )


def sha256_file(path: Path, expected_size: int | None = None) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as source:
        while True:
            chunk = source.read(DOWNLOAD_CHUNK_BYTES)
            if not chunk:
                break
            size += len(chunk)
            if expected_size is not None and size > expected_size:
                raise PackLifecycleError(
                    "file_size_mismatch",
                    "file exceeds its pinned size",
                )
            digest.update(chunk)
    return size, digest.hexdigest()


def _safe_member_relative(
    member: tarfile.TarInfo,
    prefix: PurePosixPath,
) -> PurePosixPath | None:
    normalized_name = unicodedata.normalize("NFC", member.name.replace("\\", "/"))
    path = PurePosixPath(normalized_name)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        raise PackLifecycleError(
            "unsafe_archive_path",
            "archive contains an unsafe path",
        )
    if not path.parts or tuple(part.casefold() for part in path.parts[: len(prefix.parts)]) != tuple(
        part.casefold() for part in prefix.parts
    ):
        raise PackLifecycleError(
            "unexpected_archive_root",
            "archive contains data outside its pinned root",
        )
    if len(path.parts) == len(prefix.parts):
        return None
    relative = PurePosixPath(*path.parts[len(prefix.parts) :])
    if ":" in relative.parts[0] or str(relative).startswith("//"):
        raise PackLifecycleError(
            "unsafe_archive_path",
            "archive path selects a device or UNC location",
        )
    return relative


def extract_verified_tar_bz2(
    archive: Path,
    artifact: Artifact,
    stage_root: Path,
) -> None:
    destination = stage_root / artifact.destination
    destination.mkdir(parents=True, exist_ok=False)
    prefix = PurePosixPath(artifact.strip_prefix)
    seen: set[str] = set()
    extracted_bytes = 0
    try:
        with tarfile.open(archive, mode="r:bz2") as bundle:
            members = bundle.getmembers()
            if len(members) > MAX_ARCHIVE_MEMBERS:
                raise PackLifecycleError(
                    "archive_member_limit",
                    "archive contains too many entries",
                )
            for member in members:
                relative = _safe_member_relative(member, prefix)
                if relative is None:
                    continue
                collision_key = unicodedata.normalize("NFC", str(relative)).casefold()
                if collision_key in seen:
                    raise PackLifecycleError(
                        "archive_case_collision",
                        "archive contains colliding Windows paths",
                    )
                seen.add(collision_key)
                if member.issym() or member.islnk() or member.isdev() or member.isfifo():
                    raise PackLifecycleError(
                        "unsafe_archive_member",
                        "archive links, devices and FIFOs are prohibited",
                    )
                target = destination.joinpath(*relative.parts)
                resolved_parent = target.parent.resolve(strict=False)
                destination_resolved = destination.resolve()
                if destination_resolved not in (resolved_parent, *resolved_parent.parents):
                    raise PackLifecycleError(
                        "unsafe_archive_path",
                        "archive path escaped the staging root",
                    )
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                if not member.isfile() or member.size < 0:
                    raise PackLifecycleError(
                        "unsupported_archive_member",
                        "archive contains an unsupported entry",
                    )
                extracted_bytes += member.size
                if extracted_bytes > MAX_EXTRACTED_BYTES:
                    raise PackLifecycleError(
                        "archive_expansion_limit",
                        "archive expands beyond the bounded install size",
                    )
                target.parent.mkdir(parents=True, exist_ok=True)
                source = bundle.extractfile(member)
                if source is None:
                    raise PackLifecycleError(
                        "archive_read_failed",
                        "archive member could not be read",
                    )
                with source, target.open("xb") as output:
                    copied = 0
                    while copied < member.size:
                        chunk = source.read(
                            min(DOWNLOAD_CHUNK_BYTES, member.size - copied)
                        )
                        if not chunk:
                            raise PackLifecycleError(
                                "archive_truncated",
                                "archive member ended before its declared size",
                            )
                        copied += len(chunk)
                        output.write(chunk)
                    if source.read(1):
                        raise PackLifecycleError(
                            "archive_member_overflow",
                            "archive member exceeded its declared size",
                        )
                    output.flush()
                    os.fsync(output.fileno())
    except PackLifecycleError:
        raise
    except (tarfile.TarError, OSError) as error:
        raise PackLifecycleError(
            "archive_extract_failed",
            "verified archive could not be safely extracted",
        ) from error


def _record_without_digest(record: dict[str, object]) -> dict[str, object]:
    return {key: value for key, value in record.items() if key != "record_sha256"}


def _record_digest(record: dict[str, object]) -> str:
    return hashlib.sha256(
        canonical_json_bytes(_record_without_digest(record))
    ).hexdigest()


@dataclass(slots=True)
class PackManager:
    root: Path
    downloader: Downloader

    def __post_init__(self) -> None:
        self.root = self.root.resolve()
        if self.root.name.casefold() != "model-packs":
            raise PackLifecycleError(
                "unsafe_pack_root",
                "local TTS lifecycle root must be a dedicated model-packs directory",
            )
        self.root.mkdir(parents=True, exist_ok=True)
        if self.root.is_symlink():
            raise PackLifecycleError(
                "unsafe_pack_root",
                "model-packs root cannot be a symbolic link",
            )

    def target(self, manifest: PackManifest) -> Path:
        return self.root / manifest.pack_id / manifest.revision

    @staticmethod
    def authorize(
        manifest: PackManifest,
        *,
        expected_manifest_sha256: str,
        confirmed_pack_id: str,
        confirmed_revision: str,
        accepted_license_ids: set[str],
    ) -> None:
        if expected_manifest_sha256 != manifest.canonical_sha256:
            raise PackLifecycleError(
                "manifest_trust_mismatch",
                "manifest does not match the caller trusted digest",
            )
        if (
            confirmed_pack_id != manifest.pack_id
            or confirmed_revision != manifest.revision
        ):
            raise PackLifecycleError(
                "explicit_confirmation_mismatch",
                "pack ID and revision must be confirmed exactly",
            )
        missing = manifest.license_acceptance_ids - accepted_license_ids
        if missing:
            raise PackLifecycleError(
                "license_acceptance_required",
                f"license acknowledgements are missing: {sorted(missing)}",
            )

    def _build_stage(self, manifest: PackManifest) -> Path:
        staging_parent = self.root / ".staging"
        staging_parent.mkdir(exist_ok=True)
        stage = Path(
            tempfile.mkdtemp(
                prefix=f"{manifest.pack_id}.",
                dir=staging_parent,
            )
        )
        try:
            for artifact in manifest.artifacts:
                archive = stage / f"{artifact.artifact_id}.tar.bz2"
                self.downloader.fetch(
                    artifact,
                    archive,
                    manifest.allowed_redirect_hosts,
                )
                size, digest = sha256_file(archive, artifact.size_bytes)
                if size != artifact.size_bytes or digest != artifact.sha256:
                    raise PackLifecycleError(
                        "artifact_reverification_failed",
                        "artifact changed before extraction",
                    )
                extract_verified_tar_bz2(archive, artifact, stage)
                archive.unlink()
            self._verify_manifest_paths(stage, manifest)
            inventory = self._inventory(stage)
            record: dict[str, object] = {
                "schema": "npc.local-tts-installation/v1",
                "pack_id": manifest.pack_id,
                "revision": manifest.revision,
                "manifest_sha256": manifest.canonical_sha256,
                "installed_unix_ms": int(time.time() * 1000),
                "state": "verified",
                "artifacts": [
                    {
                        "artifact_id": item.artifact_id,
                        "size_bytes": item.size_bytes,
                        "sha256": item.sha256,
                    }
                    for item in manifest.artifacts
                ],
                "inventory": inventory,
            }
            record["record_sha256"] = _record_digest(record)
            record_path = stage / INSTALL_RECORD
            with record_path.open("x", encoding="utf-8", newline="\n") as output:
                json.dump(
                    record,
                    output,
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                )
                output.write("\n")
                output.flush()
                os.fsync(output.fileno())
            return stage
        except Exception:
            shutil.rmtree(stage, ignore_errors=True)
            raise

    def _verify_manifest_paths(
        self,
        root: Path,
        manifest: PackManifest,
    ) -> None:
        for artifact in manifest.artifacts:
            destination = root / artifact.destination
            for relative in artifact.required_paths:
                path = destination / relative
                if not path.is_file() or path.is_symlink():
                    raise PackLifecycleError(
                        "required_file_missing",
                        f"required pack file is missing: {artifact.destination}/{relative}",
                    )
        for critical in manifest.critical_files:
            path = root / critical.path
            if not path.is_file() or path.is_symlink():
                raise PackLifecycleError(
                    "critical_file_missing",
                    f"critical pack file is missing: {critical.path}",
                )
            size, digest = sha256_file(path, critical.size_bytes)
            if size != critical.size_bytes or digest != critical.sha256:
                raise PackLifecycleError(
                    "critical_file_mismatch",
                    f"critical pack file failed verification: {critical.path}",
                )

    @staticmethod
    def _inventory(root: Path) -> list[dict[str, object]]:
        rows: list[dict[str, object]] = []
        for path in sorted(
            root.rglob("*"),
            key=lambda item: item.relative_to(root).as_posix().casefold(),
        ):
            if path.is_symlink():
                raise PackLifecycleError(
                    "install_link_rejected",
                    "installed pack cannot contain links",
                )
            if not path.is_file():
                continue
            relative = path.relative_to(root).as_posix()
            if relative == INSTALL_RECORD:
                continue
            size, digest = sha256_file(path)
            rows.append(
                {
                    "path": relative,
                    "size_bytes": size,
                    "sha256": digest,
                }
            )
            if len(rows) > MAX_INSTALL_INVENTORY_FILES:
                raise PackLifecycleError(
                    "inventory_limit",
                    "installed pack contains too many files",
                )
        return rows

    def install(
        self,
        manifest: PackManifest,
        **authorization: object,
    ) -> Path:
        self.authorize(manifest, **authorization)  # type: ignore[arg-type]
        target = self.target(manifest)
        if target.exists():
            raise PackLifecycleError(
                "already_installed",
                "this exact pack revision is already installed",
            )
        stage = self._build_stage(manifest)
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            os.replace(stage, target)
        except Exception:
            shutil.rmtree(stage, ignore_errors=True)
            raise
        self.verify(manifest)
        return target

    def repair(
        self,
        manifest: PackManifest,
        **authorization: object,
    ) -> Path:
        self.authorize(manifest, **authorization)  # type: ignore[arg-type]
        target = self.target(manifest)
        if not target.exists():
            return self.install(manifest, **authorization)
        stage = self._build_stage(manifest)
        trash = self.root / ".trash"
        trash.mkdir(exist_ok=True)
        backup = trash / (
            f"{manifest.pack_id}.{manifest.revision}.{time.time_ns()}"
        )
        try:
            os.replace(target, backup)
            try:
                os.replace(stage, target)
            except Exception:
                os.replace(backup, target)
                raise
            shutil.rmtree(backup)
        except Exception:
            shutil.rmtree(stage, ignore_errors=True)
            raise
        self.verify(manifest)
        return target

    def verify(self, manifest: PackManifest) -> dict[str, object]:
        target = self.target(manifest)
        record_path = target / INSTALL_RECORD
        if (
            not target.is_dir()
            or target.is_symlink()
            or not record_path.is_file()
            or record_path.is_symlink()
        ):
            raise PackLifecycleError(
                "not_installed",
                "verified installation is absent",
            )
        try:
            record = json.loads(record_path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError) as error:
            raise PackLifecycleError(
                "installation_record_invalid",
                "installation record is unreadable",
            ) from error
        if (
            not isinstance(record, dict)
            or record.get("record_sha256") != _record_digest(record)
        ):
            raise PackLifecycleError(
                "installation_record_invalid",
                "installation record digest is invalid",
            )
        if (
            record.get("pack_id") != manifest.pack_id
            or record.get("revision") != manifest.revision
            or record.get("manifest_sha256") != manifest.canonical_sha256
        ):
            raise PackLifecycleError(
                "installation_record_mismatch",
                "installation record does not match the trusted manifest",
            )
        self._verify_manifest_paths(target, manifest)
        inventory = record.get("inventory")
        if (
            not isinstance(inventory, list)
            or len(inventory) > MAX_INSTALL_INVENTORY_FILES
        ):
            raise PackLifecycleError(
                "installation_record_invalid",
                "installation inventory is invalid",
            )
        expected_paths: set[str] = set()
        for row in inventory:
            if (
                not isinstance(row, dict)
                or set(row) != {"path", "size_bytes", "sha256"}
            ):
                raise PackLifecycleError(
                    "installation_record_invalid",
                    "installation inventory row is invalid",
                )
            relative = row.get("path")
            size = row.get("size_bytes")
            digest = row.get("sha256")
            if (
                not isinstance(relative, str)
                or not isinstance(size, int)
                or not isinstance(digest, str)
            ):
                raise PackLifecycleError(
                    "installation_record_invalid",
                    "installation inventory types are invalid",
                )
            pure = PurePosixPath(relative)
            if (
                pure.is_absolute()
                or any(part in ("", ".", "..") for part in pure.parts)
                or relative in expected_paths
            ):
                raise PackLifecycleError(
                    "installation_record_invalid",
                    "installation inventory contains unsafe or duplicate paths",
                )
            expected_paths.add(relative)
            path = target.joinpath(*pure.parts)
            if not path.is_file() or path.is_symlink():
                raise PackLifecycleError(
                    "installed_file_missing",
                    f"installed file is missing: {relative}",
                )
            actual_size, actual_digest = sha256_file(path, size)
            if actual_size != size or actual_digest != digest:
                raise PackLifecycleError(
                    "installed_file_mismatch",
                    f"installed file changed: {relative}",
                )
        actual_paths = {
            path.relative_to(target).as_posix()
            for path in target.rglob("*")
            if path.is_file()
            and path.relative_to(target).as_posix() != INSTALL_RECORD
        }
        if actual_paths != expected_paths:
            raise PackLifecycleError(
                "unexpected_installed_file",
                "installed pack contains unrecorded or omitted files",
            )
        return record

    def remove(
        self,
        manifest: PackManifest,
        *,
        expected_manifest_sha256: str,
        confirm_remove: str,
    ) -> None:
        if expected_manifest_sha256 != manifest.canonical_sha256:
            raise PackLifecycleError(
                "manifest_trust_mismatch",
                "manifest does not match the caller trusted digest",
            )
        if confirm_remove != manifest.install_key:
            raise PackLifecycleError(
                "explicit_confirmation_mismatch",
                "removal requires the exact pack@revision token",
            )
        target = self.target(manifest)
        self.verify(manifest)
        trash = self.root / ".trash"
        trash.mkdir(exist_ok=True)
        tombstone = trash / (
            f"removed.{manifest.pack_id}.{manifest.revision}.{time.time_ns()}"
        )
        os.replace(target, tombstone)
        shutil.rmtree(tombstone)
        try:
            target.parent.rmdir()
        except OSError:
            pass


def manifest_summary(manifest: PackManifest) -> dict[str, object]:
    return {
        "pack_id": manifest.pack_id,
        "revision": manifest.revision,
        "manifest_sha256": manifest.canonical_sha256,
        "download_bytes": sum(item.size_bytes for item in manifest.artifacts),
        "artifacts": [
            {
                "artifact_id": item.artifact_id,
                "url": item.url,
                "size_bytes": item.size_bytes,
                "sha256": item.sha256,
            }
            for item in manifest.artifacts
        ],
        "license_acceptance_required": sorted(
            manifest.license_acceptance_ids
        ),
        "activation": "blocked_pending_measurement",
    }


def _authorization(args: argparse.Namespace) -> dict[str, object]:
    return {
        "expected_manifest_sha256": args.expected_manifest_sha256 or "",
        "confirmed_pack_id": args.confirm_pack_id or "",
        "confirmed_revision": args.confirm_revision or "",
        "accepted_license_ids": set(args.accept_license or []),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "operation",
        choices=("plan", "install", "verify", "repair", "remove"),
    )
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--expected-manifest-sha256")
    parser.add_argument("--confirm-pack-id")
    parser.add_argument("--confirm-revision")
    parser.add_argument("--accept-license", action="append")
    parser.add_argument("--confirm-remove")
    args = parser.parse_args(argv)
    try:
        manifest = load_manifest(args.manifest)
        if args.operation == "plan":
            print(
                json.dumps(
                    manifest_summary(manifest),
                    indent=2,
                    ensure_ascii=False,
                )
            )
            return 0
        manager = PackManager(args.root, HttpsDownloader())
        if args.operation == "verify":
            record = manager.verify(manifest)
            print(
                json.dumps(
                    {"status": "verified", "record": record},
                    indent=2,
                )
            )
        elif args.operation == "remove":
            manager.remove(
                manifest,
                expected_manifest_sha256=args.expected_manifest_sha256 or "",
                confirm_remove=args.confirm_remove or "",
            )
            print(
                json.dumps(
                    {"status": "removed", "pack": manifest.install_key}
                )
            )
        else:
            authorization = _authorization(args)
            if args.operation == "install":
                path = manager.install(manifest, **authorization)
            else:
                path = manager.repair(manifest, **authorization)
            print(
                json.dumps(
                    {
                        "status": "verified",
                        "path": str(path),
                        "pack": manifest.install_key,
                    }
                )
            )
        return 0
    except (ManifestError, PackLifecycleError) as error:
        code = (
            error.code
            if isinstance(error, PackLifecycleError)
            else "manifest_invalid"
        )
        print(
            json.dumps(
                {
                    "status": "error",
                    "error": {"code": code, "message": str(error)},
                }
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
