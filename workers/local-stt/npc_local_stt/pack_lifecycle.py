from __future__ import annotations

from contextlib import AbstractContextManager
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tempfile
import time
from typing import Any, BinaryIO, Callable, Iterator
import urllib.error
import urllib.request
import uuid


class PackError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.safe_message = message


@dataclass(frozen=True)
class Artifact:
    artifact_id: str
    url: str
    bytes: int
    sha256: str
    required_top_level: str
    maximum_entries: int
    maximum_expanded_bytes: int


@dataclass(frozen=True)
class PackManifest:
    source_path: Path
    pack_id: str
    revision: str
    display_name: str
    status: str
    artifact: Artifact
    installed_root: str
    required_files: tuple[str, ...]
    license_spdx: str
    license_evidence: str
    license_notice_path: Path | None
    license_notice_sha256: str | None
    activation_gates: tuple[str, ...]

    @classmethod
    def load(cls, path: Path) -> "PackManifest":
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise PackError("manifest_invalid", "local STT pack manifest could not be read") from exc
        if not isinstance(raw, dict):
            raise PackError("manifest_invalid", "local STT pack manifest must be an object")
        if raw.get("schema") == "npc.model-pack/v2":
            return cls._load_canonical_v2(path, raw)
        if raw.get("schemaVersion") != "npc.local-model-pack/v2" or raw.get("kind") != "stt":
            raise PackError("manifest_invalid", "manifest is not an npc.local-model-pack/v2 STT pack")
        artifacts = raw.get("artifacts")
        if not isinstance(artifacts, list) or len(artifacts) != 1:
            raise PackError("manifest_invalid", "exactly one immutable archive is required")
        item = artifacts[0]
        if not isinstance(item, dict):
            raise PackError("manifest_invalid", "artifact entry is invalid")
        archive = item.get("archive", {})
        if not isinstance(archive, dict):
            raise PackError("manifest_invalid", "artifact archive policy is invalid")
        digest = item.get("sha256", "")
        if not isinstance(digest, str) or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise PackError("manifest_invalid", "artifact SHA-256 is invalid")
        url = item.get("url", "")
        if not isinstance(url, str) or not url.startswith("https://github.com/moonshine-ai/moonshine/releases/download/"):
            raise PackError("manifest_invalid", "artifact URL is not an allowed pinned Moonshine release")
        installed_layout = raw.get("installedLayout", {})
        if not isinstance(installed_layout, dict):
            raise PackError("manifest_invalid", "installed layout is invalid")
        installed_root = _validate_relative_posix(
            installed_layout.get("rootDirectory"), "installed root", leaf_only=True
        )
        required_top_level = _validate_relative_posix(
            archive.get("requiredTopLevelDirectory"), "archive top-level directory", leaf_only=True
        )
        if installed_root != required_top_level:
            raise PackError("manifest_invalid", "installed root must match the archive top-level directory")
        required = installed_layout.get("requiredFiles")
        if not isinstance(required, list) or not required or not all(isinstance(v, str) for v in required):
            raise PackError("manifest_invalid", "required installed file list is invalid")
        required = [_validate_relative_posix(value, "required installed file") for value in required]
        if len(set(required)) != len(required):
            raise PackError("manifest_invalid", "required installed file list contains duplicates")
        license_info = raw.get("license", {})
        lifecycle = raw.get("lifecycle", {})
        if not isinstance(license_info, dict) or not isinstance(lifecycle, dict):
            raise PackError("manifest_invalid", "license and lifecycle policies are invalid")
        gates = lifecycle.get("activationGates", [])
        if license_info.get("explicitUserAcceptanceRequired") is not True or not isinstance(gates, list):
            raise PackError("manifest_invalid", "license and activation gates are incomplete")
        if isinstance(item.get("bytes"), bool) or isinstance(archive.get("maximumEntries"), bool) or isinstance(
            archive.get("maximumExpandedBytes"), bool
        ):
            raise PackError("manifest_invalid", "artifact size bounds are invalid")
        try:
            artifact_bytes = int(item["bytes"])
            maximum_entries = int(archive["maximumEntries"])
            maximum_expanded_bytes = int(archive["maximumExpandedBytes"])
        except (KeyError, TypeError, ValueError) as exc:
            raise PackError("manifest_invalid", "artifact size bounds are invalid") from exc
        if artifact_bytes <= 0 or maximum_entries <= 0 or maximum_expanded_bytes <= 0:
            raise PackError("manifest_invalid", "artifact size bounds must be positive")
        required_string_fields = {
            "packId": raw.get("packId"),
            "revision": raw.get("revision"),
            "displayName": raw.get("displayName"),
            "status": raw.get("status"),
            "artifactId": item.get("artifactId"),
            "license SPDX": license_info.get("spdx"),
            "license evidence": license_info.get("evidence"),
        }
        if any(not isinstance(value, str) or not value for value in required_string_fields.values()):
            raise PackError("manifest_invalid", "manifest identity and license fields must be non-empty strings")
        notice_path: Path | None = None
        notice_sha256: str | None = None
        notice_value = license_info.get("localNotice")
        notice_digest = license_info.get("evidenceSha256")
        if notice_value is not None or notice_digest is not None:
            relative_notice = _validate_relative_posix(notice_value, "local license notice")
            if (
                not isinstance(notice_digest, str)
                or len(notice_digest) != 64
                or any(c not in "0123456789abcdef" for c in notice_digest)
            ):
                raise PackError("manifest_invalid", "local license notice SHA-256 is invalid")
            # Production manifests live at <repo>/packaging/model-packs. Keep
            # this resolution explicit and fail closed rather than searching
            # arbitrary ancestors.
            if path.parent.name != "model-packs" or path.parent.parent.name != "packaging":
                raise PackError("manifest_invalid", "local notice is allowed only for a repository pack manifest")
            repository_root = path.parent.parent.parent.resolve()
            notice_path = repository_root.joinpath(*PurePosixPath(relative_notice).parts)
            if not notice_path.is_file() or notice_path.is_symlink() or sha256_file(notice_path) != notice_digest:
                raise PackError("manifest_invalid", "local license notice is missing or does not match its SHA-256")
            notice_sha256 = notice_digest
        return cls(
            source_path=path.resolve(),
            pack_id=str(raw["packId"]),
            revision=str(raw["revision"]),
            display_name=str(raw["displayName"]),
            status=str(raw["status"]),
            artifact=Artifact(
                artifact_id=str(item["artifactId"]),
                url=url,
                bytes=artifact_bytes,
                sha256=digest,
                required_top_level=required_top_level,
                maximum_entries=maximum_entries,
                maximum_expanded_bytes=maximum_expanded_bytes,
            ),
            installed_root=installed_root,
            required_files=tuple(required),
            license_spdx=str(license_info["spdx"]),
            license_evidence=str(license_info["evidence"]),
            license_notice_path=notice_path,
            license_notice_sha256=notice_sha256,
            activation_gates=tuple(str(value) for value in gates),
        )

    @classmethod
    def _load_canonical_v2(cls, path: Path, raw: dict[str, Any]) -> "PackManifest":
        if raw.get("$schema") != "./model-pack-manifest.schema.json":
            raise PackError("manifest_invalid", "canonical model pack schema URI is invalid")
        capability = raw.get("capability")
        if not isinstance(capability, dict) or capability.get("kind") != "speech_recognition":
            raise PackError("manifest_invalid", "canonical pack is not speech recognition")
        artifacts = raw.get("artifacts")
        if not isinstance(artifacts, list) or len(artifacts) != 1 or not isinstance(artifacts[0], dict):
            raise PackError("manifest_invalid", "exactly one combined Moonshine archive is required")
        item = artifacts[0]
        urls = item.get("source_urls")
        digest = item.get("sha256")
        if (
            item.get("kind") != "archive"
            or item.get("archive_format") != "tar_gz"
            or not isinstance(urls, list)
            or len(urls) != 1
            or not isinstance(urls[0], str)
            or not urls[0].startswith("https://github.com/moonshine-ai/moonshine/releases/download/")
            or not isinstance(digest, str)
            or len(digest) != 64
            or any(c not in "0123456789abcdef" for c in digest)
        ):
            raise PackError("manifest_invalid", "canonical Moonshine archive pin is invalid")
        strip_prefix = _validate_relative_posix(item.get("strip_prefix"), "archive strip prefix", leaf_only=True)
        required = item.get("required_paths")
        if not isinstance(required, list) or not required or not all(isinstance(value, str) for value in required):
            raise PackError("manifest_invalid", "canonical required path list is invalid")
        required = [_validate_relative_posix(value, "canonical required path") for value in required]
        if len(required) != len(set(required)):
            raise PackError("manifest_invalid", "canonical required path list contains duplicates")
        try:
            artifact_bytes = int(item["size_bytes"])
            storage_bytes = int(raw["resources"]["storage_bytes"])
            peak_install_bytes = int(raw["resources"]["peak_install_bytes"])
        except (KeyError, TypeError, ValueError) as exc:
            raise PackError("manifest_invalid", "canonical resource bounds are invalid") from exc
        maximum_expanded = peak_install_bytes - storage_bytes
        if artifact_bytes <= 0 or storage_bytes != artifact_bytes or maximum_expanded <= 0:
            raise PackError("manifest_invalid", "canonical archive and extraction bounds are inconsistent")
        license_info = raw.get("license")
        lifecycle = raw.get("lifecycle")
        admission = raw.get("admission")
        if not isinstance(license_info, dict) or not isinstance(lifecycle, dict) or not isinstance(admission, dict):
            raise PackError("manifest_invalid", "canonical lifecycle, license, or admission policy is invalid")
        gates = lifecycle.get("activation_gates")
        if license_info.get("acceptance_required") is not True or not isinstance(gates, list):
            raise PackError("manifest_invalid", "canonical license acceptance and gates are incomplete")
        pack_id = raw.get("pack_id")
        revision = raw.get("revision")
        display_name = raw.get("display_name")
        if not all(isinstance(value, str) and value for value in (pack_id, revision, display_name)):
            raise PackError("manifest_invalid", "canonical pack identity is invalid")
        notice_path: Path | None = None
        notice_sha256: str | None = None
        if pack_id == "npc.stt.moonshine-v2-medium-streaming-en.win-x64":
            repository_root = path.parent.parent.parent.resolve()
            notice_path = repository_root / "workers" / "local-stt" / "THIRD_PARTY_LICENSES" / "MOONSHINE-v0.1.5-LICENSE.txt"
            notice_sha256 = "fa7d1174dd8af6a7cd280be20b80d10095ed4c19b5b20b61a7715c3ad790dc5f"
            if not notice_path.is_file() or notice_path.is_symlink() or sha256_file(notice_path) != notice_sha256:
                raise PackError("manifest_invalid", "pinned Moonshine license notice is missing or invalid")
        return cls(
            source_path=path.resolve(),
            pack_id=pack_id,
            revision=revision,
            display_name=display_name,
            status=str(admission.get("state", "blocked_pending_measurement")),
            artifact=Artifact(
                artifact_id=str(item.get("id", "")),
                url=urls[0],
                bytes=artifact_bytes,
                sha256=digest,
                required_top_level=strip_prefix,
                maximum_entries=256,
                maximum_expanded_bytes=maximum_expanded,
            ),
            # The isolated lifecycle preserves the upstream top-level directory;
            # the shared generic installer applies destination + strip_prefix.
            installed_root=strip_prefix,
            required_files=tuple(required),
            license_spdx=str(license_info.get("spdx_expression", "")),
            license_evidence=str(license_info.get("license_url", "")),
            license_notice_path=notice_path,
            license_notice_sha256=notice_sha256,
            activation_gates=tuple(str(value) for value in gates),
        )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _validate_relative_posix(value: Any, label: str, *, leaf_only: bool = False) -> str:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise PackError("manifest_invalid", f"{label} is not a safe relative POSIX path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise PackError("manifest_invalid", f"{label} is not a safe relative POSIX path")
    if leaf_only and len(path.parts) != 1:
        raise PackError("manifest_invalid", f"{label} must be one directory name")
    normalized = path.as_posix()
    if normalized != value:
        raise PackError("manifest_invalid", f"{label} must use its canonical POSIX spelling")
    return normalized


def _atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    encoded = json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    with temporary.open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


class PackLock(AbstractContextManager["PackLock"]):
    def __init__(self, path: Path) -> None:
        self._path = path
        self._stream: BinaryIO | None = None

    def __enter__(self) -> "PackLock":
        self._path.parent.mkdir(parents=True, exist_ok=True)
        self._stream = self._path.open("a+b")
        try:
            if os.name == "nt":
                import msvcrt

                self._stream.seek(0)
                if self._stream.tell() == 0 and self._path.stat().st_size == 0:
                    self._stream.write(b"0")
                    self._stream.flush()
                self._stream.seek(0)
                msvcrt.locking(self._stream.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(self._stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except (OSError, IOError) as exc:
            self._stream.close()
            self._stream = None
            raise PackError("pack_busy", "another local STT lifecycle operation is running") from exc
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        if self._stream is None:
            return
        try:
            if os.name == "nt":
                import msvcrt

                self._stream.seek(0)
                msvcrt.locking(self._stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl

                fcntl.flock(self._stream.fileno(), fcntl.LOCK_UN)
        finally:
            self._stream.close()
            self._stream = None


def _validate_leaf(value: str, label: str) -> str:
    if not value or len(value) > 160 or value in {".", ".."} or any(c in value for c in "/\\:\0"):
        raise PackError("manifest_invalid", f"{label} cannot be used as an install directory")
    return value


class PackLifecycle:
    def __init__(
        self,
        manifest: PackManifest,
        root: Path,
        *,
        opener: Callable[..., Any] = urllib.request.urlopen,
    ) -> None:
        self.manifest = manifest
        self.root = root.resolve(strict=False)
        if self.root == Path(self.root.anchor) or len(self.root.parts) < 3:
            raise PackError("unsafe_root", "model pack root is too broad")
        pack_leaf = _validate_leaf(manifest.pack_id, "packId")
        revision_leaf = _validate_leaf(manifest.revision, "revision")
        self.pack_root = self.root / pack_leaf / revision_leaf
        self._opener = opener
        self._lock_path = self.root / ".locks" / f"{pack_leaf}.lock"

    @property
    def receipt_path(self) -> Path:
        return self.pack_root / ".npc-pack-receipt.json"

    def inspect(self) -> dict[str, Any]:
        return {
            "packId": self.manifest.pack_id,
            "revision": self.manifest.revision,
            "status": self.manifest.status,
            "artifactBytes": self.manifest.artifact.bytes,
            "artifactSha256": self.manifest.artifact.sha256,
            "license": self.manifest.license_spdx,
            "installed": self.receipt_path.is_file(),
            "activationAllowed": False,
            "remainingGates": list(self.manifest.activation_gates),
        }

    def _download(self, destination: Path) -> None:
        artifact = self.manifest.artifact
        partial = destination.with_suffix(destination.suffix + ".part")
        offset = partial.stat().st_size if partial.exists() else 0
        if offset > artifact.bytes:
            partial.unlink()
            offset = 0
        headers = {"User-Agent": "Interactive-NPCs-local-STT-pack/2.0"}
        if offset:
            headers["Range"] = f"bytes={offset}-"
        request = urllib.request.Request(artifact.url, headers=headers)
        try:
            response = self._opener(request, timeout=60)
        except (OSError, urllib.error.URLError) as exc:
            raise PackError("download_failed", "local STT archive download failed") from exc
        status = getattr(response, "status", response.getcode())
        if offset and status != 206:
            partial.unlink(missing_ok=True)
            offset = 0
        mode = "ab" if offset and status == 206 else "wb"
        total = offset
        try:
            with partial.open(mode) as stream:
                while True:
                    block = response.read(1024 * 1024)
                    if not block:
                        break
                    total += len(block)
                    if total > artifact.bytes:
                        raise PackError("artifact_size_mismatch", "download exceeded the pinned artifact size")
                    stream.write(block)
                stream.flush()
                os.fsync(stream.fileno())
        finally:
            response.close()
        if total != artifact.bytes:
            raise PackError("artifact_size_mismatch", "download size does not match the pinned artifact")
        if sha256_file(partial) != artifact.sha256:
            partial.unlink(missing_ok=True)
            raise PackError("artifact_digest_mismatch", "download SHA-256 does not match the pinned artifact")
        os.replace(partial, destination)

    @staticmethod
    def _safe_member_path(member: tarfile.TarInfo, expected_top_level: str) -> PurePosixPath:
        name = member.name.replace("\\", "/")
        path = PurePosixPath(name)
        if path.is_absolute() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
            raise PackError("unsafe_archive", "archive contains an unsafe path")
        if path.parts[0] != expected_top_level:
            raise PackError("unsafe_archive", "archive top-level directory is unexpected")
        if member.issym() or member.islnk() or member.isdev() or member.isfifo():
            raise PackError("unsafe_archive", "archive links and special files are forbidden")
        if not (member.isdir() or member.isfile()):
            raise PackError("unsafe_archive", "archive contains an unsupported entry type")
        return path

    def _safe_extract(self, archive_path: Path, staging: Path) -> None:
        artifact = self.manifest.artifact
        with tarfile.open(archive_path, mode="r:gz") as archive:
            members = archive.getmembers()
            if not members or len(members) > artifact.maximum_entries:
                raise PackError("unsafe_archive", "archive entry count exceeds the allowed bound")
            expanded = 0
            normalized: list[tuple[tarfile.TarInfo, PurePosixPath]] = []
            seen: set[PurePosixPath] = set()
            for member in members:
                path = self._safe_member_path(member, artifact.required_top_level)
                if path in seen:
                    raise PackError("unsafe_archive", "archive contains duplicate paths")
                seen.add(path)
                expanded += max(0, member.size)
                if expanded > artifact.maximum_expanded_bytes:
                    raise PackError("unsafe_archive", "archive expanded size exceeds the allowed bound")
                normalized.append((member, path))
            for member, path in normalized:
                destination = staging.joinpath(*path.parts)
                if member.isdir():
                    destination.mkdir(parents=True, exist_ok=True)
                    continue
                destination.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                if source is None:
                    raise PackError("unsafe_archive", "archive file data is unavailable")
                with source, destination.open("xb") as output:
                    shutil.copyfileobj(source, output, length=1024 * 1024)
                    output.flush()
                    os.fsync(output.fileno())
                if destination.stat().st_size != member.size:
                    raise PackError("unsafe_archive", "archive file length changed during extraction")

    def _inventory(self, install_root: Path) -> list[dict[str, Any]]:
        records: list[dict[str, Any]] = []
        for path in sorted(install_root.rglob("*")):
            if path.is_symlink():
                raise PackError("install_invalid", "installed pack contains a symbolic link")
            if not path.is_file() or path.name == ".npc-pack-receipt.json":
                continue
            relative = path.relative_to(install_root).as_posix()
            records.append({"path": relative, "bytes": path.stat().st_size, "sha256": sha256_file(path)})
        return records

    def _required_files_exist(self, content_root: Path) -> None:
        for relative in self.manifest.required_files:
            candidate = content_root.joinpath(*PurePosixPath(relative).parts)
            if not candidate.is_file() or candidate.is_symlink():
                raise PackError("install_invalid", "installed pack is missing a required file")

    def install(self, *, confirmed: bool) -> dict[str, Any]:
        if not confirmed:
            raise PackError("confirmation_required", "explicit license and model download confirmation is required")
        with PackLock(self._lock_path):
            if self.receipt_path.is_file():
                verified = self.verify(acquire_lock=False)
                return {**verified, "idempotent": True}
            self.pack_root.parent.mkdir(parents=True, exist_ok=True)
            staging = self.pack_root.parent / f".{self.pack_root.name}.installing-{uuid.uuid4().hex}"
            staging.mkdir(mode=0o700)
            archive_path = staging / "artifact.tar.gz"
            try:
                self._download(archive_path)
                self._safe_extract(archive_path, staging)
                content_root = staging / self.manifest.installed_root
                self._required_files_exist(content_root)
                artifact_store = staging / ".artifacts"
                artifact_store.mkdir()
                retained_archive = artifact_store / f"{self.manifest.artifact.sha256}.tar.gz"
                os.replace(archive_path, retained_archive)
                if self.manifest.license_notice_path is not None:
                    notices = staging / ".notices"
                    notices.mkdir()
                    installed_notice = notices / "MOONSHINE-v0.1.5-LICENSE.txt"
                    shutil.copyfile(self.manifest.license_notice_path, installed_notice)
                    if sha256_file(installed_notice) != self.manifest.license_notice_sha256:
                        raise PackError("install_invalid", "installed license notice digest does not match")
                inventory = self._inventory(staging)
                receipt = {
                    "schemaVersion": "npc.local-model-pack-receipt/v1",
                    "receiptId": str(uuid.uuid4()),
                    "operation": "install",
                    "packId": self.manifest.pack_id,
                    "revision": self.manifest.revision,
                    "artifactSha256": self.manifest.artifact.sha256,
                    "installedAtUnixMs": int(time.time() * 1000),
                    "license": self.manifest.license_spdx,
                    "licenseEvidence": self.manifest.license_evidence,
                    "activationAllowed": False,
                    "remainingGates": list(self.manifest.activation_gates[3:]),
                    "files": inventory,
                }
                _atomic_json(staging / ".npc-pack-receipt.json", receipt)
                os.replace(staging, self.pack_root)
                return receipt
            except Exception:
                if staging.exists():
                    shutil.rmtree(staging)
                raise

    def verify(self, *, acquire_lock: bool = True) -> dict[str, Any]:
        if acquire_lock:
            with PackLock(self._lock_path):
                return self.verify(acquire_lock=False)
        try:
            receipt = json.loads(self.receipt_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise PackError("install_missing", "local STT pack receipt is missing or invalid") from exc
        if (
            receipt.get("packId") != self.manifest.pack_id
            or receipt.get("revision") != self.manifest.revision
            or receipt.get("artifactSha256") != self.manifest.artifact.sha256
        ):
            raise PackError("install_invalid", "local STT pack receipt does not match the manifest")
        archive = self.pack_root / ".artifacts" / f"{self.manifest.artifact.sha256}.tar.gz"
        if not archive.is_file() or archive.stat().st_size != self.manifest.artifact.bytes:
            raise PackError("install_invalid", "retained immutable artifact is missing or has the wrong size")
        if sha256_file(archive) != self.manifest.artifact.sha256:
            raise PackError("install_invalid", "retained immutable artifact digest does not match")
        self._required_files_exist(self.pack_root / self.manifest.installed_root)
        expected = receipt.get("files")
        if not isinstance(expected, list):
            raise PackError("install_invalid", "installed file inventory is missing")
        expected_by_path: dict[str, dict[str, Any]] = {}
        for record in expected:
            if not isinstance(record, dict) or set(record) != {"path", "bytes", "sha256"}:
                raise PackError("install_invalid", "installed file inventory is malformed")
            try:
                canonical = _validate_relative_posix(record["path"], "installed file inventory path")
            except PackError as exc:
                raise PackError("install_invalid", "installed file inventory path is unsafe")
            if canonical in expected_by_path:
                raise PackError("install_invalid", "installed file inventory contains duplicate paths")
            relative = PurePosixPath(canonical)
            path = self.pack_root.joinpath(*relative.parts)
            if (
                not path.is_file()
                or path.is_symlink()
                or path.stat().st_size != int(record["bytes"])
                or sha256_file(path) != record["sha256"]
            ):
                raise PackError("install_invalid", "installed file inventory verification failed")
            expected_by_path[canonical] = record
        actual_by_path = {record["path"]: record for record in self._inventory(self.pack_root)}
        if actual_by_path != expected_by_path:
            raise PackError("install_invalid", "installed pack contains files outside its immutable inventory")
        return {
            "receiptId": receipt.get("receiptId"),
            "operation": "verify",
            "packId": self.manifest.pack_id,
            "revision": self.manifest.revision,
            "verified": True,
            "activationAllowed": False,
            "remainingGates": receipt.get("remainingGates", []),
            "fileCount": len(expected),
        }

    def _quarantine(self, operation: str) -> Path | None:
        if not self.pack_root.exists():
            return None
        quarantine_root = self.root / ".quarantine"
        quarantine_root.mkdir(parents=True, exist_ok=True)
        destination = quarantine_root / f"{_validate_leaf(self.manifest.pack_id, 'packId')}-{int(time.time())}-{uuid.uuid4().hex}"
        os.replace(self.pack_root, destination)
        _atomic_json(
            destination / ".npc-quarantine-receipt.json",
            {
                "schemaVersion": "npc.local-model-pack-quarantine/v1",
                "operation": operation,
                "packId": self.manifest.pack_id,
                "revision": self.manifest.revision,
                "quarantinedAtUnixMs": int(time.time() * 1000),
            },
        )
        return destination

    def repair(self, *, confirmed: bool) -> dict[str, Any]:
        if not confirmed:
            raise PackError("confirmation_required", "explicit license and model download confirmation is required")
        with PackLock(self._lock_path):
            try:
                return {**self.verify(acquire_lock=False), "idempotent": True}
            except PackError:
                quarantine = self._quarantine("repair")
            # install obtains the same lock, so perform the body after releasing it.
        result = self.install(confirmed=True)
        result["operation"] = "repair"
        result["quarantinedPreviousInstall"] = quarantine is not None
        return result

    def remove(self) -> dict[str, Any]:
        with PackLock(self._lock_path):
            quarantine = self._quarantine("remove")
            if quarantine is None:
                return {
                    "operation": "remove",
                    "packId": self.manifest.pack_id,
                    "revision": self.manifest.revision,
                    "removed": False,
                    "idempotent": True,
                }
            shutil.rmtree(quarantine)
            return {
                "operation": "remove",
                "packId": self.manifest.pack_id,
                "revision": self.manifest.revision,
                "removed": True,
                "recoverable": False,
            }
