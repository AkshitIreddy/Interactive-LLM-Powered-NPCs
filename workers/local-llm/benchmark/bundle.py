"""Exact local install/verification for the two-file upstream CUDA runtime."""

from __future__ import annotations

import json
import hashlib
import os
import secrets
import shutil
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from urllib.parse import urlparse

from npc_local_llm.archive import secure_extract_zip
from npc_local_llm.digest import canonical_json_bytes, sha256_file
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.manifest import safe_relative_path

RECEIPT_SCHEMA = "npc.local-llm.cuda-benchmark-install-receipt/v1"


@dataclass(frozen=True, slots=True)
class BundleAsset:
    asset_id: str
    filename: str
    size_bytes: int
    sha256: str
    expected_member_count: int
    expected_expanded_bytes: int
    extraction_ceiling_bytes: int
    required_files: tuple[PurePosixPath, ...]


@dataclass(frozen=True, slots=True)
class CudaBenchmarkBundle:
    path: Path
    release_tag: str
    source_commit: str
    cuda_runtime: str
    entrypoint: str
    assets: tuple[BundleAsset, ...]
    expected_member_count: int
    expected_expanded_bytes: int

    @classmethod
    def load(cls, path: Path) -> "CudaBenchmarkBundle":
        try:
            value = json.loads(path.read_bytes())
            if value["schema"] != "npc.local-llm.cuda-benchmark-bundle/v1":
                raise ValueError("unexpected schema")
            if value["purpose"] != "benchmark_only_not_production_activation":
                raise ValueError("unexpected purpose")
            if value["runtime"] != "llama.cpp" or value["backend"] != "cuda":
                raise ValueError("unexpected runtime")
            if value["platform"] != "windows-x86_64" or value["entrypoint"] != "llama-server.exe":
                raise ValueError("unexpected platform")
            if len(value["source_commit"]) != 40:
                raise ValueError("source commit must be immutable")
            assets: list[BundleAsset] = []
            for raw in value["assets"]:
                parsed = urlparse(raw["url"])
                if parsed.scheme != "https" or parsed.hostname != "github.com" or "/releases/download/" not in parsed.path:
                    raise ValueError("asset URL is not an immutable GitHub release asset")
                filename = Path(parsed.path).name
                if not filename.endswith(".zip"):
                    raise ValueError("runtime asset must be ZIP")
                required = tuple(safe_relative_path(item, "required_file") for item in raw["required_files"])
                assets.append(
                    BundleAsset(
                        asset_id=raw["id"],
                        filename=filename,
                        size_bytes=_positive_int(raw["size_bytes"]),
                        sha256=_sha256(raw["sha256"]),
                        expected_member_count=_positive_int(raw["expected_member_count"]),
                        expected_expanded_bytes=_positive_int(raw["expected_expanded_bytes"]),
                        extraction_ceiling_bytes=_positive_int(raw["extraction_ceiling_bytes"]),
                        required_files=required,
                    )
                )
            if len(assets) != 2 or len({item.asset_id for item in assets}) != 2:
                raise ValueError("CUDA runtime requires exactly two unique assets")
            merged = value["merged_payload"]
            if merged["strategy"] != "same_volume_hardlinks_no_overwrite":
                raise ValueError("unexpected merge strategy")
            bundle = cls(
                path=path.resolve(strict=True),
                release_tag=value["release_tag"],
                source_commit=value["source_commit"],
                cuda_runtime=value["cuda_runtime"],
                entrypoint=value["entrypoint"],
                assets=tuple(assets),
                expected_member_count=_positive_int(merged["expected_member_count"]),
                expected_expanded_bytes=_positive_int(merged["expected_expanded_bytes"]),
            )
            if sum(item.expected_member_count for item in assets) != bundle.expected_member_count:
                raise ValueError("merged member count is inconsistent")
            if sum(item.expected_expanded_bytes for item in assets) != bundle.expected_expanded_bytes:
                raise ValueError("merged expanded size is inconsistent")
            return bundle
        except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
            raise LocalLlmError("invalid_cuda_bundle", "CUDA benchmark bundle is missing or invalid") from error


def install_bundle(bundle: CudaBenchmarkBundle, archive_root: Path, install_root: Path) -> dict[str, object]:
    """Verify, extract, hardlink-merge, receipt, and atomically install."""
    install_root = install_root.resolve(strict=False)
    install_root.parent.mkdir(parents=True, exist_ok=True)
    if install_root.exists():
        return verify_bundle(bundle, install_root)
    stage = install_root.with_name(f".{install_root.name}.{secrets.token_hex(12)}.staging")
    stage.mkdir(parents=False, exist_ok=False)
    archive_evidence: list[dict[str, object]] = []
    file_evidence: list[dict[str, object]] = []
    try:
        extracts = stage / "extracts"
        extracts.mkdir()
        payload = stage / "payload"
        payload.mkdir()
        seen: set[str] = set()
        for index, asset in enumerate(bundle.assets):
            archive = archive_root / asset.filename
            actual_sha = sha256_file(archive, expected_size=asset.size_bytes)
            if actual_sha != asset.sha256:
                raise LocalLlmError("cuda_archive_mismatch", "CUDA runtime archive digest does not match the trust record")
            extracted = extracts / str(index)
            count, expanded = secure_extract_zip(
                archive,
                extracted,
                maximum_expanded_bytes=asset.extraction_ceiling_bytes,
                expected_member_count=asset.expected_member_count,
                expected_expanded_bytes=asset.expected_expanded_bytes,
            )
            archive_evidence.append(
                {
                    "asset_id": asset.asset_id,
                    "filename": asset.filename,
                    "size_bytes": asset.size_bytes,
                    "sha256": actual_sha,
                    "extracted_members": count,
                    "extracted_bytes": expanded,
                }
            )
            for source in sorted(item for item in extracted.rglob("*") if item.is_file()):
                relative = source.relative_to(extracted)
                folded = relative.as_posix().casefold()
                if folded in seen:
                    raise LocalLlmError("cuda_payload_collision", "CUDA runtime assets contain a path collision")
                seen.add(folded)
                target = payload / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                os.link(source, target)
            for required in asset.required_files:
                candidates = [item for item in extracted.rglob(required.name) if item.is_file()]
                if len(candidates) != 1:
                    raise LocalLlmError("cuda_required_file_missing", "CUDA runtime required file is not unique")
        payload_files = sorted(item for item in payload.rglob("*") if item.is_file())
        expanded_bytes = sum(item.stat().st_size for item in payload_files)
        if len(payload_files) != bundle.expected_member_count or expanded_bytes != bundle.expected_expanded_bytes:
            raise LocalLlmError("cuda_payload_identity_mismatch", "merged CUDA runtime identity is inconsistent")
        entrypoints = [item for item in payload_files if item.name.casefold() == bundle.entrypoint.casefold()]
        if len(entrypoints) != 1:
            raise LocalLlmError("cuda_entrypoint_mismatch", "CUDA runtime entrypoint is not unique")
        for item in payload_files:
            file_evidence.append(
                {
                    "path": item.relative_to(payload).as_posix(),
                    "size_bytes": item.stat().st_size,
                    "sha256": sha256_file(item, expected_size=item.stat().st_size),
                }
            )
        shutil.rmtree(extracts)
        receipt: dict[str, object] = {
            "schema": RECEIPT_SCHEMA,
            "release_tag": bundle.release_tag,
            "source_commit": bundle.source_commit,
            "cuda_runtime": bundle.cuda_runtime,
            "bundle_sha256": sha256_file(bundle.path),
            "archives": archive_evidence,
            "payload": {
                "member_count": len(file_evidence),
                "expanded_bytes": expanded_bytes,
                "files": file_evidence,
            },
        }
        receipt_path = stage / "installation.receipt.json"
        receipt_path.write_bytes(canonical_json_bytes(receipt) + b"\n")
        os.replace(stage, install_root)
        return verify_bundle(bundle, install_root)
    except Exception:
        if stage.exists():
            shutil.rmtree(stage)
        raise


def verify_bundle(
    bundle: CudaBenchmarkBundle, install_root: Path, archive_root: Path | None = None
) -> dict[str, object]:
    try:
        receipt = json.loads((install_root / "installation.receipt.json").read_bytes())
        if receipt["schema"] != RECEIPT_SCHEMA:
            raise ValueError("receipt schema")
        if receipt["release_tag"] != bundle.release_tag or receipt["source_commit"] != bundle.source_commit:
            raise ValueError("runtime revision")
        if receipt["cuda_runtime"] != bundle.cuda_runtime or receipt["bundle_sha256"] != sha256_file(bundle.path):
            raise ValueError("bundle identity")
        archives = {item["asset_id"]: item for item in receipt["archives"]}
        for asset in bundle.assets:
            evidence = archives[asset.asset_id]
            if evidence["filename"] != asset.filename or evidence["size_bytes"] != asset.size_bytes or evidence["sha256"] != asset.sha256:
                raise ValueError("archive evidence")
        payload = receipt["payload"]
        if payload["member_count"] != bundle.expected_member_count or payload["expanded_bytes"] != bundle.expected_expanded_bytes:
            raise ValueError("payload totals")
        expected = {item["path"]: item for item in payload["files"]}
        if archive_root is not None:
            archive_files = _payload_evidence_from_archives(bundle, archive_root)
            if archive_files != expected:
                raise ValueError("receipt payload is not derived from the pinned archives")
        actual_paths = {
            item.relative_to(install_root / "payload").as_posix()
            for item in (install_root / "payload").rglob("*")
            if item.is_file()
        }
        if actual_paths != set(expected):
            raise ValueError("payload file set")
        for relative, evidence in expected.items():
            path = install_root / "payload" / PurePosixPath(relative)
            if path.is_symlink() or evidence["sha256"] != sha256_file(path, expected_size=evidence["size_bytes"]):
                raise ValueError("payload file")
        return {"verified": True, "install_root": str(install_root), "receipt": receipt}
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise LocalLlmError("cuda_install_invalid", "installed CUDA benchmark runtime failed verification") from error


def _payload_evidence_from_archives(
    bundle: CudaBenchmarkBundle, archive_root: Path
) -> dict[str, dict[str, object]]:
    evidence: dict[str, dict[str, object]] = {}
    for asset in bundle.assets:
        archive_path = archive_root / asset.filename
        if sha256_file(archive_path, expected_size=asset.size_bytes) != asset.sha256:
            raise ValueError("pinned archive digest")
        with zipfile.ZipFile(archive_path, "r") as archive:
            members = archive.infolist()
            if len(members) != asset.expected_member_count:
                raise ValueError("pinned archive member count")
            if sum(member.file_size for member in members) != asset.expected_expanded_bytes:
                raise ValueError("pinned archive expanded size")
            for member in members:
                relative = safe_relative_path(member.filename.rstrip("/"), "archive_member").as_posix()
                if member.is_dir():
                    continue
                if relative in evidence:
                    raise ValueError("archive payload collision")
                digest = hashlib.sha256()
                total = 0
                with archive.open(member, "r") as source:
                    while True:
                        block = source.read(4 * 1_048_576)
                        if not block:
                            break
                        digest.update(block)
                        total += len(block)
                if total != member.file_size:
                    raise ValueError("archive member size")
                evidence[relative] = {"path": relative, "size_bytes": total, "sha256": digest.hexdigest()}
    return evidence


def _positive_int(value: object) -> int:
    if type(value) is not int or value <= 0:
        raise ValueError("positive integer required")
    return value


def _sha256(value: object) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(character not in "0123456789abcdef" for character in value):
        raise ValueError("lowercase SHA-256 required")
    return value
