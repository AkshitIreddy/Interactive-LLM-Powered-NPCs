"""Move a process's top-level window(s) to a screen rectangle (physical pixels).

Usage:
  python -I place_window.py --process interactive-npcs-synthetic-target.exe --rect 4980,700,976,639 --top
  python -I place_window.py --list-monitors
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wt

from capture_window import process_image_name, user32

SW_RESTORE = 9
SWP_NOZORDER = 0x0004
SWP_NOACTIVATE = 0x0010
HWND_TOP = 0
HWND_TOPMOST = -1
HWND_NOTOPMOST = -2
# HWND_TOPMOST/NOTOPMOST are negative handles; without pointer-sized argtypes
# ctypes passes them as 32-bit ints and SetWindowPos rejects the handle.
user32.SetWindowPos.argtypes = [wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                ctypes.c_int, ctypes.c_uint]
EnumWindowsProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
MonitorEnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HMONITOR, wt.HDC, ctypes.POINTER(wt.RECT), wt.LPARAM)


class MONITORINFOEXW(ctypes.Structure):
    _fields_ = [("cbSize", wt.DWORD), ("rcMonitor", wt.RECT), ("rcWork", wt.RECT),
                ("dwFlags", wt.DWORD), ("szDevice", wt.WCHAR * 32)]


def list_monitors() -> None:
    def callback(monitor, _dc, _rect, _param):
        info = MONITORINFOEXW()
        info.cbSize = ctypes.sizeof(MONITORINFOEXW)
        user32.GetMonitorInfoW(monitor, ctypes.byref(info))
        rect = info.rcMonitor
        primary = " primary" if info.dwFlags & 1 else ""
        print(f"{info.szDevice}{primary}: {rect.left},{rect.top} -> {rect.right},{rect.bottom}")
        return True

    user32.EnumDisplayMonitors(0, 0, MonitorEnumProc(callback), 0)


def windows_for(process: str) -> list[int]:
    found: list[int] = []

    def callback(hwnd, _):
        if not user32.IsWindowVisible(hwnd) or user32.GetWindowTextLengthW(hwnd) == 0:
            return True
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if process_image_name(pid.value) == process.lower():
            found.append(hwnd)
        return True

    user32.EnumWindows(EnumWindowsProc(callback), 0)
    return found


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--process")
    parser.add_argument("--rect", help="x,y,width,height")
    parser.add_argument("--list-monitors", action="store_true")
    parser.add_argument("--top", action="store_true",
                        help="raise above other normal windows without activating")
    parser.add_argument("--topmost", action="store_true")
    parser.add_argument("--no-topmost", action="store_true",
                        help="drop a previous --topmost, leaving the window just above normal ones")
    args = parser.parse_args()
    if args.list_monitors:
        list_monitors()
        return
    if not args.process or not args.rect:
        raise SystemExit("give --process and --rect, or --list-monitors")
    x, y, width, height = (int(value) for value in args.rect.split(","))
    hwnds = windows_for(args.process)
    if not hwnds:
        raise SystemExit(f"no visible titled window for {args.process}")
    for hwnd in hwnds:
        user32.ShowWindow(hwnd, SW_RESTORE)
        if args.topmost or args.top or args.no_topmost:
            insert_after = (HWND_TOPMOST if args.topmost
                            else HWND_NOTOPMOST if args.no_topmost else HWND_TOP)
            if not user32.SetWindowPos(hwnd, insert_after, x, y, width, height, SWP_NOACTIVATE):
                raise OSError(ctypes.get_last_error(), "SetWindowPos failed")
        else:
            user32.SetWindowPos(hwnd, 0, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE)
        rect = wt.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(rect))
        print(f"hwnd {hwnd:#x} now at {rect.left},{rect.top} -> {rect.right},{rect.bottom}")


if __name__ == "__main__":
    main()
