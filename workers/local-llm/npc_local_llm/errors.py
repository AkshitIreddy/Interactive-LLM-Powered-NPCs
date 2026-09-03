"""Stable, content-free failures returned across the worker boundary."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(slots=True)
class LocalLlmError(Exception):
    code: str
    safe_message: str
    retryable: bool = False
    details: dict[str, Any] = field(default_factory=dict)

    def __post_init__(self) -> None:
        Exception.__init__(self, self.safe_message)

    def event_error(self) -> dict[str, Any]:
        safe_details = {
            key: value
            for key, value in self.details.items()
            if isinstance(key, str) and isinstance(value, (str, int, float, bool, type(None)))
        }
        return {
            "code": self.code,
            "message": self.safe_message,
            "retryable": self.retryable,
            "details": safe_details,
        }


def invalid(message: str, *, field: str | None = None) -> LocalLlmError:
    return LocalLlmError(
        "invalid_payload",
        message,
        details={} if field is None else {"field": field},
    )
