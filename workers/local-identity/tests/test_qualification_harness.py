from __future__ import annotations

import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from model_spec import PACK_ID, PACK_MANIFEST_SHA256, PACK_REVISION, SpecError  # noqa: E402
from qualification import RAW_SCHEMA, load_plan  # noqa: E402
from qualification_harness import (  # noqa: E402
    BUNDLE_SCHEMA,
    CLEANUP_SCHEMA,
    DRIVER_DESCRIPTOR_SCHEMA,
    FIXTURE_SCHEMA,
    GRANT_SCHEMA,
    QUALITY_SCHEMA,
    parse_args,
    validate_cleanup_report,
    validate_driver_bundle,
    validate_fixture_manifest,
    validate_grant,
)

PLAN = ROOT / "qualification-plan.v1.json"


def complete_raw(plan: dict) -> dict:
    samples = []
    for phase in plan["performance_phases"]:
        for index in range(1, phase["minimum_successful_samples"] + 1):
            metrics = {name: float(index) for name in phase["required_metrics"]}
            for name in metrics:
                if "vram" in name or name.endswith("_count"):
                    metrics[name] = 0.0
            samples.append({"phase_id": phase["id"], "sample_index": index, "success": True, "metrics": metrics})
    return {
        "schema": RAW_SCHEMA,
        "suite_revision": plan["suite_revision"],
        "pack_id": PACK_ID,
        "pack_revision": PACK_REVISION,
        "manifest_file_sha256": PACK_MANIFEST_SHA256,
        "device_fingerprint_sha256": "a" * 64,
        "measured_unix_seconds": 1000,
        "expires_unix_seconds": 2000,
        "sequence": 1,
        "samples": samples,
    }


def quality(plan: dict) -> dict:
    return {
        "schema": QUALITY_SCHEMA,
        "suite_revision": plan["suite_revision"],
        "partition": "held_out_only",
        "threshold_source": "calibration_only",
        "counts": {"known_characters": 20, "unknown_characters": 20, "known_samples": 100, "unknown_samples": 60, "hard_negative_pairs": 100, "multi_face_sequences": 10, "tracking_frames": 600},
        "metrics": {"false_accept_rate": 0.01, "false_reject_rate": 0.02, "unknown_false_accept_rate": 0.01, "ambiguity_rate": 0.03, "manual_correction_rate": 0.01, "association_accuracy": 0.98, "reacquisition_accuracy": 0.97},
        "tracking": {"identity_switches": 1, "track_fragmentation": 2, "offscreen_continuity_errors": 0, "reacquisition_failures": 1, "false_matches": 1},
        "calibration": {"match_threshold": 0.5, "ambiguity_margin": 0.1, "consensus_window": 5, "minimum_votes": 3, "top1_top2_margin_quantiles": {"p50": 0.2, "p95": 0.4, "p99": 0.5, "min": 0.01, "max": 0.8}},
        "raw_decisions_sha256": "b" * 64,
        "protected_trait_labels_used": False,
        "admission_ready": False,
    }


def cleanup() -> dict:
    return {
        "schema": CLEANUP_SCHEMA,
        "worker_processes_remaining": 0,
        "driver_children_remaining": 0,
        "open_mapping_count": 0,
        "unreleased_lease_count": 0,
        "late_result_count": 0,
        "remote_endpoint_count": 0,
        "gpu_lock_unchanged": True,
        "job_object_closed": True,
        "pixels_persisted": False,
        "embeddings_persisted": False,
        "raw_network_inventory_sha256": "c" * 64,
        "passed": True,
    }


class QualificationHarnessTests(unittest.TestCase):
    def test_parent_grant_is_exact_short_lived_cpu_authorization(self) -> None:
        plan = load_plan(PLAN)
        grant = {
            "schema": GRANT_SCHEMA,
            "grant_id": "root-grant-1234",
            "suite_revision": plan["suite_revision"],
            "pack_id": PACK_ID,
            "pack_revision": PACK_REVISION,
            "manifest_file_sha256": PACK_MANIFEST_SHA256,
            "backend": "opencv-dnn-cpu",
            "allow_identity_model_execution": True,
            "cpu_still_requires_gpu_lane": True,
            "single_use": True,
            "issued_unix_seconds": 1000,
            "expires_unix_seconds": 1100,
            "maximum_run_seconds": 100,
            "token_sha256": hashlib.sha256(b"a-parent-secret-token-longer-than-32-bytes").hexdigest(),
            "authorized_by": "root_orchestrator",
        }
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "grant.json"
            path.write_text(json.dumps(grant), encoding="utf-8")
            self.assertEqual(validate_grant(path, plan, 1050)["grant_id"], "root-grant-1234")
            changed = copy.deepcopy(grant)
            changed["cpu_still_requires_gpu_lane"] = False
            path.write_text(json.dumps(changed), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "one-time authorization"):
                validate_grant(path, plan, 1050)
            path.write_text(json.dumps(grant), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "expired"):
                validate_grant(path, plan, 1200)

    def test_fixture_manifest_rejects_protected_traits_before_file_access(self) -> None:
        plan = load_plan(PLAN)
        value = {"schema": FIXTURE_SCHEMA, "suite_revision": plan["suite_revision"], "local_only": True, "partitions": [], "files": [], "rights_attestation": {"attested_by_local_user": True, "attested_unix_seconds": 1, "no_web_scrape": True, "no_third_party_biometrics": True, "protected_trait_labels_absent": True}, "ethnicity": "forbidden"}
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "fixtures.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(SpecError, "protected-trait"):
                validate_fixture_manifest(path, plan)

    def test_real_driver_bundle_is_unsigned_and_meets_every_minimum(self) -> None:
        plan = load_plan(PLAN)
        bundle = {
            "schema": BUNDLE_SCHEMA,
            "descriptor": {"schema": DRIVER_DESCRIPTOR_SCHEMA, "source": "windows_graphics_capture", "command_ids": [16, 17], "advancing_wgc_frames": True, "fixture_backend": False, "hidden_processes": True, "no_download": True, "cpu_backend": "opencv-dnn-cpu", "game_load_measured": True, "vram_measured": True, "network_egress_measured": True},
            "raw_samples": complete_raw(plan),
            "quality_report": quality(plan),
            "cleanup_report": cleanup(),
            "signatures": [],
            "admission_ready": False,
        }
        raw, report, cleaned = validate_driver_bundle(plan, bundle)
        self.assertEqual(len([sample for sample in raw["samples"] if sample["phase_id"] == "single_face_inference"]), 100)
        self.assertEqual(report["partition"], "held_out_only")
        self.assertTrue(cleaned["passed"])
        changed = copy.deepcopy(bundle)
        changed["descriptor"]["fixture_backend"] = True
        with self.assertRaisesRegex(SpecError, "real WGC"):
            validate_driver_bundle(plan, changed)

    def test_cleanup_fails_on_egress_or_orphans(self) -> None:
        for key in ("remote_endpoint_count", "open_mapping_count", "worker_processes_remaining"):
            changed = cleanup()
            changed[key] = 1
            with self.assertRaisesRegex(SpecError, "cleanup/no-egress"):
                validate_cleanup_report(changed)

    def test_help_contract_defaults_to_model_free_preflight(self) -> None:
        args = parse_args(["--config", "run.json"])
        self.assertFalse(args.execute)
        self.assertFalse(args.acknowledge_real_model_run)
        with self.assertRaises(SystemExit) as raised:
            parse_args(["--help"])
        self.assertEqual(raised.exception.code, 0)


if __name__ == "__main__":
    unittest.main()
