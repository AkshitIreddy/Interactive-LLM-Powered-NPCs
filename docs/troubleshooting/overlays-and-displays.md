# Overlays and displays

## Black or frozen capture

Confirm the game is visible, not minimized/protected, and running windowed/borderless. Refresh the selected HWND after launcher-to-game transition or alt-tab. True exclusive fullscreen may be unsupported externally; use borderless/windowed mode or audio/subtitles. There is no injected/native adapter fallback.

Never use the legacy opaque OpenCV mirror or rely on `PrintWindow` as a fix.

## Overlay is offset or scaled incorrectly

Record monitor arrangement including negative origins, per-monitor DPI, resolution/aspect ratio, game content rectangle, window mode, HDR, and resize/move sequence. Move to one monitor, refresh capture mapping, then reproduce. Hard-coded 1920×1080 coordinates are not accepted.

## Overlay disappears

This can be correct after capture loss, occlusion, stale frames, uncertain identity, device reset, or resource degradation. Audio/subtitles should continue. Check the explicit degradation reason before re-enabling visuals.

## Mouth patch drifts or corrupts the frame

Disable experimental screen-space animation. It must restore the untouched game within one displayed frame and never paste a full rectangle/freeze the scene. Report the profile/build, capture mode, face size/pose/occlusion, motion, frame age, FPS, and a safe synthetic reproduction.

## HDR/color mismatch or flicker

Record Windows/game HDR modes, monitor, format/color space, driver, and whether the issue follows the window to another display. Try SDR to isolate, but do not call that a permanent fix. Device loss/format transition should rebuild capture/composition without ending conversation.
