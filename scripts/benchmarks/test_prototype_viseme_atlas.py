#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np


MODULE_PATH = Path(__file__).with_name("prototype-viseme-atlas.py")
SPEC = importlib.util.spec_from_file_location("prototype_viseme_atlas", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
prototype = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = prototype
SPEC.loader.exec_module(prototype)


class AtlasProofTests(unittest.TestCase):
    def test_box_parser_is_strict(self) -> None:
        self.assertEqual(prototype.parse_box("10,20,110,220"), (10, 20, 110, 220))
        for invalid in ("1,2,3", "10,20,5,30", "-1,2,3,4", "a,b,c,d"):
            with self.subTest(invalid=invalid), self.assertRaises(argparse.ArgumentTypeError):
                prototype.parse_box(invalid)

    def test_output_root_must_be_new_and_on_e_temp(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "E"):
                prototype.require_e_temp(Path(directory))

    def test_face_ratios_produce_bounded_mouth_box(self) -> None:
        self.assertEqual(
            prototype.derive_mouth_box((100, 100, 300, 500), 800, 600),
            (136, 332, 268, 472),
        )

    def test_feather_mask_has_opaque_core_and_transparent_edges(self) -> None:
        mask = prototype.feather_mask(100, 60)
        self.assertEqual(mask.shape, (60, 100))
        self.assertEqual(int(mask[0, 0]), 0)
        self.assertGreaterEqual(int(mask[30, 50]), 250)

    def test_k_medoids_is_deterministic_and_preserves_neutral_state_zero(self) -> None:
        features = np.asarray([[0.0], [0.1], [5.0], [5.1], [10.0], [10.1]], np.float32)
        first_medoids, first_labels = prototype.k_medoids(features, 3, np.asarray([0.0]))
        second_medoids, second_labels = prototype.k_medoids(features, 3, np.asarray([0.0]))
        self.assertEqual(first_medoids, second_medoids)
        self.assertEqual(first_medoids[0], 0)
        np.testing.assert_array_equal(first_labels, second_labels)
        self.assertEqual(len(set(first_labels.tolist())), 3)

    def test_single_frame_spikes_are_smoothed(self) -> None:
        self.assertEqual(prototype.smooth_labels([1, 1, 4, 1, 2]), [1, 1, 1, 1, 2])

    def test_centroid_classifier_reconstructs_separated_training_labels(self) -> None:
        features = np.asarray([[0.0, 0.0], [0.2, 0.1], [5.0, 5.0], [5.2, 5.1]], np.float32)
        labels = [0, 0, 1, 1]
        centroids = prototype.state_centroids(features, labels, 2)
        predicted = prototype.nearest_centroid(features, centroids)
        np.testing.assert_array_equal(predicted, np.asarray(labels, np.int32))


if __name__ == "__main__":
    unittest.main(verbosity=2)
