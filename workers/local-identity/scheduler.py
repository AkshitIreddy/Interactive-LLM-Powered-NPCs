"""Queue-depth-one scheduler with exact cancellation-generation barriers."""

from __future__ import annotations

import threading
import time
from dataclasses import dataclass
from typing import Callable

from backend import Backend, BackendError, Cancelled


class SchedulerError(RuntimeError):
    def __init__(self, code: str, message: str, *, retryable: bool = False) -> None:
        super().__init__(message)
        self.code = code
        self.retryable = retryable


@dataclass(frozen=True, slots=True)
class Job:
    request_id: str
    generation: int
    deadline_unix_ms: int
    execute: Callable[[threading.Event], dict[str, object]]
    completed: Callable[[dict[str, object]], None]
    failed: Callable[[SchedulerError], None]


class IdentityScheduler:
    def __init__(self, backend: Backend) -> None:
        self.backend = backend
        self._condition = threading.Condition()
        self._generation = 0
        self._job: Job | None = None
        self._active: Job | None = None
        self._active_cancel: threading.Event | None = None
        self._stopping = False
        self._thread = threading.Thread(target=self._run, name="npc-identity-worker", daemon=True)
        self._thread.start()

    @property
    def generation(self) -> int:
        with self._condition:
            return self._generation

    @property
    def busy(self) -> bool:
        with self._condition:
            return self._job is not None or self._active is not None

    def submit(self, job: Job) -> None:
        with self._condition:
            if self._stopping:
                raise SchedulerError("worker_stopping", "identity worker is stopping")
            if job.generation != self._generation:
                raise SchedulerError("stale_generation", "identity request belongs to a cancelled generation")
            if self._job is not None or self._active is not None:
                raise SchedulerError("worker_busy", "identity inference queue depth is one", retryable=True)
            self._job = job
            self._condition.notify_all()

    def _run(self) -> None:
        while True:
            with self._condition:
                while self._job is None and not self._stopping:
                    self._condition.wait()
                if self._job is None and self._stopping:
                    return
                job = self._job
                self._job = None
                cancel = threading.Event()
                self._active = job
                self._active_cancel = cancel
            assert job is not None
            if job.deadline_unix_ms and int(time.time() * 1000) >= job.deadline_unix_ms:
                job.failed(SchedulerError("deadline_exceeded", "identity request deadline passed", retryable=True))
            else:
                try:
                    result = job.execute(cancel)
                    with self._condition:
                        publish = job.generation == self._generation and not cancel.is_set() and not self._stopping
                    if publish:
                        if job.deadline_unix_ms and int(time.time() * 1000) >= job.deadline_unix_ms:
                            job.failed(SchedulerError("deadline_exceeded", "identity request completed after its deadline", retryable=True))
                        else:
                            job.completed(result)
                except Cancelled:
                    pass
                except BackendError as exc:
                    with self._condition:
                        publish = job.generation == self._generation and not cancel.is_set() and not self._stopping
                    if publish:
                        if job.deadline_unix_ms and int(time.time() * 1000) >= job.deadline_unix_ms:
                            job.failed(SchedulerError("deadline_exceeded", "identity request completed after its deadline", retryable=True))
                        else:
                            job.failed(SchedulerError(exc.code, str(exc), retryable=exc.code in {"lease_unavailable", "inference_failed"}))
                except Exception:
                    with self._condition:
                        publish = job.generation == self._generation and not cancel.is_set() and not self._stopping
                    if publish:
                        job.failed(SchedulerError("internal_error", "identity inference failed safely"))
            with self._condition:
                self._active = None
                self._active_cancel = None
                self._condition.notify_all()

    def cancel_to(self, generation: int) -> None:
        with self._condition:
            if generation == self._generation:
                return
            if generation != self._generation + 1:
                raise SchedulerError("generation_gap", "cancellation generation must advance by exactly one")
            self._generation = generation
            self._job = None
            if self._active_cancel is not None:
                self._active_cancel.set()
            self.backend.cancel_active()
            self._condition.notify_all()

    def wait_generation_barrier(self, generation: int, timeout_seconds: float) -> bool:
        deadline = time.monotonic() + timeout_seconds
        with self._condition:
            while self._active is not None and self._active.generation < generation:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(remaining)
            return True

    def drain(self, timeout_seconds: float) -> bool:
        deadline = time.monotonic() + timeout_seconds
        with self._condition:
            while self._job is not None or self._active is not None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(remaining)
            return True

    def stop(self, timeout_seconds: float = 2.0) -> bool:
        with self._condition:
            self._stopping = True
            self._job = None
            if self._active_cancel is not None:
                self._active_cancel.set()
            self.backend.cancel_active()
            self._condition.notify_all()
        self._thread.join(timeout_seconds)
        return not self._thread.is_alive()
