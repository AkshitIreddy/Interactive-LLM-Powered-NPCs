# Overlay and display behavior

## Supported external path

The C++/WinRT media broker targets Windows Graphics Capture by game window, D3D11 textures, per-monitor DPI/HDR/content-rect mapping, and DirectComposition presentation. DXGI Desktop Duplication is a declared fallback where appropriate.

The current `native/media-broker` implements the authenticated coordinator, policy/recovery/cancellation state, validated PID/HWND target, exact-HWND WinRT WGC frame pool, D3D11 source leases for bounded worker crops, a premultiplied DirectComposition residual visual, event-driven shared-mode WASAPI clients, and authenticated one-use PCM playback transport. Command 23 exposes broker-owned client geometry, frame/device generations, DPI, and Windows advanced-color/SDR-white evidence with explicit availability bits. Missing or stale evidence reports unavailable; Control must not replace it with WebView measurements or fixed planning values.

These seams have portable contract tests and Windows native/synthetic-target proofs. They are not evidence that every game, exclusive-fullscreen mode, protected surface, HDR driver, monitor transition, or installed-machine matrix has passed. Release claims remain limited to the exact synthetic and live matrices recorded in the verification ledger.

Windowed and borderless modes are the reliable external-overlay targets. True exclusive fullscreen, protected content, minimized windows, and unsupported swapchains fall back to audio/subtitles when policy permits; there is no native/injected adapter path.

## Display changes

Capture and overlay must recover from resize, alt-tab, monitor move, negative desktop origins, DPI changes, 720p–4K, ultrawide, SDR/HDR transitions, and device loss. Coordinates are transformed from captured content—not hard-coded to 1920×1080.

## External dimmers and perceived brightness

External desktop dimmer overlays are separate display/perception effects. They may change what a person sees or what a desktop screenshot contains, but they are never accepted as the game's HDR mode, color encoding, SDR-white level, capture luminance, or subtitle tone-mapping truth. The broker derives those fields from the selected HWND's monitor and Windows display-path evidence. Product evidence records the generic perceived-brightness caveat without enumerating, naming, counting, focusing, or reconfiguring overlay applications. If native evidence is unavailable, subtitles use the explicitly unavailable console fallback; the app does not infer color truth from a dimmer's opacity or presence.

## Visual animation safety

Experimental generic screen-space mode edits only a tracked mouth residual over the current frame. It must not freeze the scene or paste a rectangular face. Occlusion, identity uncertainty, stale output, or resource pressure restores the untouched game within one displayed frame. Lip-sync models are optional local packs selected by the user; no pack is bundled, auto-downloaded, or presented as available before qualification.

## If the NPC is offscreen

Select/name the intended NPC. Audio/subtitles and memory continue without capture identity or facial animation. Offscreen interaction is a supported fallback, not an error.

## Performance

Optional visuals receive the lowest GPU priority. They are disabled before speech/conversation when frame time or memory crosses the selected limit. See [Performance](performance-and-benchmarking.md) and [overlay troubleshooting](../troubleshooting/overlays-and-displays.md).

## Native build verification

```powershell
cmake -S native/media-broker -B build/media-broker -G "Visual Studio 17 2022" -A x64 -DNPC_MEDIA_BROKER_REGISTER_INTERACTIVE_WINDOWS_TESTS=OFF
cmake --build build/media-broker --config Debug
ctest --test-dir build/media-broker -C Debug --output-on-failure
```

The default CTest inventory contains only noninteractive tests. The Windows WGC,
display, playback, input, and identity smoke tests can create visible windows or
use live user devices, so they are not registered unless the build is configured
with `-DNPC_MEDIA_BROKER_REGISTER_INTERACTIVE_WINDOWS_TESTS=ON`. Use that opt-in
only for a deliberately scheduled interactive qualification, then run the
`interactive_windows_smoke` label explicitly. An existing build directory keeps
its previous CTest inventory until CMake is run again; reconfigure it with the
option set to `OFF` before running an aggregate `ctest` command.

Portable tests verify the contracts; only rendered/native Windows evidence can certify capture and overlay behavior.
