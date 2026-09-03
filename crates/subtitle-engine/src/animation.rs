use serde::{Deserialize, Serialize};

use crate::{AnimationStyle, DpiScale};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationPhase {
    Hidden,
    Entering,
    Holding,
    Exiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationSample {
    pub phase: AnimationPhase,
    pub opacity: f32,
    pub scale: f32,
    pub translate_y_px: f32,
    pub reveal_fraction: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CueTiming {
    pub start_ms: u64,
    pub end_ms: u64,
    pub grapheme_count: u32,
}

#[must_use]
pub fn sample_animation(
    now_ms: u64,
    cue: CueTiming,
    style: &AnimationStyle,
    dpi: DpiScale,
    reduced_motion: bool,
) -> AnimationSample {
    if now_ms < cue.start_ms || now_ms >= cue.end_ms.saturating_add(u64::from(style.exit_ms)) {
        return AnimationSample {
            phase: AnimationPhase::Hidden,
            opacity: 0.0,
            scale: 1.0,
            translate_y_px: 0.0,
            reveal_fraction: 0.0,
        };
    }
    let motion_disabled = reduced_motion || !style.enabled;
    let enter_end = cue.start_ms.saturating_add(u64::from(style.enter_ms));
    let phase = if !motion_disabled && style.enter_ms > 0 && now_ms < enter_end {
        AnimationPhase::Entering
    } else if !motion_disabled && style.exit_ms > 0 && now_ms >= cue.end_ms {
        AnimationPhase::Exiting
    } else {
        AnimationPhase::Holding
    };
    let opacity = match phase {
        AnimationPhase::Hidden => 0.0,
        AnimationPhase::Entering => smoothstep(unit_progress(
            now_ms,
            cue.start_ms,
            u64::from(style.enter_ms),
        )),
        AnimationPhase::Holding => 1.0,
        AnimationPhase::Exiting => {
            1.0 - smoothstep(unit_progress(now_ms, cue.end_ms, u64::from(style.exit_ms)))
        }
    };
    let enter_progress = if motion_disabled {
        1.0
    } else {
        smoothstep(unit_progress(
            now_ms,
            cue.start_ms,
            u64::from(style.enter_ms),
        ))
    };
    let reveal_start = cue
        .start_ms
        .saturating_add(u64::from(style.reveal_delay_ms));
    let reveal_duration =
        u64::from(style.reveal_ms_per_grapheme).saturating_mul(u64::from(cue.grapheme_count));
    let reveal_fraction = if motion_disabled || reveal_duration == 0 {
        1.0
    } else {
        unit_progress(now_ms, reveal_start, reveal_duration)
    };

    AnimationSample {
        phase,
        opacity,
        scale: if motion_disabled {
            1.0
        } else {
            style.initial_scale + (1.0 - style.initial_scale) * enter_progress
        },
        translate_y_px: if motion_disabled {
            0.0
        } else {
            dpi.px(style.initial_offset_y_dp) * (1.0 - enter_progress)
        },
        reveal_fraction,
    }
}

fn unit_progress(now_ms: u64, start_ms: u64, duration_ms: u64) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    let elapsed = now_ms.saturating_sub(start_ms).min(duration_ms);
    elapsed as f32 / duration_ms as f32
}

fn smoothstep(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}
