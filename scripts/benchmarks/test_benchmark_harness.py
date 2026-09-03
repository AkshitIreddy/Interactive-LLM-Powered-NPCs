#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import benchmark_harness as harness


FIXED_TIME = "2026-08-30T00:00:00Z"


def complete_rows(*, execution_mode: str = "mocked", measurement_kind: str = "measured") -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for metric in harness.METRICS:
        for index, value in enumerate((1.0, 2.0, 3.0, 4.0), start=1):
            rows.append(
                {
                    "schema_version": harness.SAMPLE_SCHEMA_VERSION,
                    "sample_id": f"sample-{index}:{metric.name}",
                    "turn_id": f"turn-{index}",
                    "metric": metric.name,
                    "value": value,
                    "unit": metric.unit,
                    "observed_at_utc": FIXED_TIME,
                    "execution_mode": execution_mode,
                    "measurement_kind": measurement_kind,
                }
            )
    return rows


class PercentileTests(unittest.TestCase):
    def test_r7_percentiles_are_exact_and_deterministic(self) -> None:
        values = [1.0, 2.0, 3.0, 4.0]
        self.assertEqual(harness.percentile(values, 0.50), 2.5)
        self.assertAlmostEqual(harness.percentile(values, 0.95), 3.85)
        self.assertAlmostEqual(harness.percentile(values, 0.99), 3.97)


class EvidenceTests(unittest.TestCase):
    def test_every_metric_emits_p50_p95_p99(self) -> None:
        rows = complete_rows()
        report = harness.summarize_rows(
            rows,
            report_id="unit-test",
            source_sha256="0" * 64,
            source_bytes=100,
            timeout_seconds=10.0,
            max_records=1000,
            elapsed_ms=1.0,
            generated_at_utc=FIXED_TIME,
        )
        harness.validate_report_shape(report)
        self.assertEqual(len(report["metrics"]), len(harness.METRICS))
        for metric in report["metrics"]:
            self.assertEqual(metric["p50"], 2.5)
            self.assertEqual(metric["p95"], 3.85)
            self.assertEqual(metric["p99"], 3.97)
        self.assertFalse(report["classification"]["acceptance_eligible"])
        self.assertEqual(report["classification"]["execution_mode"], "mocked")
        self.assertEqual(report["classification"]["measurement_kind"], "measured")

    def test_simulation_values_repeat_for_same_seed(self) -> None:
        first = harness.deterministic_simulation(3, 17, FIXED_TIME)
        second = harness.deterministic_simulation(3, 17, FIXED_TIME)
        third = harness.deterministic_simulation(3, 18, FIXED_TIME)
        self.assertEqual(first, second)
        self.assertNotEqual(first, third)

    def test_missing_metric_is_rejected(self) -> None:
        rows = [row for row in complete_rows() if row["metric"] != harness.METRICS[-1].name]
        with self.assertRaisesRegex(harness.BenchmarkError, "Missing required metric"):
            harness.summarize_rows(
                rows,
                report_id="missing-metric",
                source_sha256="0" * 64,
                source_bytes=1,
                timeout_seconds=10,
                max_records=1000,
                elapsed_ms=1,
            )

    def test_mixed_evidence_labels_are_rejected(self) -> None:
        rows = complete_rows()
        rows[0]["execution_mode"] = "live"
        with self.assertRaisesRegex(harness.BenchmarkError, "same execution_mode"):
            harness.summarize_rows(
                rows,
                report_id="mixed-labels",
                source_sha256="0" * 64,
                source_bytes=1,
                timeout_seconds=10,
                max_records=1000,
                elapsed_ms=1,
            )

    def test_secret_like_or_unknown_fields_are_rejected_without_value_echo(self) -> None:
        row = complete_rows()[0]
        row["api_key"] = "SHOULD-NOT-APPEAR"
        with self.assertRaises(harness.BenchmarkError) as caught:
            harness.validate_sample(row, 1)
        self.assertNotIn("SHOULD-NOT-APPEAR", str(caught.exception))
        self.assertIn("<secret-like-field>", str(caught.exception))

    def test_jsonl_limits_are_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "samples.jsonl"
            path.write_text("".join(json.dumps(row) + "\n" for row in complete_rows()), encoding="utf-8")
            with self.assertRaisesRegex(harness.BenchmarkError, "record limit"):
                harness.read_jsonl(path, max_records=1, max_input_bytes=1_000_000, timeout_seconds=10)
            with self.assertRaisesRegex(harness.BenchmarkError, "byte limit"):
                harness.read_jsonl(path, max_records=1000, max_input_bytes=1, timeout_seconds=10)

    def test_planning_estimate_is_never_acceptance_eligible(self) -> None:
        report = harness.summarize_rows(
            complete_rows(execution_mode="live", measurement_kind="planning_estimate"),
            report_id="estimate",
            source_sha256="0" * 64,
            source_bytes=1,
            timeout_seconds=10,
            max_records=1000,
            elapsed_ms=1,
        )
        self.assertFalse(report["classification"]["acceptance_eligible"])
        self.assertIn("never benchmark evidence", report["classification"]["reason"])

    def test_report_validator_rejects_tampered_classification_and_units(self) -> None:
        report = harness.summarize_rows(
            complete_rows(),
            report_id="tamper-test",
            source_sha256="0" * 64,
            source_bytes=100,
            timeout_seconds=10,
            max_records=1000,
            elapsed_ms=1,
            generated_at_utc=FIXED_TIME,
        )
        report["classification"]["acceptance_eligible"] = True
        with self.assertRaisesRegex(harness.BenchmarkError, "eligibility conflicts"):
            harness.validate_report_shape(report)
        report["classification"]["acceptance_eligible"] = False
        report["metrics"][0]["unit"] = "count"
        with self.assertRaisesRegex(harness.BenchmarkError, "invalid unit"):
            harness.validate_report_shape(report)

    def test_count_and_token_samples_must_be_whole_numbers(self) -> None:
        row = complete_rows()[0]
        row["metric"] = "compositor.stale_drops"
        row["unit"] = "count"
        row["value"] = 1.5
        with self.assertRaisesRegex(harness.BenchmarkError, "whole numbers"):
            harness.validate_sample(row, 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
