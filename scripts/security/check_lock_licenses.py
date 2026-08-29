#!/usr/bin/env python3
"""Offline, fail-closed license and provenance validation for committed locks."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from lock_inventory import InventoryError, load_inventory


LICENSE_TOKEN = re.compile(r"[A-Za-z0-9][A-Za-z0-9.-]*(?:\+)?")
OPERATORS = {"AND", "OR", "WITH"}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        inventory, sources = load_inventory(root)
    except InventoryError as exc:
        parser.error(str(exc))
    ledger_path = root / "packaging/security/dependency-licenses.json"
    policy_path = root / "packaging/security/license-policy.json"
    try:
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        policy = json.loads(policy_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        parser.error(f"cannot read license policy inputs: {exc}")
    expected = {item["purl"]: item for item in inventory}
    declared = ledger.get("components")
    if not isinstance(declared, dict):
        parser.error("dependency license ledger has no component map")
    if ledger.get("source_locks") != sources:
        parser.error("dependency license ledger source-lock list is stale")
    missing = sorted(set(expected) - set(declared))
    extra = sorted(set(declared) - set(expected))
    if missing or extra:
        parser.error(f"license ledger does not exactly match locks (missing={len(missing)}, stale={len(extra)})")

    allowed = set(policy.get("allowed_identifiers", []))
    disallowed = set(policy.get("disallowed_identifiers", []))
    exceptions = policy.get("reviewed_exceptions", {})
    failures: list[str] = []
    for purl, expression in sorted(declared.items()):
        if not isinstance(expression, str) or not expression.strip():
            failures.append(f"{purl}: unknown license")
            continue
        identifiers = {token for token in LICENSE_TOKEN.findall(expression) if token not in OPERATORS}
        exception = exceptions.get(purl)
        forbidden = identifiers & disallowed
        if forbidden:
            sources_for_component = {
                item["value"] for item in expected[purl].get("properties", [])
                if item.get("name") == "interactive-npcs:source-lock"
            }
            if not exception or exception.get("license") != expression or exception.get("distributed_in_base_installer") is not False or exception.get("required_source_lock") not in sources_for_component:
                failures.append(f"{purl}: disallowed license {expression}")
            continue
        unknown = identifiers - allowed
        if unknown:
            failures.append(f"{purl}: unreviewed identifier(s) {','.join(sorted(unknown))}")
    if failures:
        for failure in failures:
            print(failure)
        return 1

    model_root = root / "packaging/model-packs"
    for manifest_path in sorted(model_root.rglob("*.json")) if model_root.is_dir() else []:
        if re.search(r"\.(example|schema)\.json$", manifest_path.name):
            continue
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        for field in ("license", "source"):
            if not manifest.get(field):
                print(f"{manifest_path.relative_to(root)}: missing {field} provenance")
                return 1
    print(json.dumps({"status": "passed", "components": len(expected), "source_locks": sources, "reviewed_exceptions": len(exceptions)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
