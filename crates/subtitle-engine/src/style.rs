use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleManifest {
    pub schema_version: u32,
    pub default_style_id: String,
    pub styles: Vec<SubtitleStyle>,
}

impl StyleManifest {
    #[must_use]
    pub fn style(&self, id: &str) -> Option<&SubtitleStyle> {
        self.styles.iter().find(|style| style.id == id)
    }

    pub fn validate(&self) -> Result<(), StyleError> {
        if self.schema_version != 1 {
            return Err(StyleError::UnsupportedSchema(self.schema_version));
        }
        if self.styles.is_empty() {
            return Err(StyleError::NoStyles);
        }
        if self.style(&self.default_style_id).is_none() {
            return Err(StyleError::MissingDefault(self.default_style_id.clone()));
        }
        for style in &self.styles {
            style.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleStyle {
    pub id: String,
    pub typography: TypographyStyle,
    pub geometry: GeometryStyle,
    pub effects: EffectsStyle,
    pub animation: AnimationStyle,
    pub colors: ColorStyle,
    pub behavior: BehaviorStyle,
}

impl SubtitleStyle {
    pub fn validate(&self) -> Result<(), StyleError> {
        if self.id.trim().is_empty() {
            return Err(StyleError::EmptyId);
        }
        if self.typography.body_font_role.trim().is_empty()
            || self.typography.speaker_font_role.trim().is_empty()
        {
            return Err(StyleError::InvalidFontRole(self.id.clone()));
        }
        let finite_positive = [
            self.typography.body_size_dp,
            self.typography.speaker_size_dp,
            self.typography.line_height,
            self.geometry.max_width_fraction,
            self.geometry.min_width_dp,
            self.geometry.max_width_dp,
            self.geometry.padding_x_dp,
            self.geometry.padding_y_dp,
            self.geometry.safe_margin_dp,
            self.geometry.speaker_gap_dp,
            self.geometry.head_gap_dp,
            self.geometry.candidate_gap_dp,
            self.geometry.fallback_bottom_dp,
        ];
        if finite_positive
            .into_iter()
            .any(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(StyleError::InvalidNumber(self.id.clone()));
        }
        if !(0.1..=1.0).contains(&self.geometry.max_width_fraction)
            || self.geometry.min_width_dp > self.geometry.max_width_dp
            || !(0.0..=1.0).contains(&self.behavior.head_confidence_threshold)
            || self.typography.max_body_lines == 0
            || self.behavior.collision_search_steps == 0
            || !self.typography.speaker_tracking_em.is_finite()
            || !(-0.2..=0.5).contains(&self.typography.speaker_tracking_em)
            || !self.animation.initial_scale.is_finite()
            || !(0.5..=1.5).contains(&self.animation.initial_scale)
            || !self.animation.initial_offset_y_dp.is_finite()
        {
            return Err(StyleError::InvalidRange(self.id.clone()));
        }
        self.colors.validate(&self.id)?;
        if !self.effects.outline.color.is_unit_finite()
            || !self.effects.shadow.color.is_unit_finite()
            || !self.effects.backplate.fill.is_unit_finite()
            || !self.effects.backplate.border.is_unit_finite()
            || [
                self.effects.outline.width_dp,
                self.effects.shadow.blur_dp,
                self.effects.shadow.offset_x_dp,
                self.effects.shadow.offset_y_dp,
                self.effects.backplate.blur_behind_dp,
                self.effects.backplate.border_width_dp,
                self.geometry.corner_radius_dp,
            ]
            .into_iter()
            .enumerate()
            .any(|(index, value)| !value.is_finite() || (!matches!(index, 2 | 3) && value < 0.0))
        {
            return Err(StyleError::InvalidEffects(self.id.clone()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypographyStyle {
    pub body_font_role: String,
    pub speaker_font_role: String,
    pub body_size_dp: f32,
    pub speaker_size_dp: f32,
    pub line_height: f32,
    pub speaker_tracking_em: f32,
    pub max_body_lines: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeometryStyle {
    pub max_width_fraction: f32,
    pub min_width_dp: f32,
    pub max_width_dp: f32,
    pub padding_x_dp: f32,
    pub padding_y_dp: f32,
    pub speaker_gap_dp: f32,
    pub safe_margin_dp: f32,
    pub head_gap_dp: f32,
    pub candidate_gap_dp: f32,
    pub fallback_bottom_dp: f32,
    pub corner_radius_dp: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectsStyle {
    pub outline: OutlineStyle,
    pub shadow: ShadowStyle,
    pub backplate: BackplateStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutlineStyle {
    pub enabled: bool,
    pub width_dp: f32,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShadowStyle {
    pub enabled: bool,
    pub offset_x_dp: f32,
    pub offset_y_dp: f32,
    pub blur_dp: f32,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackplateStyle {
    pub enabled: bool,
    pub blur_behind_dp: f32,
    pub border_width_dp: f32,
    pub fill: Rgba,
    pub border: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationStyle {
    pub enabled: bool,
    pub enter_ms: u32,
    pub exit_ms: u32,
    pub initial_offset_y_dp: f32,
    pub initial_scale: f32,
    pub reveal_delay_ms: u32,
    pub reveal_ms_per_grapheme: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BehaviorStyle {
    pub head_confidence_threshold: f32,
    pub max_track_age_ms: u32,
    pub collision_search_steps: u8,
    pub prefer_head_anchor: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    fn is_unit_finite(self) -> bool {
        [self.r, self.g, self.b, self.a]
            .into_iter()
            .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorStyle {
    pub body: Rgba,
    pub speaker: Rgba,
    pub encoding: ColorEncoding,
    pub hdr: HdrSafeMetadata,
}

impl ColorStyle {
    fn validate(&self, style_id: &str) -> Result<(), StyleError> {
        if !self.body.is_unit_finite()
            || !self.speaker.is_unit_finite()
            || self.hdr.target_spaces.is_empty()
            || self.hdr.custom_pq_or_scrgb_shader
            || self.hdr.tone_map != ToneMapPolicy::NoneInRenderer
            || self.hdr.source_surface != HdrSourceSurface::Bgra8SrgbPremultiplied
            || self.hdr.sdr_white_mapping != SdrWhiteMapping::WindowsCompositorManaged
            || !self.hdr.legacy_metadata_valid()
        {
            return Err(StyleError::InvalidColor(style_id.to_owned()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorEncoding {
    Srgb,
    LinearSrgb,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HdrSafeMetadata {
    pub target_spaces: Vec<OutputColorSpace>,
    #[serde(default)]
    pub source_surface: HdrSourceSurface,
    #[serde(default)]
    pub sdr_white_mapping: SdrWhiteMapping,
    #[serde(alias = "hdr_treatment")]
    pub tone_map: ToneMapPolicy,
    #[serde(default)]
    pub custom_pq_or_scrgb_shader: bool,
    /// Compatibility-only fields accepted from pre-v1 review manifests. They
    /// never authorize renderer-side tone mapping and are omitted by the
    /// canonical asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_white_nits: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_subtitle_nits: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_alpha_linear: Option<bool>,
}

impl HdrSafeMetadata {
    #[must_use]
    pub fn compatibility(&self) -> HdrMetadataCompatibility {
        if self.reference_white_nits.is_some()
            || self.max_subtitle_nits.is_some()
            || self.preserve_alpha_linear.is_some()
        {
            HdrMetadataCompatibility::LegacyNormalized
        } else {
            HdrMetadataCompatibility::Canonical
        }
    }

    fn legacy_metadata_valid(&self) -> bool {
        match (self.reference_white_nits, self.max_subtitle_nits) {
            (None, None) => true,
            (Some(reference), Some(peak)) => {
                reference.is_finite()
                    && peak.is_finite()
                    && reference >= 80.0
                    && peak >= reference
                    && peak <= 500.0
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrSourceSurface {
    #[default]
    Bgra8SrgbPremultiplied,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdrWhiteMapping {
    #[default]
    WindowsCompositorManaged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrMetadataCompatibility {
    Canonical,
    LegacyNormalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputColorSpace {
    Srgb,
    Scrgb,
    Rec2020Pq,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToneMapPolicy {
    #[serde(
        rename = "none_in_renderer",
        alias = "clamp_to_subtitle_peak",
        alias = "relative_to_reference_white"
    )]
    NoneInRenderer,
}

/// Sparse override applied in order: base style, game, then character.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StylePatch {
    pub body_font_role: Option<String>,
    pub speaker_font_role: Option<String>,
    pub body_size_dp: Option<f32>,
    pub speaker_size_dp: Option<f32>,
    pub line_height: Option<f32>,
    pub max_body_lines: Option<u16>,
    pub max_width_fraction: Option<f32>,
    pub min_width_dp: Option<f32>,
    pub max_width_dp: Option<f32>,
    pub padding_x_dp: Option<f32>,
    pub padding_y_dp: Option<f32>,
    pub speaker_gap_dp: Option<f32>,
    pub safe_margin_dp: Option<f32>,
    pub head_gap_dp: Option<f32>,
    pub candidate_gap_dp: Option<f32>,
    pub fallback_bottom_dp: Option<f32>,
    pub corner_radius_dp: Option<f32>,
    pub body_color: Option<Rgba>,
    pub speaker_color: Option<Rgba>,
    pub outline_enabled: Option<bool>,
    pub outline_width_dp: Option<f32>,
    pub outline_color: Option<Rgba>,
    pub shadow_enabled: Option<bool>,
    pub shadow_offset_x_dp: Option<f32>,
    pub shadow_offset_y_dp: Option<f32>,
    pub shadow_blur_dp: Option<f32>,
    pub shadow_color: Option<Rgba>,
    pub backplate_enabled: Option<bool>,
    pub backplate_blur_behind_dp: Option<f32>,
    pub backplate_border_width_dp: Option<f32>,
    pub backplate_fill: Option<Rgba>,
    pub backplate_border: Option<Rgba>,
    pub animation_enabled: Option<bool>,
    pub enter_ms: Option<u32>,
    pub exit_ms: Option<u32>,
    pub initial_offset_y_dp: Option<f32>,
    pub initial_scale: Option<f32>,
    pub reveal_delay_ms: Option<u32>,
    pub reveal_ms_per_grapheme: Option<u32>,
    pub prefer_head_anchor: Option<bool>,
    pub head_confidence_threshold: Option<f32>,
    pub max_track_age_ms: Option<u32>,
    pub collision_search_steps: Option<u8>,
    pub hdr_reference_white_nits: Option<f32>,
    pub hdr_max_subtitle_nits: Option<f32>,
}

impl StylePatch {
    pub fn apply_to(&self, style: &mut SubtitleStyle) {
        if let Some(value) = &self.body_font_role {
            style.typography.body_font_role.clone_from(value);
        }
        if let Some(value) = &self.speaker_font_role {
            style.typography.speaker_font_role.clone_from(value);
        }
        if let Some(value) = self.body_size_dp {
            style.typography.body_size_dp = value;
        }
        if let Some(value) = self.speaker_size_dp {
            style.typography.speaker_size_dp = value;
        }
        if let Some(value) = self.line_height {
            style.typography.line_height = value;
        }
        if let Some(value) = self.max_body_lines {
            style.typography.max_body_lines = value;
        }
        if let Some(value) = self.max_width_fraction {
            style.geometry.max_width_fraction = value;
        }
        if let Some(value) = self.min_width_dp {
            style.geometry.min_width_dp = value;
        }
        if let Some(value) = self.max_width_dp {
            style.geometry.max_width_dp = value;
        }
        if let Some(value) = self.padding_x_dp {
            style.geometry.padding_x_dp = value;
        }
        if let Some(value) = self.padding_y_dp {
            style.geometry.padding_y_dp = value;
        }
        if let Some(value) = self.speaker_gap_dp {
            style.geometry.speaker_gap_dp = value;
        }
        if let Some(value) = self.safe_margin_dp {
            style.geometry.safe_margin_dp = value;
        }
        if let Some(value) = self.head_gap_dp {
            style.geometry.head_gap_dp = value;
        }
        if let Some(value) = self.candidate_gap_dp {
            style.geometry.candidate_gap_dp = value;
        }
        if let Some(value) = self.fallback_bottom_dp {
            style.geometry.fallback_bottom_dp = value;
        }
        if let Some(value) = self.corner_radius_dp {
            style.geometry.corner_radius_dp = value;
        }
        if let Some(value) = self.body_color {
            style.colors.body = value;
        }
        if let Some(value) = self.speaker_color {
            style.colors.speaker = value;
        }
        if let Some(value) = self.outline_enabled {
            style.effects.outline.enabled = value;
        }
        if let Some(value) = self.outline_width_dp {
            style.effects.outline.width_dp = value;
        }
        if let Some(value) = self.outline_color {
            style.effects.outline.color = value;
        }
        if let Some(value) = self.shadow_enabled {
            style.effects.shadow.enabled = value;
        }
        if let Some(value) = self.shadow_offset_x_dp {
            style.effects.shadow.offset_x_dp = value;
        }
        if let Some(value) = self.shadow_offset_y_dp {
            style.effects.shadow.offset_y_dp = value;
        }
        if let Some(value) = self.shadow_blur_dp {
            style.effects.shadow.blur_dp = value;
        }
        if let Some(value) = self.shadow_color {
            style.effects.shadow.color = value;
        }
        if let Some(value) = self.backplate_enabled {
            style.effects.backplate.enabled = value;
        }
        if let Some(value) = self.backplate_blur_behind_dp {
            style.effects.backplate.blur_behind_dp = value;
        }
        if let Some(value) = self.backplate_border_width_dp {
            style.effects.backplate.border_width_dp = value;
        }
        if let Some(value) = self.backplate_fill {
            style.effects.backplate.fill = value;
        }
        if let Some(value) = self.backplate_border {
            style.effects.backplate.border = value;
        }
        if let Some(value) = self.animation_enabled {
            style.animation.enabled = value;
        }
        if let Some(value) = self.enter_ms {
            style.animation.enter_ms = value;
        }
        if let Some(value) = self.exit_ms {
            style.animation.exit_ms = value;
        }
        if let Some(value) = self.initial_offset_y_dp {
            style.animation.initial_offset_y_dp = value;
        }
        if let Some(value) = self.initial_scale {
            style.animation.initial_scale = value;
        }
        if let Some(value) = self.reveal_delay_ms {
            style.animation.reveal_delay_ms = value;
        }
        if let Some(value) = self.reveal_ms_per_grapheme {
            style.animation.reveal_ms_per_grapheme = value;
        }
        if let Some(value) = self.prefer_head_anchor {
            style.behavior.prefer_head_anchor = value;
        }
        if let Some(value) = self.head_confidence_threshold {
            style.behavior.head_confidence_threshold = value;
        }
        if let Some(value) = self.max_track_age_ms {
            style.behavior.max_track_age_ms = value;
        }
        if let Some(value) = self.collision_search_steps {
            style.behavior.collision_search_steps = value;
        }
        if let Some(value) = self.hdr_reference_white_nits {
            style.colors.hdr.reference_white_nits = Some(value);
        }
        if let Some(value) = self.hdr_max_subtitle_nits {
            style.colors.hdr.max_subtitle_nits = Some(value);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SubtitleOverrides {
    pub games: BTreeMap<String, StylePatch>,
    pub characters: BTreeMap<String, StylePatch>,
}

impl SubtitleOverrides {
    pub fn resolve(
        &self,
        base: &SubtitleStyle,
        game_id: Option<&str>,
        character_id: Option<&str>,
    ) -> Result<SubtitleStyle, StyleError> {
        let mut resolved = base.clone();
        if let Some(patch) = game_id.and_then(|id| self.games.get(id)) {
            patch.apply_to(&mut resolved);
        }
        if let Some(patch) = character_id.and_then(|id| self.characters.get(id)) {
            patch.apply_to(&mut resolved);
        }
        resolved.validate()?;
        Ok(resolved)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StyleError {
    #[error("subtitle style schema {0} is not supported")]
    UnsupportedSchema(u32),
    #[error("subtitle style manifest contains no styles")]
    NoStyles,
    #[error("default subtitle style `{0}` does not exist")]
    MissingDefault(String),
    #[error("subtitle style id must not be empty")]
    EmptyId,
    #[error("subtitle style `{0}` references an empty font role")]
    InvalidFontRole(String),
    #[error("subtitle style `{0}` contains a non-positive or non-finite number")]
    InvalidNumber(String),
    #[error("subtitle style `{0}` contains an invalid range")]
    InvalidRange(String),
    #[error("subtitle style `{0}` contains unsafe color/HDR metadata")]
    InvalidColor(String),
    #[error("subtitle style `{0}` contains invalid effect geometry or color")]
    InvalidEffects(String),
}
