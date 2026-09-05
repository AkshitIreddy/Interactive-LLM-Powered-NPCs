from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import tempfile
import unittest

import cv2
import numpy as np


SCRIPT = Path(__file__).resolve().parents[1] / "prepare-photometric-reference-atlas.py"
SPEC = importlib.util.spec_from_file_location("prepare_photometric_reference_atlas", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
atlas = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(atlas)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def mouth_frame(name: str, *, center_x: float = 0.5, center_y: float = 0.54, half_width: float = 0.16, half_height: float = 0.035) -> dict[str, object]:
    left = [center_x - half_width, center_y, 0.0]
    right = [center_x + half_width, center_y, 0.0]
    outer_upper = [
        left,
        [center_x - half_width * 0.5, center_y - half_height * 0.72, 0.0],
        [center_x, center_y - half_height, 0.0],
        [center_x + half_width * 0.5, center_y - half_height * 0.72, 0.0],
        right,
    ]
    outer_lower = [
        left,
        [center_x - half_width * 0.5, center_y + half_height * 0.72, 0.0],
        [center_x, center_y + half_height, 0.0],
        [center_x + half_width * 0.5, center_y + half_height * 0.72, 0.0],
        right,
    ]
    return {
        "file": name,
        "accepted": True,
        "reason": "accepted",
        "width": 128,
        "height": 96,
        "center": [center_x, center_y],
        "cornerWidth": half_width * 2.0,
        "rollRadians": 0.0,
        "outerUpper": outer_upper,
        "outerLower": outer_lower,
    }


class Fixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.specs = root / "state-specs.json"
        self.metadata = root / "landmarks.json"
        self.neutral = root / "neutral.png"
        self.images = ["neutral.png", "ah.png", "ee.png", "oo.png"]
        for index, name in enumerate(self.images):
            image = np.full((96, 128, 3), (34 + index * 7, 48 + index * 5, 82 + index * 9), np.uint8)
            cv2.ellipse(
                image,
                (64, 52),
                (20 - index, 4 + index * 2),
                0,
                0,
                360,
                (12 + index * 4, 18 + index * 3, 42 + index * 6),
                thickness=-1,
            )
            if not cv2.imwrite(str(root / name), image):
                raise RuntimeError(f"could not write fixture {name}")
        self.frames = [
            mouth_frame("neutral.png", half_height=0.025),
            mouth_frame("ah.png", center_y=0.545, half_height=0.09),
            mouth_frame("ee.png", center_x=0.497, half_width=0.18, half_height=0.045),
            mouth_frame("oo.png", center_x=0.503, center_y=0.548, half_width=0.11, half_height=0.07),
        ]
        self.states = [
            {
                "name": "neutral",
                "image": "neutral.png",
                "metadata": "landmarks.json",
                "coefficients": [0, 1, 0, 0, 0, 0, 0, 0],
                "transparent": True,
                "coverage": "generated-reference-not-observed-game-anatomy",
            },
            {
                "name": "ah-open",
                "image": "ah.png",
                "metadata": "landmarks.json",
                "coefficients": [0.9, 0, 0, 0, 0, 0, 0.2, 0.5],
                "coverage": "generated-reference-not-observed-game-anatomy",
            },
            {
                "name": "ee-spread",
                "image": "ee.png",
                "metadata": "landmarks.json",
                "coefficients": [0.3, 0, 0, 0, 0.8, 0.8, 0, 0],
                "coverage": "generated-reference-not-observed-game-anatomy",
            },
            {
                "name": "oo-rounded",
                "image": "oo.png",
                "metadata": "landmarks.json",
                "coefficients": [0.1, 0.7, 0.8, 0.8, 0, 0, 0, 0],
                "coverage": "generated-reference-not-observed-game-anatomy",
            },
        ]
        self.write()

    def write(self) -> None:
        self.metadata.write_text(
            json.dumps(
                {
                    "schema": atlas.METADATA_SCHEMA,
                    "model": "fixture",
                    "accepted": len(self.frames),
                    "total": len(self.frames),
                    "frames": self.frames,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        self.specs.write_text(
            json.dumps(
                {
                    "schema": "interactive-npcs-landmarked-oral-state-specs/v1",
                    "scope": "private-fixture-review",
                    "notes": ["fixture only"],
                    "states": self.states,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )


def binding() -> dict[str, object]:
    result = atlas.SHARED.build_enrollment_binding(
        "fixture-game",
        "fixture-character",
        ["a" * 64],
        "reviewed-private",
        "b" * 64,
    )
    assert result is not None
    return result


class PhotometricReferenceAtlasTests(unittest.TestCase):
    def test_non_square_reference_uses_physical_pixel_roll_for_registration(self) -> None:
        frame = mouth_frame("tilted.png")
        frame["width"] = 200
        frame["height"] = 100
        for contour_name in ("outerUpper", "outerLower"):
            contour = frame[contour_name]
            contour[0][1] = 0.49
            contour[-1][1] = 0.59
        normalized_dx = frame["outerUpper"][-1][0] - frame["outerUpper"][0][0]
        normalized_dy = frame["outerUpper"][-1][1] - frame["outerUpper"][0][1]
        frame["cornerWidth"] = math.hypot(normalized_dx, normalized_dy)
        frame["rollRadians"] = math.atan2(normalized_dy, normalized_dx)

        parsed = atlas.parse_frame(frame, "tilted.png", (200, 100))
        expected_physical_roll = math.atan2(normalized_dy * 100, normalized_dx * 200)

        self.assertAlmostEqual(parsed["_rollRadiansNormalized"], frame["rollRadians"])
        self.assertAlmostEqual(parsed["_rollRadiansPixels"], expected_physical_roll)
        self.assertNotAlmostEqual(parsed["_rollRadiansPixels"], frame["rollRadians"])
        _, _, canonical = atlas.canonical_map(parsed, 96, 64)
        self.assertAlmostEqual(canonical["rollRadians"], expected_physical_roll)

    def test_export_is_deterministic_and_has_one_common_alpha(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            output_a = root / "output-a"
            output_b = root / "output-b"

            result_a = atlas.export_atlas(
                fixture.specs, fixture.neutral, fixture.metadata, output_a, 96, 64, binding()
            )
            result_b = atlas.export_atlas(
                fixture.specs, fixture.neutral, fixture.metadata, output_b, 96, 64, binding()
            )

            self.assertEqual(result_a["textureSha256"], result_b["textureSha256"])
            self.assertEqual(result_a["manifestSha256"], result_b["manifestSha256"])
            self.assertEqual(digest(output_a / "atlas-bgra8-premultiplied.bin"), result_a["textureSha256"])
            self.assertGreaterEqual(result_a["nearOpaquePixels"], 16)
            self.assertGreaterEqual(result_a["zeroAlphaPixels"], 16)

            manifest = json.loads((output_a / "atlas.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["schemaVersion"], 3)
            self.assertEqual(manifest["neutralStateIndex"], 0)
            self.assertEqual(manifest["texture"]["representation"], "photometric-full-lip-reference-v1")
            self.assertEqual(manifest["texture"]["stateCount"], 4)
            self.assertEqual(manifest["states"][0]["coefficients"], [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
            self.assertEqual(manifest["enrollmentBinding"], binding())

            texture = np.frombuffer(
                (output_a / "atlas-bgra8-premultiplied.bin").read_bytes(), dtype=np.uint8
            ).reshape(4, 64, 96, 4)
            for index in range(1, 4):
                np.testing.assert_array_equal(texture[0, :, :, 3], texture[index, :, :, 3])
            self.assertTrue(np.all(texture[:, :, :, :3] <= texture[:, :, :, 3:4]))

            quality = json.loads((output_a / "atlas-quality.json").read_text(encoding="utf-8"))
            self.assertEqual(quality["qualityStatus"], "rendered-not-qualified")
            self.assertFalse(quality["teacherQualified"])
            self.assertEqual(quality["coverage"]["referenceTypeCounts"]["generated"], 4)
            self.assertEqual(quality["coverage"]["status"], "incomplete")
            self.assertTrue(quality["commonAlpha"]["sharedByEveryState"])
            self.assertEqual(
                quality["commonAlpha"]["sha256"],
                hashlib.sha256(texture[0, :, :, 3].tobytes()).hexdigest(),
            )

    def test_state_zero_must_be_exact_neutral_contact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            fixture.states[0]["coefficients"] = [0.01, 0.99, 0, 0, 0, 0, 0, 0]
            fixture.write()

            with self.assertRaisesRegex(ValueError, "exact neutral-contact"):
                atlas.export_atlas(
                    fixture.specs,
                    fixture.neutral,
                    fixture.metadata,
                    fixture.root / "output",
                    96,
                    64,
                    binding(),
                )

    def test_mismatched_image_dimensions_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            self.assertTrue(cv2.imwrite(str(fixture.root / "ah.png"), np.zeros((95, 128, 3), np.uint8)))

            with self.assertRaisesRegex(ValueError, "dimensions do not match neutral"):
                atlas.export_atlas(
                    fixture.specs,
                    fixture.neutral,
                    fixture.metadata,
                    fixture.root / "output",
                    96,
                    64,
                    binding(),
                )

    def test_unsafe_state_image_path_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            fixture.states[1]["image"] = "../ah.png"
            fixture.write()

            with self.assertRaisesRegex(ValueError, "single filename"):
                atlas.load_state_inputs(fixture.specs, fixture.neutral, fixture.metadata)

    def test_non_finite_json_number_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            text = fixture.specs.read_text(encoding="utf-8")
            text = text.replace('0.9,', 'NaN,', 1)
            fixture.specs.write_text(text, encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "non-finite JSON"):
                atlas.load_state_inputs(fixture.specs, fixture.neutral, fixture.metadata)

    def test_pose_and_registration_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            shifted = copy.deepcopy(fixture.frames[1])
            for contour in (shifted["outerUpper"], shifted["outerLower"]):
                for point in contour:
                    point[0] += 0.09
            shifted["center"][0] += 0.09
            fixture.frames[1] = shifted
            fixture.write()

            with self.assertRaisesRegex(ValueError, "shared registration"):
                atlas.load_state_inputs(fixture.specs, fixture.neutral, fixture.metadata)

    def test_observed_image_cannot_claim_multiple_articulations(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            fixture.states[1]["referenceType"] = "observed"
            fixture.states[1]["articulation"] = "open"
            fixture.states[2]["image"] = "ah.png"
            fixture.states[2]["referenceType"] = "observed"
            fixture.states[2]["articulation"] = "spread"
            fixture.write()

            with self.assertRaisesRegex(ValueError, "cannot prove multiple articulations"):
                atlas.load_state_inputs(fixture.specs, fixture.neutral, fixture.metadata)

    def test_common_alpha_rejects_missing_opaque_or_transparent_support(self) -> None:
        with self.assertRaisesRegex(ValueError, "near-opaque"):
            atlas.validate_common_alpha(np.zeros((64, 96), np.uint8), 96, 64)
        with self.assertRaisesRegex(ValueError, "zero-alpha"):
            atlas.validate_common_alpha(np.full((64, 96), 255, np.uint8), 96, 64)

    def test_existing_output_and_invalid_dimensions_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            output = fixture.root / "output"
            output.mkdir()
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                atlas.export_atlas(
                    fixture.specs, fixture.neutral, fixture.metadata, output, 96, 64, binding()
                )
            with self.assertRaisesRegex(ValueError, "64..512"):
                atlas.export_atlas(
                    fixture.specs,
                    fixture.neutral,
                    fixture.metadata,
                    fixture.root / "other",
                    63,
                    64,
                    binding(),
                )

    def test_export_api_revalidates_binding_before_creating_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            output = fixture.root / "output"
            forged = {
                "schemaVersion": 3,
                "gameProfileId": "fixture-game",
                "characterId": "fixture-character",
                "referenceProvenanceSha256": ["A" * 64],
                "reviewStatus": "reviewed-private",
                "reviewEvidenceSha256": "b" * 64,
            }

            with self.assertRaisesRegex(ValueError, "schemaVersion must be 1"):
                atlas.export_atlas(
                    fixture.specs,
                    fixture.neutral,
                    fixture.metadata,
                    output,
                    96,
                    64,
                    forged,
                )
            self.assertFalse(output.exists())

    def test_oversized_decoded_reference_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory))
            oversized = np.zeros((16, atlas.MAX_SOURCE_DIMENSION + 1, 3), np.uint8)
            self.assertTrue(cv2.imwrite(str(fixture.neutral), oversized))

            with self.assertRaisesRegex(ValueError, "decode limit"):
                atlas.load_state_inputs(fixture.specs, fixture.neutral, fixture.metadata)


if __name__ == "__main__":
    unittest.main()
