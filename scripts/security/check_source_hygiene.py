#!/usr/bin/env python3
"""Reject source-tree artifacts that cannot belong to a Windows review build."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from pathlib import Path


WINDOWS_DEVICE = re.compile(
    r"(?i)^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$"
)
NATIVE_BUILD_FILENAMES = {
    "cmakecache.txt",
    "cmake_install.cmake",
    "install_manifest.txt",
    "build.ninja",
    ".ninja_deps",
    ".ninja_log",
}
NATIVE_BUILD_SUFFIXES = {
    ".sln",
    ".vcxproj",
    ".filters",
    ".user",
    ".obj",
    ".pdb",
    ".ilk",
    ".idb",
    ".tlog",
    ".exe",
    ".dll",
    ".lib",
    ".exp",
}


class HygieneError(ValueError):
    pass


def windows_unaddressable(relative: str) -> bool:
    return any(
        WINDOWS_DEVICE.match(part.rstrip(" ."))
        for part in relative.replace("\\", "/").split("/")
    )


def git_candidates(root: Path) -> list[str]:
    result = subprocess.run(
        [
            "git",
            "-C",
            str(root),
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise HygieneError(f"git source inventory failed with exit code {result.returncode}")
    return sorted(
        item
        for item in result.stdout.decode("utf-8", "surrogateescape").split("\0")
        if item
    )


def inspect(root: Path) -> dict[str, object]:
    violations: list[dict[str, str]] = []
    for relative in git_candidates(root):
        if windows_unaddressable(relative):
            violations.append({"path": relative, "rule": "windows-reserved-device-name"})

    with os.scandir(root) as entries:
        for entry in entries:
            if entry.name.startswith("="):
                violations.append(
                    {"path": entry.name, "rule": "root-shell-redirection-artifact"}
                )

    # Native configuration is deliberately out-of-source under repo/out or the
    # short task-owned LOCALAPPDATA cache. A repo-root build/ directory and any
    # build artifacts beneath native/ are stale/in-checkout review contaminants.
    for scan_root in (root / "native", root / "build"):
        if not scan_root.is_dir():
            continue
        for directory, directory_names, file_names in os.walk(scan_root, followlinks=False):
            directory_names[:] = [name for name in directory_names if name != ".git"]
            for file_name in file_names:
                lower = file_name.lower()
                suffix = Path(file_name).suffix.lower()
                if lower not in NATIVE_BUILD_FILENAMES and suffix not in NATIVE_BUILD_SUFFIXES:
                    continue
                path = Path(directory) / file_name
                violations.append(
                    {
                        "path": path.relative_to(root).as_posix(),
                        "rule": "native-in-checkout-build-artifact",
                    }
                )

    unique = {(item["path"], item["rule"]): item for item in violations}
    ordered = [unique[key] for key in sorted(unique)]
    if ordered:
        summary = ", ".join(
            f"{item['path']} ({item['rule']})" for item in ordered[:20]
        )
        if len(ordered) > 20:
            summary += f", ... and {len(ordered) - 20} more"
        raise HygieneError(f"source hygiene violations: {summary}")
    return {
        "schema_version": 1,
        "status": "passed",
        "reserved_name_count": 0,
        "root_redirection_artifact_count": 0,
        "native_build_artifact_count": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    try:
        result = inspect(args.root.resolve())
    except HygieneError as exc:
        parser.error(str(exc))
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
