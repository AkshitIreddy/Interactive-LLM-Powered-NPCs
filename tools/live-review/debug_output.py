"""Print OutputDebugString messages from processes in this session (like DebugView).

Usage:
  python -I debug_output.py --seconds 20 [--match "Synthetic replay"]

Uses the DBWIN_BUFFER shared-memory protocol, so it only sees processes that
have no debugger attached. .NET Debug.WriteLine/Trace output arrives this way.
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wt
import sys
import time

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.CreateEventW.restype = wt.HANDLE
kernel32.CreateFileMappingW.restype = wt.HANDLE
kernel32.CreateFileMappingW.argtypes = [wt.HANDLE, ctypes.c_void_p, wt.DWORD, wt.DWORD, wt.DWORD,
                                        wt.LPCWSTR]
kernel32.MapViewOfFile.restype = ctypes.c_void_p
kernel32.MapViewOfFile.argtypes = [wt.HANDLE, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_size_t]

PAGE_READWRITE = 0x04
FILE_MAP_READ = 0x0004
INVALID_HANDLE_VALUE = wt.HANDLE(-1)
WAIT_OBJECT_0 = 0


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seconds", type=float, default=20.0)
    parser.add_argument("--match", default="")
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")

    buffer_ready = kernel32.CreateEventW(None, False, True, "DBWIN_BUFFER_READY")
    data_ready = kernel32.CreateEventW(None, False, False, "DBWIN_DATA_READY")
    mapping = kernel32.CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, 4096,
                                          "DBWIN_BUFFER")
    if not buffer_ready or not data_ready or not mapping:
        raise SystemExit(f"could not open DBWIN objects: {ctypes.get_last_error()}")
    view = kernel32.MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 4096)
    deadline = time.monotonic() + args.seconds
    print("listening", flush=True)
    while time.monotonic() < deadline:
        kernel32.SetEvent(buffer_ready)
        if kernel32.WaitForSingleObject(data_ready, 250) != WAIT_OBJECT_0:
            continue
        pid = ctypes.c_uint32.from_address(view).value
        text = ctypes.string_at(view + 4).decode("mbcs", errors="replace").rstrip()
        if args.match in text:
            print(f"[{pid}] {text}", flush=True)


if __name__ == "__main__":
    main()
