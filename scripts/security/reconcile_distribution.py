#!/usr/bin/env python3
"""Fail-closed reconciliation of an extracted distribution against its signed manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path, PurePosixPath


SHA256 = re.compile(r"^[a-f0-9]{64}$")
REQUIRED_FILE_FIELDS = (
    "path", "sha256", "component_id", "spdx", "distribution_scope",
    "source_reference", "notice_reference",
)
DEBUG_CRT_IMPORTS = (
    b"MSVCP140D.dll", b"MSVCP140D_ATOMIC_WAIT.dll", b"VCRUNTIME140D.dll",
    b"VCRUNTIME140_1D.dll", b"ucrtbased.dll",
)


class ReconciliationError(ValueError):
    pass


def safe_relative(value: object) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise ReconciliationError(f"invalid manifest path {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise ReconciliationError(f"unsafe manifest path {value!r}")
    return path.as_posix()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_installer_legal_materials(root: Path, ledger: dict) -> None:
    legal = root / "product-audit/legal"
    installed_ledger_path = legal / "distribution-components.json"
    scope_path = legal / "windows-artifact-scope.json"
    license_index_path = legal / "packages/THIRD-PARTY-LICENSE-FILES.json"
    sbom_path = legal / "lockfiles.cdx.json"
    try:
        installed_ledger = json.loads(installed_ledger_path.read_text(encoding="utf-8"))
        scope = json.loads(scope_path.read_text(encoding="utf-8"))
        license_index = json.loads(license_index_path.read_text(encoding="utf-8"))
        sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ReconciliationError(f"cannot read installed legal evidence: {exc}") from exc
    if installed_ledger != ledger:
        raise ReconciliationError("installed distribution ledger differs from the validator ledger")
    if scope.get("schema_version") != 1 or license_index.get("schema_version") != 1:
        raise ReconciliationError("installed artifact scope/license index schema is invalid")
    if license_index.get("artifact_id") != scope.get("artifact_id"):
        raise ReconciliationError("installed artifact scope and license index identify different artifacts")
    required_purls = {purl for purl, value in scope.get("components", {}).items() if value in {"required", "optional"}}
    indexed_purls = set(license_index.get("components", {}))
    if required_purls != indexed_purls:
        raise ReconciliationError(f"installed license corpus differs from artifact scope (missing={sorted(required_purls - indexed_purls)}, stale={sorted(indexed_purls - required_purls)})")
    expected_license_files: set[str] = set()
    for purl, records in license_index["components"].items():
        if not isinstance(records, list) or not records:
            raise ReconciliationError(f"installed license corpus has no bodies for {purl}")
        for record in records:
            relative = safe_relative(record.get("path"))
            material = license_index_path.parent / relative
            expected_license_files.add(material.relative_to(root).as_posix())
            if not material.is_file() or not SHA256.fullmatch(str(record.get("sha256", ""))) or sha256(material) != record["sha256"]:
                raise ReconciliationError(f"installed license body is missing or hash-mismatched: {relative}")
    actual_license_files = {
        path.relative_to(root).as_posix()
        for path in (license_index_path.parent / "components").rglob("*")
        if path.is_file()
    }
    if actual_license_files != expected_license_files:
        raise ReconciliationError(f"installed license body set differs from index (unexplained={sorted(actual_license_files - expected_license_files)}, missing={sorted(expected_license_files - actual_license_files)})")
    properties = {entry.get("name"): entry.get("value") for entry in sbom.get("metadata", {}).get("properties", [])}
    if properties.get("interactive-npcs:artifact-id") != scope.get("artifact_id") or properties.get("interactive-npcs:artifact-scope-resolved") != "true":
        raise ReconciliationError("installed SBOM is not bound to the resolved artifact scope")
    if properties.get("interactive-npcs:license-material-index-sha256") != sha256(license_index_path):
        raise ReconciliationError("installed SBOM is not bound to the license-material index")
    sbom_required = {component.get("purl") for component in sbom.get("components", []) if component.get("scope") in {"required", "optional"} and component.get("purl")}
    if sbom_required != required_purls:
        raise ReconciliationError(f"installed SBOM required set differs from artifact scope (missing={sorted(required_purls - sbom_required)}, stale={sorted(sbom_required - required_purls)})")
    for entry in ledger.get("static_files", []):
        installed = entry.get("install_path", entry["path"])
        if installed in {"icon-resource-in-executable", "frontend-resource-in-executable"}:
            continue
        material = root / installed
        if not material.is_file() or sha256(material) != entry["sha256"]:
            raise ReconciliationError(f"installed static resource is missing or differs from canonical hash: {installed}")


def reconcile(root: Path, manifest_path: Path, ledger_path: Path, kind: str) -> dict[str, object]:
    root = root.resolve()
    manifest_path = manifest_path.resolve()
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        profile = ledger["distribution_profiles"][kind]
        components = ledger["components"]
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        raise ReconciliationError(f"cannot read reconciliation inputs: {exc}") from exc
    if manifest.get("schema_version") != 1:
        raise ReconciliationError("distribution manifest schema_version must be 1")
    files = manifest.get("files")
    if not isinstance(files, list):
        raise ReconciliationError("distribution manifest files must be an array")
    manifest_relative = manifest_path.relative_to(root).as_posix()
    declared: dict[str, dict] = {}
    forbidden = set(profile.get("forbidden_component_ids", []))
    expected_scope = profile["scope"]
    for entry in files:
        if not isinstance(entry, dict):
            raise ReconciliationError("distribution manifest file entries must be objects")
        missing_fields = [field for field in REQUIRED_FILE_FIELDS if not entry.get(field)]
        if missing_fields:
            raise ReconciliationError(f"manifest entry is missing {','.join(missing_fields)}")
        relative = safe_relative(entry["path"])
        if relative == manifest_relative:
            raise ReconciliationError("manifest must use the explicit self-exclusion policy, not a circular self hash")
        if relative in declared:
            raise ReconciliationError(f"duplicate manifest path: {relative}")
        if not SHA256.fullmatch(entry["sha256"]):
            raise ReconciliationError(f"invalid SHA-256 for {relative}")
        component_id = entry["component_id"]
        component = components.get(component_id)
        if not component:
            raise ReconciliationError(f"unknown component_id for {relative}: {component_id}")
        if component_id in forbidden:
            raise ReconciliationError(f"forbidden component in {kind}: {component_id}")
        if str(component.get("release_status", "")).startswith("blocked"):
            raise ReconciliationError(f"component still has an active release blocker in {kind}: {component_id} ({component['release_status']})")
        if entry["distribution_scope"] != expected_scope:
            raise ReconciliationError(f"distribution scope mismatch for {relative}: {entry['distribution_scope']} != {expected_scope}")
        if not component.get("redistributed") or expected_scope not in component.get("scopes", []):
            raise ReconciliationError(f"component is not approved for {expected_scope}: {component_id}")
        if entry["spdx"] != component["spdx"]:
            raise ReconciliationError(f"SPDX mismatch for {relative}: {entry['spdx']} != {component['spdx']}")
        declared[relative] = entry
    if manifest.get("manifest_self") != {"path": manifest_relative, "hash": "excluded-to-avoid-circularity"}:
        raise ReconciliationError("manifest_self must explicitly declare circular hash exclusion")
    if kind == "test-game" and manifest.get("third_party_binaries") != []:
        raise ReconciliationError("review test game must declare third_party_binaries=[]")

    actual: set[str] = set()
    for path in root.rglob("*"):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise ReconciliationError(f"symlink is forbidden in distribution: {relative}")
        if path.is_file():
            actual.add(relative)
    expected = set(declared) | {manifest_relative}
    unexplained = sorted(actual - expected)
    missing = sorted(expected - actual)
    if unexplained or missing:
        raise ReconciliationError(f"distribution file set differs from manifest (unexplained={unexplained}, missing={missing})")
    required = set(profile.get("required_paths", profile.get("required_install_paths", [])))
    absent_required = sorted(required - actual)
    if absent_required:
        raise ReconciliationError(f"required {kind} files are missing: {absent_required}")
    for relative, entry in sorted(declared.items()):
        path = root / relative
        digest = sha256(path)
        if digest != entry["sha256"]:
            raise ReconciliationError(f"SHA-256 mismatch for {relative}: expected {entry['sha256']}, got {digest}")
        if path.suffix.lower() in {".exe", ".dll"}:
            data = path.read_bytes()
            imports = [name.decode() for name in DEBUG_CRT_IMPORTS if name.lower() in data.lower()]
            if imports:
                raise ReconciliationError(f"debug CRT import marker in shipping binary {relative}: {imports}")
    if kind == "installer":
        validate_installer_legal_materials(root, ledger)
    return {"status": "passed", "kind": kind, "files": len(actual), "manifested_files": len(declared)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True, help="Extracted distribution directory")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--ledger", type=Path, default=Path(__file__).resolve().parents[2] / "packaging/security/distribution-components.json")
    parser.add_argument("--kind", choices=("installer", "test-game"), required=True)
    args = parser.parse_args()
    try:
        result = reconcile(args.root, args.manifest, args.ledger, args.kind)
    except (ReconciliationError, ValueError) as exc:
        parser.error(str(exc))
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
