"""Windows hidden-process and kill-on-close Job Object support."""

from __future__ import annotations

import ctypes
import os
from ctypes import wintypes

from .errors import LocalLlmError

CREATE_NEW_PROCESS_GROUP = 0x00000200
CREATE_NO_WINDOW = 0x08000000
JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 0x00000008
JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x00000100
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS = 9


if os.name == "nt":
    ULONG_PTR = ctypes.c_size_t

    class IO_COUNTERS(ctypes.Structure):
        _fields_ = [
            ("ReadOperationCount", ctypes.c_uint64),
            ("WriteOperationCount", ctypes.c_uint64),
            ("OtherOperationCount", ctypes.c_uint64),
            ("ReadTransferCount", ctypes.c_uint64),
            ("WriteTransferCount", ctypes.c_uint64),
            ("OtherTransferCount", ctypes.c_uint64),
        ]

    class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ULONG_PTR),
            ("MaximumWorkingSetSize", ULONG_PTR),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ULONG_PTR),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
            ("IoInfo", IO_COUNTERS),
            ("ProcessMemoryLimit", ULONG_PTR),
            ("JobMemoryLimit", ULONG_PTR),
            ("PeakProcessMemoryUsed", ULONG_PTR),
            ("PeakJobMemoryUsed", ULONG_PTR),
        ]


class WindowsJob:
    def __init__(self, *, process_memory_limit_bytes: int | None = None) -> None:
        self.handle = None
        if os.name != "nt":
            return
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.CreateJobObjectW.restype = wintypes.HANDLE
        handle = kernel32.CreateJobObjectW(None, None)
        if not handle:
            raise LocalLlmError("job_creation_failed", "Windows process supervisor could not create a Job Object")
        limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        limits.BasicLimitInformation.LimitFlags = (
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        )
        limits.BasicLimitInformation.ActiveProcessLimit = 1
        if process_memory_limit_bytes is not None:
            if process_memory_limit_bytes < 512 * 1_048_576:
                kernel32.CloseHandle(handle)
                raise LocalLlmError("invalid_resource_limit", "runtime memory limit is below the safe minimum")
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY
            limits.ProcessMemoryLimit = process_memory_limit_bytes
        ok = kernel32.SetInformationJobObject(
            handle,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            ctypes.byref(limits),
            ctypes.sizeof(limits),
        )
        if not ok:
            kernel32.CloseHandle(handle)
            raise LocalLlmError("job_creation_failed", "Windows process supervisor could not configure its Job Object")
        self.handle = handle

    def assign_pid_handle(self, process_handle: int) -> None:
        if os.name != "nt" or self.handle is None:
            return
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        if not kernel32.AssignProcessToJobObject(self.handle, wintypes.HANDLE(process_handle)):
            self.close()
            raise LocalLlmError("job_assignment_failed", "llama-server could not be assigned to its Job Object")

    def terminate(self, exit_code: int = 1) -> None:
        if os.name != "nt" or self.handle is None:
            return
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.TerminateJobObject(self.handle, exit_code)

    def close(self) -> None:
        if os.name == "nt" and self.handle is not None:
            ctypes.WinDLL("kernel32", use_last_error=True).CloseHandle(self.handle)
            self.handle = None

    def __enter__(self) -> "WindowsJob":
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:  # type: ignore[no-untyped-def]
        self.close()
