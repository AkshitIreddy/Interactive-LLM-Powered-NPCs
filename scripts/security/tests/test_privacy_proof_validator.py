from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path


SECURITY_DIR = Path(__file__).resolve().parents[1]
REPOSITORY_ROOT = SECURITY_DIR.parents[1]
sys.path.insert(0, str(SECURITY_DIR))

from validate_installed_privacy_proof import (  # noqa: E402
    ProofError,
    validate_artifact,
    validate_capture_evidence,
    validate_proof,
    validate_proof_capture_binding,
)


class InstalledPrivacyProofValidatorTests(unittest.TestCase):
    def artifact(self) -> dict[str, str]:
        return {
            "applicationVersion": "2.0.0-alpha.1",
            "releaseCandidateId": "rc-test-candidate",
            "packageManifestSha256": "a" * 64,
            "packageSha256": "b" * 64,
            "installedDistributionManifestSha256": "c" * 64,
            "executableSha256": "d" * 64,
        }

    def proof(self) -> dict:
        artifact = self.artifact()
        header = {
            "schemaVersion": "1.0.0",
            "artifact": artifact,
            "provenance": "measured",
            "evidenceSource": "packaged_executable_observation",
            "observedAtUtc": "2026-08-30T10:00:00Z",
            "outcome": "passed",
        }
        return {
            "schemaVersion": "1.0.0",
            "captureEvidenceSha256": "e" * 64,
            "remoteTelemetryAbsence": {
                **header,
                "evidenceRunId": "run-telemetry",
                "dependencyInventoryScanned": True,
                "endpointInventoryScanned": True,
                "automaticUploadEntryPoints": 0,
                "remoteTelemetryDestinations": 0,
            },
            "denyAllEgress": [
                {
                    **header,
                    "evidenceRunId": "run-offline",
                    "scenario": "offline_mode",
                    "enforcement": "os_deny_all",
                    "monitoredProcessCount": 1,
                    "observedConnectionAttempts": 0,
                    "observedProviderRequests": 0,
                    "externalDestinationCount": 0,
                },
                {
                    **header,
                    "evidenceRunId": "run-local-lip-sync",
                    "scenario": "local_lip_sync",
                    "enforcement": "os_deny_all",
                    "monitoredProcessCount": 1,
                    "observedConnectionAttempts": 0,
                    "observedProviderRequests": 0,
                    "externalDestinationCount": 0,
                },
            ],
        }

    def installed_manifest(self) -> dict:
        return {
            "schema_version": 1,
            "files": [
                {"path": "interactive-npcs-control.exe", "sha256": "d" * 64},
                {"path": "npc-runtime.exe", "sha256": "f" * 64},
            ],
        }

    def capture(self) -> dict:
        rows = []
        for scenario in ("offline_mode", "local_lip_sync"):
            rows.append({
                "scenario": scenario,
                "runId": {
                    "offline_mode": "run-offline",
                    "local_lip_sync": "run-local-lip-sync",
                }[scenario],
                "receiptSource": "packaged_executable",
                "scenarioCompleted": True,
                "observedAtUtc": "2026-08-30T10:00:00Z",
                "monitoredProcesses": [{
                    "imageName": "interactive-npcs-control.exe",
                    "sha256": "d" * 64,
                    "processId": 4242,
                    "parentProcessId": 100,
                }],
                "securityEventRecordStart": 1000,
                "securityEventRecordEnd": 1001,
                "observedConnectionAttempts": 0,
                "observedProviderRequests": 0,
                "externalDestinationCount": 0,
                "loopbackConnectionCount": 1,
            })
        return {
            "schemaVersion": "1.0.0",
            "provenance": "measured",
            "evidenceSource": "packaged_executable_observation",
            "releaseCandidateId": "rc-test-candidate",
            "packageManifestSha256": "a" * 64,
            "packageSha256": "b" * 64,
            "installedDistributionManifestSha256": "c" * 64,
            "executableSha256": "d" * 64,
            "platform": "windows",
            "enforcementMechanism": "windows_defender_firewall_program_deny",
            "auditSource": "windows_security_filtering_platform",
            "firewallRuleGroupId": "privacy-proof-test",
            "firewallRuleCount": 2,
            "auditPolicyVerified": True,
            "telemetryInventory": {
                "provenance": "measured",
                "evidenceSource": "packaged_binary_inventory",
                "dependencyInventoryScanned": True,
                "endpointInventoryScanned": True,
                "automaticUploadEntryPoints": 0,
                "remoteTelemetryDestinations": 0,
            },
            "scenarios": rows,
        }

    def schema(self) -> dict:
        path = REPOSITORY_ROOT / "crates/diagnostics/schemas/privacy-proof-v1.schema.json"
        return json.loads(path.read_text(encoding="utf-8"))

    def test_fixture_provenance_cannot_validate_as_packaged_evidence(self) -> None:
        proof = self.proof()
        proof["denyAllEgress"][0]["provenance"] = "fixture"
        proof["denyAllEgress"][0]["evidenceSource"] = "fixture_harness"
        with self.assertRaisesRegex(ProofError, "fixture, unmeasured"):
            validate_proof(proof, self.artifact(), self.schema())

    def test_internal_harness_cannot_validate_as_os_deny_all(self) -> None:
        proof = self.proof()
        proof["denyAllEgress"][1]["enforcement"] = "test_harness"
        with self.assertRaisesRegex(ProofError, "OS deny-all"):
            validate_proof(proof, self.artifact(), self.schema())

    def test_boolean_counters_cannot_masquerade_as_numeric_zero(self) -> None:
        proof = self.proof()
        proof["remoteTelemetryAbsence"]["automaticUploadEntryPoints"] = False
        with self.assertRaisesRegex(ProofError, "telemetry absence"):
            validate_proof(proof, self.artifact(), self.schema())

    def test_duplicate_run_or_missing_scenario_fails_closed(self) -> None:
        duplicate = self.proof()
        duplicate["denyAllEgress"][1]["evidenceRunId"] = "run-offline"
        with self.assertRaisesRegex(ProofError, "run IDs"):
            validate_proof(duplicate, self.artifact(), self.schema())

        missing = self.proof()
        missing["denyAllEgress"][1]["scenario"] = "offline_mode"
        with self.assertRaisesRegex(ProofError, "both required"):
            validate_proof(missing, self.artifact(), self.schema())

    def test_candidate_hash_binding_is_exact(self) -> None:
        expected = {
            "packageManifestSha256": "a" * 64,
            "packageSha256": "b" * 64,
            "installedDistributionManifestSha256": "c" * 64,
            "executableSha256": "d" * 64,
        }
        validate_artifact(self.artifact(), "rc-test-candidate", expected)
        changed = copy.deepcopy(expected)
        changed["executableSha256"] = "e" * 64
        with self.assertRaisesRegex(ProofError, "executableSha256"):
            validate_artifact(self.artifact(), "rc-test-candidate", changed)

    def test_capture_requires_windows_sources_and_installed_process_hashes(self) -> None:
        validate_capture_evidence(self.capture(), self.artifact(), self.installed_manifest())
        fixture = self.capture()
        fixture["evidenceSource"] = "fixture_harness"
        with self.assertRaisesRegex(ProofError, "measured installed-candidate"):
            validate_capture_evidence(fixture, self.artifact(), self.installed_manifest())
        unbound = self.capture()
        unbound["scenarios"][0]["monitoredProcesses"][0]["sha256"] = "1" * 64
        with self.assertRaisesRegex(ProofError, "installed manifest"):
            validate_capture_evidence(unbound, self.artifact(), self.installed_manifest())

    def test_capture_rejects_uncompleted_or_egressing_scenario(self) -> None:
        incomplete = self.capture()
        incomplete["scenarios"][1]["scenarioCompleted"] = False
        with self.assertRaisesRegex(ProofError, "fixture, incomplete, or observed egress"):
            validate_capture_evidence(incomplete, self.artifact(), self.installed_manifest())
        egress = self.capture()
        egress["scenarios"][0]["observedConnectionAttempts"] = 1
        with self.assertRaisesRegex(ProofError, "fixture, incomplete, or observed egress"):
            validate_capture_evidence(egress, self.artifact(), self.installed_manifest())

    def test_compact_proof_must_exactly_project_retained_capture(self) -> None:
        proof = self.proof()
        capture = self.capture()
        validate_proof_capture_binding(proof, capture)

        stale_run = self.proof()
        stale_run["denyAllEgress"][0]["evidenceRunId"] = "stale-run"
        with self.assertRaisesRegex(ProofError, "offline_mode capture for evidenceRunId"):
            validate_proof_capture_binding(stale_run, capture)

        wrong_process_count = self.proof()
        wrong_process_count["denyAllEgress"][1]["monitoredProcessCount"] = 2
        with self.assertRaisesRegex(ProofError, "local_lip_sync capture for monitoredProcessCount"):
            validate_proof_capture_binding(wrong_process_count, capture)

        stale_inventory = self.capture()
        stale_inventory["telemetryInventory"]["remoteTelemetryDestinations"] = 1
        with self.assertRaisesRegex(ProofError, "telemetry remoteTelemetryDestinations"):
            validate_proof_capture_binding(proof, stale_inventory)


if __name__ == "__main__":
    unittest.main()
