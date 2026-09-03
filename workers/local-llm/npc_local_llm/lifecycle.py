"""Transactional install, verify, repair, and removal for optional local packs."""

from __future__ import annotations

import json
import os
import secrets
import shutil
import stat
import time
from contextlib import AbstractContextManager
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable

from .archive import secure_extract_zip
from .digest import canonical_json_bytes, fsync_parent, sha256_file
from .download import ArtifactFetcher, DownloadEvidence
from .errors import LocalLlmError
from .manifest import Artifact, ModelPack, RuntimeBundle

RECEIPT_SCHEMA = "npc.local-llm.install-receipt/v1"
WINDOWS_REPARSE_POINT = 0x400


@dataclass(frozen=True, slots=True)
class InstalledArtifact:
    artifact_id: str
    relative_path: str
    size_bytes: int
    sha256: str
    source_url: str


@dataclass(frozen=True, slots=True)
class InstallReceipt:
    schema: str
    unit_kind: str
    unit_id: str
    revision: str
    transaction_id: str
    installed_unix_ms: int
    runtime_abi: str
    artifacts: tuple[InstalledArtifact, ...]
    extracted_members: int = 0
    extracted_bytes: int = 0

    def to_json(self) -> dict[str, object]:
        value = asdict(self)
        value["artifacts"] = [asdict(artifact) for artifact in self.artifacts]
        return value


class PackLock(AbstractContextManager["PackLock"]):
    """OS-released lock: a crashed owner cannot leave a permanent stale lock."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.stream = None

    def __enter__(self) -> "PackLock":
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.stream = self.path.open("a+b")
        try:
            if os.name == "nt":
                import msvcrt

                self.stream.seek(0)
                if self.stream.tell() == 0:
                    self.stream.write(b"0")
                    self.stream.flush()
                self.stream.seek(0)
                msvcrt.locking(self.stream.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(self.stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except (OSError, BlockingIOError) as error:
            self.stream.close()
            self.stream = None
            raise LocalLlmError("install_busy", "another model-pack transaction is active", retryable=True) from error
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:  # type: ignore[no-untyped-def]
        if self.stream is None:
            return
        try:
            if os.name == "nt":
                import msvcrt

                self.stream.seek(0)
                msvcrt.locking(self.stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl

                fcntl.flock(self.stream.fileno(), fcntl.LOCK_UN)
        finally:
            self.stream.close()
            self.stream = None


def _assert_safe_root(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    resolved = root.resolve(strict=True)
    current = resolved
    while True:
        info = current.lstat()
        attributes = getattr(info, "st_file_attributes", 0)
        if stat.S_ISLNK(info.st_mode) or attributes & WINDOWS_REPARSE_POINT:
            raise LocalLlmError("unsafe_install_root", "install root crosses a link or reparse point")
        if current.parent == current:
            break
        current = current.parent
    return resolved


def _assert_beneath(root: Path, target: Path) -> None:
    try:
        target.resolve(strict=False).relative_to(root.resolve(strict=True))
    except (OSError, ValueError) as error:
        raise LocalLlmError("unsafe_install_path", "install path escapes the protected root") from error


def _safe_delete_tree(root: Path, target: Path) -> None:
    _assert_beneath(root, target)
    if not target.exists():
        return
    for current, directories, files in os.walk(target, topdown=False, followlinks=False):
        base = Path(current)
        for name in files:
            item = base / name
            info = item.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & WINDOWS_REPARSE_POINT:
                raise LocalLlmError("unsafe_remove", "installed tree contains a link or reparse point")
            item.unlink()
        for name in directories:
            item = base / name
            info = item.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & WINDOWS_REPARSE_POINT:
                raise LocalLlmError("unsafe_remove", "installed tree contains a link or reparse point")
            item.rmdir()
    target.rmdir()


def _write_receipt(path: Path, receipt: InstallReceipt) -> None:
    payload = canonical_json_bytes(receipt.to_json())
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb", buffering=0) as output:
        output.write(payload)
        output.flush()
        os.fsync(output.fileno())


def read_receipt(path: Path) -> InstallReceipt:
    try:
        value = json.loads(path.read_bytes())
        artifacts = tuple(InstalledArtifact(**entry) for entry in value["artifacts"])
        return InstallReceipt(
            schema=value["schema"],
            unit_kind=value["unit_kind"],
            unit_id=value["unit_id"],
            revision=value["revision"],
            transaction_id=value["transaction_id"],
            installed_unix_ms=value["installed_unix_ms"],
            runtime_abi=value["runtime_abi"],
            artifacts=artifacts,
            extracted_members=value.get("extracted_members", 0),
            extracted_bytes=value.get("extracted_bytes", 0),
        )
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise LocalLlmError("invalid_install_receipt", "installation receipt is missing or invalid") from error


class ModelPackLifecycle:
    def __init__(self, root: Path, fetcher: ArtifactFetcher) -> None:
        self.root = _assert_safe_root(root)
        self.fetcher = fetcher

    def install(
        self,
        pack: ModelPack,
        *,
        explicit_user_confirmation: bool,
        cancelled: Callable[[], bool] | None = None,
    ) -> InstallReceipt:
        if not explicit_user_confirmation:
            raise LocalLlmError("confirmation_required", "model download requires an explicit user action")
        target = self.root / "packs" / pack.pack_id / pack.revision
        _assert_beneath(self.root, target)
        with PackLock(self.root / ".locks" / f"{pack.pack_id}.lock"):
            if target.exists():
                return self.verify(pack)
            receipt, stage = self._stage_model(pack, cancelled=cancelled)
            target.parent.mkdir(parents=True, exist_ok=True)
            os.replace(stage, target)
            fsync_parent(target)
            return receipt

    def _stage_model(
        self,
        pack: ModelPack,
        *,
        cancelled: Callable[[], bool] | None,
    ) -> tuple[InstallReceipt, Path]:
        transaction_id = secrets.token_hex(16)
        stage = self.root / ".staging" / f"{pack.pack_id}-{transaction_id}"
        _assert_beneath(self.root, stage)
        stage.mkdir(parents=True, exist_ok=False, mode=0o700)
        installed: list[InstalledArtifact] = []
        try:
            for artifact in pack.artifacts:
                if artifact.kind != "file":
                    raise LocalLlmError("unsupported_artifact", "model pack may contain only regular file artifacts")
                target = stage.joinpath(*artifact.destination.parts)
                _assert_beneath(stage, target)
                target.parent.mkdir(parents=True, exist_ok=True)
                partial = target.with_name(f".{target.name}.{transaction_id}.partial")
                evidence = self.fetcher.fetch(artifact, partial, cancelled=cancelled)
                os.replace(partial, target)
                installed.append(_installed_artifact(artifact, evidence))
            receipt = InstallReceipt(
                schema=RECEIPT_SCHEMA,
                unit_kind="model_pack",
                unit_id=pack.pack_id,
                revision=pack.revision,
                transaction_id=transaction_id,
                installed_unix_ms=int(time.time() * 1000),
                runtime_abi=pack.runtime_abi,
                artifacts=tuple(installed),
            )
            _write_receipt(stage / "installation.receipt.json", receipt)
            self._verify_tree(pack, stage, receipt)
            return receipt, stage
        except Exception:
            if stage.exists():
                _safe_delete_tree(self.root, stage)
            raise

    def verify(self, pack: ModelPack) -> InstallReceipt:
        target = self.root / "packs" / pack.pack_id / pack.revision
        _assert_beneath(self.root, target)
        receipt = read_receipt(target / "installation.receipt.json")
        self._verify_tree(pack, target, receipt)
        return receipt

    def _verify_tree(self, pack: ModelPack, target: Path, receipt: InstallReceipt) -> None:
        if (
            receipt.schema != RECEIPT_SCHEMA
            or receipt.unit_kind != "model_pack"
            or receipt.unit_id != pack.pack_id
            or receipt.revision != pack.revision
            or receipt.runtime_abi != pack.runtime_abi
        ):
            raise LocalLlmError("install_receipt_mismatch", "installation receipt does not match the model pack")
        receipt_artifacts = {item.artifact_id: item for item in receipt.artifacts}
        if set(receipt_artifacts) != {artifact.artifact_id for artifact in pack.artifacts}:
            raise LocalLlmError("install_receipt_mismatch", "installation receipt artifact set is incomplete")
        for artifact in pack.artifacts:
            path = target.joinpath(*artifact.destination.parts)
            _assert_beneath(target, path)
            actual = sha256_file(path, expected_size=artifact.size_bytes)
            evidence = receipt_artifacts[artifact.artifact_id]
            if (
                actual != artifact.sha256
                or evidence.sha256 != artifact.sha256
                or evidence.size_bytes != artifact.size_bytes
                or evidence.relative_path != str(artifact.destination)
            ):
                raise LocalLlmError("installed_artifact_mismatch", "installed artifact does not match the manifest")

    def repair(
        self,
        pack: ModelPack,
        *,
        explicit_user_confirmation: bool,
        cancelled: Callable[[], bool] | None = None,
    ) -> InstallReceipt:
        if not explicit_user_confirmation:
            raise LocalLlmError("confirmation_required", "model repair requires an explicit user action")
        target = self.root / "packs" / pack.pack_id / pack.revision
        with PackLock(self.root / ".locks" / f"{pack.pack_id}.lock"):
            try:
                return self.verify(pack)
            except LocalLlmError:
                pass
            receipt, stage = self._stage_model(pack, cancelled=cancelled)
            quarantine = self.root / ".quarantine" / f"{pack.pack_id}-{secrets.token_hex(16)}"
            quarantine.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                os.replace(target, quarantine)
            try:
                target.parent.mkdir(parents=True, exist_ok=True)
                os.replace(stage, target)
            except Exception:
                if quarantine.exists() and not target.exists():
                    os.replace(quarantine, target)
                raise
            if quarantine.exists():
                _safe_delete_tree(self.root, quarantine)
            return receipt

    def remove(
        self,
        pack: ModelPack,
        *,
        explicit_user_confirmation: bool,
        require_unreferenced: bool,
        is_referenced: Callable[[str, str], bool],
    ) -> bool:
        if not explicit_user_confirmation:
            raise LocalLlmError("confirmation_required", "model removal requires an explicit user action")
        if not require_unreferenced or is_referenced(pack.pack_id, pack.revision):
            raise LocalLlmError("model_in_use", "model pack is active or referenced")
        target = self.root / "packs" / pack.pack_id / pack.revision
        with PackLock(self.root / ".locks" / f"{pack.pack_id}.lock"):
            if not target.exists():
                return False
            trash = self.root / ".trash" / f"{pack.pack_id}-{secrets.token_hex(16)}"
            trash.parent.mkdir(parents=True, exist_ok=True)
            os.replace(target, trash)
            _safe_delete_tree(self.root, trash)
            return True


class RuntimeBundleLifecycle:
    """Separate MIT runtime trust unit; never mutates the Apache model pack."""

    def __init__(self, root: Path, fetcher: ArtifactFetcher) -> None:
        self.root = _assert_safe_root(root)
        self.fetcher = fetcher

    def install(
        self,
        bundle: RuntimeBundle,
        variant_id: str,
        *,
        explicit_user_confirmation: bool,
        cancelled: Callable[[], bool] | None = None,
    ) -> InstallReceipt:
        if not explicit_user_confirmation:
            raise LocalLlmError("confirmation_required", "runtime download requires an explicit user action")
        variant = bundle.variant(variant_id)
        target = self.root / "runtimes" / bundle.abi / variant.variant_id
        with PackLock(self.root / ".locks" / f"runtime-{variant.variant_id}.lock"):
            if target.exists():
                return self.verify(bundle, variant_id)
            transaction_id = secrets.token_hex(16)
            stage = self.root / ".staging" / f"runtime-{variant.variant_id}-{transaction_id}"
            stage.mkdir(parents=True, exist_ok=False, mode=0o700)
            archive = stage / "runtime.zip.partial"
            try:
                evidence = self.fetcher.fetch(variant.artifact, archive, cancelled=cancelled)
                extracted = stage / "payload"
                members, expanded = secure_extract_zip(archive, extracted)
                archive.unlink()
                entrypoints = list(extracted.rglob(bundle.entrypoint))
                if len(entrypoints) != 1 or entrypoints[0].is_symlink():
                    raise LocalLlmError("runtime_entrypoint_mismatch", "runtime archive has an invalid entrypoint layout")
                receipt = InstallReceipt(
                    schema=RECEIPT_SCHEMA,
                    unit_kind="runtime_bundle",
                    unit_id=variant.variant_id,
                    revision=bundle.release_tag,
                    transaction_id=transaction_id,
                    installed_unix_ms=int(time.time() * 1000),
                    runtime_abi=bundle.abi,
                    artifacts=(_installed_artifact(variant.artifact, evidence),),
                    extracted_members=members,
                    extracted_bytes=expanded,
                )
                _write_receipt(stage / "installation.receipt.json", receipt)
                target.parent.mkdir(parents=True, exist_ok=True)
                os.replace(stage, target)
                return receipt
            except Exception:
                if stage.exists():
                    _safe_delete_tree(self.root, stage)
                raise

    def verify(self, bundle: RuntimeBundle, variant_id: str) -> InstallReceipt:
        variant = bundle.variant(variant_id)
        target = self.root / "runtimes" / bundle.abi / variant.variant_id
        receipt = read_receipt(target / "installation.receipt.json")
        if (
            receipt.schema != RECEIPT_SCHEMA
            or receipt.unit_kind != "runtime_bundle"
            or receipt.unit_id != variant.variant_id
            or receipt.revision != bundle.release_tag
            or receipt.runtime_abi != bundle.abi
            or len(receipt.artifacts) != 1
            or receipt.artifacts[0].sha256 != variant.artifact.sha256
            or receipt.artifacts[0].size_bytes != variant.artifact.size_bytes
        ):
            raise LocalLlmError("runtime_receipt_mismatch", "runtime receipt does not match the trust record")
        entrypoints = list((target / "payload").rglob(bundle.entrypoint))
        if len(entrypoints) != 1 or not entrypoints[0].is_file() or entrypoints[0].is_symlink():
            raise LocalLlmError("runtime_entrypoint_mismatch", "runtime entrypoint is missing or unsafe")
        return receipt

    def remove(
        self,
        bundle: RuntimeBundle,
        variant_id: str,
        *,
        explicit_user_confirmation: bool,
        require_unreferenced: bool,
        is_referenced: Callable[[str, str], bool],
    ) -> bool:
        if not explicit_user_confirmation:
            raise LocalLlmError("confirmation_required", "runtime removal requires an explicit user action")
        variant = bundle.variant(variant_id)
        if not require_unreferenced or is_referenced(bundle.abi, variant.variant_id):
            raise LocalLlmError("runtime_in_use", "runtime is active or referenced")
        target = self.root / "runtimes" / bundle.abi / variant.variant_id
        with PackLock(self.root / ".locks" / f"runtime-{variant.variant_id}.lock"):
            if not target.exists():
                return False
            trash = self.root / ".trash" / f"runtime-{variant.variant_id}-{secrets.token_hex(16)}"
            trash.parent.mkdir(parents=True, exist_ok=True)
            os.replace(target, trash)
            _safe_delete_tree(self.root, trash)
            return True


def _installed_artifact(artifact: Artifact, evidence: DownloadEvidence) -> InstalledArtifact:
    return InstalledArtifact(
        artifact_id=artifact.artifact_id,
        relative_path=str(artifact.destination),
        size_bytes=evidence.size_bytes,
        sha256=evidence.sha256,
        source_url=evidence.url,
    )
