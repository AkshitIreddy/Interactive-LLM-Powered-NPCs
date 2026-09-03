# NPC subtitle engine

`npc-subtitle-engine` computes subtitle geometry and animation properties without assuming a game,
capture API, compositor, text rasterizer, or monitor arrangement. Inputs and outputs use physical
desktop pixels; authored theme measurements use density-independent pixels and are converted once.

Integration flow:

1. Parse and validate `DEFAULT_STYLES_JSON` and `FONT_CATALOG_JSON` once.
2. Resolve the base style, then game and character `StylePatch` values.
3. Build shaping plans for speaker and body text. Shape using DirectWrite or HarfBuzz and implement
   `TextMeasurer` with the resulting wrapped metrics.
4. Supply the captured game viewport, an optional reliable tracked-head rectangle, HUD exclusion
   rectangles, and already occupied subtitle rectangles to `layout_subtitle`.
5. Composite the returned outline, shadow, backplate, speaker label, and body bounds. Apply
   `sample_animation` without feeding animated bounds back into collision layout.
6. Keep the authored subtitle surface BGRA8 sRGB premultiplied. The subtitle engine performs no HDR
   tone mapping or custom PQ/scRGB shading; the Windows compositor owns SDR-white mapping for scRGB,
   HDR, and unavailable color-space targets. Presentation receipts report that treatment explicitly.

`schemas/subtitle-style-v1.json` defines `tone_map: "none_in_renderer"` as the sole canonical v1
value. The Rust reader accepts the two pre-v1 spellings (`clamp_to_subtitle_peak` and
`relative_to_reference_white`, including the former `hdr_treatment` key) only as input
compatibility aliases and normalizes both to `none_in_renderer` before serialization or use.

The layout engine never injects into a game and never needs game-specific code. Game and character
customization is data-only.

## Native subtitle preferences

`SubtitlePreferenceManager` owns the versioned native preference file
`subtitle-preferences-v1.json`. It resolves global, game, and character scopes
in that order and reports the exact source of every effective value. Writes are
optimistic-revision guarded, flushed and atomically replaced; schema v0 global
settings migrate once to scoped v1, while corrupt or future schemas fail closed
without overwriting the saved file.

The public mutation surface is deliberately smaller than `StylePatch`. It
allows only fields already represented by the current native renderer:

- a validated bundled `selectedStyleId`;
- `safeAreaDp` for layout/fallback safe margin;
- `textScale` for the DirectWrite body and speaker sizes;
- `backplateEnabled` for `ResolvedStyle.backplate.enabled`; and
- `opacity` for `ResolvedStyle.global_alpha`.

Font family, color, HDR, animation, arbitrary CSS, and remote asset fields are
not accepted. All DTOs reject unknown fields. Read snapshots disclose the
ordered system-font fallback and its license reference, but use
`systemLookupRequired` availability: the manager does not claim that a family
is installed. No font binary is bundled, downloaded, or installed.
