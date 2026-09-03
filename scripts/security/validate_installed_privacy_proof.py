#!/usr/bin/env python3
"""Validate privacy evidence for one already-packaged and reconciled install.

This tool never manufactures proof fields and never launches a fixture. It
accepts only the measured packaged-candidate shape and independently binds the
four supplied artifacts by SHA-256.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from datetime import datetime
from pathlib import Path
from typing import Any


MAX_JSON_BYTES = 2 * 1024 * 1024
SHA256 = re.compile(r"^[0-9a-f]{64}$")
IDENTIFIER = re.compile(r"^[A-Za-z0-9._:/-]{1,128}$")
ARTIFACT_KEYS = {
    "applicationVersion",
    "releaseCandidateId",
    "packageManifestSha256",
    "packageSha256",
    "installedDistributionManifestSha256",
    "executableSha256",
}
TELEMETRY_KEYS = {
    "schemaVersion", "artifact", "provenance", "evidenceSource", "evidenceRunId",
    "observedAtUtc", "dependencyInventoryScanned", "endpointInventoryScanned",
    "automaticUploadEntryPoints", "remoteTelemetryDestinations", "outcome",
}
DENY_KEYS = {
    "schemaVersion", "artifact", "scenario", "provenance", "evidenceSource",
    "evidenceRunId", "observedAtUtc", "enforcement", "monitoredProcessCount",
    "observedConnectionAttempts", "observedProviderRequests",
    "externalDestinationCount", "outcome",
}
CAPTURE_KEYS = {
    "schemaVersion", "provenance", "evidenceSource", "releaseCandidateId",
    "packageManifestSha256", "packageSha256",
    "installedDistributionManifestSha256", "executableSha256", "platform",
    "enforcementMechanism", "auditSource", "firewallRuleGroupId",
    "firewallRuleCount", "auditPolicyVerified", "telemetryInventory", "scenarios",
}
TELEMETRY_CAPTURE_KEYS = {
    "provenance", "evidenceSource", "dependencyInventoryScanned",
    "endpointInventoryScanned", "automaticUploadEntryPoints",
    "remoteTelemetryDestinations",
}
SCENARIO_CAPTURE_KEYS = {
    "scenario", "runId", "receiptSource", "scenarioCompleted", "observedAtUtc",
    "monitoredProcesses", "securityEventRecordStart", "securityEventRecordEnd",
    "observedConnectionAttempts", "observedProviderRequests",
    "externalDestinationCount", "loopbackConnectionCount",
}
PROCESS_CAPTURE_KEYS = {"imageName", "sha256", "processId", "parentProcessId"}


class ProofError(ValueError):
    pass


def load_json(path: Path) -> dict[str, Any]:
    if not path.is_file() or path.stat().st_size > MAX_JSON_BYTES:
        raise ProofError(f"required bounded JSON artifact is unavailable: {path.name}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ProofError(f"JSON artifact is not an object: {path.name}")
    return value


def digest(path: Path) -> str:
    if not path.is_file():
        raise ProofError(f"required candidate artifact is unavailable: {path.name}")
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ProofError(f"{label} fields differ from the closed schema")


def valid_utc(value: Any) -> bool:
    if not isinstance(value, str) or not value.endswith("Z"):
        return False
    try:
        datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        return False
    return True


def exact_zero(value: Any) -> bool:
    return type(value) is int and value == 0


def validate_artifact(
    artifact: Any,
    candidate_id: str,
    expected_hashes: dict[str, str],
) -> dict[str, Any]:
    if not isinstance(artifact, dict):
        raise ProofError("proof artifact identity is not an object")
    exact_keys(artifact, ARTIFACT_KEYS, "artifact identity")
    if artifact["releaseCandidateId"] != candidate_id:
        raise ProofError("proof release candidate differs from the requested candidate")
    if not isinstance(artifact["applicationVersion"], str) or not artifact["applicationVersion"]:
        raise ProofError("application version is invalid")
    for field, expected in expected_hashes.items():
        observed = artifact.get(field)
        if not isinstance(observed, str) or not SHA256.fullmatch(observed) or observed != expected:
            raise ProofError(f"proof artifact binding failed for {field}")
    return artifact


def validate_header(value: dict[str, Any], artifact: dict[str, Any]) -> None:
    if value["schemaVersion"] != "1.0.0" or value["artifact"] != artifact:
        raise ProofError("proof schema or artifact identity differs")
    if (
        value["provenance"] != "measured"
        or value["evidenceSource"] != "packaged_executable_observation"
        or value["outcome"] != "passed"
        or not isinstance(value["evidenceRunId"], str)
        or not IDENTIFIER.fullmatch(value["evidenceRunId"])
        or not valid_utc(value["observedAtUtc"])
    ):
        raise ProofError("fixture, unmeasured, ambiguous, or failed proof cannot pass")


def validate_proof(
    proof: dict[str, Any], artifact: dict[str, Any], schema: dict[str, Any],
    capture_evidence_sha256: str | None = None,
) -> None:
    if schema.get("$id") != "interactive-npcs/privacy-proof-v1":
        raise ProofError("bundled privacy schema identity is invalid")
    exact_keys(
        proof,
        {"schemaVersion", "captureEvidenceSha256", "remoteTelemetryAbsence", "denyAllEgress"},
        "proof",
    )
    if proof["schemaVersion"] != "1.0.0":
        raise ProofError("unsupported privacy proof schema")
    if not isinstance(proof["captureEvidenceSha256"], str) or not SHA256.fullmatch(proof["captureEvidenceSha256"]):
        raise ProofError("privacy capture evidence hash is invalid")
    if capture_evidence_sha256 is not None and proof["captureEvidenceSha256"] != capture_evidence_sha256:
        raise ProofError("privacy capture evidence hash differs from the retained capture")

    telemetry = proof["remoteTelemetryAbsence"]
    if not isinstance(telemetry, dict):
        raise ProofError("telemetry proof is invalid")
    exact_keys(telemetry, TELEMETRY_KEYS, "telemetry proof")
    validate_header(telemetry, artifact)
    if not (
        telemetry["dependencyInventoryScanned"] is True
        and telemetry["endpointInventoryScanned"] is True
        and exact_zero(telemetry["automaticUploadEntryPoints"])
        and exact_zero(telemetry["remoteTelemetryDestinations"])
    ):
        raise ProofError("remote telemetry absence was not measured")

    rows = proof["denyAllEgress"]
    if not isinstance(rows, list) or len(rows) != 2:
        raise ProofError("exactly two deny-all scenarios are required")
    scenarios: set[str] = set()
    run_ids: set[str] = {telemetry["evidenceRunId"]}
    for row in rows:
        if not isinstance(row, dict):
            raise ProofError("deny-all row is invalid")
        exact_keys(row, DENY_KEYS, "deny-all proof")
        validate_header(row, artifact)
        if row["evidenceRunId"] in run_ids:
            raise ProofError("privacy evidence run IDs must be unique")
        run_ids.add(row["evidenceRunId"])
        scenarios.add(row["scenario"])
        if not (
            row["enforcement"] == "os_deny_all"
            and type(row["monitoredProcessCount"]) is int
            and row["monitoredProcessCount"] > 0
            and exact_zero(row["observedConnectionAttempts"])
            and exact_zero(row["observedProviderRequests"])
            and exact_zero(row["externalDestinationCount"])
        ):
            raise ProofError("OS deny-all evidence did not pass")
    if scenarios != {"offline_mode", "local_lip_sync"}:
        raise ProofError("Offline and local lip-sync deny-all scenarios are both required")


def validate_capture_evidence(
    capture: dict[str, Any], artifact: dict[str, Any], installed_manifest: dict[str, Any]
) -> None:
    exact_keys(capture, CAPTURE_KEYS, "capture evidence")
    if (
        capture["schemaVersion"] != "1.0.0"
        or capture["provenance"] != "measured"
        or capture["evidenceSource"] != "packaged_executable_observation"
        or capture["releaseCandidateId"] != artifact["releaseCandidateId"]
        or capture["platform"] != "windows"
        or capture["enforcementMechanism"] != "windows_defender_firewall_program_deny"
        or capture["auditSource"] != "windows_security_filtering_platform"
        or capture["auditPolicyVerified"] is not True
        or not isinstance(capture["firewallRuleGroupId"], str)
        or not IDENTIFIER.fullmatch(capture["firewallRuleGroupId"])
        or type(capture["firewallRuleCount"]) is not int
        or capture["firewallRuleCount"] < 1
    ):
        raise ProofError("capture is not measured installed-candidate Windows deny-all evidence")
    for key in (
        "packageManifestSha256", "packageSha256",
        "installedDistributionManifestSha256", "executableSha256",
    ):
        if capture[key] != artifact[key]:
            raise ProofError(f"capture artifact binding failed for {key}")

    inventory = capture["telemetryInventory"]
    if not isinstance(inventory, dict):
        raise ProofError("telemetry capture inventory is invalid")
    exact_keys(inventory, TELEMETRY_CAPTURE_KEYS, "telemetry capture inventory")
    if not (
        inventory["provenance"] == "measured"
        and inventory["evidenceSource"] == "packaged_binary_inventory"
        and inventory["dependencyInventoryScanned"] is True
        and inventory["endpointInventoryScanned"] is True
        and exact_zero(inventory["automaticUploadEntryPoints"])
        and exact_zero(inventory["remoteTelemetryDestinations"])
    ):
        raise ProofError("fixture or incomplete telemetry inventory cannot pass")

    installed_hashes = {
        row.get("sha256")
        for row in installed_manifest.get("files", [])
        if isinstance(row, dict) and isinstance(row.get("sha256"), str)
    }
    rows = capture["scenarios"]
    if not isinstance(rows, list) or len(rows) != 2:
        raise ProofError("capture must contain exactly two scenarios")
    scenarios: set[str] = set()
    run_ids: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            raise ProofError("capture scenario is invalid")
        exact_keys(row, SCENARIO_CAPTURE_KEYS, "capture scenario")
        if (
            row["scenario"] not in {"offline_mode", "local_lip_sync"}
            or not isinstance(row["runId"], str)
            or not IDENTIFIER.fullmatch(row["runId"])
            or row["receiptSource"] != "packaged_executable"
            or row["scenarioCompleted"] is not True
            or not valid_utc(row["observedAtUtc"])
            or type(row["securityEventRecordStart"]) is not int
            or type(row["securityEventRecordEnd"]) is not int
            or row["securityEventRecordEnd"] < row["securityEventRecordStart"]
            or not exact_zero(row["observedConnectionAttempts"])
            or not exact_zero(row["observedProviderRequests"])
            or not exact_zero(row["externalDestinationCount"])
            or type(row["loopbackConnectionCount"]) is not int
            or row["loopbackConnectionCount"] < 0
        ):
            raise ProofError("capture scenario is fixture, incomplete, or observed egress")
        if row["scenario"] in scenarios or row["runId"] in run_ids:
            raise ProofError("capture scenario or run ID is duplicated")
        scenarios.add(row["scenario"])
        run_ids.add(row["runId"])
        processes = row["monitoredProcesses"]
        if not isinstance(processes, list) or not processes:
            raise ProofError("capture omitted installed process observations")
        main_seen = False
        for process in processes:
            if not isinstance(process, dict):
                raise ProofError("capture process observation is invalid")
            exact_keys(process, PROCESS_CAPTURE_KEYS, "capture process")
            if (
                not isinstance(process["imageName"], str)
                or not IDENTIFIER.fullmatch(process["imageName"])
                or not isinstance(process["sha256"], str)
                or process["sha256"] not in installed_hashes
                or type(process["processId"]) is not int
                or process["processId"] <= 0
                or type(process["parentProcessId"]) is not int
                or process["parentProcessId"] < 0
            ):
                raise ProofError("capture process is not bound to the installed manifest")
            main_seen = main_seen or process["sha256"] == artifact["executableSha256"]
        if not main_seen:
            raise ProofError("capture did not observe the bound installed executable")
    if scenarios != {"offline_mode", "local_lip_sync"}:
        raise ProofError("capture omits a required installed scenario")


def validate_proof_capture_binding(proof: dict[str, Any], capture: dict[str, Any]) -> None:
    """Require the compact proof to be an exact projection of retained capture.

    Independent validation of two individually valid documents is insufficient:
    a stale proof row could otherwise be paired with a different valid capture.
    The capture hash binds bytes, while these checks bind every projected field.
    """
    telemetry = proof["remoteTelemetryAbsence"]
    inventory = capture["telemetryInventory"]
    for field in (
        "dependencyInventoryScanned",
        "endpointInventoryScanned",
        "automaticUploadEntryPoints",
        "remoteTelemetryDestinations",
    ):
        if telemetry[field] != inventory[field]:
            raise ProofError(f"proof differs from retained capture for telemetry {field}")

    proof_rows = {row["scenario"]: row for row in proof["denyAllEgress"]}
    capture_rows = {row["scenario"]: row for row in capture["scenarios"]}
    if set(proof_rows) != set(capture_rows):
        raise ProofError("proof and retained capture scenarios differ")
    for scenario, proof_row in proof_rows.items():
        capture_row = capture_rows[scenario]
        expected = {
            "evidenceRunId": capture_row["runId"],
            "observedAtUtc": capture_row["observedAtUtc"],
            "monitoredProcessCount": len(capture_row["monitoredProcesses"]),
            "observedConnectionAttempts": capture_row["observedConnectionAttempts"],
            "observedProviderRequests": capture_row["observedProviderRequests"],
            "externalDestinationCount": capture_row["externalDestinationCount"],
        }
        for field, value in expected.items():
            if proof_row[field] != value:
                raise ProofError(
                    f"proof differs from retained {scenario} capture for {field}"
                )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--capture-evidence", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--package-manifest", type=Path, required=True)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--installed-manifest", type=Path, required=True)
    parser.add_argument("--installed-executable", type=Path, required=True)
    parser.add_argument("--release-candidate-id", required=True)
    args = parser.parse_args()

    try:
        if not IDENTIFIER.fullmatch(args.release_candidate_id):
            raise ProofError("release candidate ID is invalid")
        package_manifest = load_json(args.package_manifest)
        installed_manifest = load_json(args.installed_manifest)
        schema = load_json(args.schema)
        proof = load_json(args.proof)
        capture = load_json(args.capture_evidence)
        if package_manifest.get("schema_version") != 1 or package_manifest.get("distribution") != "local-review-only":
            raise ProofError("proof validation requires a real package manifest")
        if installed_manifest.get("schema_version") != 1 or not isinstance(installed_manifest.get("files"), list):
            raise ProofError("proof validation requires installed reconciliation evidence")

        expected_hashes = {
            "packageManifestSha256": digest(args.package_manifest),
            "packageSha256": digest(args.package),
            "installedDistributionManifestSha256": digest(args.installed_manifest),
            "executableSha256": digest(args.installed_executable),
        }
        artifact = validate_artifact(
            proof.get("remoteTelemetryAbsence", {}).get("artifact"),
            args.release_candidate_id,
            expected_hashes,
        )
        executable_name = args.installed_executable.name
        installed_matches = [
            row for row in installed_manifest["files"]
            if isinstance(row, dict)
            and Path(str(row.get("path", ""))).name == executable_name
            and row.get("sha256") == expected_hashes["executableSha256"]
        ]
        if len(installed_matches) != 1:
            raise ProofError("installed executable is not uniquely bound by reconciliation evidence")
        validate_proof(proof, artifact, schema, digest(args.capture_evidence))
        validate_capture_evidence(capture, artifact, installed_manifest)
        validate_proof_capture_binding(proof, capture)
    except (OSError, json.JSONDecodeError, ProofError) as error:
        print(f"Installed privacy proof rejected: {error}")
        return 1
    print("Installed privacy proof passed for the exact reconciled packaged candidate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
