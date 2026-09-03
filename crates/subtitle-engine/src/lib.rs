//! Renderer-agnostic subtitle layout for arbitrary captured games.
//!
//! All geometry entering and leaving [`layout_subtitle`] is expressed in physical desktop
//! pixels. Theme dimensions are density-independent pixels and are scaled exactly once through
//! [`DpiScale`]. This makes the contract safe for mixed-DPI desktops and viewports whose desktop
//! origin is negative. Text shaping and rasterization remain the responsibility of the host
//! renderer; this crate supplies an explicit [`ShapingPlan`] and consumes measured text metrics.

#![forbid(unsafe_code)]

mod animation;
mod geometry;
mod layout;
mod preferences;
mod shaping;
mod style;

pub use animation::*;
pub use geometry::*;
pub use layout::*;
pub use preferences::*;
pub use shaping::*;
pub use style::*;

/// Original, MIT-licensed default visual styles bundled as data.
pub const DEFAULT_STYLES_JSON: &str = include_str!("../../../assets/subtitles/styles.v1.json");

/// System-font fallback catalog. No font bytes are bundled by this crate.
pub const FONT_CATALOG_JSON: &str = include_str!("../../../assets/subtitles/fonts.v1.json");

/// Machine-readable licensing and redistribution policy for every subtitle asset.
pub const ASSET_LICENSES_JSON: &str = include_str!("../../../assets/subtitles/licenses.v1.json");

/// Canonical JSON Schema for the v1 subtitle style asset. Runtime serde also
/// accepts two pre-v1 tone-map spellings and normalizes them to the one
/// renderer-neutral policy.
pub const STYLE_SCHEMA_JSON: &str = include_str!("../../../schemas/subtitle-style-v1.json");
