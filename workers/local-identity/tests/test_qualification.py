from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from model_spec import PACK_ID, PACK_MANIFEST_SHA256, PACK_REVISION, SpecError  # noqa: E402
from qualification import (  # noqa: E402
    RAW_SCHEMA,
    build_unsigned_candidate,
    load_plan,
    validate_raw_samples,
)

PLAN = ROOT / "qualification-plan.v1.json"


def raw_samples(plan: dict) -> dict:
    samples = []
    for phase in plan["performance_phases"]:
        for index in range(1, phase["minimum_successful_samples"] + 1):
            metrics = {metric: float(index) for metric in phase["required_metrics"]}
            for metric in metrics:
                if "vram" in metric or metric.endswith("_count"):
                    metrics[metric] = 0.0
            samples.append(
                {
                    "phase_id": phase["id"],
                    "sample_index": index,
                    "success": True,
                    "metrics": metrics,
                }
            )
    return {
        "schema": RAW_SCHEMA,
        "suite_revision": plan["suite_revision"],
        "pack_id": PACK_ID,
        "pack_revision": PACK_REVISION,
        "manifest_file_sha256": PACK_MANIFEST_SHA256,
        "device_fingerprint_sha256": "a" * 64,
        "measured_unix_seconds": 1_000_000,
        "expires_unix_seconds": 1_000_000 + 86400,
        "sequence": 1,
        "samples": samples,
    }


class QualificationPlanTests(unittest.TestCase):
    def test_plan_is_strict_prepared_and_every_phase_requires_at_least_twenty(self) -> None:
        plan = load_plan(PLAN)
        self.assertEqual(plan["status"], "prepared_not_executed")
        self.assertFalse(plan["execution_gate"]["model_download_performed"])
        self.assertFalse(plan["execution_gate"]["model_inference_performed"])
        self.assertTrue(
            all(phase["minimum_successful_samples"] >= 20 for phase in plan["performance_phases"])
        )
        operations = {phase["operation"] for phase in plan["performance_phases"]}
        self.assertTrue({"load", "reload", "infer", "cancel", "unload", "restart"} <= operations)

    def test_complete_raw_samples_only_build_an_unsigned_non_admissible_candidate(self) -> None:
        plan = load_plan(PLAN)
        candidate = build_unsigned_candidate(plan, raw_samples(plan))
        self.assertEqual(candidate["evidence_authority"], "unsigned_candidate_not_admissible")
        self.assertFalse(candidate["admission_ready"])
        self.assertEqual(candidate["resource_envelope_candidate"]["signatures"], [])
        placement = candidate["resource_envelope_candidate"]["signed"]["placements"]["cpu_resident"]
        self.assertEqual(placement["resident_vram_bytes"], 0)
        self.assertEqual(placement["p99_workspace_vram_bytes"], 0)

    def test_missing_successful_samples_and_cpu_mode_vram_fail_closed(self) -> None:
        plan = load_plan(PLAN)
        raw = raw_samples(plan)
        raw["samples"] = [
            sample
            for sample in raw["samples"]
            if not (sample["phase_id"] == "cold_load" and sample["sample_index"] == 1)
        ]
        with self.assertRaisesRegex(SpecError, "cold_load"):
            validate_raw_samples(plan, raw)

        raw = raw_samples(plan)
        changed = next(sample for sample in raw["samples"] if sample["phase_id"] == "single_face_inference")
        changed["metrics"]["dedicated_vram_bytes_peak"] = 1.0
        with self.assertRaisesRegex(SpecError, "dedicated model VRAM"):
            build_unsigned_candidate(plan, raw)

    def test_unknown_metrics_and_binding_drift_fail_closed(self) -> None:
        plan = load_plan(PLAN)
        raw = raw_samples(plan)
        changed = copy.deepcopy(raw)
        changed["samples"][0]["metrics"]["invented"] = 1.0
        with self.assertRaisesRegex(SpecError, "metrics differ"):
            validate_raw_samples(plan, changed)
        changed = copy.deepcopy(raw)
        changed["manifest_file_sha256"] = "b" * 64
        with self.assertRaisesRegex(SpecError, "not bound"):
            validate_raw_samples(plan, changed)


if __name__ == "__main__":
    unittest.main()
