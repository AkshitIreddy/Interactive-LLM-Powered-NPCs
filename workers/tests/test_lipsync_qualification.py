from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path


WORKERS = Path(__file__).resolve().parents[1]
STUBS = WORKERS / "stubs"
sys.path.insert(0, str(STUBS))

from contract import ContractError  # noqa: E402
from lipsync_catalog import LipSyncPackCatalog  # noqa: E402
from lipsync_qualification import (  # noqa: E402
    CANDIDATE_ID,
    MODEL_REVISION,
    PLAN_ID,
    LipSyncQualificationPlan,
)


PLAN_PATH = WORKERS / "packs" / "audio2face-3d-regression-v2.3-mark.qualification-plan.json"
CATALOG_PATH = WORKERS / "packs" / "lipsync-pack-catalog-v1.development.json"
SCHEMA_PATH = WORKERS / "protocol" / "lipsync-qualification-plan-v1.schema.json"


class Audio2FaceQualificationPlanTests(unittest.TestCase):
    def setUp(self) -> None:
        self.plan = LipSyncQualificationPlan.load(PLAN_PATH)

    def test_plan_is_pinned_non_downloadable_and_bound_to_deferred_catalog_candidate(self) -> None:
        raw = self.plan.raw
        self.assertEqual(raw["plan_id"], PLAN_ID)
        self.assertEqual(raw["candidate_id"], CANDIDATE_ID)
        self.assertEqual(raw["source_identity"]["model_variant"], "Mark")
        self.assertEqual(raw["source_identity"]["model_version"], "2.3")
        self.assertEqual(raw["source_identity"]["model_revision"], MODEL_REVISION)
        self.assertEqual(len(MODEL_REVISION), 40)
        self.assertEqual(raw["source_identity"]["access"], "public_ungated")
        self.assertEqual(raw["selection"]["activation_state"], "blocked_pending_qualification")
        self.assertFalse(raw["selection"]["contains_download_locations"])
        self.assertFalse(raw["selection"]["contains_model_payloads"])
        self.assertFalse(raw["selection"]["live_selectable"])
        self.assertFalse(raw["selection"]["offline_selectable"])

        self.plan.assert_catalog_binding(LipSyncPackCatalog.load(CATALOG_PATH))

    def test_plan_requires_timestamped_coefficients_and_current_frame_mouth_residual_only(self) -> None:
        raw = self.plan.raw
        output = raw["io_contract"]["output"]
        self.assertEqual(output["modality"], "timestamped_arkit_blendshape_coefficients")
        self.assertEqual(output["source_timestamp_basis"], "source_audio_sample_index")
        self.assertEqual(output["delivery_timestamp_basis"], "query_performance_counter")
        self.assertEqual(output["sequence_policy"], "strictly_monotonic")

        mapper = raw["residual_mapper_contract"]
        self.assertEqual(mapper["output_contract"], "npc.mouth-residual/v1")
        self.assertEqual(mapper["non_mouth_coefficient_policy"], "discard")
        self.assertFalse(mapper["full_frame_replacement"])
        self.assertFalse(mapper["static_avatar_source"])
        self.assertFalse(mapper["base_frame_mutation"])
        self.assertTrue(mapper["alpha_outside_mask_zero"])
        self.assertEqual(mapper["maximum_source_frame_advance"], 0)
        self.assertEqual(mapper["maximum_displayed_frames"], 1)
        self.assertTrue(mapper["fail_open_to_unmodified_frame"])

    def test_expected_environment_resource_formula_and_measurements_are_explicit(self) -> None:
        raw = self.plan.raw
        environment = raw["expected_environment"]
        self.assertEqual(environment["platforms"], ["windows_10_22h2_x64", "windows_11_x64"])
        self.assertEqual(environment["cuda"], {
            "minimum": "12.8.0",
            "maximum_exclusive": "13.0.0",
            "reference": "12.9.0",
        })
        self.assertEqual(environment["tensorrt"], {
            "minimum": "10.13.0",
            "maximum_exclusive": "11.0.0",
            "reference": "10.13.0",
        })
        self.assertEqual(environment["gpu"]["reference_target"], "rtx_4080_laptop_12_gib")
        self.assertIn("silent_cpu_boost_disabled", environment["qualification_power_profiles"])

        admission = raw["resource_admission"]
        self.assertEqual(admission["budget_source"], "dxgi_query_video_memory_info")
        self.assertEqual(admission["local_visual_gpu_lease"], "exclusive")
        self.assertEqual(admission["required_peak_statistic"], "measured_vram_p99_mib")
        self.assertIn("configured_game_reserve_mib", admission["admission_formula"])
        self.assertEqual(admission["on_rejection"], "remain_unselectable_without_cloud_fallback")

        measurement = raw["measurement_plan"]
        self.assertEqual(measurement["status"], "not_measured")
        self.assertEqual(measurement["observations"], {})
        self.assertEqual(measurement["workload"]["minimum_measured_frames"], 18000)
        self.assertEqual(measurement["workload"]["soak_minutes"], 30)
        self.assertLessEqual(
            measurement["acceptance_thresholds"]["coefficient_compute_ms_p95_max"],
            1000 / measurement["workload"]["coefficient_cadence_hz"],
        )
        self.assertEqual(
            measurement["acceptance_thresholds"]["stale_or_wrong_track_presentations_max"],
            0,
        )

    def test_schema_carries_the_same_pinned_safety_contract(self) -> None:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        self.assertEqual(schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertFalse(schema["additionalProperties"])
        self.assertEqual(schema["properties"]["plan_id"]["const"], PLAN_ID)
        self.assertEqual(schema["properties"]["candidate_id"]["const"], CANDIDATE_ID)
        source = schema["properties"]["source_identity"]["properties"]
        self.assertEqual(source["model_revision"]["const"], MODEL_REVISION)
        selection = schema["properties"]["selection"]["properties"]
        self.assertFalse(selection["contains_download_locations"]["const"])
        self.assertFalse(selection["live_selectable"]["const"])
        mapper = schema["properties"]["residual_mapper_contract"]["properties"]
        self.assertFalse(mapper["full_frame_replacement"]["const"])
        self.assertEqual(mapper["maximum_displayed_frames"]["const"], 1)

    def test_validator_rejects_selection_source_mapper_and_measurement_drift(self) -> None:
        mutations = []

        selectable = copy.deepcopy(self.plan.raw)
        selectable["selection"]["live_selectable"] = True
        mutations.append(selectable)

        moving_revision = copy.deepcopy(self.plan.raw)
        moving_revision["source_identity"]["model_revision"] = "main"
        mutations.append(moving_revision)

        full_frame = copy.deepcopy(self.plan.raw)
        full_frame["residual_mapper_contract"]["full_frame_replacement"] = True
        mutations.append(full_frame)

        stale_two_frames = copy.deepcopy(self.plan.raw)
        stale_two_frames["residual_mapper_contract"]["maximum_displayed_frames"] = 2
        mutations.append(stale_two_frames)

        guessed_observation = copy.deepcopy(self.plan.raw)
        guessed_observation["measurement_plan"]["observations"] = {"vram_mib": 1024}
        mutations.append(guessed_observation)

        relaxed_latency = copy.deepcopy(self.plan.raw)
        relaxed_latency["measurement_plan"]["acceptance_thresholds"]["coefficient_compute_ms_p95_max"] = 30
        mutations.append(relaxed_latency)

        for raw in mutations:
            with self.subTest(raw=raw):
                with self.assertRaises(ContractError):
                    LipSyncQualificationPlan.validate(raw)

    def test_validator_rejects_download_locations_and_credential_fields_at_any_depth(self) -> None:
        location = copy.deepcopy(self.plan.raw)
        location["source_identity"]["download_url"] = "https://invalid.example/model"

        credential = copy.deepcopy(self.plan.raw)
        credential["expected_environment"]["api_token"] = "not-a-real-secret"

        embedded_location = copy.deepcopy(self.plan.raw)
        embedded_location["display_name"] = "https://invalid.example/model"

        for raw in (location, credential, embedded_location):
            with self.subTest(raw=raw):
                with self.assertRaises(ContractError):
                    LipSyncQualificationPlan.validate(raw)


if __name__ == "__main__":
    unittest.main()
