#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("qualify-musetalk-realtime-path.py")
SPEC = importlib.util.spec_from_file_location("musetalk_realtime_qualification", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
probe = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = probe
SPEC.loader.exec_module(probe)


class ParsingTests(unittest.TestCase):
    def test_batch_sizes_are_bounded_and_unique(self) -> None:
        self.assertEqual(probe.parse_batch_sizes("1, 4,20"), [1, 4, 20])
        for invalid in ("", "0", "33", "4,4", "x"):
            with self.subTest(invalid=invalid), self.assertRaises(argparse.ArgumentTypeError):
                probe.parse_batch_sizes(invalid)

    def test_face_box_is_ordered(self) -> None:
        self.assertEqual(probe.parse_face_box("10,20,110,220"), (10, 20, 110, 220))
        for invalid in ("1,2,3", "10,20,5,30", "-1,2,3,4", "a,b,c,d"):
            with self.subTest(invalid=invalid), self.assertRaises(argparse.ArgumentTypeError):
                probe.parse_face_box(invalid)

    def test_percentile_uses_r7_interpolation(self) -> None:
        self.assertEqual(probe.percentile([1, 2, 3, 4], 0.5), 2.5)
        self.assertAlmostEqual(probe.percentile([1, 2, 3, 4], 0.95), 3.85)

    def test_existing_output_root_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "E"):
                probe.require_e_temp(Path(directory))


if __name__ == "__main__":
    unittest.main(verbosity=2)
