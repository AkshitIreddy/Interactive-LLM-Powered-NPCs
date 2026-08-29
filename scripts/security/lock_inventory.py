#!/usr/bin/env python3
"""Dependency-free parsers for every dependency lock used by the product."""

from __future__ import annotations

import base64
import json
import re
import tomllib
import urllib.parse
from pathlib import Path


LOCK_SPECS = (
    ("Cargo.lock", "cargo"),
    ("apps/control/src-tauri/Cargo.lock", "cargo"),
    ("pnpm-lock.yaml", "npm"),
    ("demo/readme/package-lock.json", "npm"),
)


class InventoryError(RuntimeError):
    """The committed dependency inventory is incomplete or malformed."""


def _cargo_purl(name: str, version: str) -> str:
    return f"pkg:cargo/{urllib.parse.quote(name, safe='')}@{version}"


def _npm_purl(name: str, version: str) -> str:
    return f"pkg:npm/{urllib.parse.quote(name, safe='@/')}@{version}"


def cargo_components(lockfile: Path, source_name: str) -> list[dict]:
    try:
        document = tomllib.loads(lockfile.read_text(encoding="utf-8"))
        packages = document["package"]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as exc:
        raise InventoryError(f"cannot parse {source_name}: {exc}") from exc
    if not isinstance(packages, list) or not packages:
        raise InventoryError(f"{source_name} has no package inventory")
    components: list[dict] = []
    for index, package in enumerate(packages):
        try:
            name = str(package["name"])
            version = str(package["version"])
        except (KeyError, TypeError) as exc:
            raise InventoryError(f"{source_name} package {index} is incomplete") from exc
        reference = _cargo_purl(name, version)
        component = {
            "type": "library", "bom-ref": reference, "name": name,
            "version": version, "purl": reference,
            "properties": [
                {"name": "interactive-npcs:ecosystem", "value": "cargo"},
                {"name": "interactive-npcs:source-lock", "value": source_name},
            ],
        }
        checksum = package.get("checksum")
        if checksum:
            component["hashes"] = [{"alg": "SHA-256", "content": str(checksum).lower()}]
        components.append(component)
    return components


def _split_pnpm_key(key: str) -> tuple[str, str] | None:
    key = key.strip("'").split("(", 1)[0]
    if "@" not in key:
        return None
    name, version = key.rsplit("@", 1)
    return (name, version) if name and version else None


def pnpm_components(lockfile: Path, source_name: str) -> list[dict]:
    try:
        lines = lockfile.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise InventoryError(f"cannot read {source_name}: {exc}") from exc
    if not any(line.startswith("lockfileVersion:") for line in lines[:20]):
        raise InventoryError(f"{source_name} is not a supported pnpm lock")
    in_packages = False
    current: tuple[str, str] | None = None
    integrity: str | None = None
    entries: list[tuple[str, str, str | None]] = []

    def commit() -> None:
        nonlocal current, integrity
        if current:
            entries.append((current[0], current[1], integrity))
        current = None
        integrity = None

    for line in lines:
        if line == "packages:":
            in_packages = True
            continue
        if not in_packages:
            continue
        if line and not line.startswith(" "):
            commit()
            break
        # Package keys are indented exactly two spaces. Nested metadata and
        # peer-dependency maps are more deeply indented and must not be parsed
        # as packages.
        match = re.match(r"^  (?:'([^']+)'|(\S[^:]*)):\s*$", line)
        if match:
            commit()
            raw_key = match.group(1) or match.group(2)
            current = _split_pnpm_key(raw_key)
            if current is None:
                raise InventoryError(f"{source_name} package key could not be parsed: {raw_key}")
            continue
        if current:
            match = re.search(r"integrity:\s*(sha512-[A-Za-z0-9+/=]+)", line)
            if match:
                integrity = match.group(1)
    commit()
    if not entries:
        raise InventoryError(f"{source_name} packages section could not be inventoried")
    components: list[dict] = []
    for name, version, sri in entries:
        reference = _npm_purl(name, version)
        component = {
            "type": "library", "bom-ref": reference, "name": name,
            "version": version, "purl": reference,
            "properties": [
                {"name": "interactive-npcs:ecosystem", "value": "npm"},
                {"name": "interactive-npcs:source-lock", "value": source_name},
            ],
        }
        if sri:
            try:
                digest = base64.b64decode(sri.removeprefix("sha512-"), validate=True).hex()
            except ValueError as exc:
                raise InventoryError(f"invalid integrity for {reference} in {source_name}") from exc
            component["hashes"] = [{"alg": "SHA-512", "content": digest}]
        components.append(component)
    return components


def package_lock_components(lockfile: Path, source_name: str) -> list[dict]:
    try:
        document = json.loads(lockfile.read_text(encoding="utf-8"))
        packages = document["packages"]
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        raise InventoryError(f"cannot parse {source_name}: {exc}") from exc
    if not isinstance(packages, dict) or len(packages) <= 1:
        raise InventoryError(f"{source_name} has no dependency package inventory")
    components: list[dict] = []
    for install_path, package in packages.items():
        if not install_path or "node_modules/" not in install_path:
            continue
        name = package.get("name") or install_path.rsplit("node_modules/", 1)[1]
        version = package.get("version")
        if not name or not version:
            raise InventoryError(f"{source_name} entry {install_path!r} is incomplete")
        reference = _npm_purl(str(name), str(version))
        component = {
            "type": "library", "bom-ref": reference, "name": str(name),
            "version": str(version), "purl": reference,
            "properties": [
                {"name": "interactive-npcs:ecosystem", "value": "npm"},
                {"name": "interactive-npcs:source-lock", "value": source_name},
            ],
        }
        integrity = package.get("integrity")
        if isinstance(integrity, str) and integrity.startswith("sha512-"):
            try:
                digest = base64.b64decode(integrity.removeprefix("sha512-"), validate=True).hex()
            except ValueError as exc:
                raise InventoryError(f"invalid integrity for {reference} in {source_name}") from exc
            component["hashes"] = [{"alg": "SHA-512", "content": digest}]
        components.append(component)
    if not components:
        raise InventoryError(f"{source_name} dependencies could not be inventoried")
    return components


def load_inventory(root: Path) -> tuple[list[dict], list[str]]:
    """Load required locks and deterministically merge duplicate purls."""
    raw: list[dict] = []
    sources: list[str] = []
    for relative, ecosystem in LOCK_SPECS:
        lockfile = root / relative
        if not lockfile.is_file():
            raise InventoryError(f"required lockfile is missing: {relative}")
        sources.append(relative)
        if ecosystem == "cargo":
            raw.extend(cargo_components(lockfile, relative))
        elif relative.endswith("pnpm-lock.yaml"):
            raw.extend(pnpm_components(lockfile, relative))
        else:
            raw.extend(package_lock_components(lockfile, relative))
    merged: dict[str, dict] = {}
    for component in raw:
        reference = component["bom-ref"]
        if reference not in merged:
            merged[reference] = component
            continue
        existing = merged[reference]
        properties = {(item["name"], item["value"]) for item in existing.get("properties", []) + component.get("properties", [])}
        existing["properties"] = [{"name": name, "value": value} for name, value in sorted(properties)]
        hashes = {(item["alg"], item["content"]) for item in existing.get("hashes", []) + component.get("hashes", [])}
        if hashes:
            existing["hashes"] = [{"alg": algorithm, "content": content} for algorithm, content in sorted(hashes)]
    result = sorted(merged.values(), key=lambda item: item["bom-ref"])
    observed_sources = {
        item["value"]
        for component in result
        for item in component.get("properties", [])
        if item.get("name") == "interactive-npcs:source-lock"
    }
    missing_sources = set(sources) - observed_sources
    if missing_sources:
        raise InventoryError(f"lock parse omitted sources: {', '.join(sorted(missing_sources))}")
    return result, sources
