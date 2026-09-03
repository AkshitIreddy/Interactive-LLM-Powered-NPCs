#!/usr/bin/env python3
"""Emit a content-free installed Python runtime/license inventory."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path


PACKAGES = (
    "anyio",
    "certifi",
    "click",
    "colorama",
    "filelock",
    "flatbuffers",
    "fsspec",
    "h11",
    "hf-xet",
    "httpcore",
    "httpx",
    "huggingface-hub",
    "idna",
    "numpy",
    "onnxruntime",
    "packaging",
    "protobuf",
    "PyYAML",
    "tokenizers",
    "tqdm",
    "typing-extensions",
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wheelhouse", type=Path, required=True)
    args = parser.parse_args()
    values = []
    for name in PACKAGES:
        distribution = importlib.metadata.distribution(name)
        licenses = []
        for entry in distribution.files or ():
            leaf = entry.name.lower()
            if not (leaf.startswith("license") or leaf.startswith("copying")):
                continue
            path = distribution.locate_file(entry)
            if path.is_file():
                data = path.read_bytes()
                licenses.append(
                    {
                        "file": entry.as_posix(),
                        "size_bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(),
                    }
                )
        values.append(
            {
                "name": distribution.metadata["Name"],
                "version": distribution.version,
                "license_expression": distribution.metadata.get("License-Expression"),
                "license_files": licenses,
            }
        )
    wheels = []
    for wheel in sorted(args.wheelhouse.glob("*.whl"), key=lambda value: value.name.lower()):
        data = wheel.read_bytes()
        wheels.append(
            {
                "filename": wheel.name,
                "size_bytes": len(data),
                "sha256": hashlib.sha256(data).hexdigest(),
            }
        )
    print(
        json.dumps(
            {
                "schema": "npc.embedding-python-runtime-inventory/v1",
                "python_abi": "cp312-win_amd64",
                "packages": values,
                "wheels": wheels,
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
