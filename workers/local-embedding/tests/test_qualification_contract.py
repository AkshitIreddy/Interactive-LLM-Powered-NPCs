from __future__ import annotations

import json
import sys
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

import qualify  # noqa: E402
from backend import Cancelled  # noqa: E402


class QualificationContractTests(unittest.TestCase):
    def test_plan_requires_distinct_load_reload_and_operation_fields(self) -> None:
        plan = json.loads((ROOT / "benchmark-plan.v1.json").read_text(encoding="utf-8"))
        self.assertIn("cold_load_millis", plan["measurements"])
        self.assertIn("in_process_reload_millis", plan["measurements"])
        required = plan["resource_governor_mapping"]["required_measurement_fields"]
        self.assertEqual(
            required,
            [
                "resident_ram_bytes",
                "p99_total_ram_bytes",
                "resident_vram_bytes",
                "p99_workspace_vram_bytes",
                "p99_load_millis",
                "p99_reload_millis",
                "p99_operation_millis",
            ],
        )

    def test_p99_rounds_up_and_rejects_empty_distribution(self) -> None:
        self.assertEqual(qualify._p99_int([1.01]), 2)
        with self.assertRaisesRegex(RuntimeError, "no measurements"):
            qualify._p99_int([])

    def test_real_cancel_probe_requires_no_output_cancelled_barrier(self) -> None:
        class CancelBackend:
            def __init__(self) -> None:
                self.active = threading.Event()

            def infer(self, texts, cancelled):  # type: ignore[no-untyped-def]
                self.active.set()
                while not cancelled.wait(0.001):
                    pass
                raise Cancelled()

            def wait_until_active(self, timeout_seconds):  # type: ignore[no-untyped-def]
                return self.active.wait(timeout_seconds)

            def cancel_active(self) -> None:
                return None

        result = qualify._cancel_probe(CancelBackend(), attempts=1)  # type: ignore[arg-type]
        self.assertTrue(result["cancelled_without_output"])
        self.assertLess(result["barrier_millis"], 2000)

    def test_real_cancel_probe_rejects_late_tensor(self) -> None:
        class LateOutputBackend:
            def __init__(self) -> None:
                self.active = threading.Event()

            def infer(self, texts, cancelled):  # type: ignore[no-untyped-def]
                self.active.set()
                time.sleep(0.01)
                return [tuple()] * len(texts)

            def wait_until_active(self, timeout_seconds):  # type: ignore[no-untyped-def]
                return self.active.wait(timeout_seconds)

            def cancel_active(self) -> None:
                return None

        with self.assertRaisesRegex(RuntimeError, "emitted tensor"):
            qualify._cancel_probe(LateOutputBackend(), attempts=1)  # type: ignore[arg-type]


if __name__ == "__main__":
    unittest.main()
