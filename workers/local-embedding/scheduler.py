"""Bounded hidden batching and generation-barrier cancellation."""

from __future__ import annotations

import threading
import time
from collections import deque
from dataclasses import dataclass, field
from typing import Callable

from backend import Backend, BackendError, Cancelled
from model_spec import EmbeddingRequest, SpecError, tensor_payload
from telemetry import Telemetry

MAX_QUEUED_REQUESTS = 64
MAX_QUEUED_ITEMS = 256
DEFAULT_BATCH_ITEMS = 32
DEFAULT_BATCH_WINDOW_MS = 4


class SchedulerError(RuntimeError):
    def __init__(self, code: str, message: str, *, retryable: bool = False) -> None:
        super().__init__(message)
        self.code = code
        self.retryable = retryable


@dataclass(slots=True)
class ScheduledRequest:
    request_id: str
    generation: int
    deadline_unix_ms: int
    embedding: EmbeddingRequest
    completed: Callable[[dict[str, object]], None]
    failed: Callable[[SchedulerError], None]
    enqueued_ns: int = field(default_factory=time.monotonic_ns)
    cancelled: threading.Event = field(default_factory=threading.Event)


class EmbeddingScheduler:
    """One inference lane, intentionally lower priority than speech and LLM work.

    The supervisor sees individual requests.  The worker privately coalesces a
    few compatible requests for throughput, then splits the result back into the
    original correlation envelopes.  Queue depth and dwell time are bounded.
    """

    def __init__(
        self,
        backend: Backend,
        telemetry: Telemetry,
        *,
        batch_items: int = DEFAULT_BATCH_ITEMS,
        batch_window_ms: int = DEFAULT_BATCH_WINDOW_MS,
    ) -> None:
        if not 1 <= batch_items <= MAX_QUEUED_ITEMS:
            raise ValueError("batch_items is out of bounds")
        if not 0 <= batch_window_ms <= 25:
            raise ValueError("batch_window_ms is out of bounds")
        self.backend = backend
        self.telemetry = telemetry
        self.batch_items = batch_items
        self.batch_window_seconds = batch_window_ms / 1000.0
        self._condition = threading.Condition(threading.RLock())
        self._queue: deque[ScheduledRequest] = deque()
        self._queued_items = 0
        self._active: list[ScheduledRequest] = []
        self._active_cancel: threading.Event | None = None
        self._generation = 0
        self._stopping = False
        self._thread = threading.Thread(target=self._run, name="npc-embedding-batcher", daemon=True)
        self._thread.start()

    @property
    def generation(self) -> int:
        with self._condition:
            return self._generation

    @property
    def queue_depth(self) -> tuple[int, int]:
        with self._condition:
            return len(self._queue), self._queued_items

    def submit(self, job: ScheduledRequest) -> None:
        with self._condition:
            if self._stopping:
                raise SchedulerError("worker_stopping", "embedding worker is stopping")
            if job.generation < self._generation:
                raise SchedulerError("stale_generation", "embedding request belongs to a cancelled generation")
            if job.generation > self._generation:
                raise SchedulerError("generation_gap", "embedding request skipped the cancellation barrier")
            item_count = len(job.embedding.items)
            if len(self._queue) >= MAX_QUEUED_REQUESTS or self._queued_items + item_count > MAX_QUEUED_ITEMS:
                raise SchedulerError("worker_busy", "background embedding queue is full", retryable=True)
            self._queue.append(job)
            self._queued_items += item_count
            self._condition.notify_all()

    def cancel_to(self, generation: int) -> None:
        with self._condition:
            if generation == self._generation:
                return
            if generation != self._generation + 1:
                raise SchedulerError("generation_gap", "cancel generation must advance by exactly one")
            self._generation = generation
            cancelled_items = 0
            retained: deque[ScheduledRequest] = deque()
            while self._queue:
                job = self._queue.popleft()
                if job.generation < generation:
                    job.cancelled.set()
                    cancelled_items += len(job.embedding.items)
                    self._queued_items -= len(job.embedding.items)
                else:
                    retained.append(job)
            self._queue = retained
            for job in self._active:
                if job.generation < generation:
                    job.cancelled.set()
                    cancelled_items += len(job.embedding.items)
            if self._active_cancel is not None:
                self._active_cancel.set()
            self.telemetry.record_cancelled(cancelled_items)
            self.backend.cancel_active()
            self._condition.notify_all()

    def drain(self, timeout_seconds: float) -> bool:
        deadline = time.monotonic() + timeout_seconds
        with self._condition:
            while self._queue or self._active:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(remaining)
            return True

    def wait_generation_barrier(self, generation: int, timeout_seconds: float) -> bool:
        """Wait until no active work from an older generation can emit output."""

        deadline = time.monotonic() + timeout_seconds
        with self._condition:
            while any(job.generation < generation for job in self._active):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(remaining)
            return True

    def stop(self, timeout_seconds: float = 2.0) -> bool:
        with self._condition:
            if self._stopping:
                stopping_now = False
            else:
                stopping_now = True
                self._stopping = True
                cancelled = 0
                while self._queue:
                    job = self._queue.popleft()
                    job.cancelled.set()
                    cancelled += len(job.embedding.items)
                self._queued_items = 0
                for job in self._active:
                    job.cancelled.set()
                    cancelled += len(job.embedding.items)
                if self._active_cancel is not None:
                    self._active_cancel.set()
                self.telemetry.record_cancelled(cancelled)
                self._condition.notify_all()
        if stopping_now:
            self.backend.cancel_active()
        self._thread.join(timeout_seconds)
        return not self._thread.is_alive()

    @staticmethod
    def _expired(job: ScheduledRequest) -> bool:
        return job.deadline_unix_ms > 0 and int(time.time() * 1000) >= job.deadline_unix_ms

    def _take_batch(self) -> list[ScheduledRequest]:
        with self._condition:
            while not self._queue and not self._stopping:
                self._condition.wait()
            if self._stopping:
                return []
            window_deadline = time.monotonic() + self.batch_window_seconds
            while time.monotonic() < window_deadline and len(self._queue) < MAX_QUEUED_REQUESTS:
                remaining = window_deadline - time.monotonic()
                if remaining > 0:
                    self._condition.wait(remaining)
            batch: list[ScheduledRequest] = []
            items = 0
            while self._queue:
                next_job = self._queue[0]
                count = len(next_job.embedding.items)
                if batch and items + count > self.batch_items:
                    break
                if count > self.batch_items and batch:
                    break
                job = self._queue.popleft()
                self._queued_items -= count
                batch.append(job)
                items += count
                if items >= self.batch_items:
                    break
            self._active = batch
            self._active_cancel = threading.Event()
            return batch

    def _finish_active(self) -> None:
        with self._condition:
            self._active = []
            self._active_cancel = None
            self._condition.notify_all()

    def _infer_bounded(self, texts: list[str], cancelled: threading.Event) -> list[tuple[float, ...]]:
        """Run one logical batch through bounded private backend calls."""

        vectors: list[tuple[float, ...]] = []
        for offset in range(0, len(texts), self.batch_items):
            if cancelled.is_set():
                raise Cancelled()
            chunk = texts[offset : offset + self.batch_items]
            selected = self.backend.infer(chunk, cancelled)
            if len(selected) != len(chunk):
                raise BackendError("model_abi_mismatch", "embedding backend returned an incorrect result count")
            vectors.extend(selected)
        return vectors

    def _run(self) -> None:
        while True:
            jobs = self._take_batch()
            if not jobs:
                return
            live: list[ScheduledRequest] = []
            for job in jobs:
                if job.cancelled.is_set() or job.generation != self.generation:
                    continue
                if self._expired(job):
                    job.failed(SchedulerError("deadline_exceeded", "embedding request expired before execution", retryable=True))
                    self.telemetry.record_failed(len(job.embedding.items))
                else:
                    live.append(job)
            if not live:
                self._finish_active()
                continue
            combined_texts = [text for job in live for text in job.embedding.prepared_texts()]
            with self._condition:
                combined_cancel = self._active_cancel or threading.Event()
            started = self.telemetry.snapshot()
            earliest_enqueue = min(job.enqueued_ns for job in live)
            try:
                vectors = self._infer_bounded(combined_texts, combined_cancel)
                offset = 0
                completed_items = 0
                for job in live:
                    count = len(job.embedding.items)
                    selected = vectors[offset : offset + count]
                    offset += count
                    if job.cancelled.is_set() or job.generation != self.generation:
                        continue
                    if self._expired(job):
                        job.failed(SchedulerError("deadline_exceeded", "embedding request expired during execution", retryable=True))
                        self.telemetry.record_failed(count)
                        continue
                    job.completed(tensor_payload(job.embedding, selected))
                    completed_items += count
                self.telemetry.record_batch(
                    started,
                    queue_wait_ms=(started.monotonic_ns - earliest_enqueue) / 1_000_000.0,
                    batch_size=min(self.batch_items, len(combined_texts)),
                    completed_items=completed_items,
                )
            except Cancelled:
                pass
            except (BackendError, SpecError) as exc:
                code = getattr(exc, "code", "inference_failed")
                for job in live:
                    if not job.cancelled.is_set() and job.generation == self.generation:
                        job.failed(SchedulerError(code, "local embedding inference failed"))
                        self.telemetry.record_failed(len(job.embedding.items))
            except Exception:
                for job in live:
                    if not job.cancelled.is_set() and job.generation == self.generation:
                        job.failed(SchedulerError("internal_error", "local embedding worker failed safely"))
                        self.telemetry.record_failed(len(job.embedding.items))
            finally:
                self._finish_active()
