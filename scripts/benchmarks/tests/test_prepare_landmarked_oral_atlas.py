from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "prepare-landmarked-oral-atlas.py"
SPEC = importlib.util.spec_from_file_location("prepare_landmarked_oral_atlas", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
atlas = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(atlas)


def state(
    articulation: str,
    reference_type: str,
    aperture_pixels: float,
    aperture_ratio: float,
) -> dict[str, object]:
    return {
        "articulation": articulation,
        "referenceType": reference_type,
        "decodedSourceAperture": {
            "sourceCenterAperturePixels": aperture_pixels,
            "sourceCenterApertureRatioToCornerWidth": aperture_ratio,
        },
    }


class EnrollmentCoverageTests(unittest.TestCase):
    def test_legacy_labels_do_not_claim_observed_articulation(self) -> None:
        observed = {"coverage": "observed"}
        approximate = {"coverage": "observed-shape-not-phoneme-aligned"}
        generated = {"coverage": "generated-reference-not-observed-game-anatomy"}

        self.assertEqual(atlas.infer_reference_type(observed)[0], "observed")
        self.assertEqual(atlas.infer_reference_type(approximate)[0], "geometry-transfer")
        self.assertEqual(atlas.infer_reference_type(generated)[0], "generated")

    def test_decoded_aperture_reports_source_pixels_and_ratio(self) -> None:
        frame = {
            "width": 100,
            "height": 100,
            "cornerWidth": 0.4,
            "rollRadians": 0.0,
            "innerUpper": [[0.3, 0.5], [0.5, 0.48], [0.7, 0.5]],
            "innerLower": [[0.3, 0.5], [0.5, 0.52], [0.7, 0.5]],
        }

        metrics = atlas.decoded_source_aperture_metrics(frame)

        self.assertEqual(metrics["sourceCornerWidthPixels"], 40.0)
        self.assertEqual(metrics["sourceCenterAperturePixels"], 4.0)
        self.assertEqual(metrics["sourceCenterApertureRatioToCornerWidth"], 0.1)
        self.assertGreater(metrics["sourceAperturePixelCount"], 0)
        self.assertGreater(metrics["sourceAperturePixelRatioToCornerWidthSquared"], 0.0)

    def test_geometry_transfer_slots_are_reported_as_missing_observations(self) -> None:
        result = atlas.evaluate_coverage(
            [
                state("neutral", "observed", 0.0, 0.0),
                state("open", "geometry-transfer", 5.0, 0.12),
                state("spread", "generated", 6.0, 0.15),
            ],
            None,
        )

        self.assertEqual(result["observedArticulations"], [])
        self.assertEqual(
            result["missingArticulationCoverage"], ["open", "spread"]
        )
        self.assertEqual(
            result["referenceTypeCounts"],
            {"generated": 1, "geometry-transfer": 1, "observed": 1},
        )
        self.assertEqual(result["status"], "incomplete")
        self.assertIsNone(result["strictReview"]["passed"])

    def test_strict_review_uses_only_caller_defined_state_specific_minima(self) -> None:
        review = atlas.parse_coverage_review(
            {
                "requiredObservedArticulations": ["open", "rounded-open"],
                "minimumCenterAperturePixels": {"open": 6.0},
                "minimumCenterApertureRatio": {"open": 0.1},
            }
        )
        result = atlas.evaluate_coverage(
            [
                state("open", "observed", 5.5, 0.11),
                state("rounded-open", "geometry-transfer", 8.0, 0.2),
            ],
            review,
        )

        strict = result["strictReview"]
        self.assertFalse(strict["passed"])
        self.assertIn("missing observed articulation: rounded-open", strict["failures"])
        self.assertTrue(any("below caller minimum 6" in item for item in strict["failures"]))

    def test_empty_or_implicit_strict_requirements_are_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "at least one caller-defined requirement"):
            atlas.parse_coverage_review({})
        with self.assertRaisesRegex(ValueError, "finite non-negative"):
            atlas.parse_coverage_review(
                {"minimumCenterAperturePixels": {"open": True}}
            )

    def test_render_receipt_never_promotes_hashes_to_quality_qualification(self) -> None:
        coverage = atlas.evaluate_coverage(
            [state("open", "observed", 9.0, 0.2)], None
        )
        receipt = atlas.make_quality_document(
            "private-review-only", Path("observations"), {}, coverage, []
        )

        self.assertEqual(receipt["renderingStatus"], "rendered")
        self.assertEqual(receipt["qualityStatus"], "rendered-not-qualified")
        self.assertEqual(
            receipt["schema"], "interactive-npcs-landmarked-oral-atlas-quality/v2"
        )
        self.assertEqual(
            receipt["textureOwnership"], "declared-reference-oral-interior-only"
        )

    def test_passing_strict_coverage_review_remains_a_coverage_result(self) -> None:
        review = atlas.parse_coverage_review(
            {
                "requiredObservedArticulations": ["open"],
                "minimumCenterAperturePixels": {"open": 4.0},
                "minimumCenterApertureRatio": {"open": 0.1},
            }
        )

        result = atlas.evaluate_coverage(
            [state("open", "observed", 5.0, 0.12)], review
        )

        self.assertTrue(result["strictReview"]["passed"])
        self.assertEqual(result["status"], "observed-slots-present")

    def test_enrollment_binding_is_all_or_nothing_and_hash_bounded(self) -> None:
        with self.assertRaisesRegex(ValueError, "characterId"):
            atlas.build_enrollment_binding(
                "cyberpunk-2077", None, ["a" * 64], "reviewed-private", "b" * 64
            )
        with self.assertRaisesRegex(ValueError, "unique lowercase"):
            atlas.build_enrollment_binding(
                "cyberpunk-2077",
                "misty",
                ["A" * 64],
                "reviewed-private",
                "b" * 64,
            )
        with self.assertRaisesRegex(ValueError, "review evidence"):
            atlas.build_enrollment_binding(
                "cyberpunk-2077", "misty", ["a" * 64], "reviewed-private", None
            )

    def test_receipt_distinguishes_loader_binding_from_visual_quality(self) -> None:
        binding = atlas.build_enrollment_binding(
            "cyberpunk-2077",
            "misty",
            ["a" * 64],
            "reviewed-private",
            "b" * 64,
        )
        receipt = atlas.make_quality_document(
            "private-review-only",
            Path("observations"),
            {},
            atlas.evaluate_coverage([], None),
            [],
            binding,
        )

        self.assertTrue(receipt["enrollmentBinding"]["present"])
        self.assertTrue(receipt["enrollmentBinding"]["reviewedBindingDeclared"])
        self.assertFalse(receipt["enrollmentBinding"]["loaderAdmissionVerified"])
        self.assertEqual(receipt["qualityStatus"], "rendered-not-qualified")

        unreviewed = atlas.build_enrollment_binding(
            "cyberpunk-2077", "misty", ["a" * 64], "unreviewed", None
        )
        unreviewed_receipt = atlas.make_quality_document(
            "private-review-only",
            Path("observations"),
            {},
            atlas.evaluate_coverage([], None),
            [],
            unreviewed,
        )
        self.assertFalse(unreviewed_receipt["enrollmentBinding"]["reviewedBindingDeclared"])


if __name__ == "__main__":
    unittest.main()
