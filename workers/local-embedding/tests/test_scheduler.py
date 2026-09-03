from __future__ import annotations

import hashlib
import sys
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from backend import DeterministicFixtureBackend  # noqa: E402
from model_spec import REQUEST_CONTRACT_VERSION, EmbeddingRequest  # noqa: E402
from scheduler import (  # noqa: E402
    MAX_QUEUED_ITEMS,
    EmbeddingScheduler,
    ScheduledRequest,
    SchedulerError,
)
from telemetry import Telemetry, percentile  # noqa: E402


def request(text: str, identifier: str = "input-01") -> EmbeddingRequest:
    return EmbeddingRequest.parse(
        {
            "contract_version": REQUEST_CONTRACT_VERSION,
            "mode": "passage",
            "purpose": "memory_retrieval",
            "priority": "background",
            "items": [
                {
                    "input_id": identifier,
                    "text": text,
                    "source_content_sha256": hashlib.sha256(text.encode()).hexdigest(),
                }
            ],
        }
    )


class SchedulerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.backend = DeterministicFixtureBackend()
        self.telemetry = Telemetry()
        self.scheduler = EmbeddingScheduler(self.backend, self.telemetry, batch_items=8, batch_window_ms=10)

    def tearDown(self) -> None:
        self.scheduler.stop()

    def test_request_completes_with_normalized_tensor(self) -> None:
        done = threading.Event()
        results = []
        errors = []
        self.scheduler.submit(
            ScheduledRequest("req-01", 0, 0, request("blacksmith"), lambda value: (results.append(value), done.set()), lambda error: (errors.append(error), done.set()))
        )
        self.assertTrue(done.wait(2))
        self.assertFalse(errors)
        self.assertEqual(results[0]["model"]["dimensions"], 384)

    def test_hidden_batching_coalesces_requests(self) -> None:
        done = threading.Event()
        completed = []

        def callback(value):  # type: ignore[no-untyped-def]
            completed.append(value)
            if len(completed) == 3:
                done.set()

        for index in range(3):
            self.scheduler.submit(
                ScheduledRequest(f"req-{index}", 0, 0, request(str(index), f"input-{index}"), callback, self.fail)
            )
        self.assertTrue(done.wait(2))
        self.assertTrue(self.scheduler.drain(2))
        report = self.telemetry.report(backend="fixture", loaded=True)
        self.assertEqual(report["batch"]["sample_count"], 1)
        self.assertEqual(report["batch"]["maximum_items"], 3)

    def test_single_large_request_is_split_into_private_bounded_calls(self) -> None:
        self.scheduler.stop()

        class RecordingBackend(DeterministicFixtureBackend):
            def __init__(self) -> None:
                super().__init__()
                self.batch_sizes: list[int] = []

            def infer(self, texts, cancelled):  # type: ignore[no-untyped-def]
                self.batch_sizes.append(len(texts))
                return super().infer(texts, cancelled)

        backend = RecordingBackend()
        scheduler = EmbeddingScheduler(backend, self.telemetry, batch_items=8, batch_window_ms=0)
        done = threading.Event()
        items = []
        for index in range(19):
            text = f"entry-{index}"
            items.append(
                {
                    "input_id": f"input-{index}",
                    "text": text,
                    "source_content_sha256": hashlib.sha256(text.encode()).hexdigest(),
                }
            )
        embedding = EmbeddingRequest.parse(
            {
                "contract_version": REQUEST_CONTRACT_VERSION,
                "mode": "passage",
                "purpose": "character_knowledge",
                "priority": "background",
                "items": items,
            }
        )
        try:
            scheduler.submit(ScheduledRequest("req-large", 0, 0, embedding, lambda _: done.set(), self.fail))
            self.assertTrue(done.wait(2))
            self.assertEqual(backend.batch_sizes, [8, 8, 3])
        finally:
            scheduler.stop()

    def test_cancel_advances_generation_and_suppresses_old_terminal(self) -> None:
        self.scheduler.stop()
        backend = DeterministicFixtureBackend(delay_seconds=0.25)
        scheduler = EmbeddingScheduler(backend, self.telemetry, batch_items=8, batch_window_ms=0)
        completions = []
        failures = []
        scheduler.submit(ScheduledRequest("req-old", 0, 0, request("old"), completions.append, failures.append))
        time.sleep(0.03)
        scheduler.cancel_to(1)
        self.assertTrue(scheduler.wait_generation_barrier(1, 1))
        self.assertTrue(scheduler.drain(1))
        self.assertEqual(completions, [])
        self.assertEqual(failures, [])
        scheduler.stop()

    def test_stale_submit_rejected_after_cancel(self) -> None:
        self.scheduler.cancel_to(1)
        with self.assertRaisesRegex(SchedulerError, "cancelled"):
            self.scheduler.submit(ScheduledRequest("req-old", 0, 0, request("old"), self.fail, self.fail))

    def test_generation_gap_rejected(self) -> None:
        with self.assertRaisesRegex(SchedulerError, "exactly one"):
            self.scheduler.cancel_to(2)

    def test_cancel_retry_is_idempotent(self) -> None:
        self.scheduler.cancel_to(1)
        self.scheduler.cancel_to(1)
        self.assertEqual(self.scheduler.generation, 1)

    def test_expired_request_fails_without_inference(self) -> None:
        done = threading.Event()
        failures = []
        self.scheduler.submit(
            ScheduledRequest(
                "req-expired",
                0,
                int(time.time() * 1000) - 1,
                request("expired"),
                self.fail,
                lambda error: (failures.append(error), done.set()),
            )
        )
        self.assertTrue(done.wait(2))
        self.assertEqual(failures[0].code, "deadline_exceeded")

    def test_queue_limits_are_bounded(self) -> None:
        self.scheduler.stop()
        backend = DeterministicFixtureBackend(delay_seconds=0.5)
        scheduler = EmbeddingScheduler(backend, self.telemetry, batch_items=1, batch_window_ms=0)
        accepted = 0
        try:
            for index in range(MAX_QUEUED_ITEMS + 8):
                try:
                    scheduler.submit(
                        ScheduledRequest(f"req-{index}", 0, 0, request(str(index), f"input-{index}"), lambda _: None, lambda _: None)
                    )
                    accepted += 1
                except SchedulerError as exc:
                    self.assertEqual(exc.code, "worker_busy")
                    break
            self.assertLessEqual(accepted, 65)
        finally:
            scheduler.stop()

    def test_stop_reports_uncooperative_backend_timeout(self) -> None:
        self.scheduler.stop()

        class BlockingBackend(DeterministicFixtureBackend):
            def __init__(self) -> None:
                super().__init__()
                self.entered = threading.Event()
                self.release = threading.Event()

            def infer(self, texts, cancelled):  # type: ignore[no-untyped-def]
                self.entered.set()
                self.release.wait(2)
                return super().infer(texts, cancelled)

        backend = BlockingBackend()
        scheduler = EmbeddingScheduler(backend, self.telemetry, batch_items=1, batch_window_ms=0)
        scheduler.submit(ScheduledRequest("req-blocked", 0, 0, request("blocked"), lambda _: None, lambda _: None))
        self.assertTrue(backend.entered.wait(1))
        self.assertFalse(scheduler.stop(timeout_seconds=0.01))
        backend.release.set()
        self.assertTrue(scheduler.stop(timeout_seconds=1))


class TelemetryTests(unittest.TestCase):
    def test_percentile_interpolates(self) -> None:
        self.assertEqual(percentile([1, 2, 3, 4, 5], 0.5), 3)
        self.assertAlmostEqual(percentile([1, 2, 3, 4], 0.95), 3.85)

    def test_empty_percentile_is_unknown_not_zero(self) -> None:
        self.assertIsNone(percentile([], 0.99))

    def test_cpu_backend_reports_zero_vram_as_invariant(self) -> None:
        report = Telemetry().report(backend="cpu", loaded=True)
        self.assertEqual(report["vram"]["resident_bytes"], 0)
        self.assertEqual(report["vram"]["measurement"], "backend_invariant")

    def test_unknown_gpu_backend_never_guesses_vram(self) -> None:
        report = Telemetry().report(backend="directml", loaded=True)
        self.assertIsNone(report["vram"]["resident_bytes"])
        self.assertEqual(report["vram"]["measurement"], "supervisor_telemetry_required")


if __name__ == "__main__":
    unittest.main()
