#!/usr/bin/env python3
"""Generate a deterministic CycloneDX BOM from every committed dependency lock."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
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
    parser.add_argument("--distribution-ledger", type=Path)
    parser.add_argument("--distribution-profile", choices=("installer", "test-game"), default="installer")
    parser.add_argument("--artifact-scope", type=Path)
    parser.add_argument("--require-artifact-scope", action="store_true")
    parser.add_argument("--license-material-index", type=Path)
    parser.add_argument("--require-license-materials", action="store_true")
    parser.add_argument("--source-evidence", type=Path)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        components, sources = load_inventory(root)
    except InventoryError as exc:
        parser.error(str(exc))

    ledger_path = args.license_ledger or root / "packaging/security/dependency-licenses.json"
    licenses = {}
    if args.strict and not ledger_path.is_file():
        parser.error(f"license ledger is missing: {ledger_path}")
    if ledger_path.is_file():
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        licenses = ledger.get("components", {})
        missing_licenses = sorted(component["purl"] for component in components if component["purl"] not in licenses)
        if args.strict and missing_licenses:
            parser.error(f"license ledger is incomplete ({len(missing_licenses)} missing)")
    for component in components:
        if component["purl"] in licenses:
            component["licenses"] = [{"expression": licenses[component["purl"]]}]

    artifact_scope: dict[str, str] = {}
    artifact_id = "unresolved-source-lock-inventory"
    if args.require_artifact_scope and not args.artifact_scope:
        parser.error("artifact-scoped SBOM requires --artifact-scope")
    if args.artifact_scope:
        try:
            scope_document = json.loads(args.artifact_scope.read_text(encoding="utf-8"))
            artifact_scope = scope_document["components"]
            artifact_id = scope_document["artifact_id"]
        except (OSError, KeyError, json.JSONDecodeError) as exc:
            parser.error(f"cannot read artifact scope: {exc}")
        known_purls = {component["purl"] for component in components}
        unknown_purls = sorted(set(artifact_scope) - known_purls)
        invalid_scopes = sorted(purl for purl, scope in artifact_scope.items() if scope not in {"required", "optional", "excluded"})
        if scope_document.get("schema_version") != 1 or unknown_purls or invalid_scopes:
            parser.error(f"invalid artifact scope (unknown={unknown_purls}, invalid_scopes={invalid_scopes})")
    for component in components:
        scope = artifact_scope.get(component["purl"], "excluded")
        component["scope"] = scope
        component.setdefault("properties", []).append({
            "name": "interactive-npcs:artifact-scope-source",
            "value": "resolved-build-graph" if artifact_scope else "unresolved-source-lock-inventory",
        })
    license_material_index_sha256 = None
    if args.require_license_materials and not args.license_material_index:
        parser.error("artifact-scoped SBOM requires --license-material-index")
    if args.license_material_index:
        try:
            license_materials = json.loads(args.license_material_index.read_text(encoding="utf-8"))
            license_material_index_sha256 = hashlib.sha256(args.license_material_index.read_bytes()).hexdigest()
            material_components = license_materials["components"]
        except (OSError, KeyError, json.JSONDecodeError) as exc:
            parser.error(f"cannot read license-material index: {exc}")
        required_purls = {purl for purl, scope in artifact_scope.items() if scope in {"required", "optional"}}
        missing_materials = sorted(required_purls - set(material_components))
        stale_materials = sorted(set(material_components) - required_purls)
        if license_materials.get("schema_version") != 1 or license_materials.get("artifact_id") != artifact_id or missing_materials or stale_materials:
            parser.error(f"license-material index differs from artifact scope (missing={missing_materials}, stale={stale_materials})")
        for purl, records in material_components.items():
            if not records or any(not record.get("path") or not re.fullmatch(r"[a-f0-9]{64}", str(record.get("sha256", ""))) for record in records):
                parser.error(f"invalid license-material records for {purl}")

    distribution_path = args.distribution_ledger or root / "packaging/security/distribution-components.json"
    distribution_components = {}
    static_files = []
    distribution_scope = None
    if args.strict and not distribution_path.is_file():
        parser.error(f"distribution ledger is missing: {distribution_path}")
    if distribution_path.is_file():
        try:
            distribution = json.loads(distribution_path.read_text(encoding="utf-8"))
            distribution_components = distribution["components"]
            static_files = distribution["static_files"]
            distribution_scope = distribution["distribution_profiles"][args.distribution_profile]["scope"]
        except (OSError, KeyError, json.JSONDecodeError) as exc:
            parser.error(f"cannot read distribution ledger: {exc}")
    distribution_refs: dict[str, str] = {}
    for component_id, entry in sorted(distribution_components.items()):
        bom_ref = f"urn:interactive-npcs:distribution-component:{component_id}"
        distribution_refs[component_id] = bom_ref
        component_scope = "required" if entry["redistributed"] and distribution_scope in entry["scopes"] else "excluded"
        properties = [
            {"name": "interactive-npcs:component-id", "value": component_id},
            {"name": "interactive-npcs:distribution-class", "value": entry["distribution_class"]},
            {"name": "interactive-npcs:redistributed", "value": str(bool(entry["redistributed"])).lower()},
            {"name": "interactive-npcs:distribution-scopes", "value": ",".join(entry["scopes"])},
            {"name": "interactive-npcs:hash-policy", "value": entry["hash_policy"]},
            {"name": "interactive-npcs:source-reference", "value": entry["source_reference"]},
            {"name": "interactive-npcs:notice-reference", "value": entry["notice_reference"]},
        ]
        if entry.get("release_status"):
            properties.append({"name": "interactive-npcs:release-status", "value": entry["release_status"]})
        components.append({
            "type": "application" if entry["distribution_class"].startswith("project-built") else "library",
            "bom-ref": bom_ref,
            "name": entry["name"],
            "version": "review-candidate",
            "licenses": [{"expression": entry["spdx"]}],
            "scope": component_scope,
            "properties": properties,
        })
    file_refs_by_owner: dict[str, list[str]] = {}
    for entry in sorted(static_files, key=lambda item: item["path"]):
        path = root / entry["path"]
        if not path.is_file():
            parser.error(f"distribution static file is missing: {entry['path']}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != entry["sha256"]:
            parser.error(f"distribution static file hash is stale: {entry['path']} (expected {entry['sha256']}, got {digest})")
        owner = entry["component_id"]
        if owner not in distribution_refs:
            parser.error(f"distribution static file has unknown component: {owner}")
        bom_ref = f"urn:interactive-npcs:file:{entry['path']}"
        file_refs_by_owner.setdefault(owner, []).append(bom_ref)
        components.append({
            "type": "file", "bom-ref": bom_ref, "name": entry["path"],
            "hashes": [{"alg": "SHA-256", "content": digest}],
            "properties": [
                {"name": "interactive-npcs:component-id", "value": owner},
                {"name": "interactive-npcs:install-path", "value": entry.get("install_path", entry["path"])},
            ],
        })

    root_ref = "pkg:generic/interactive-llm-powered-npcs@2.0.0-alpha.1"
    serial = uuid.uuid5(uuid.NAMESPACE_URL, "https://github.com/AkshitIreddy/Interactive-LLM-Powered-NPCs/v2-lock-inventory")
    metadata_properties = [
        {"name": "interactive-npcs:source-lock", "value": source}
        for source in sources
    ] + [
        {"name": "interactive-npcs:distribution", "value": "local-review-only"},
        {"name": "interactive-npcs:artifact-id", "value": artifact_id},
        {"name": "interactive-npcs:artifact-scope-resolved", "value": str(bool(artifact_scope)).lower()},
        {"name": "interactive-npcs:distribution-profile", "value": args.distribution_profile},
        {"name": "interactive-npcs:distribution-scope", "value": distribution_scope or "unresolved"},
    ]
    if license_material_index_sha256:
        metadata_properties.append({"name": "interactive-npcs:license-material-index-sha256", "value": license_material_index_sha256})
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
        "dependencies": [
            {"ref": root_ref, "dependsOn": [item["bom-ref"] for item in components if item.get("purl") and item.get("scope") != "excluded"] + sorted(
                distribution_refs[component_id]
                for component_id, entry in distribution_components.items()
                if entry["redistributed"] and distribution_scope in entry["scopes"]
            )},
            *[
                {"ref": distribution_refs[owner], "dependsOn": sorted(refs)}
                for owner, refs in sorted(file_refs_by_owner.items())
            ],
        ],
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
