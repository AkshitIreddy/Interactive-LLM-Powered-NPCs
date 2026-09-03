"""Production-shaped optional local speech-to-text worker."""

from .contract import PROTOCOL_VERSION, ProtocolFault
from .runtime import WorkerRuntime

__all__ = ["PROTOCOL_VERSION", "ProtocolFault", "WorkerRuntime"]
