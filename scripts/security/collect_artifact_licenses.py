#!/usr/bin/env python3
"""Collect exact LICENSE/COPYING/NOTICE files for an artifact-scoped dependency set."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import urllib.parse
from pathlib import Path


LICENSE_NAME = re.compile(r"^(?:licen[cs]e|copying|notice|copyright|unlicense)(?:[._-].*)?$", re.IGNORECASE)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def split_purl(purl: str) -> tuple[str, str, str]:
    match = re.fullmatch(r"pkg:(cargo|npm)/(.+)@([^@]+)", purl)
    if not match:
        raise ValueError(f"unsupported purl: {purl}")
    return match.group(1), urllib.parse.unquote(match.group(2)), match.group(3)


def metadata_path(value: str) -> Path:
    match = re.match(r"^([A-Za-z]):[\\/](.*)$", value)
    if os.name != "nt" and match:
        return Path("/mnt") / match.group(1).lower() / Path(match.group(2).replace("\\", "/"))
    return Path(value)


def cargo_sources(metadata_paths: list[Path]) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for path in metadata_paths:
        document = json.loads(path.read_text(encoding="utf-8"))
        for package in document.get("packages", []):
            purl = f"pkg:cargo/{urllib.parse.quote(str(package['name']), safe='')}@{package['version']}"
            result[purl] = metadata_path(str(package["manifest_path"])).resolve().parent
    return result


def npm_sources(roots: list[Path], wanted: set[tuple[str, str]]) -> dict[tuple[str, str], Path]:
    result: dict[tuple[str, str], Path] = {}
    for root in roots:
        for directory, child_dirs, files in os.walk(root, followlinks=False):
            child_dirs[:] = [name for name in child_dirs if name != ".bin"]
            if "package.json" not in files:
                continue
            package_path = Path(directory) / "package.json"
            try:
                package = json.loads(package_path.read_text(encoding="utf-8"))
                identity = (str(package["name"]), str(package["version"]))
            except (OSError, KeyError, json.JSONDecodeError):
                continue
            if identity in wanted and identity not in result:
                result[identity] = Path(directory).resolve()
    return result


def license_files(package_root: Path, repo_root: Path, purl: str) -> list[Path]:
    files = sorted(path for path in package_root.iterdir() if path.is_file() and LICENSE_NAME.match(path.name))
    if files:
        return files
    try:
        package_root.relative_to(repo_root)
        is_workspace_package = True
    except ValueError:
        is_workspace_package = False
    if is_workspace_package and (repo_root / "LICENSE").is_file():
        return [repo_root / "LICENSE"]
    return []


def override_sources(
    package_root: Path,
    purl: str,
    declared_spdx: str,
    override: dict,
    override_files: dict,
    repo_root: Path,
) -> list[Path]:
    if override.get("declared_spdx") != declared_spdx:
        raise ValueError(f"override SPDX does not match ledger for {purl}")
    revision = str(override.get("crate_source_revision", ""))
    vcs_path = package_root / ".cargo_vcs_info.json"
    cargo_orig_path = package_root / "Cargo.toml.orig"
    if not re.fullmatch(r"[a-f0-9]{40}", revision) or not vcs_path.is_file() or not cargo_orig_path.is_file():
        raise ValueError(f"override lacks pinned crate source linkage for {purl}")
    try:
        vcs = json.loads(vcs_path.read_text(encoding="utf-8"))
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read crate VCS linkage for {purl}: {exc}") from exc
    if vcs.get("git", {}).get("sha1") != revision or sha256(vcs_path) != override.get("crate_vcs_info_sha256"):
        raise ValueError(f"crate VCS linkage differs from override for {purl}")
    if override.get("vcs_path") != vcs.get("path_in_vcs", "") or sha256(cargo_orig_path) != override.get("cargo_toml_orig_sha256"):
        raise ValueError(f"crate path/Cargo metadata differs from override for {purl}")
    readme_hash = override.get("readme_sha256")
    if readme_hash:
        readmes = sorted(package_root.glob("README*"))
        if len(readmes) != 1 or sha256(readmes[0]) != readme_hash:
            raise ValueError(f"crate README differs from override for {purl}")
    file_ids = override.get("license_file_ids")
    if not isinstance(file_ids, list):
        file_ids = [override.get("license_file_id")]
    if not file_ids or any(not isinstance(file_id, str) for file_id in file_ids):
        raise ValueError(f"override has no exact license bodies for {purl}")
    sources: list[Path] = []
    required_link_targets: set[str] = set()
    for file_id in file_ids:
        license_file = override_files.get(file_id, {})
        override_path = repo_root / str(license_file.get("path", ""))
        if not override_path.is_file() or sha256(override_path) != license_file.get("sha256"):
            raise ValueError(f"override legal body differs for {purl}: {file_id}")
        for rewrite in license_file.get("markdown_link_rewrites", []):
            target = rewrite.get("vendored")
            if not isinstance(target, str) or Path(target).name != target:
                raise ValueError(f"override legal body has unsafe Markdown target for {purl}: {file_id}")
            required_link_targets.add(target)
        sources.append(override_path)
    missing_link_targets = sorted(required_link_targets - {source.name for source in sources})
    if missing_link_targets:
        raise ValueError(f"override legal body has unresolved collected Markdown targets for {purl}: {missing_link_targets}")
    return sources


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--scope", type=Path, required=True)
    parser.add_argument("--cargo-metadata", type=Path, action="append", default=[])
    parser.add_argument("--node-modules", type=Path, action="append", default=[])
    parser.add_argument("--license-ledger", type=Path)
    parser.add_argument("--license-material-overrides", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    ledger_path = args.license_ledger or root / "packaging/security/dependency-licenses.json"
    overrides_path = args.license_material_overrides or root / "packaging/security/license-material-overrides.json"
    try:
        scope = json.loads(args.scope.read_text(encoding="utf-8"))
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))["components"]
        required = sorted(purl for purl, value in scope["components"].items() if value in {"required", "optional"})
        cargo_map = cargo_sources([path.resolve() for path in args.cargo_metadata])
    except (OSError, KeyError, ValueError, json.JSONDecodeError) as exc:
        parser.error(f"cannot read license-material inputs: {exc}")
    npm_wanted = {(name, version) for purl in required for ecosystem, name, version in [split_purl(purl)] if ecosystem == "npm"}
    override_components = {}
    override_files = {}
    if overrides_path.is_file():
        try:
            overrides_document = json.loads(overrides_path.read_text(encoding="utf-8"))
            override_components = overrides_document["components"]
            override_files = overrides_document["license_files"]
        except (OSError, KeyError, json.JSONDecodeError) as exc:
            parser.error(f"cannot read license-material overrides: {exc}")
    elif args.license_material_overrides:
        parser.error(f"license-material overrides file is missing: {overrides_path}")
    npm_map = npm_sources([path.resolve() for path in args.node_modules], npm_wanted)
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        parser.error(f"license output directory must be absent or empty: {out}")
    out.mkdir(parents=True, exist_ok=True)
    missing: list[str] = []
    index: dict[str, list[dict[str, object]]] = {}
    for ordinal, purl in enumerate(required, 1):
        ecosystem, name, version = split_purl(purl)
        package_root = cargo_map.get(purl) if ecosystem == "cargo" else npm_map.get((name, version))
        if package_root is None or not package_root.is_dir() or purl not in ledger:
            missing.append(purl)
            continue
        sources = license_files(package_root, root, purl)
        override = override_components.get(purl)
        if not sources and override:
            try:
                sources = override_sources(package_root, purl, ledger[purl], override, override_files, root)
            except ValueError as exc:
                parser.error(str(exc))
        if not sources:
            missing.append(purl)
            continue
        component_dir = out / "components" / f"{ordinal:04d}-{re.sub(r'[^A-Za-z0-9._-]+', '_', name)}-{version}"
        component_dir.mkdir(parents=True)
        records = []
        for source in sources:
            destination = component_dir / source.name
            shutil.copyfile(source, destination)
            relative = destination.relative_to(out).as_posix()
            records.append({"path": relative, "sha256": sha256(destination), "bytes": destination.stat().st_size})
        index[purl] = records
    if missing:
        parser.error(f"artifact dependencies lack exact license/notice files or source mapping ({len(missing)}): {missing}")
    document = {
        "schema_version": 1,
        "artifact_id": scope["artifact_id"],
        "scope_sha256": sha256(args.scope),
        "components": index,
    }
    index_path = out / "THIRD-PARTY-LICENSE-FILES.json"
    encoded = (json.dumps(document, indent=2, sort_keys=True) + "\n").encode()
    index_path.write_bytes(encoded)
    print(json.dumps({"status": "collected", "artifact_id": scope["artifact_id"], "components": len(index), "license_files": sum(len(value) for value in index.values()), "index": str(index_path), "sha256": hashlib.sha256(encoded).hexdigest()}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
