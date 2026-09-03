#!/usr/bin/env python3
"""Create a deterministic artifact dependency scope from resolved build graphs."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import urllib.parse
from pathlib import Path

from lock_inventory import InventoryError, load_inventory


CARGO_LINE = re.compile(r"^(?P<name>.+?) v(?P<version>[^ ]+)(?: \(.*\))?(?: \(\*\))?$")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cargo_purls(path: Path) -> set[str]:
    result: set[str] = set()
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("warning:") or line.startswith("Blocking waiting"):
            continue
        line = re.sub(r" \(\*\)$", "", line)
        match = CARGO_LINE.match(line)
        if not match:
            raise ValueError(f"{path}:{line_number}: unrecognized cargo tree line {raw!r}")
        result.add(f"pkg:cargo/{match.group('name')}@{match.group('version')}")
    return result


def npm_purls(path: Path) -> set[str]:
    data = json.loads(path.read_text(encoding="utf-8"))
    values = data.get("purls") if isinstance(data, dict) else data
    if not isinstance(values, list) or not all(isinstance(value, str) and value.startswith("pkg:npm/") for value in values):
        raise ValueError(f"{path}: expected an array of npm purls or {{\"purls\": [...]}}")
    return set(values)


def pnpm_list_purls(path: Path) -> set[str]:
    document = json.loads(path.read_text(encoding="utf-8"))
    roots = document if isinstance(document, list) else [document]
    result: set[str] = set()

    def visit(node: object) -> None:
        if not isinstance(node, dict):
            return
        name, version = node.get("name"), node.get("version")
        if isinstance(name, str) and isinstance(version, str):
            result.add(f"pkg:npm/{urllib.parse.quote(name, safe='@/')}@{version}")
        dependencies = node.get("dependencies", {})
        if isinstance(dependencies, dict):
            for dependency_name, dependency in dependencies.items():
                if isinstance(dependency, dict) and "name" not in dependency:
                    dependency = {"name": dependency_name, **dependency}
                visit(dependency)

    for root in roots:
        if isinstance(root, dict):
            dependencies = root.get("dependencies", {})
            if isinstance(dependencies, dict):
                for dependency_name, dependency in dependencies.items():
                    if isinstance(dependency, dict) and "name" not in dependency:
                        dependency = {"name": dependency_name, **dependency}
                    visit(dependency)
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--artifact-id", required=True)
    parser.add_argument("--cargo-tree", type=Path, action="append", default=[])
    parser.add_argument("--npm-purls", type=Path, action="append", default=[])
    parser.add_argument("--pnpm-list-json", type=Path, action="append", default=[])
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        inventory, sources = load_inventory(root)
        selected: set[str] = set()
        inputs = []
        for path in args.cargo_tree:
            path = path.resolve()
            selected.update(cargo_purls(path))
            inputs.append({"kind": "cargo-tree-normal", "file": path.name, "sha256": digest(path)})
        for path in args.npm_purls:
            path = path.resolve()
            selected.update(npm_purls(path))
            inputs.append({"kind": "npm-production-purls", "file": path.name, "sha256": digest(path)})
        for path in args.pnpm_list_json:
            path = path.resolve()
            selected.update(pnpm_list_purls(path))
            inputs.append({"kind": "pnpm-list-production", "file": path.name, "sha256": digest(path)})
    except (OSError, ValueError, json.JSONDecodeError, InventoryError) as exc:
        parser.error(str(exc))
    if not selected:
        parser.error("artifact scope has no resolved components")
    known = {component["purl"] for component in inventory}
    unknown = sorted(selected - known)
    if unknown:
        parser.error(f"artifact graph contains components absent from committed locks: {unknown}")
    document = {
        "schema_version": 1,
        "artifact_id": args.artifact_id,
        "scope_semantics": "required means linked, bundled, or compiled into this artifact; absent lock components remain source-inventory-only",
        "source_locks": sources,
        "inputs": sorted(inputs, key=lambda item: (item["kind"], item["file"])),
        "components": {purl: "required" for purl in sorted(selected)},
    }
    encoded = (json.dumps(document, indent=2, sort_keys=True) + "\n").encode()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(encoded)
    print(json.dumps({"status": "generated", "artifact_id": args.artifact_id, "components": len(selected), "output": str(args.out), "sha256": hashlib.sha256(encoded).hexdigest()}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
