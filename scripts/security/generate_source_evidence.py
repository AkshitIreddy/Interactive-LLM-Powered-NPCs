#!/usr/bin/env python3
"""Emit local source identity without reading ignored credentials or build output."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


EXCLUDED_PREFIXES = (".secrets/", "artifacts/", "target/", "node_modules/")
EVIDENCE_PATHS = (
    "Cargo.lock",
    "apps/control/src-tauri/Cargo.lock",
    "pnpm-lock.yaml",
    "demo/readme/package-lock.json",
    "schemas/game-profile-v2.schema.json",
    "catalog/v1/catalog.json",
    "docs/product-rework/original-brief-gap-map.json",
    "docs/product-rework/original-brief-acceptance.md",
    "docs/requirements/local-review-evidence-report.md",
)
WINDOWS_DEVICE = re.compile(r"(?i)^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$")


def windows_unaddressable(relative: str) -> bool:
    return any(WINDOWS_DEVICE.match(part.rstrip(" .")) for part in relative.replace("\\", "/").split("/"))


def git(root: Path, *args: str) -> bytes:
    result = subprocess.run(
        ["git", "-C", str(root), *args], capture_output=True, check=False
    )
    if result.returncode:
        raise RuntimeError(f"git {' '.join(args)} failed with exit code {result.returncode}")
    return result.stdout


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def corpus_digest(paths: list[Path], root: Path) -> dict:
    digest = hashlib.sha256()
    count = 0
    for path in sorted(paths, key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix()
        content_hash = sha256(path)
        digest.update(relative.encode("utf-8") + b"\0" + content_hash.encode("ascii") + b"\0")
        count += 1
    if not count:
        raise RuntimeError("profile corpus is empty")
    return {"path": "profiles/games/*/profile.json", "files": count, "sha256": digest.hexdigest()}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    head = git(root, "rev-parse", "HEAD").decode().strip()
    status = git(root, "status", "--porcelain=v1", "-z", "--untracked-files=all")
    dirty = bool(status)
    candidates = sorted(set(filter(None, git(
        root, "ls-files", "-z", "--cached", "--others", "--exclude-standard"
    ).decode("utf-8", "surrogateescape").split("\0"))))
    safe_candidates = [
        item for item in candidates
        if not item.replace("\\", "/").startswith(EXCLUDED_PREFIXES)
        and "/.secrets/" not in item.replace("\\", "/")
    ]
    unaddressable = [item for item in safe_candidates if windows_unaddressable(item)]
    if unaddressable:
        raise RuntimeError(
            "source candidate contains Windows-reserved device path(s): "
            + ", ".join(unaddressable)
        )
    source_digest = hashlib.sha256()
    source_files = 0
    deleted_files = 0
    for relative in safe_candidates:
        path = root / relative
        if path.is_file():
            marker = sha256(path)
            source_files += 1
        elif path.is_symlink():
            marker = hashlib.sha256(path.readlink().as_posix().encode()).hexdigest()
            source_files += 1
        else:
            marker = "deleted"
            deleted_files += 1
        source_digest.update(relative.encode("utf-8", "surrogateescape") + b"\0" + marker.encode("ascii") + b"\0")

    evidence: dict[str, object] = {
        "schema_version": 1,
        "head_commit": head,
        "dirty": dirty,
        "source_tree_clean": not dirty,
        "distribution": "local-review-only",
        "source_candidate_digest": {
            "algorithm": "SHA-256",
            "sha256": source_digest.hexdigest(),
            "files": source_files,
            "tracked_missing": deleted_files,
            "includes_untracked_nonignored": True,
            "excludes_ignored_and_secrets": True,
        },
        "inputs": {},
    }
    inputs = evidence["inputs"]
    assert isinstance(inputs, dict)
    for relative in EVIDENCE_PATHS:
        path = root / relative
        if not path.is_file():
            raise RuntimeError(f"required evidence input is missing: {relative}")
        inputs[relative] = {"sha256": sha256(path), "size_bytes": path.stat().st_size}
    inputs["profile_corpus"] = corpus_digest(list((root / "profiles/games").glob("*/profile.json")), root)
    encoded = (json.dumps(evidence, indent=2, sort_keys=True) + "\n").encode()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(encoded)
    print(json.dumps({"output": str(args.out), "dirty": dirty, "head_commit": head, "sha256": hashlib.sha256(encoded).hexdigest()}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
