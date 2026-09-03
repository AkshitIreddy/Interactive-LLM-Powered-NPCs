# Native subtitle renderer

This directory turns the renderer-agnostic output from `crates/subtitle-engine` into a native
subtitle layer without injecting into, hooking, or modifying a game. It is deliberately isolated
from the active media-broker and subtitle-engine sources.

## Contract

The runtime performs text shaping and layout first. A `RenderRequest` then contains only:

- validated physical-desktop-pixel viewport, clip, subtitle, speaker, and body rectangles;
- already-rasterized 8-bit glyph coverage masks tagged as body or speaker text;
- resolved physical-pixel colors and effect sizes; and
- presentation, capture-sequence, and graphics-generation identity.

`CpuSubtitleRenderer` produces a tightly cropped `RenderedLayer` in BGRA8 sRGB with premultiplied
alpha. It draws the rounded backplate and border, a deterministic two-pass soft text shadow, text
outline, body text, and speaker label in that order. Every output pixel is checked to satisfy
`B <= A`, `G <= A`, and `R <= A`. Viewport, run, glyph, effect, allocation, and protocol limits are
validated before allocation. A layout wholly outside the viewport can be translated to a safe
bottom-center placement; the result explicitly records that fallback.

The portable renderer consumes coverage, not text. Therefore platform shaping retains ownership of
bidirectional ordering, grapheme clusters, OpenType features, locale fallback, and font licensing.
The block glyphs in `tests/` are only deterministic test fixtures. They are not a font family,
theme, user-facing style, or product visual preset.

`protocol/subtitle-layer-v1.proto` documents the versioned process-boundary envelope. The C++
types are the authoritative in-process API. A transport must authenticate its peer and enforce
`RenderLimits` before deserializing large `bytes` fields; the schema does not grant trust.

`SubtitlePresentationService` is the delivery boundary above the renderer. It binds the 32-byte
launch nonce to session ID, PID, process creation time, and executable name; consumes strictly
increasing request sequences; enforces deadlines and cancellation generation; and synchronously
hides the surface on cancellation, target loss, shutdown, or destruction. RTL requests are
rejected unless bidi shaping and grapheme preservation are attested. Mixed-DPI metadata is bounded
while all layout remains in physical pixels. HDR requests require explicit color-space and SDR-white
metadata. A `SubtitlePresentationReceipt` exists only after `ISubtitleSurface::present` reports a
committed native presentation with a nonzero QPC timestamp. Shaping, rendering, or enqueueing alone
never counts as delivered.

## Windows DirectWrite and Direct2D

On Windows, `npc_subtitle_renderer_windows` provides `DirectWriteD2DBackend` behind `_WIN32`:

1. `resolve` converts a shaped `DWRITE_GLYPH_RUN` into grayscale coverage with
   `IDWriteGlyphRunAnalysis`. ClearType RGB coverage is collapsed to alpha because LCD subpixel
   color is unsafe on a transparent moving overlay.
2. The portable CPU renderer applies all layer effects and alpha rules.
3. `create_bitmap` uploads the result as an `ID2D1Bitmap1` with
   `DXGI_FORMAT_B8G8R8A8_UNORM` and `D2D1_ALPHA_MODE_PREMULTIPLIED`.

`resolve_text` is the product text-shaping entry point. It accepts bounded UTF-8, locale, physical
pixel rectangles and font sizes, sets DirectWrite paragraph reading direction, preserves native
bidi/script/grapheme behavior, uses the Windows system font fallback resolver, and emits coverage
masks tagged as speaker or body. One layout unit intentionally equals one physical pixel at this
boundary so DPI scaling is applied exactly once by `crates/subtitle-engine`.

The adapter never claims that every script can be rendered by one family. The packaged presenter
starts from Segoe UI and delegates missing-script resolution to DirectWrite's system font fallback.
No font binary is bundled or downloaded. `assets/subtitles/fonts.v1.json` records the platform
families and licensing policy used for review; the user's installed Windows fonts remain the only
font source.

## Product presenter integration

The media broker should treat subtitle presentation separately from the mouth residual texture:

```text
subtitle-engine layout + platform shaping
                 |
                 v
       RenderSubtitleRequest
                 |
                 v
  CpuSubtitleRenderer / DWrite adapter
                 |
                 v
 BGRA8 premultiplied subtitle layer
                 |
                 v
 private-pipe `npc-subtitle-presenter.exe`
                 |
                 v
 capture-excluded DirectComposition visual above the target
```

The runtime launches the presenter lazily as a GUI-subsystem sibling sidecar over inherited private
stdin/stdout handles. Authentication binds a fresh 32-byte launch nonce to the runtime session, PID,
process creation time, and executable name. Trusted target mode additionally revalidates HWND/PID,
executable leaf, physical client rect, per-window DPI, geometry epoch, capture sequence, and graphics
generation. Stale/replayed/deadline-expired requests fail closed. Cancellation kills the child, and
EOF, shutdown, target loss, service destruction, or cancellation synchronously hides the surface.

The private launch also pins one atomic subtitle-preference authority: persisted revision,
lowercase SHA-256, effective source provenance, resolved style, safe area, text scale, backplate,
and opacity. The presenter converts the resolved density-independent dimensions to physical pixels
once at the measured target/system DPI and consumes the resolved colors and effects. Every PRESENT
command repeats the revision/digest binding; a mismatch consumes its sequence and fails closed,
while each committed receipt repeats the exact authority fields.

When trusted target/capture evidence is unavailable, the only legal request is an all-zero/unknown
sentinel. The presenter measures the current Windows virtual screen and system DPI locally and emits
a receipt with `console_bottom_center_unavailable` provenance and zero capture/geometry/generation
IDs. It never relabels that fallback as an in-game overlay. Measured HUD exclusion rectangles are
validated inside the trusted viewport and move the bottom-center candidate upward until it no longer
collides.

The surface is BGRA8/sRGB-authored. Only a measured SDR-sRGB target reports direct SDR source-over.
scRGB, HDR, and unknown console color states truthfully report Windows-compositor SDR-white mapping;
the presenter does not claim a custom PQ/scRGB shader.

External desktop dimmers such as `TransparencyApp.exe` are separate perceived/display overlays.
Their opacity, screenshot effect, or mere presence is not HDR, color-space, SDR-white, or luminance
evidence. Trusted target receipts use only broker-owned Windows display-path evidence; unavailable
native color evidence stays unavailable and cannot be replaced with a dimmer-derived value.

## Build and test

Portable:

```sh
cmake -S native/subtitle-renderer -B out/subtitle-renderer -G Ninja
cmake --build out/subtitle-renderer
ctest --test-dir out/subtitle-renderer --output-on-failure
```

Windows (DirectWrite/Direct2D adapter included):

```powershell
cmake -S native/subtitle-renderer -B out/subtitle-renderer-win -G "Visual Studio 17 2022" -A x64
cmake --build out/subtitle-renderer-win --config Debug
ctest --test-dir out/subtitle-renderer-win -C Debug --output-on-failure
```

The Windows build also produces the Release GUI product `npc-subtitle-presenter.exe` from CMake
target `npc_subtitle_presenter`. Packaging stages it as
`npc-subtitle-presenter-x86_64-pc-windows-msvc.exe` beside `npc-runtime` and audits x64 machine type,
Windows GUI subsystem, imports, debug-CRT absence, and SHA-256 before review/release bundling.

The tests cover deterministic golden hashing, negative desktop coordinates, bottom-center fallback,
hard clipping, distinct speaker/body colors, premultiplied-alpha safety, malformed coverage, resource
budgets, unsupported versions, empty visible regions, authenticated peer binding, replay/deadline
rejection, cancellation cleanup, committed-only receipts, mixed-DPI/HDR receipt evidence, and real
Windows DirectWrite shaping for both LTR and Arabic RTL text.
