#!/usr/bin/env python3
"""Measure remote-telemetry surfaces for one reconciled installed candidate.

The scanner reads only fixed dependency manifests and files enumerated by the
closed-world installed manifest. It never reads credential stores and reports
only rule counts, never matched content.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import tempfile
from pathlib import Path
from typing import Any, Iterable


MAX_JSON = 8 * 1024 * 1024
MAX_BINARY = 256 * 1024 * 1024
DEPENDENCY_RULES = {
    b"sentry": "dependency.sentry",
    b"datadog": "dependency.datadog",
    b"applicationinsights": "dependency.application-insights",
    b"opentelemetry-otlp": "dependency.opentelemetry-otlp",
    b"mixpanel": "dependency.mixpanel",
    b"amplitude": "dependency.amplitude",
    b"segment-analytics": "dependency.segment",
}
UPLOAD_RULES = {
    b"sentry_dsn": "entry.sentry-dsn",
    b"telemetry_upload": "entry.telemetry-upload",
    b"upload_diagnostics": "entry.diagnostics-upload",
    b"remote_crash_report": "entry.remote-crash-report",
    b"analytics.track": "entry.analytics-track",
}
DESTINATION_RULES = {
    b"sentry.io": "destination.sentry",
    b"datadoghq.com": "destination.datadog",
    b"applicationinsights.azure.com": "destination.application-insights",
    b"ingest.segment.io": "destination.segment",
    b"api.mixpanel.com": "destination.mixpanel",
    b"api2.amplitude.com": "destination.amplitude",
}
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class InventoryError(ValueError):
    pass


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    if not path.is_file() or path.stat().st_size > MAX_JSON:
        raise InventoryError(f"bounded JSON artifact unavailable: {path.name}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise InventoryError(f"JSON artifact is not an object: {path.name}")
    return value


def scan_bytes(chunks: Iterable[bytes], rules: dict[bytes, str]) -> set[str]:
    found: set[str] = set()
    overlap = max(map(len, rules), default=1) - 1
    prior = b""
    for chunk in chunks:
        lowered = prior + chunk.lower()
        for marker, rule_id in rules.items():
            if marker in lowered:
                found.add(rule_id)
        prior = lowered[-overlap:] if overlap else b""
    return found


def scan_file(path: Path, rules: dict[bytes, str]) -> set[str]:
    if not path.is_file() or path.stat().st_size > MAX_BINARY:
        raise InventoryError(f"bounded candidate file unavailable: {path.name}")
    with path.open("rb") as stream:
        return scan_bytes(iter(lambda: stream.read(1024 * 1024), b""), rules)


def installed_files(root: Path, manifest: dict[str, Any]) -> list[Path]:
    root = root.resolve()
    rows = manifest.get("files")
    if not isinstance(rows, list) or not rows:
        raise InventoryError("installed manifest has no closed-world file inventory")
    result: list[Path] = []
    for row in rows:
        if not isinstance(row, dict) or set(row) < {"path", "sha256"}:
            raise InventoryError("installed manifest file row is invalid")
        expected = row["sha256"]
        if not isinstance(expected, str) or not SHA256.fullmatch(expected):
            raise InventoryError("installed manifest contains an invalid hash")
        candidate = (root / str(row["path"])).resolve()
        try:
            candidate.relative_to(root)
        except ValueError as error:
            raise InventoryError("installed manifest path escapes the install root") from error
        if digest(candidate) != expected:
            raise InventoryError("installed file differs from reconciliation evidence")
        result.append(candidate)
    return result


def measure(
    repository_root: Path,
    package_manifest_path: Path,
    installed_manifest_path: Path,
    install_root: Path,
) -> dict[str, Any]:
    package = load_json(package_manifest_path)
    installed = load_json(installed_manifest_path)
    if package.get("schema_version") != 1 or package.get("distribution") != "local-review-only":
        raise InventoryError("package manifest is not a real local-review candidate")
    if installed.get("schema_version") != 1:
        raise InventoryError("installed reconciliation schema is invalid")

    repository_root = repository_root.resolve()
    dependency_paths = [repository_root / "Cargo.lock", repository_root / "pnpm-lock.yaml"]
    source_identity = package.get("source")
    if not isinstance(source_identity, dict):
        raise InventoryError("package manifest omits frozen source identity")
    inputs = source_identity.get("inputs")
    if not isinstance(inputs, dict):
        raise InventoryError("package source identity omits fixed inputs")
    expected_source_hashes = {
        "Cargo.lock": dependency_paths[0],
        "pnpm-lock.yaml": dependency_paths[1],
    }
    for field, path in expected_source_hashes.items():
        row = inputs.get(field)
        expected = row.get("sha256") if isinstance(row, dict) else None
        if not isinstance(expected, str) or not SHA256.fullmatch(expected) or digest(path) != expected:
            raise InventoryError(f"package source binding failed for {field}")

    dependency_findings: set[str] = set()
    for path in dependency_paths:
        dependency_findings.update(scan_file(path, DEPENDENCY_RULES))

    upload_findings: set[str] = set()
    destination_findings: set[str] = set()
    for path in installed_files(install_root, installed):
        upload_findings.update(scan_file(path, UPLOAD_RULES))
        destination_findings.update(scan_file(path, DESTINATION_RULES))

    return {
        "schemaVersion": "1.0.0",
        "provenance": "measured",
        "evidenceSource": "packaged_binary_inventory",
        "packageManifestSha256": digest(package_manifest_path),
        "installedDistributionManifestSha256": digest(installed_manifest_path),
        "dependencyInventoryScanned": True,
        "endpointInventoryScanned": True,
        "automaticUploadEntryPoints": len(upload_findings) + len(dependency_findings),
        "remoteTelemetryDestinations": len(destination_findings),
    }


def write_atomic(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository-root", type=Path, required=True)
    parser.add_argument("--package-manifest", type=Path, required=True)
    parser.add_argument("--installed-manifest", type=Path, required=True)
    parser.add_argument("--install-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = measure(args.repository_root, args.package_manifest, args.installed_manifest, args.install_root)
        if args.output.exists():
            raise InventoryError("telemetry inventory refuses to overwrite existing evidence")
        write_atomic(args.output, result)
    except (InventoryError, OSError, json.JSONDecodeError) as error:
        print(f"packaged telemetry inventory failed: {error}")
        return 1
    print(json.dumps({"status": "passed" if result["automaticUploadEntryPoints"] == 0 and result["remoteTelemetryDestinations"] == 0 else "failed", "output": str(args.output)}))
    return 0 if result["automaticUploadEntryPoints"] == 0 and result["remoteTelemetryDestinations"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
