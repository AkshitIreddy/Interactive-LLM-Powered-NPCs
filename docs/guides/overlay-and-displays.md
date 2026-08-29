# Overlay and display behavior

## Supported external path

The C++/WinRT media broker targets Windows Graphics Capture by game window, D3D11 textures, per-monitor DPI/HDR/content-rect mapping, and DirectComposition presentation. DXGI Desktop Duplication is a declared fallback where appropriate.

The current `native/media-broker` foundation implements/test-drives the coordinator, policy/recovery/cancellation state, validated HWND/process target, Desktop Duplication opening, D3D11/DirectComposition device setup, WASAPI device discovery, global non-hooking PTT polling, geometry, latest-frame mailbox, and fail-open compositing gates. It does **not** yet link the WinRT WGC frame pool, shared D3D IPC, DirectComposition visual/swap-chain presentation, event-driven audio clients/PCM rings, or validated HDR shader path. Those capabilities report unavailable rather than simulating success.

Windowed and borderless modes are the reliable external-overlay targets. True exclusive fullscreen, protected content, minimized windows, and unsupported swapchains fall back to audio/subtitles when policy permits; there is no native/injected adapter path.

## Display changes

Capture and overlay must recover from resize, alt-tab, monitor move, negative desktop origins, DPI changes, 720p–4K, ultrawide, SDR/HDR transitions, and device loss. Coordinates are transformed from captured content—not hard-coded to 1920×1080.

## Visual animation safety

Experimental generic screen-space mode edits only a tracked mouth residual over the current frame. It must not freeze the scene or paste a rectangular face. Occlusion, identity uncertainty, stale output, or resource pressure restores the untouched game within one displayed frame. Lip-sync models are optional local packs selected by the user; no pack is bundled, auto-downloaded, or presented as available before qualification.

## If the NPC is offscreen

Select/name the intended NPC. Audio/subtitles and memory continue without capture identity or facial animation. Offscreen interaction is a supported fallback, not an error.

## Performance

Optional visuals receive the lowest GPU priority. They are disabled before speech/conversation when frame time or memory crosses the selected limit. See [Performance](performance-and-benchmarking.md) and [overlay troubleshooting](../troubleshooting/overlays-and-displays.md).

## Native build verification

```powershell
cmake -S native/media-broker -B build/media-broker -G "Visual Studio 17 2022" -A x64
cmake --build build/media-broker --config Debug
ctest --test-dir build/media-broker -C Debug --output-on-failure
```

Portable tests verify the contracts; only rendered/native Windows evidence can certify capture and overlay behavior.
