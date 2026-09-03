use std::convert::Infallible;

use npc_subtitle_engine::{
    layout_subtitle, plan_shaping, sample_animation, ActorHeadAnchor, AnchorKind, AnimationPhase,
    CueTiming, DpiScale, FallbackReason, FontCatalog, HdrMetadataCompatibility, HdrSafeMetadata,
    LayoutRequest, PointPx, RectPx, ShapingPlan, SizePx, StyleManifest, StylePatch,
    SubtitleContent, SubtitleOverrides, TextDirection, TextLayoutMetrics, TextMeasurer,
    ToneMapPolicy, ASSET_LICENSES_JSON, DEFAULT_STYLES_JSON, FONT_CATALOG_JSON, STYLE_SCHEMA_JSON,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use serde_json::Value;

#[derive(Clone, Copy)]
struct DeterministicMeasurer;

impl TextMeasurer for DeterministicMeasurer {
    type Error = Infallible;

    fn measure(
        &self,
        text: &str,
        font_size_dp: f32,
        line_height: f32,
        max_width_dp: f32,
        max_lines: u16,
        shaping: &ShapingPlan,
    ) -> Result<TextLayoutMetrics, Self::Error> {
        let direction_factor = if shaping.direction == TextDirection::RightToLeft {
            0.61
        } else {
            0.56
        };
        let unwrapped = text.chars().count() as f32 * font_size_dp * direction_factor;
        let line_count =
            ((unwrapped / max_width_dp.max(1.0)).ceil() as u16).clamp(1, max_lines.max(1));
        Ok(TextLayoutMetrics {
            size_dp: SizePx::new(
                unwrapped.min(max_width_dp),
                font_size_dp * line_height * f32::from(line_count),
            ),
            line_count,
            baseline_dp: font_size_dp * 0.82,
        })
    }
}

fn manifests() -> (StyleManifest, FontCatalog) {
    (
        serde_json::from_str(DEFAULT_STYLES_JSON).expect("style fixture must parse"),
        serde_json::from_str(FONT_CATALOG_JSON).expect("font fixture must parse"),
    )
}

fn content() -> SubtitleContent {
    SubtitleContent {
        speaker: Some("Captain Imani".to_owned()),
        body: "The north gate is watched. Take the river path before dawn.".to_owned(),
        locale: Some("en-US".to_owned()),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_layout(
    viewport: RectPx,
    dpi: f32,
    actor_head: Option<ActorHeadAnchor>,
    hud: &[RectPx],
    occupied: &[RectPx],
) -> npc_subtitle_engine::SubtitleLayout {
    let (styles, fonts) = manifests();
    styles.validate().expect("default styles must validate");
    let style = styles
        .style(&styles.default_style_id)
        .expect("default style must exist");
    let content = content();
    let body = plan_shaping(
        &content.body,
        content.locale.as_deref(),
        &style.typography.body_font_role,
        &fonts,
    )
    .expect("body shaping plan");
    let speaker = plan_shaping(
        content.speaker.as_deref().expect("speaker"),
        content.locale.as_deref(),
        &style.typography.speaker_font_role,
        &fonts,
    )
    .expect("speaker shaping plan");
    layout_subtitle(
        &LayoutRequest {
            viewport_px: viewport,
            dpi_scale: DpiScale::new(dpi).expect("valid test DPI"),
            content: &content,
            style,
            body_shaping: &body,
            speaker_shaping: Some(&speaker),
            actor_head,
            hud_exclusions_px: hud,
            occupied_subtitles_px: occupied,
        },
        &DeterministicMeasurer,
    )
    .expect("layout succeeds")
}

#[test]
fn bundled_manifests_are_valid_and_do_not_bundle_font_bytes() {
    let (styles, fonts) = manifests();
    styles.validate().expect("style manifest validates");
    fonts.validate().expect("font manifest validates");
    assert_eq!(fonts.schema_version, 1);
    assert!(fonts.roles.len() >= 2);
    assert!(fonts
        .roles
        .iter()
        .all(|role| !role.system_families.is_empty()));
    assert!(fonts
        .roles
        .iter()
        .flat_map(|role| &role.system_families)
        .all(|font| !font.redistribute));

    let licenses: Value = serde_json::from_str(ASSET_LICENSES_JSON).expect("license JSON parses");
    assert_eq!(licenses["schema_version"], 1);
    assert!(licenses["font_license_references"]
        .as_array()
        .expect("font licenses array")
        .iter()
        .all(|entry| entry["binary_bundled"] == false));
}

#[test]
fn hdr_metadata_has_one_canonical_renderer_neutral_value_and_normalizes_legacy_aliases() {
    let (styles, _) = manifests();
    for style in &styles.styles {
        assert_eq!(style.colors.hdr.tone_map, ToneMapPolicy::NoneInRenderer);
        assert_eq!(
            style.colors.hdr.compatibility(),
            HdrMetadataCompatibility::Canonical
        );
        assert!(!style.colors.hdr.custom_pq_or_scrgb_shader);
    }

    let schema: Value = serde_json::from_str(STYLE_SCHEMA_JSON).expect("style schema parses");
    assert_eq!(
        schema["$defs"]["hdr"]["properties"]["tone_map"]["const"],
        "none_in_renderer"
    );
    assert_eq!(
        schema["$defs"]["hdr"]["properties"]["custom_pq_or_scrgb_shader"]["const"],
        false
    );

    for legacy_key in ["tone_map", "hdr_treatment"] {
        for legacy_value in ["clamp_to_subtitle_peak", "relative_to_reference_white"] {
            let legacy: HdrSafeMetadata = serde_json::from_value(serde_json::json!({
                "target_spaces": ["srgb", "scrgb", "rec2020_pq"],
                (legacy_key): legacy_value,
                "reference_white_nits": 203.0,
                "max_subtitle_nits": 300.0,
                "preserve_alpha_linear": true
            }))
            .expect("legacy HDR metadata normalizes");
            assert_eq!(legacy.tone_map, ToneMapPolicy::NoneInRenderer);
            assert_eq!(
                legacy.compatibility(),
                HdrMetadataCompatibility::LegacyNormalized
            );
            assert_eq!(
                serde_json::to_value(&legacy).expect("normalized HDR serialization")["tone_map"],
                "none_in_renderer"
            );

            let mut compatible_style = styles.styles[0].clone();
            compatible_style.colors.hdr = legacy;
            compatible_style
                .validate()
                .expect("complete legacy nit metadata remains valid after normalization");
        }
    }

    let mut incomplete_legacy = styles.styles[0].clone();
    incomplete_legacy.colors.hdr.reference_white_nits = Some(203.0);
    incomplete_legacy.colors.hdr.max_subtitle_nits = None;
    assert!(incomplete_legacy.validate().is_err());
}

#[test]
fn reliable_head_uses_safe_head_anchor_and_lays_out_speaker_label() {
    let viewport = RectPx::new(0.0, 0.0, 1920.0, 1080.0);
    let head = ActorHeadAnchor {
        head_bounds_px: RectPx::new(880.0, 390.0, 160.0, 190.0),
        confidence: 0.96,
        track_age_ms: 16,
        occluded: false,
    };
    let result = run_layout(viewport, 1.0, Some(head), &[], &[]);
    assert_eq!(result.anchor, AnchorKind::HeadAbove);
    assert_eq!(result.fallback_reason, FallbackReason::None);
    assert!(result.speaker_bounds_px.is_some());
    assert!(result.bounds_px.bottom() < head.head_bounds_px.y);
    assert!(result.bounds_px.contains(result.body_bounds_px));
    assert!(result.visual_bounds_px.width > result.bounds_px.width);
    assert!(result.effects_px.outline_enabled);
    assert!(result.effects_px.shadow_enabled);
    assert!(result.effects_px.backplate_enabled);
}

#[test]
fn hud_exclusion_forces_an_alternate_collision_free_candidate() {
    let viewport = RectPx::new(0.0, 0.0, 1920.0, 1080.0);
    let head = ActorHeadAnchor {
        head_bounds_px: RectPx::new(860.0, 430.0, 180.0, 200.0),
        confidence: 0.95,
        track_age_ms: 12,
        occluded: false,
    };
    let block_above = RectPx::new(500.0, 120.0, 920.0, 310.0);
    let result = run_layout(viewport, 1.0, Some(head), &[block_above], &[]);
    assert_ne!(result.anchor, AnchorKind::HeadAbove);
    assert!(!result.collides);
    assert_eq!(result.overlap_area_px2, 0.0);
}

#[test]
fn collision_can_demote_a_reliable_head_to_bottom_fallback_with_provenance() {
    let viewport = RectPx::new(0.0, 0.0, 1920.0, 1080.0);
    let head = ActorHeadAnchor {
        head_bounds_px: RectPx::new(860.0, 430.0, 180.0, 200.0),
        confidence: 0.99,
        track_age_ms: 8,
        occluded: false,
    };
    let gameplay_hud = RectPx::new(100.0, 40.0, 1720.0, 790.0);
    let result = run_layout(viewport, 1.0, Some(head), &[gameplay_hud], &[]);
    assert_eq!(result.anchor, AnchorKind::BottomCenter);
    assert_eq!(result.fallback_reason, FallbackReason::CollisionAvoidance);
    assert!(!result.visual_bounds_px.intersects(gameplay_hud));
}

#[test]
fn identical_inputs_produce_byte_equivalent_serialized_layouts() {
    let viewport = RectPx::new(-3440.0, 120.0, 3440.0, 1440.0);
    let head = ActorHeadAnchor {
        head_bounds_px: RectPx::new(-1700.0, 510.0, 130.0, 160.0),
        confidence: 0.92,
        track_age_ms: 24,
        occluded: false,
    };
    let hud = [RectPx::new(-3400.0, 1250.0, 520.0, 250.0)];
    let first = run_layout(viewport, 2.0, Some(head), &hud, &[]);
    let second = run_layout(viewport, 2.0, Some(head), &hud, &[]);
    assert_eq!(
        serde_json::to_vec(&first).expect("serialize first layout"),
        serde_json::to_vec(&second).expect("serialize second layout")
    );
}

#[test]
fn unreliable_occluded_and_offscreen_heads_fall_back_with_reason() {
    let viewport = RectPx::new(0.0, 0.0, 1920.0, 1080.0);
    let low_confidence = ActorHeadAnchor {
        head_bounds_px: RectPx::new(800.0, 300.0, 120.0, 160.0),
        confidence: 0.2,
        track_age_ms: 10,
        occluded: false,
    };
    let low = run_layout(viewport, 1.0, Some(low_confidence), &[], &[]);
    assert_eq!(low.anchor, AnchorKind::BottomCenter);
    assert_eq!(low.fallback_reason, FallbackReason::UnreliableTrack);

    let occluded = run_layout(
        viewport,
        1.0,
        Some(ActorHeadAnchor {
            confidence: 1.0,
            occluded: true,
            ..low_confidence
        }),
        &[],
        &[],
    );
    assert_eq!(occluded.fallback_reason, FallbackReason::OccludedActor);

    let offscreen = run_layout(
        viewport,
        1.0,
        Some(ActorHeadAnchor {
            head_bounds_px: RectPx::new(5000.0, -3000.0, 100.0, 100.0),
            confidence: 1.0,
            occluded: false,
            ..low_confidence
        }),
        &[],
        &[],
    );
    assert_eq!(offscreen.anchor, AnchorKind::BottomCenter);
    assert_eq!(offscreen.fallback_reason, FallbackReason::OffscreenActor);
}

#[test]
fn negative_desktop_origin_is_preserved_and_clamped_in_that_coordinate_space() {
    let viewport = RectPx::new(-2560.0, -180.0, 2560.0, 1440.0);
    let result = run_layout(viewport, 1.5, None, &[], &[]);
    let safe = viewport.inset(npc_subtitle_engine::InsetsPx::uniform(48.0));
    assert!(result.bounds_px.x < 0.0);
    assert!(safe.contains(result.bounds_px));
    assert_eq!(result.anchor, AnchorKind::BottomCenter);
    assert_eq!(result.fallback_reason, FallbackReason::NoActor);
}

#[test]
fn ultrawide_layout_keeps_a_readable_bounded_measure() {
    let viewport = RectPx::new(1920.0, 0.0, 5120.0, 1440.0);
    let result = run_layout(viewport, 1.0, None, &[], &[]);
    assert!(result.max_text_width_px <= 760.0);
    assert!(result.bounds_px.width < viewport.width * 0.25);
    assert!(result.bounds_px.x >= viewport.x);
    assert!(result.bounds_px.right() <= viewport.right());
}

#[test]
fn physical_geometry_scales_at_100_150_and_200_percent() {
    let logical_viewport = RectPx::new(-300.0, 80.0, 1600.0, 900.0);
    let logical_head = RectPx::new(680.0, 360.0, 120.0, 150.0);
    let baseline = run_layout(
        logical_viewport,
        1.0,
        Some(ActorHeadAnchor {
            head_bounds_px: logical_head,
            confidence: 0.99,
            track_age_ms: 8,
            occluded: false,
        }),
        &[],
        &[],
    );
    for scale in [1.5_f32, 2.0] {
        let scaled_viewport = scale_rect(logical_viewport, scale);
        let scaled = run_layout(
            scaled_viewport,
            scale,
            Some(ActorHeadAnchor {
                head_bounds_px: scale_rect(logical_head, scale),
                confidence: 0.99,
                track_age_ms: 8,
                occluded: false,
            }),
            &[],
            &[],
        );
        assert_rect_approximately(scale_rect(baseline.bounds_px, scale), scaled.bounds_px);
        assert_rect_approximately(
            scale_rect(baseline.body_bounds_px, scale),
            scaled.body_bounds_px,
        );
    }
}

#[test]
fn game_then_character_overrides_have_deterministic_precedence() {
    let (styles, _) = manifests();
    let base = styles.style(&styles.default_style_id).expect("base style");
    let mut overrides = SubtitleOverrides::default();
    overrides.games.insert(
        "sample-game".to_owned(),
        StylePatch {
            body_size_dp: Some(25.0),
            backplate_enabled: Some(false),
            ..StylePatch::default()
        },
    );
    overrides.characters.insert(
        "captain-imani".to_owned(),
        StylePatch {
            body_size_dp: Some(29.0),
            speaker_font_role: Some("subtitle_sans".to_owned()),
            ..StylePatch::default()
        },
    );
    let resolved = overrides
        .resolve(base, Some("sample-game"), Some("captain-imani"))
        .expect("resolved style validates");
    assert_eq!(resolved.typography.body_size_dp, 29.0);
    assert_eq!(resolved.typography.speaker_font_role, "subtitle_sans");
    assert!(!resolved.effects.backplate.enabled);
}

#[test]
fn rtl_and_non_latin_plans_request_bidi_complex_shaping_and_fallbacks() {
    let (_, fonts) = manifests();
    let arabic =
        plan_shaping("مرحباً بالعالم", Some("ar"), "subtitle_sans", &fonts).expect("Arabic plan");
    assert_eq!(arabic.direction, TextDirection::RightToLeft);
    assert!(arabic.require_bidi_reordering);
    assert!(arabic.require_complex_shaping);
    assert!(arabic
        .family_fallback_chain
        .iter()
        .any(|family| family == "Segoe UI" || family == "Noto Sans"));

    let japanese = plan_shaping("夜明け前に川へ", Some("ja-JP"), "subtitle_sans", &fonts)
        .expect("Japanese plan");
    assert_eq!(japanese.direction, TextDirection::LeftToRight);
    assert!(japanese
        .family_fallback_chain
        .iter()
        .any(|family| family == "Yu Gothic UI" || family == "Noto Sans"));
    assert!(japanese.preserve_grapheme_clusters);
}

#[test]
fn animation_has_enter_hold_exit_reveal_and_reduced_motion_paths() {
    let (styles, _) = manifests();
    let animation = &styles
        .style(&styles.default_style_id)
        .expect("style")
        .animation;
    let cue = CueTiming {
        start_ms: 1_000,
        end_ms: 3_000,
        grapheme_count: 40,
    };
    let entering = sample_animation(
        1_080,
        cue,
        animation,
        DpiScale::new(1.5).expect("DPI"),
        false,
    );
    assert_eq!(entering.phase, AnimationPhase::Entering);
    assert!(entering.opacity > 0.0 && entering.opacity < 1.0);
    assert!(entering.scale < 1.0);
    assert!(entering.translate_y_px > 0.0);
    assert!(entering.reveal_fraction < 1.0);

    let exiting = sample_animation(
        3_050,
        cue,
        animation,
        DpiScale::new(1.0).expect("DPI"),
        false,
    );
    assert_eq!(exiting.phase, AnimationPhase::Exiting);
    assert!(exiting.opacity < 1.0);

    let reduced = sample_animation(
        1_001,
        cue,
        animation,
        DpiScale::new(2.0).expect("DPI"),
        true,
    );
    assert_eq!(reduced.phase, AnimationPhase::Holding);
    assert_eq!(reduced.opacity, 1.0);
    assert_eq!(reduced.scale, 1.0);
    assert_eq!(reduced.translate_y_px, 0.0);
    assert_eq!(reduced.reveal_fraction, 1.0);
}

proptest! {
    #[test]
    fn layout_is_finite_and_inside_safe_area_for_supported_dpi_and_desktop_origins(
        origin_x in -8000.0_f32..8000.0,
        origin_y in -4000.0_f32..4000.0,
        width in 900.0_f32..8000.0,
        height in 600.0_f32..3000.0,
        dpi_index in 0_usize..3,
        head_x_ratio in -0.5_f32..1.5,
        head_y_ratio in -0.5_f32..1.5,
        confidence in 0.0_f32..1.0,
    ) {
        let dpi = [1.0_f32, 1.5, 2.0][dpi_index];
        let viewport = RectPx::new(origin_x, origin_y, width, height);
        let head = ActorHeadAnchor {
            head_bounds_px: RectPx::new(
                origin_x + width * head_x_ratio,
                origin_y + height * head_y_ratio,
                80.0 * dpi,
                110.0 * dpi,
            ),
            confidence,
            track_age_ms: 16,
            occluded: false,
        };
        let result = run_layout(viewport, dpi, Some(head), &[], &[]);
        let safe = viewport.inset(npc_subtitle_engine::InsetsPx::uniform(32.0 * dpi));
        prop_assert!(result.bounds_px.is_finite_positive());
        prop_assert!(safe.contains(result.bounds_px));
        prop_assert!(safe.contains(result.visual_bounds_px));
        prop_assert!(result.bounds_px.contains(result.body_bounds_px));
        prop_assert!(result.max_text_width_px > 0.0);
    }

    #[test]
    fn rect_clamping_handles_negative_origins(
        container_x in -10000.0_f32..0.0,
        container_y in -5000.0_f32..1000.0,
        width in 100.0_f32..5000.0,
        height in 100.0_f32..3000.0,
        rect_x in -15000.0_f32..5000.0,
        rect_y in -10000.0_f32..5000.0,
        rect_width in 1.0_f32..6000.0,
        rect_height in 1.0_f32..4000.0,
    ) {
        let container = RectPx::new(container_x, container_y, width, height);
        let result = RectPx::new(rect_x, rect_y, rect_width, rect_height).clamp_inside(container);
        prop_assert!(container.contains(result));
        prop_assert!(result.is_finite_positive());
    }
}

fn scale_rect(rect: RectPx, scale: f32) -> RectPx {
    RectPx::new(
        rect.x * scale,
        rect.y * scale,
        rect.width * scale,
        rect.height * scale,
    )
}

fn assert_rect_approximately(expected: RectPx, actual: RectPx) {
    for (expected, actual) in [
        (expected.x, actual.x),
        (expected.y, actual.y),
        (expected.width, actual.width),
        (expected.height, actual.height),
    ] {
        assert!(
            (expected - actual).abs() < 0.05,
            "expected {expected}, got {actual}"
        );
    }
}

#[test]
fn anchor_point_is_reported_in_physical_desktop_pixels() {
    let viewport = RectPx::new(-1920.0, 0.0, 1920.0, 1080.0);
    let head = RectPx::new(-1100.0, 350.0, 100.0, 140.0);
    let result = run_layout(
        viewport,
        1.0,
        Some(ActorHeadAnchor {
            head_bounds_px: head,
            confidence: 1.0,
            track_age_ms: 1,
            occluded: false,
        }),
        &[],
        &[],
    );
    assert_eq!(result.anchor_point_px, PointPx::new(-1050.0, 420.0));
}
