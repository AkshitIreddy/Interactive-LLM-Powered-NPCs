# Windows display certification evidence — 2026-08-30

This report records the bounded R13/SC08 evidence produced on the current Windows host. It separates
live native observations from deterministic simulated contracts. A simulated row is not physical
monitor, DPI, HDR, game-mode, or installed-product certification.

## Source-bound harnesses

- `native/media-broker/tests/windows_display_smoke_tests.cpp` is the current-host live exact-HWND
  Windows test. Its test HWND uses `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`; it chooses the first active
  non-primary monitor when one exists and never changes display configuration.
- `native/media-broker/tests/display_matrix_tests.cpp` is the portable deterministic matrix for
  unavailable physical modes.
- `native/media-broker/src/windows_platform.cpp` owns the tested minimize suspension and same-HWND
  WGC rebind. It preserves graphics device generation while keeping the broker-facing frame sequence
  monotonic across the internal capture-session restart.
- `native/media-broker/tests/windows_smoke_tests.cpp` remains the separate live exact-HWND pixel
  evidence for the independently running TransparencyApp overlay and the broker overlay-affinity
  smoke.
- `native/media-broker/CMakeLists.txt` registers both new tests. The Windows test shares the native
  media resource lock with the other WGC/WASAPI smoke tests.

All Windows commands were launched through hidden, noninteractive PowerShell processes with stdout
and stderr redirected under
`%LOCALAPPDATA%\InteractiveNPCs\build\wgc-lifecycle`. No terminal window was shown. The test moved,
resized, minimized, restored, and destroyed only its own HWND. It did not focus any test window,
change monitor topology/scaling/HDR, or stop, move, inspect pixels from, or reconfigure
TransparencyApp.

## Live current-host evidence

| Dimension | Observed result | Evidence class |
|---|---|---|
| Topology | 2 active monitors; non-primary `\\.\DISPLAY5` selected | Live |
| Selected bounds | `(2560, 0)–(6000, 1440)`, 3440×1440 | Live |
| Negative origin | Not present on this host | Live negative finding |
| DPI | 96 DPI / 100% on the selected HWND | Live |
| Advanced color | Evidence API available; HDR supported; HDR user setting off; HDR inactive; 8 bits/channel | Live SDR state |
| Exact target | WGC by the task-owned selected HWND; no display-duplication claim | Live |
| Activation | Target remained nonforeground through every state | Live |
| Windowed sizes | Exact 1280×720 and 1920×1080 client geometry with advancing WGC frames | Live |
| Borderless | 1920×1080 `WS_POPUP | WS_VISIBLE`, no caption, advancing exact-HWND WGC | Live |
| Resize | Windowed 720p → 1080p → borderless → windowed 720p recovered advancing frames | Live |
| Nonforeground continuity | Deterministic content change advanced while target was not foreground | Live; no literal Alt+Tab generated |
| Minimize/restore | Raw WGC required a same-HWND rebind; product platform automatically suspended/rebound and produced strictly advancing post-restore sequence and QPC | Live |
| Target loss/reselect | Destroyed target emitted WGC `Closed`; a new task-owned HWND was selected and advanced | Live |
| Graphics recreation | D3D11 device and WGC session were recreated and rebound to the exact HWND | Live recreation; not forced driver device removal |
| Broker overlay exclusion | Overlay reported applied `WDA_EXCLUDEFROMCAPTURE`; selected target affinity remained `WDA_NONE` | Live affinity attestation |
| Separate dimmer overlay | Earlier bounded smoke observed one visible layered/click-through TransparencyApp window on the selected monitor while exact-HWND target pixels retained the deterministic target color | Live test-only environment provenance |
| Cleanup | Exit 0 and no display-smoke or broker process remained | Live |

The TransparencyApp count above is test-only environment provenance, not a production receipt field.
The authoritative pixel statement remains exact-selected-HWND WGC source/scope; desktop/display
luminance is not accepted as target-pixel or HDR evidence. The broker's own full-window transparent
overlay is certified here by applied capture affinity, while the separate TransparencyApp exact-HWND
pixel check remains the reliable external-overlay pixel proof.

## Deterministic simulated matrix

`npc_media_broker_display_matrix_tests` passed the following rows and labels all of them simulated:

| Topology/origin | Resolution | DPI | Color |
|---|---:|---:|---|
| Single monitor `(0,0)` | 1280×720 | 96 / 100% | SDR sRGB |
| Single monitor `(0,0)` | 1920×1080 | 144 / 150% | SDR sRGB |
| Single monitor `(0,0)` | 2560×1440 | 192 / 200% | SDR sRGB |
| Single monitor `(0,0)` | 3840×2160 | 96 / 100% | HDR10/PQ |
| Secondary left of primary `(-1920,0)` | 1920×1080 | 144 / 150% | SDR sRGB |
| Secondary above primary `(0,-1440)` | 2560×1440 | 192 / 200% | HDR scRGB |
| Secondary right of primary `(2560,0)` | 3440×1440 (21:9) | 144 / 150% | HDR10/PQ |
| Secondary left of primary `(-5120,0)` | 5120×1440 (32:9) | 192 / 200% | SDR sRGB |

Every row verifies full physical-pixel crop/mapping, normalized patch projection, DPI scale exactly
once, and HDR tone-map intent. A partial negative-origin clipping fixture verifies source crop
`(460,100)–(2560,1200)` for a client straddling a monitor edge.

The simulated broker lifecycle additionally verifies:

- single-monitor exact-HWND WGC and capture-excluded overlay without a mirror workaround;
- windowed, borderless-geometry, and resize geometry-epoch advancement;
- minimize/restore invalidation;
- target close and replacement-HWND reselection;
- device-reset generation and geometry-epoch advancement;
- explicit true-exclusive-fullscreen degradation to audio/subtitles with capture and overlay disabled.

## Honest gaps and acceptance status

R13 and SC08 have substantially more executable evidence but are not fully physically certified.
The remaining physical gaps are:

- an actual one-monitor Windows topology with foreground PTT, exact-HWND capture, subtitles, and
  optional overlay running together;
- a real negative-origin topology;
- physical 150% and 200% per-monitor DPI;
- physical 2560×1440, 3840×2160, 3440×1440, and 5120×1440 target modes (the host monitor is
  3440×1440, but this pass used 720p/1080p target clients);
- active HDR/WCG capture and composition; the current panel reports HDR capable but HDR is off;
- a literal user Alt+Tab transition, intentionally omitted because this run was forbidden from
  focusing windows;
- a real game in borderless mode and a real exclusive-fullscreen transition; exclusive-fullscreen
  remains the documented audio/subtitle fallback and was tested only as a simulated state;
- forced driver/device removal rather than a controlled D3D recreate/rebind;
- installed-candidate and long-duration monitor-change/attach/detach evidence.

No monitor configuration change or user workaround was used to manufacture a passing row.

## Final verification

- Hidden MSVC Debug build with `NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS=ON`: passed.
- `npc_media_broker_display_matrix_tests`: passed.
- CTest selection for the simulated matrix, direct→shared lifecycle regression, and Windows display
  smoke: 3/3 passed.
- Windows display smoke repeated through the bounded crash-loop harness: 3/3 passed, exit code 0.
- Original combined Windows WGC/D3D/WASAPI/service smoke after the platform change: passed, exit code
  0. It again observed one visible TransparencyApp layered/click-through window on the selected monitor
  and retained the deterministic exact-HWND target pixel; full-display capture was explicitly not
  tested.
- Final process check: no display-smoke, Windows-smoke, or broker process remained.
