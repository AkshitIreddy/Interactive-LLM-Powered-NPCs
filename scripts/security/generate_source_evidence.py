#!/usr/bin/env python3
"""Emit local source identity without reading ignored credentials or build output."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
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
        if path.is_symlink():
            raise RuntimeError(f"profile corpus entry must not be a symlink: {path.relative_to(root)}")
        relative = path.relative_to(root).as_posix()
        content_hash = sha256(path)
        digest.update(relative.encode("utf-8") + b"\0" + content_hash.encode("ascii") + b"\0")
        count += 1
    if not count:
        raise RuntimeError("profile corpus is empty")
    return {"path": "profiles/games/*/profile.json", "files": count, "sha256": digest.hexdigest()}


def working_tree_changes(root: Path) -> list[dict[str, object]]:
    """Return a content-addressed manifest of every non-ignored Git change."""
    raw = git(
        root,
        "-c",
        "status.renames=false",
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
    )
    changes: list[dict[str, object]] = []
    for record in filter(None, raw.decode("utf-8", "surrogateescape").split("\0")):
        if len(record) < 4 or record[2] != " ":
            raise RuntimeError("git status returned an unsupported porcelain record")
        status = record[:2]
        relative = record[3:].replace("\\", "/")
        if relative.startswith(EXCLUDED_PREFIXES) or "/.secrets/" in relative:
            continue
        path = root / relative
        entry: dict[str, object] = {"path": relative, "status": status}
        # Check the directory entry before is_file(), which follows symlinks.
        # Source identity binds the link text and must never read a target that
        # can live outside the repository.
        if path.is_symlink():
            target = os.fsencode(os.readlink(path))
            entry.update(kind="symlink", sha256=hashlib.sha256(target).hexdigest(), size_bytes=len(target))
        elif path.is_file():
            entry.update(kind="file", sha256=sha256(path), size_bytes=path.stat().st_size)
        else:
            entry.update(kind="deleted", sha256=None, size_bytes=0)
        changes.append(entry)
    return sorted(changes, key=lambda item: str(item["path"]))


def collect_source_evidence(root: Path) -> dict[str, object]:
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
        # is_file() dereferences links. Bind the link target text instead so a
        # target outside the source tree cannot be read into package evidence.
        if path.is_symlink():
            marker = hashlib.sha256(os.fsencode(os.readlink(path))).hexdigest()
            source_files += 1
        elif path.is_file():
            marker = sha256(path)
            source_files += 1
        else:
            marker = "deleted"
            deleted_files += 1
        source_digest.update(relative.encode("utf-8", "surrogateescape") + b"\0" + marker.encode("ascii") + b"\0")

    changes = working_tree_changes(root)
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
        "working_tree_changes": changes,
        "inputs": {},
    }
    inputs = evidence["inputs"]
    assert isinstance(inputs, dict)
    for relative in EVIDENCE_PATHS:
        path = root / relative
        if path.is_symlink() or not path.is_file():
            raise RuntimeError(f"required evidence input is missing: {relative}")
        inputs[relative] = {"sha256": sha256(path), "size_bytes": path.stat().st_size}
    inputs["profile_corpus"] = corpus_digest(list((root / "profiles/games").glob("*/profile.json")), root)
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    first = collect_source_evidence(root)
    evidence = collect_source_evidence(root)
    if first != evidence:
        raise RuntimeError("source changed while source identity evidence was being generated")
    encoded = (json.dumps(evidence, indent=2, sort_keys=True) + "\n").encode()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(encoded)
    print(json.dumps({
        "output": str(args.out),
        "dirty": evidence["dirty"],
        "head_commit": evidence["head_commit"],
        "sha256": hashlib.sha256(encoded).hexdigest(),
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
