"""Pinned Qwen3/llama.cpp optional-local worker implementation.

This package has no third-party Python dependencies.  It is intentionally kept
outside the application runtime until the signed catalog and resource governor
bind it to the product's trusted integration path.
"""

from .constants import MODEL_ARTIFACT_SHA256, MODEL_ARTIFACT_SIZE, MODEL_ID, RUNTIME_ABI
from .errors import LocalLlmError

__all__ = [
    "LocalLlmError",
    "MODEL_ARTIFACT_SHA256",
    "MODEL_ARTIFACT_SIZE",
    "MODEL_ID",
    "RUNTIME_ABI",
]
