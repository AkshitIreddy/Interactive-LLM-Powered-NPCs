"""Capture a top-level window's own pixels with PrintWindow.

PrintWindow(PW_RENDERFULLCONTENT) asks DWM for the window's content, so other
windows covering it (or a screen-dimming overlay) do not leak into the image.
The game overlay is a separate layered window, so to see the composited mouth
use --screen, which copies the on-screen rectangle instead.

Usage:
  python -I capture_window.py --process interactive-npcs-synthetic-target.exe \
      --out DIR --count 20 --interval-ms 50 [--screen [--crop x,y,w,h]]
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wt
import pathlib
import time

from PIL import Image

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

PW_RENDERFULLCONTENT = 2
SRCCOPY = 0x00CC0020
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
EnumWindowsProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)



class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG),
                ("biPlanes", wt.WORD), ("biBitCount", wt.WORD),
                ("biCompression", wt.DWORD), ("biSizeImage", wt.DWORD),
                ("biXPelsPerMeter", wt.LONG), ("biYPelsPerMeter", wt.LONG),
                ("biClrUsed", wt.DWORD), ("biClrImportant", wt.DWORD)]


# Per-monitor-v2 awareness keeps window and screen coordinates physical on every
# monitor, even when monitors use different scale factors.
if not user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4)):
    user32.SetProcessDPIAware()


def process_image_name(pid: int) -> str:
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return ""
    try:
        size = wt.DWORD(1024)
        buffer = ctypes.create_unicode_buffer(size.value)
        if kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            return pathlib.PureWindowsPath(buffer.value).name.lower()
        return ""
    finally:
        kernel32.CloseHandle(handle)


def find_window(process: str | None, title: str | None) -> int:
    found: list[tuple[int, int]] = []

    def callback(hwnd: int, _: int) -> bool:
        if not user32.IsWindowVisible(hwnd):
            return True
        length = user32.GetWindowTextLengthW(hwnd)
        text = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, text, length + 1)
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if process and process_image_name(pid.value) != process.lower():
            return True
        if title and title.lower() not in text.value.lower():
            return True
        rect = wt.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(rect))
        area = max(0, rect.right - rect.left) * max(0, rect.bottom - rect.top)
        found.append((area, hwnd))
        return True

    user32.EnumWindows(EnumWindowsProc(callback), 0)
    if not found:
        raise SystemExit(f"no visible window for process={process!r} title={title!r}")
    return max(found)[1]


def print_window(hwnd: int) -> Image.Image:
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    width, height = rect.right, rect.bottom
    window_dc = user32.GetDC(hwnd)
    memory_dc = gdi32.CreateCompatibleDC(window_dc)
    bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
    gdi32.SelectObject(memory_dc, bitmap)
    try:
        # PW_CLIENTONLY (1) | PW_RENDERFULLCONTENT (2)
        if not user32.PrintWindow(hwnd, memory_dc, 1 | PW_RENDERFULLCONTENT):
            raise OSError(ctypes.get_last_error(), "PrintWindow failed")

        header = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), width, -height, 1, 32, 0,
                                  0, 0, 0, 0, 0)
        pixels = ctypes.create_string_buffer(width * height * 4)
        gdi32.GetDIBits(memory_dc, bitmap, 0, height, pixels, ctypes.byref(header), 0)
        return Image.frombuffer("RGB", (width, height), pixels, "raw", "BGRX", 0, 1)
    finally:
        gdi32.DeleteObject(bitmap)
        gdi32.DeleteDC(memory_dc)
        user32.ReleaseDC(hwnd, window_dc)


def client_box(hwnd: int, crop: tuple[int, int, int, int] | None) -> tuple[int, int, int, int]:
    """Screen-space x, y, width, height of the client area, or of a client-relative crop."""
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    if crop:
        x, y, width, height = crop
        return origin.x + x, origin.y + y, width, height
    return origin.x, origin.y, rect.right, rect.bottom


def screen_capture(box: tuple[int, int, int, int]) -> bytes:
    """Copy a rectangle of the composed desktop as BGRX bytes.

    A plain SRCCOPY BitBlt of the screen DC reads DWM's composed output, so the
    layered overlay is included. CAPTUREBLT is left out on purpose: it hides the
    cursor for each grab, which at 15+ grabs per second makes it flicker.
    """
    x, y, width, height = box
    screen_dc = user32.GetDC(0)
    memory_dc = gdi32.CreateCompatibleDC(screen_dc)
    bitmap = gdi32.CreateCompatibleBitmap(screen_dc, width, height)
    gdi32.SelectObject(memory_dc, bitmap)
    try:
        if not gdi32.BitBlt(memory_dc, 0, 0, width, height, screen_dc, x, y, SRCCOPY):
            raise OSError(ctypes.get_last_error(), "BitBlt failed")
        header = BITMAPINFOHEADER(ctypes.sizeof(BITMAPINFOHEADER), width, -height, 1, 32, 0,
                                  0, 0, 0, 0, 0)
        pixels = ctypes.create_string_buffer(width * height * 4)
        gdi32.GetDIBits(memory_dc, bitmap, 0, height, pixels, ctypes.byref(header), 0)
        return pixels.raw
    finally:
        gdi32.DeleteObject(bitmap)
        gdi32.DeleteDC(memory_dc)
        user32.ReleaseDC(0, screen_dc)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--process")
    parser.add_argument("--title")
    parser.add_argument("--out", required=True)
    parser.add_argument("--prefix", default="frame")
    parser.add_argument("--count", type=int, default=1)
    parser.add_argument("--interval-ms", type=int, default=100)
    parser.add_argument("--screen", action="store_true",
                        help="copy the on-screen rectangle (includes overlays and covering windows)")
    parser.add_argument("--crop", help="client-relative x,y,width,height to keep (--screen only)")
    args = parser.parse_args()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    hwnd = find_window(args.process, args.title)
    crop = tuple(int(value) for value in args.crop.split(",")) if args.crop else None
    box = client_box(hwnd, crop) if args.screen else None
    # Grab into memory first and encode afterwards, so PNG encoding does not
    # stretch the sampling interval.
    grabbed: list[tuple[int, object]] = []
    start = time.perf_counter()
    for index in range(args.count):
        due = start + index * args.interval_ms / 1000
        time.sleep(max(0.0, due - time.perf_counter()))
        image = screen_capture(box) if box else print_window(hwnd)
        grabbed.append((int((time.perf_counter() - start) * 1000), image))
    for index, (elapsed_ms, image) in enumerate(grabbed):
        if isinstance(image, bytes):
            image = Image.frombuffer("RGB", box[2:], image, "raw", "BGRX", 0, 1)
        image.save(out / f"{args.prefix}-{index:03d}-{elapsed_ms:06d}ms.png")
    span = grabbed[-1][0] if grabbed else 0
    print(f"captured {args.count} frame(s) of hwnd {hwnd:#x} over {span} ms to {out}")

if __name__ == "__main__":
    main()
