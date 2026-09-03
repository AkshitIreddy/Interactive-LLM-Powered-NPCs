# NPC Media Broker

This directory is the native media boundary for Interactive LLM Powered NPCs 2.0. It is deliberately a separate process/library boundary: game pixels and PCM remain outside the WebView, and no code is injected into a game.

## What is implemented

- A portable C++20 coordinator with explicit target, capture, overlay, audio, policy, recovery, cancellation, and degradation states.
- Game-window targeting by validated native window and owning process identity.
- Windows Graphics Capture by HWND through a free-threaded WinRT `Direct3D11CaptureFramePool`, including content resize, target-close, latest-frame, and device-loss paths. DXGI Desktop Duplication remains the declared fallback.
- A transparent, click-through, no-activate, tool-window overlay backed by a premultiplied-alpha DXGI composition swap chain and DirectComposition visual. It accepts a clipped same-device BGRA8 residual texture and hides immediately on pristine, stale, occluded, invalid, or busy presentation paths.
- Command 31 provides a separate native-only manual actor picker. It freezes one fresh, advancing exact-HWND WGC frame in a capture-excluded `WS_EX_NOACTIVATE` overlay, draws only native visual-authority ROIs, and accepts exactly one hardware-origin mouse/touch/pen down-and-matching-up sequence in the same ROI. The overlay holds the pointer release, rejects injected input, extra pointers/buttons, and mismatched releases, and closes only after consuming the matching up. It synchronously rechecks session/generation, HWND/PID, WGC backend, device, geometry, monitor, and DPI immediately before show and again inside the terminal receipt transaction; its timer is only a supplemental invalidation check. Escape, authenticated cancel, timeout, target loss, resize, DPI change, capture change, device generation change, a click outside every ROI, or an ambiguous overlap all terminate fail-closed without choosing the highest score or a remembered actor.
- Event-driven shared-mode `IAudioClient3` capture and render clients on MMCSS `Pro Audio` threads. Bounded SPSC PCM rings publish the negotiated hardware formats, account for overflow/underflow exactly, clear queued render audio on cancellation, and are rebuilt after endpoint loss.
- Global PTT press/release sensing with `GetAsyncKeyState`, which requires neither foreground focus nor a keyboard hook and does not consume the game's key.
- Physical-pixel geometry for negative desktop origins, per-monitor DPI, monitor clipping, full-output capture crops, and HDR-to-overlay tone-map intent.
- A single-slot latest-frame mailbox. Producers never wait for rendering or inference; unread stale frames are overwritten and counted.
- Generation-based cancellation plus strict source-frame, graphics-epoch, and source-capture-time matching for late inference output.
- Manual selection receipts bind the typed actor/track/track-epoch result to the exact session, HWND/PID/executable, cancellation/device/geometry/frame generations, source QPC, a random receipt nonce, and a SHA-256 digest of the candidate set. Receipts expose neither the frozen pixels, candidate rectangles, nor pointer coordinates to the WebView.
- Finite confidence, visibility, occlusion, age, texture-presence, and residual-bound gates. A patch wider than 45%, taller than 35%, or covering more than 12% of the captured frame is rejected before platform presentation; full-frame replacement cannot pass the broker policy. Any uncertainty immediately suppresses a prior residual and presents the untouched game on the next frame.
- Deterministic simulation and dependency-free tests usable on Linux/macOS CI.
- A persistent control service that refuses to launch without an exact parent PID, 256-bit launch nonce, valid session identifier, and inherited Job Object. It monitors the parent, polls media continuously, and exits after authenticated shutdown, parent death, or pipe disconnect.
- A one-client, local-only Windows named pipe with a current-user DACL, `PIPE_REJECT_REMOTE_CLIENTS`, exact parent-PID verification, 64 KiB frame limit, and a bounded request queue. The versioned protobuf-wire-compatible envelope authenticates nonce/session and validates monotonic sequence, QPC deadline, cancellation generation, command kind, and payload size.
- Typed control commands for health, HWND selection, target clearing, PTT configuration, audio format/ring metrics, occlusion metadata, patch/shared-texture descriptors, cancellation, diagnostics, and shutdown. Responses use typed status codes; dialogue, pixels, PCM, credentials, nonces, and payload contents are never logged.
- Independent HWND/process inspection before capture: exact PID, same Windows session and user SID, process protection level, process basename, and loaded module basenames. Local evidence is combined with the authenticated runtime's data-only executable allowlist. Unknown inspection, minimized/protected/cross-user/cross-session targets, known anti-cheat markers, and same-executable online ambiguity fail closed to audio/subtitles.

The Windows layer uses public out-of-process APIs only: HWND validation, Windows/DXGI capture, D3D11, DirectComposition, event-driven WASAPI, and non-invasive key-state polling. It does not hook, inject, patch, read game memory, or attempt to bypass protected content or anti-cheat.

## Build and test

```powershell
cmake -S native/media-broker -B build/media-broker -G "Visual Studio 17 2022" -A x64
cmake --build build/media-broker --config Debug
ctest --test-dir build/media-broker -C Debug --output-on-failure
```

Portable CI uses the same commands with Ninja or the host default generator for the core library and deterministic tests. The production `npc-media-broker` service itself is Windows-only and intentionally exits when its authenticated launch context is absent.

On Windows, CTest additionally creates and resizes a real HWND, receives WGC frames from the free-threaded frame pool, presents then hides a premultiplied residual through DirectComposition, and starts event-driven capture/render WASAPI clients. It also launches the service inside a kill-on-close Job Object, verifies malformed-launch rejection, nonce authentication, health and shutdown transactions, and exit after parent-pipe disconnect. A machine without an interactive desktop or audio endpoints cannot satisfy that smoke test and must report the missing capability rather than silently pass it.

## Ownership and ordering

The parent runtime owns session and turn truth. The broker owns media-device generations and presentation truth. Platform callbacks must be serialized onto the broker loop; incoming PCM and GPU resources are transported by separate bounded IPC rings/handles rather than copied through the control UI.

On device loss the broker clears stale frame/patch mailboxes, increments the graphics generation, recreates the device, and reselects primary capture. Patch and occlusion messages carry that generation, their exact source-frame sequence, QPC-derived source timing, selected actor-track ID, and nonzero track epoch. A recycled sequence from a prior graphics epoch or a late residual from a replaced/reacquired actor track cannot be presented. Repeated primary failures select Desktop Duplication; protected content or low tracking confidence degrades to audio/subtitles without retaining a copied or frozen game frame.

## Deliberate limitations and remaining gates

The implemented native paths do not imply that every downstream integration is complete:

1. The residual overlay currently accepts only an `ID3D11Texture2D` already opened on the broker device. Cross-process D3D shared-handle lifetime, nonce/ACL negotiation, and keyed synchronization with an inference worker are not implemented or claimed.
2. The PCM ring contract is implemented and tested in process. Mapping its storage/control header into SID-restricted cross-process shared memory belongs to the IPC transport layer.
3. WASAPI uses each endpoint's negotiated mix format without resampling or channel conversion. Tests measure buffer continuity, peak, clipping, silence, overflow, underflow, cancellation, and reset behavior; they do not establish perceptual audio quality.
4. Geometry carries DPI, rotation, color-space, and tone-map intent. DXGI HDR metadata discovery and an SDR/HDR residual shader have not been validated, so HDR compositing perfection is not claimed.
5. Protected, exclusive-fullscreen, or policy-blocked content can prevent WGC and Desktop Duplication. The broker fails open to audio/subtitles; it never attempts a bypass.

The control protocol accepts and validates shared-texture descriptors, but currently returns `capability_unavailable` instead of duplicating the numeric handle. Safe completion requires authenticated parent-handle duplication, adapter-LUID agreement, format/extent validation, and keyed-mutex ownership transitions. Likewise, audio status returns exact format/capacity/availability/overrun/underrun metrics plus an explicit `cross-process mapping unavailable` flag; it never returns a fake mapping handle.

These are reported as unavailable capabilities rather than simulated successes.
