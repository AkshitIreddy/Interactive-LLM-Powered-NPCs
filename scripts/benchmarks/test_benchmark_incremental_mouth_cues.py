#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import threading
import time
import unittest


MODULE_PATH = Path(__file__).with_name("benchmark-incremental-mouth-cues.py")
SPEC = importlib.util.spec_from_file_location("benchmark_incremental_mouth_cues", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
benchmark = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = benchmark
SPEC.loader.exec_module(benchmark)


class ImmutableCueLedgerTests(unittest.TestCase):
    def test_commits_exact_sample_intervals_without_revising_the_past(self) -> None:
        ledger = benchmark.ImmutableCueLedger(generation=7)
        first = [
            benchmark.Cue(0, 30, "A"),
            benchmark.Cue(30, 70, "B"),
            benchmark.Cue(70, 100, "C"),
        ]
        self.assertTrue(ledger.accept(7, first, 60))
        immutable_prefix = tuple(ledger.cues)

        # The later window disagrees about uncommitted frames 60..70.  Only the
        # future is allowed to use that updated recognition.
        later = [
            benchmark.Cue(40, 65, "D"),
            benchmark.Cue(65, 110, "C"),
            benchmark.Cue(110, 140, "X"),
        ]
        self.assertTrue(ledger.accept(7, later, 120))
        self.assertEqual(tuple(ledger.cues[: len(immutable_prefix)]), immutable_prefix)
        self.assertEqual(ledger.committed_until_frame, 120)
        self.assertEqual(ledger.cues[-1], benchmark.Cue(110, 120, "X"))

    def test_stale_and_cancelled_generations_cannot_publish(self) -> None:
        cues = [benchmark.Cue(0, 100, "A")]
        ledger = benchmark.ImmutableCueLedger(generation=4)
        self.assertFalse(ledger.accept(3, cues, 50))
        self.assertEqual(ledger.committed_until_frame, 0)
        ledger.cancel(4)
        self.assertFalse(ledger.accept(4, cues, 50))
        self.assertEqual(ledger.cues, [])

    def test_gap_at_commit_cursor_is_rejected(self) -> None:
        ledger = benchmark.ImmutableCueLedger(generation=1)
        with self.assertRaisesRegex(benchmark.IncrementalCueError, "cover commit cursor"):
            ledger.accept(1, [benchmark.Cue(5, 100, "A")], 50)


class SchedulingTests(unittest.TestCase):
    def test_no_underrun_delay_accounts_for_next_result(self) -> None:
        runs = [
            benchmark.RecognitionRun(50, 0, 900, 500, 1400, 25, 2),
            benchmark.RecognitionRun(150, 0, 1000, 1500, 2500, 125, 8),
            benchmark.RecognitionRun(200, 0, 900, 2500, 3400, 200, 5),
        ]
        # At 100 frames/s, the first horizon is 250 ms.  Starting playback at
        # 2250 ms is therefore required to reach, but not overrun, result two.
        self.assertEqual(benchmark.safe_playback_start_ms(runs, 100), 2250)

    def test_disagreement_is_counted_on_exact_frame_intersections(self) -> None:
        left = [benchmark.Cue(0, 40, "A"), benchmark.Cue(40, 100, "B")]
        right = [benchmark.Cue(0, 20, "A"), benchmark.Cue(20, 60, "B"), benchmark.Cue(60, 100, "A")]
        self.assertEqual(benchmark.disagreement_frames(left, right), 60)


class ProcessCancellationTests(unittest.TestCase):
    def test_pending_child_is_killed_promptly_on_cancel(self) -> None:
        cancelled = threading.Event()
        result: list[BaseException] = []

        def invoke() -> None:
            try:
                benchmark.run_process_cancellable(
                    [sys.executable, "-c", "import time; time.sleep(30)"],
                    timeout_seconds=5,
                    cancelled=cancelled,
                    poll_seconds=0.005,
                )
            except BaseException as error:  # captured for the test thread
                result.append(error)

        started = time.perf_counter()
        worker = threading.Thread(target=invoke)
        worker.start()
        time.sleep(0.05)
        cancelled.set()
        worker.join(timeout=1)
        self.assertFalse(worker.is_alive())
        self.assertLess(time.perf_counter() - started, 1)
        self.assertEqual(len(result), 1)
        self.assertRegex(str(result[0]), "recognition cancelled")


if __name__ == "__main__":
    unittest.main(verbosity=2)
