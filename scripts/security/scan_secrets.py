#!/usr/bin/env python3
"""Scan worktree, index blobs, and reachable Git history without echoing secrets."""

from __future__ import annotations

import argparse
import math
import os
import re
import subprocess
from collections import Counter
from pathlib import Path


BLOCKED_NAMES = {"apikeys.json", ".env", ".env.local", "id_rsa", "id_ed25519", "credentials.json"}
TEXT_EXTENSIONS = {".cs", ".cpp", ".h", ".hpp", ".js", ".jsx", ".ts", ".tsx", ".rs", ".py", ".ps1", ".psm1", ".json", ".jsonc", ".yaml", ".yml", ".toml", ".xml", ".ini", ".env", ".md", ".txt", ".nsh"}
MAX_BYTES = 2 * 1024 * 1024
RULES = (
    ("private key", re.compile(rb"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----")),
    ("OpenAI-style key", re.compile(rb"\bsk-(?![A-Za-z0-9_-]*(?i:canary|fixture|do-not-log))(?!(?:test|example)-)[A-Za-z0-9_-]{20,}\b")),
    ("ElevenLabs key", re.compile(rb"\bsk_(?![A-Za-z0-9_-]*(?i:canary|fixture|do_not_log))(?!(?:test|example)_)[A-Za-z0-9_-]{20,}\b")),
    ("GitHub token", re.compile(rb"\bgh[opsu]_[A-Za-z0-9]{30,}\b")),
    ("Google API key", re.compile(rb"\bAIza[0-9A-Za-z_-]{30,}\b")),
    ("AWS access key", re.compile(rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b")),
)
CONTEXT = re.compile(rb"(?i)(api[_ -]?key|secret|access[_ -]?token|auth[_ -]?token|cohere|assembly\s*ai|eleven\s*labs|openai|anthropic|deepgram|cartesia|inworld|gemini|groq)")
NON_SECRET_CONTEXT = re.compile(rb"(?i)(sha-?256|sha-?512|checksum|integrity|digest|fingerprint|canary|fixture|placeholder|example|redacted)")
TOKEN = re.compile(rb"[A-Za-z0-9_-]{24,}")
ASSIGNED_TOKEN = re.compile(
    rb"(?i)(?:api[_ -]?key|secret|access[_ -]?token|auth[_ -]?token|cohere|assembly\s*ai|eleven\s*labs|openai|anthropic|deepgram|cartesia|inworld|gemini|groq)"
    rb"[^\n=:]{0,48}[=:]\s*[\"']?([A-Za-z0-9_-]{24,})"
)


def git(root: Path, *args: str, input_data: bytes | None = None) -> bytes:
    result = subprocess.run(["git", "-C", str(root), *args], input=input_data, capture_output=True, check=False)
    if result.returncode:
        raise RuntimeError(f"git {' '.join(args)} failed with exit code {result.returncode}")
    return result.stdout


def entropy(value: bytes) -> float:
    counts = Counter(value)
    length = len(value)
    return -sum((count / length) * math.log2(count / length) for count in counts.values())


def credential_shape(value: bytes) -> bool:
    """Reject prose-like kebab identifiers while retaining common token shapes."""
    digits = sum(48 <= byte <= 57 for byte in value)
    uppercase = sum(65 <= byte <= 90 for byte in value)
    is_hex = all(byte in b"0123456789abcdefABCDEF" for byte in value)
    return digits >= 2 and (uppercase >= 1 or is_hex)


def rules_for(content: bytes) -> set[str]:
    findings = {name for name, pattern in RULES if pattern.search(content)}
    lines = content.splitlines()
    for index, line in enumerate(lines):
        candidates = list(ASSIGNED_TOKEN.findall(line))
        if index and CONTEXT.search(lines[index - 1]) and not NON_SECRET_CONTEXT.search(lines[index - 1]):
            stripped_line = line.strip().strip(b"'\"=,;[]{}()")
            if TOKEN.fullmatch(stripped_line):
                candidates.append(stripped_line)
        if NON_SECRET_CONTEXT.search(line):
            candidates = []
        if any(len(token) >= 24 and entropy(token) >= 3.5 and credential_shape(token) for token in candidates):
            findings.add("high-entropy provider credential")
    return findings


def eligible(path: str) -> bool:
    leaf = Path(path).name
    return leaf in BLOCKED_NAMES or Path(leaf).suffix.lower() in TEXT_EXTENSIONS or leaf.startswith(".env")


def batch_blobs(root: Path, object_ids: list[str]) -> dict[str, bytes]:
    if not object_ids:
        return {}
    query = ("\n".join(object_ids) + "\n").encode()
    checked = git(root, "cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)", input_data=query)
    wanted = []
    for line in checked.decode().splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[1] == "blob" and int(parts[2]) <= MAX_BYTES:
            wanted.append(parts[0])
    payload = git(root, "cat-file", "--batch", input_data=("\n".join(wanted) + "\n").encode())
    result: dict[str, bytes] = {}
    offset = 0
    for expected in wanted:
        end = payload.find(b"\n", offset)
        header = payload[offset:end].decode().split()
        if len(header) != 3 or header[0] != expected or header[1] != "blob":
            raise RuntimeError("unexpected git cat-file batch response")
        size = int(header[2])
        start = end + 1
        result[expected] = payload[start:start + size]
        offset = start + size + 1
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--include-untracked", action="store_true")
    parser.add_argument("--no-history", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    findings: set[tuple[str, str, str]] = set()

    tracked = [item for item in git(root, "ls-files", "-z", "--cached").decode("utf-8", "surrogateescape").split("\0") if item]
    index_records: list[tuple[str, str]] = []
    for line in git(root, "ls-files", "-s", "-z").decode("utf-8", "surrogateescape").split("\0"):
        if not line:
            continue
        metadata, path = line.split("\t", 1)
        index_records.append((metadata.split()[1], path))
    index_blobs = batch_blobs(root, sorted({oid for oid, path in index_records if eligible(path)}))
    for oid, path in index_records:
        if not eligible(path):
            continue
        if Path(path).name in BLOCKED_NAMES:
            findings.add((path, "INDEX", "blocked credential filename"))
        for rule in rules_for(index_blobs.get(oid, b"")):
            findings.add((path, "INDEX", rule))

    for path in tracked:
        full_path = root / path
        if not eligible(path) or not full_path.is_file() or full_path.stat().st_size > MAX_BYTES:
            continue
        if Path(path).name in BLOCKED_NAMES:
            findings.add((path, "WORKTREE", "blocked credential filename"))
        for rule in rules_for(full_path.read_bytes()):
            findings.add((path, "WORKTREE", rule))

    if args.include_untracked:
        untracked = [item for item in git(root, "ls-files", "-z", "--others", "--exclude-standard").decode("utf-8", "surrogateescape").split("\0") if item]
        for path in untracked:
            full_path = root / path
            if not eligible(path) or not full_path.is_file() or full_path.stat().st_size > MAX_BYTES:
                continue
            if Path(path).name in BLOCKED_NAMES:
                findings.add((path, "UNTRACKED", "blocked credential filename"))
            for rule in rules_for(full_path.read_bytes()):
                findings.add((path, "UNTRACKED", rule))

    if not args.no_history:
        object_paths: dict[str, set[str]] = {}
        for line in git(root, "rev-list", "--objects", "--all").decode("utf-8", "surrogateescape").splitlines():
            if " " not in line:
                continue
            oid, path = line.split(" ", 1)
            if eligible(path):
                object_paths.setdefault(oid, set()).add(path)
        history_blobs = batch_blobs(root, sorted(object_paths))
        for oid, content in history_blobs.items():
            matched_rules = rules_for(content)
            paths = object_paths[oid]
            blocked_paths = {path for path in paths if Path(path).name in BLOCKED_NAMES}
            if not matched_rules and not blocked_paths:
                continue
            commits = git(root, "log", "--all", "--format=%H", "--find-object=" + oid, "-1").decode().splitlines()
            commit = commits[0] if commits else "REACHABLE_HISTORY"
            for path in paths:
                for rule in matched_rules:
                    findings.add((path, commit, rule))
                if path in blocked_paths:
                    findings.add((path, commit, "blocked credential filename"))

    if findings:
        print("Potential secrets found. Values and fingerprints are intentionally omitted.")
        print("file\tcommit_or_source\trule")
        for path, commit, rule in sorted(findings):
            print(f"{path}\t{commit}\t{rule}")
        return 1
    scope = "index/worktree" if args.no_history else "index/worktree and reachable history"
    if args.include_untracked:
        scope += ", plus non-ignored untracked files"
    print(f"Secret scan passed ({len(tracked)} tracked files; {scope} inspected).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
