#!/usr/bin/env python3
"""Generate a deterministic CycloneDX BOM from every committed dependency lock."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import uuid
from pathlib import Path

from lock_inventory import InventoryError, load_inventory


def deterministic_timestamp() -> str:
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    return dt.datetime.fromtimestamp(epoch, tz=dt.timezone.utc).replace(
        microsecond=0
    ).isoformat().replace("+00:00", "Z")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--license-ledger", type=Path)
    parser.add_argument("--source-evidence", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        components, sources = load_inventory(root)
    except InventoryError as exc:
        parser.error(str(exc))

    ledger_path = args.license_ledger or root / "packaging/security/dependency-licenses.json"
    if ledger_path.is_file():
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        licenses = ledger.get("components", {})
        for component in components:
            expression = licenses.get(component["purl"])
            if expression:
                component["licenses"] = [{"expression": expression}]

    root_ref = "pkg:generic/interactive-llm-powered-npcs@2.0.0-alpha.1"
    serial = uuid.uuid5(uuid.NAMESPACE_URL, "https://github.com/AkshitIreddy/Interactive-LLM-Powered-NPCs/v2-lock-inventory")
    metadata_properties = [
        {"name": "interactive-npcs:source-lock", "value": source}
        for source in sources
    ] + [{"name": "interactive-npcs:distribution", "value": "local-review-only"}]
    if args.source_evidence:
        try:
            evidence = json.loads(args.source_evidence.read_text(encoding="utf-8"))
            metadata_properties.extend([
                {"name": "interactive-npcs:git-head", "value": evidence["head_commit"]},
                {"name": "interactive-npcs:source-dirty", "value": str(bool(evidence["dirty"])).lower()},
                {"name": "interactive-npcs:source-candidate-sha256", "value": evidence["source_candidate_digest"]["sha256"]},
            ])
            for source_name, source_data in sorted(evidence.get("inputs", {}).items()):
                metadata_properties.append({"name": f"interactive-npcs:input-sha256:{source_name}", "value": source_data["sha256"]})
        except (OSError, KeyError, json.JSONDecodeError) as exc:
            parser.error(f"cannot attach source evidence: {exc}")
    bom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": f"urn:uuid:{serial}",
        "version": 1,
        "metadata": {
            "timestamp": deterministic_timestamp(),
            "tools": {"components": [{"type": "application", "name": "interactive-npcs-lock-sbom", "version": "2.0.0"}]},
            "component": {
                "type": "application", "bom-ref": root_ref,
                "name": "Interactive LLM Powered NPCs", "version": "2.0.0-alpha.1",
                "licenses": [{"license": {"id": "MIT"}}], "purl": root_ref,
            },
            "properties": metadata_properties,
        },
        "components": components,
        "dependencies": [{"ref": root_ref, "dependsOn": [item["bom-ref"] for item in components]}],
    }
    encoded = (json.dumps(bom, indent=2, sort_keys=True) + "\n").encode()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(encoded)
    print(json.dumps({
        "output": str(args.out), "components": len(components),
        "source_locks": sources, "sha256": hashlib.sha256(encoded).hexdigest(),
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
