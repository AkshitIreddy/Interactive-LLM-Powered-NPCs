# Single-monitor root-cause analysis

## Finding

The v1 single-monitor limitation is architectural, not a missing setting. Version 1 uses a second opaque OpenCV window as both the interaction UI and the surface on which the game copy and generated face video are displayed. On one monitor, that mirror must cover, overlap, or compete with the real game. It can capture itself, owns keyboard focus for push-to-talk, and substitutes old pixels while animation renders. No combination of window title or coordinates makes this a reliable in-game overlay.

## Two capture implementations

The normal notebook calls `functions/grabscreen.py`:

- it acquires the desktop DC;
- reads a fixed `(0,0)–(1920,1080)` region with GDI `BitBlt`/`SRCCOPY`;
- converts the copied BGRA buffer through OpenCV;
- displays it in a normal 1920×1080 OpenCV window titled `Screen Capture`.

The `miscellaneous/Single Monitor` variant changes only the source capture:

- it locates an HWND by exact window title, or uses the desktop;
- uses `GetWindowRect`, `GetWindowDC`, and `PrintWindow(..., 2)` into a compatible bitmap;
- still displays the result in the same opaque OpenCV mirror;
- still hard-codes the requested region and window size to 1920×1080;
- still requires the OpenCV window to receive `cv2.waitKey` keyboard input.

`PrintWindow` asks the target application to render into a supplied DC. It is not a guaranteed low-latency game capture API: accelerated, protected, minimized, occluded, or exclusive-fullscreen content can return black, stale, incomplete, or blocked frames. It also does not solve presentation.

## Failure mechanisms

| Mechanism | Single-monitor effect |
| --- | --- |
| Opaque mirror | To see the artificial face, the user watches the copy instead of the live game. The copy adds capture/presentation latency and can obscure the game. |
| Self-capture | If the desktop path sees the OpenCV window in front, subsequent frames include that window, creating recursive/stale feedback. |
| Focus-bound hotkey | `cv2.waitKey` observes keyboard events only while the OpenCV surface processes focus/input. Giving focus back to the game breaks PTT; focusing the mirror interferes with play. |
| Frozen compositing | During a response the notebook writes one screenshot, reads it back, and repeatedly replaces a rectangle in that same image. The rest of the game freezes until the generated video ends. |
| Fixed geometry | `(0,0)`, 1920×1080, `GetWindowRect`, and image-space face coordinates ignore window client borders, scaling, negative virtual origins, DPI, resize, ultrawide, and capture-to-display transforms. |
| Rectangle replacement | No alpha, segmentation, optical flow, depth/occlusion, color match, or fresh-frame re-anchor exists, so motion exposes a pasted tile. |
| Unsupported modes | GDI and `PrintWindow` are unreliable for hardware overlays and exclusive fullscreen; the code has no documented fallback. |
| No device lifecycle | Display changes, alt-tab, monitor attach/detach, HDR, and graphics-device loss have no state or recovery path. |

## Why two monitors appeared to help

With a second monitor, the user could put the OpenCV mirror on one display and the game on the other, reducing direct occlusion and accidental self-capture. That workaround did not reduce latency, fix focus-bound PTT, correct fixed coordinates, or make the generated animation part of the game. It merely provided physical space for the mirror.

## 2.0 correction

Version 2 separates capture, control, and presentation:

1. A C++/WinRT media broker captures the selected game HWND with Windows Graphics Capture; DXGI Desktop Duplication is a documented fallback for compatible modes.
2. A system-wide hotkey and event-driven WASAPI run independently of any webview or overlay focus.
3. Control UI remains in Tauri; video and PCM never traverse the WebView.
4. A transparent click-through DirectComposition overlay presents only subtitles, status, and permitted residual face pixels over fresh game frames.
5. All transforms use the captured client area, monitor identity, per-monitor DPI, capture size, presentation size, HDR/SDR mode, and a monotonically timestamped frame.
6. If capture, tracking, or composition confidence becomes stale, the visual patch disappears within one displayed frame while audio/subtitles continue.
7. True exclusive fullscreen is not falsely promised: ask for borderless/windowed mode or degrade to audio/subtitles; there is no injected/native adapter fallback.

## Required acceptance evidence

The bug is not considered fixed by DOM tests or a successful capture call. Rendered evidence and automated transform checks must cover:

- one and multiple monitors, including a secondary display left of the primary (negative origin);
- 100%, 125%, 150%, and 200% DPI;
- 720p through 4K and 16:9, 21:9, and 32:9;
- windowed and borderless mode, with resize and movement between monitors;
- alt-tab, minimize/restore, monitor attach/detach, and D3D device loss;
- SDR and HDR behavior;
- foreground game input while PTT and overlay remain functional;
- explicit exclusive-fullscreen fallback;
- no self-capture recursion, frozen full frame, or stale face patch.
