use serde::{Deserialize, Serialize};

use crate::{DpiScale, InsetsPx, PointPx, RectPx, Rgba, ShapingPlan, SizePx, SubtitleStyle};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleContent {
    pub speaker: Option<String>,
    pub body: String,
    pub locale: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayoutMetrics {
    pub size_dp: SizePx,
    pub line_count: u16,
    pub baseline_dp: f32,
}

pub trait TextMeasurer {
    type Error;

    fn measure(
        &self,
        text: &str,
        font_size_dp: f32,
        line_height: f32,
        max_width_dp: f32,
        max_lines: u16,
        shaping: &ShapingPlan,
    ) -> Result<TextLayoutMetrics, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActorHeadAnchor {
    pub head_bounds_px: RectPx,
    pub confidence: f32,
    pub track_age_ms: u32,
    pub occluded: bool,
}

#[derive(Clone, Debug)]
pub struct LayoutRequest<'a> {
    pub viewport_px: RectPx,
    pub dpi_scale: DpiScale,
    pub content: &'a SubtitleContent,
    pub style: &'a SubtitleStyle,
    pub body_shaping: &'a ShapingPlan,
    pub speaker_shaping: Option<&'a ShapingPlan>,
    pub actor_head: Option<ActorHeadAnchor>,
    pub hud_exclusions_px: &'a [RectPx],
    pub occupied_subtitles_px: &'a [RectPx],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    HeadAbove,
    HeadBelow,
    HeadLeft,
    HeadRight,
    BottomCenter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    None,
    NoActor,
    UnreliableTrack,
    OccludedActor,
    OffscreenActor,
    HeadAnchoringDisabled,
    CollisionAvoidance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleLayout {
    pub bounds_px: RectPx,
    /// Bounds including outline, shadow blur/offset, and backplate border outsets.
    pub visual_bounds_px: RectPx,
    pub speaker_bounds_px: Option<RectPx>,
    pub body_bounds_px: RectPx,
    pub anchor: AnchorKind,
    pub fallback_reason: FallbackReason,
    pub anchor_point_px: PointPx,
    pub was_clamped: bool,
    pub collides: bool,
    pub overlap_area_px2: f32,
    pub body_metrics: TextLayoutMetrics,
    pub speaker_metrics: Option<TextLayoutMetrics>,
    pub max_text_width_px: f32,
    pub effects_px: ResolvedEffectsPx,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedEffectsPx {
    pub outline_enabled: bool,
    pub outline_width_px: f32,
    pub outline_color: Rgba,
    pub shadow_enabled: bool,
    pub shadow_offset_px: PointPx,
    pub shadow_blur_px: f32,
    pub shadow_color: Rgba,
    pub backplate_enabled: bool,
    pub backplate_blur_behind_px: f32,
    pub backplate_border_width_px: f32,
    pub backplate_fill: Rgba,
    pub backplate_border: Rgba,
    pub corner_radius_px: f32,
    pub visual_outsets_px: InsetsPx,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError<E> {
    #[error("viewport must be a finite positive physical-pixel rectangle")]
    InvalidViewport,
    #[error("subtitle body must not be empty")]
    EmptyBody,
    #[error("safe area is too small for subtitle layout")]
    SafeAreaTooSmall,
    #[error("speaker text requires a speaker shaping plan")]
    MissingSpeakerShaping,
    #[error("text measurer returned non-finite, empty, or out-of-contract metrics")]
    InvalidMeasurement,
    #[error("text measurement failed")]
    Measurement(E),
}

pub fn layout_subtitle<M: TextMeasurer>(
    request: &LayoutRequest<'_>,
    measurer: &M,
) -> Result<SubtitleLayout, LayoutError<M::Error>> {
    if !request.viewport_px.is_finite_positive() {
        return Err(LayoutError::InvalidViewport);
    }
    if request.content.body.trim().is_empty() {
        return Err(LayoutError::EmptyBody);
    }

    let style = request.style;
    let dpi = request.dpi_scale;
    let safe_margin = dpi.px(style.geometry.safe_margin_dp);
    let safe = request
        .viewport_px
        .inset(crate::InsetsPx::uniform(safe_margin));
    if !safe.is_finite_positive() {
        return Err(LayoutError::SafeAreaTooSmall);
    }
    let effects_px = resolve_effects(style, dpi);
    let box_safe = safe.inset(effects_px.visual_outsets_px);
    if !box_safe.is_finite_positive() {
        return Err(LayoutError::SafeAreaTooSmall);
    }
    let maximum_text_width_px = (request.viewport_px.width * style.geometry.max_width_fraction)
        .min(dpi.px(style.geometry.max_width_dp))
        .min((safe.width - dpi.px(style.geometry.padding_x_dp) * 2.0).max(1.0));
    let maximum_text_width_dp = dpi.dp(maximum_text_width_px);
    let body_metrics = measurer
        .measure(
            &request.content.body,
            style.typography.body_size_dp,
            style.typography.line_height,
            maximum_text_width_dp,
            style.typography.max_body_lines,
            request.body_shaping,
        )
        .map_err(LayoutError::Measurement)?;
    validate_metrics(body_metrics, style.typography.max_body_lines)
        .then_some(())
        .ok_or(LayoutError::InvalidMeasurement)?;
    if request
        .content
        .speaker
        .as_deref()
        .is_some_and(|speaker| !speaker.trim().is_empty())
        && request.speaker_shaping.is_none()
    {
        return Err(LayoutError::MissingSpeakerShaping);
    }
    let speaker_metrics = match (request.content.speaker.as_deref(), request.speaker_shaping) {
        (Some(speaker), Some(shaping)) if !speaker.trim().is_empty() => {
            let metrics = measurer
                .measure(
                    speaker,
                    style.typography.speaker_size_dp,
                    1.0,
                    maximum_text_width_dp,
                    1,
                    shaping,
                )
                .map_err(LayoutError::Measurement)?;
            validate_metrics(metrics, 1)
                .then_some(())
                .ok_or(LayoutError::InvalidMeasurement)?;
            Some(metrics)
        }
        _ => None,
    };

    let padding_x = dpi.px(style.geometry.padding_x_dp);
    let padding_y = dpi.px(style.geometry.padding_y_dp);
    let speaker_gap = speaker_metrics
        .map(|_| dpi.px(style.geometry.speaker_gap_dp))
        .unwrap_or(0.0);
    let content_width = dpi
        .px(body_metrics.size_dp.width)
        .max(speaker_metrics.map_or(0.0, |metrics| dpi.px(metrics.size_dp.width)));
    let minimum_width = dpi.px(style.geometry.min_width_dp).min(box_safe.width);
    let box_size = SizePx::new(
        (content_width + padding_x * 2.0)
            .max(minimum_width)
            .min(box_safe.width),
        (dpi.px(body_metrics.size_dp.height)
            + speaker_metrics.map_or(0.0, |metrics| dpi.px(metrics.size_dp.height))
            + speaker_gap
            + padding_y * 2.0)
            .min(box_safe.height),
    );

    let (fallback_reason, reliable_head) = classify_head(request);
    let mut candidates = Vec::new();
    if let Some(head) = reliable_head {
        let gap = dpi.px(style.geometry.head_gap_dp);
        let center = head.head_bounds_px.center();
        candidates.extend([
            Candidate::new(
                AnchorKind::HeadAbove,
                center,
                RectPx::new(
                    center.x - box_size.width * 0.5,
                    head.head_bounds_px.y - gap - box_size.height,
                    box_size.width,
                    box_size.height,
                ),
                0.0,
            ),
            Candidate::new(
                AnchorKind::HeadBelow,
                center,
                RectPx::new(
                    center.x - box_size.width * 0.5,
                    head.head_bounds_px.bottom() + gap,
                    box_size.width,
                    box_size.height,
                ),
                1.0,
            ),
            Candidate::new(
                AnchorKind::HeadLeft,
                center,
                RectPx::new(
                    head.head_bounds_px.x - gap - box_size.width,
                    center.y - box_size.height * 0.5,
                    box_size.width,
                    box_size.height,
                ),
                2.0,
            ),
            Candidate::new(
                AnchorKind::HeadRight,
                center,
                RectPx::new(
                    head.head_bounds_px.right() + gap,
                    center.y - box_size.height * 0.5,
                    box_size.width,
                    box_size.height,
                ),
                3.0,
            ),
        ]);
    }
    add_bottom_candidates(&mut candidates, request, box_safe, box_size);

    let exclusions = request
        .hud_exclusions_px
        .iter()
        .chain(request.occupied_subtitles_px.iter())
        .copied()
        .filter(|rect| rect.is_finite_positive())
        .collect::<Vec<_>>();
    let selected = candidates
        .into_iter()
        .map(|candidate| {
            score_candidate(
                candidate,
                box_safe,
                effects_px.visual_outsets_px,
                &exclusions,
            )
        })
        .min_by(|left, right| left.score.total_cmp(&right.score))
        .ok_or(LayoutError::SafeAreaTooSmall)?;
    let bounds = selected.bounds_px;
    let speaker_height = speaker_metrics.map_or(0.0, |metrics| dpi.px(metrics.size_dp.height));
    let speaker_bounds = speaker_metrics.map(|metrics| {
        RectPx::new(
            bounds.x + padding_x,
            bounds.y + padding_y,
            dpi.px(metrics.size_dp.width)
                .min(bounds.width - padding_x * 2.0),
            speaker_height,
        )
    });
    let body_bounds = RectPx::new(
        bounds.x + padding_x,
        bounds.y + padding_y + speaker_height + speaker_gap,
        dpi.px(body_metrics.size_dp.width)
            .min((bounds.width - padding_x * 2.0).max(0.0)),
        dpi.px(body_metrics.size_dp.height)
            .min((bounds.height - padding_y * 2.0 - speaker_height - speaker_gap).max(0.0)),
    );

    Ok(SubtitleLayout {
        bounds_px: bounds,
        visual_bounds_px: bounds.outset(effects_px.visual_outsets_px),
        speaker_bounds_px: speaker_bounds,
        body_bounds_px: body_bounds,
        anchor: selected.anchor,
        fallback_reason: if selected.anchor == AnchorKind::BottomCenter {
            if reliable_head.is_some() {
                FallbackReason::CollisionAvoidance
            } else {
                fallback_reason
            }
        } else {
            FallbackReason::None
        },
        anchor_point_px: selected.anchor_point_px,
        was_clamped: selected.was_clamped,
        collides: selected.overlap_area_px2 > 0.5,
        overlap_area_px2: selected.overlap_area_px2,
        body_metrics,
        speaker_metrics,
        max_text_width_px: maximum_text_width_px,
        effects_px,
    })
}

fn classify_head(request: &LayoutRequest<'_>) -> (FallbackReason, Option<ActorHeadAnchor>) {
    if !request.style.behavior.prefer_head_anchor {
        return (FallbackReason::HeadAnchoringDisabled, None);
    }
    let Some(head) = request.actor_head else {
        return (FallbackReason::NoActor, None);
    };
    if head.occluded {
        return (FallbackReason::OccludedActor, None);
    }
    if !head.confidence.is_finite()
        || head.confidence < request.style.behavior.head_confidence_threshold
        || head.track_age_ms > request.style.behavior.max_track_age_ms
    {
        return (FallbackReason::UnreliableTrack, None);
    }
    if !head.head_bounds_px.is_finite_positive()
        || !head.head_bounds_px.intersects(request.viewport_px)
        || !head.head_bounds_px.center().x.is_finite()
        || !head.head_bounds_px.center().y.is_finite()
    {
        return (FallbackReason::OffscreenActor, None);
    }
    (FallbackReason::None, Some(head))
}

fn validate_metrics(metrics: TextLayoutMetrics, max_lines: u16) -> bool {
    metrics.size_dp.width.is_finite()
        && metrics.size_dp.height.is_finite()
        && metrics.size_dp.width > 0.0
        && metrics.size_dp.height > 0.0
        && metrics.baseline_dp.is_finite()
        && metrics.baseline_dp >= 0.0
        && metrics.baseline_dp <= metrics.size_dp.height
        && metrics.line_count > 0
        && metrics.line_count <= max_lines.max(1)
}

fn resolve_effects(style: &SubtitleStyle, dpi: DpiScale) -> ResolvedEffectsPx {
    let outline = if style.effects.outline.enabled {
        dpi.px(style.effects.outline.width_dp)
    } else {
        0.0
    };
    let shadow_blur = if style.effects.shadow.enabled {
        dpi.px(style.effects.shadow.blur_dp)
    } else {
        0.0
    };
    let shadow_x = if style.effects.shadow.enabled {
        dpi.px(style.effects.shadow.offset_x_dp)
    } else {
        0.0
    };
    let shadow_y = if style.effects.shadow.enabled {
        dpi.px(style.effects.shadow.offset_y_dp)
    } else {
        0.0
    };
    let border = if style.effects.backplate.enabled {
        dpi.px(style.effects.backplate.border_width_dp)
    } else {
        0.0
    };
    let base = outline.max(border);
    let visual_outsets_px = InsetsPx {
        top: base.max(shadow_blur + (-shadow_y).max(0.0)),
        right: base.max(shadow_blur + shadow_x.max(0.0)),
        bottom: base.max(shadow_blur + shadow_y.max(0.0)),
        left: base.max(shadow_blur + (-shadow_x).max(0.0)),
    };
    ResolvedEffectsPx {
        outline_enabled: style.effects.outline.enabled,
        outline_width_px: outline,
        outline_color: style.effects.outline.color,
        shadow_enabled: style.effects.shadow.enabled,
        shadow_offset_px: PointPx::new(shadow_x, shadow_y),
        shadow_blur_px: shadow_blur,
        shadow_color: style.effects.shadow.color,
        backplate_enabled: style.effects.backplate.enabled,
        backplate_blur_behind_px: dpi.px(style.effects.backplate.blur_behind_dp),
        backplate_border_width_px: border,
        backplate_fill: style.effects.backplate.fill,
        backplate_border: style.effects.backplate.border,
        corner_radius_px: dpi.px(style.geometry.corner_radius_dp),
        visual_outsets_px,
    }
}

fn add_bottom_candidates(
    candidates: &mut Vec<Candidate>,
    request: &LayoutRequest<'_>,
    safe: RectPx,
    box_size: SizePx,
) {
    let bottom = safe.bottom()
        - request
            .dpi_scale
            .px(request.style.geometry.fallback_bottom_dp);
    let step = box_size.height
        + request
            .dpi_scale
            .px(request.style.geometry.candidate_gap_dp);
    let count = request.style.behavior.collision_search_steps.max(1);
    for index in 0..count {
        let y = bottom - box_size.height - step * f32::from(index);
        for (horizontal, penalty) in [(0.5, 0.0), (0.25, 0.25), (0.75, 0.5)] {
            let center_x = safe.x + safe.width * horizontal;
            candidates.push(Candidate::new(
                AnchorKind::BottomCenter,
                PointPx::new(center_x, bottom),
                RectPx::new(
                    center_x - box_size.width * 0.5,
                    y,
                    box_size.width,
                    box_size.height,
                ),
                10.0 + f32::from(index) + penalty,
            ));
        }
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    anchor: AnchorKind,
    anchor_point_px: PointPx,
    ideal_bounds_px: RectPx,
    preference_penalty: f32,
}

impl Candidate {
    const fn new(
        anchor: AnchorKind,
        anchor_point_px: PointPx,
        ideal_bounds_px: RectPx,
        preference_penalty: f32,
    ) -> Self {
        Self {
            anchor,
            anchor_point_px,
            ideal_bounds_px,
            preference_penalty,
        }
    }
}

struct ScoredCandidate {
    anchor: AnchorKind,
    anchor_point_px: PointPx,
    bounds_px: RectPx,
    was_clamped: bool,
    overlap_area_px2: f32,
    score: f32,
}

fn score_candidate(
    candidate: Candidate,
    safe: RectPx,
    visual_outsets: InsetsPx,
    exclusions: &[RectPx],
) -> ScoredCandidate {
    let clamped = candidate.ideal_bounds_px.clamp_inside(safe);
    let displacement = (clamped.x - candidate.ideal_bounds_px.x).abs()
        + (clamped.y - candidate.ideal_bounds_px.y).abs();
    let overlap_area = exclusions
        .iter()
        .map(|exclusion| clamped.outset(visual_outsets).intersection_area(*exclusion))
        .sum::<f32>();
    // A single physical pixel of HUD overlap is more expensive than a full-monitor move.
    let score = overlap_area * 10_000.0 + displacement + candidate.preference_penalty;
    ScoredCandidate {
        anchor: candidate.anchor,
        anchor_point_px: candidate.anchor_point_px,
        bounds_px: clamped,
        was_clamped: clamped != candidate.ideal_bounds_px,
        overlap_area_px2: overlap_area,
        score,
    }
}
