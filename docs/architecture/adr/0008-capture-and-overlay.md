# ADR-0008: Windows-native capture and transparent composition

Status: Accepted  
Date: 2026-08-28

## Context

GDI `BitBlt`/`PrintWindow` plus an opaque OpenCV mirror causes self-capture, focus loss, fixed-coordinate errors and frozen pasted video. Windows gaming capture/presentation must stay outside the WebView and preserve fresh game presentation.

## Decision

The C++/WinRT media broker captures a selected game HWND with Windows Graphics Capture into D3D11 textures. DXGI Desktop Duplication is a qualified fallback for compatible desktop/display cases. A transparent, click-through, non-activating DirectComposition overlay presents status, subtitles and any permitted masked residual.

Each frame carries HWND/client geometry, capture size, monitor identity, per-monitor DPI, SDR/HDR/color-space state, QPC timestamp and generation. Overlay transforms are recomputed on resize/move/display change. Presentation never waits for optional AI; stale patches are dropped.

Game integration is external and non-injecting: WGC/DXGI capture, manual/read-only screen-based target evidence, overlay, then audio/subtitles. True exclusive fullscreen uses documented fallback to borderless/windowed mode or audio/subtitles; per-game native integration is not a fallback.

## Consequences

- Single-monitor works without a mirror and the game retains focus.
- D3D device loss, alt-tab, minimized/occluded windows and monitor changes need explicit lifecycle tests.
- Capture APIs can show a system picker/border or have protected-content limits; UX and profiles must disclose actual capability.
- Overlay must avoid anti-cheat/protected-online contexts and never attempt bypass.

## Rejected alternatives

- **OpenCV/GDI mirror:** root cause of the v1 bug.
- **WebView/canvas video path:** avoidable copies/jitter and wrong trust boundary.
- **Injection, hooks, mods, or per-game adapters:** unsafe/brittle for a generic product, conflict with protected contexts, and create an impossible compatibility burden.
- **Frozen-frame full-head video:** visually detached and stops live game motion.

## Evidence

- Windows Graphics Capture: <https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture>
- Desktop Duplication API: <https://learn.microsoft.com/en-us/windows-hardware/drivers/display/desktop-duplication-api>
- DirectComposition architecture: <https://learn.microsoft.com/en-us/windows/win32/directcomp/architecture-and-components>
- Per-monitor DPI: <https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows>
