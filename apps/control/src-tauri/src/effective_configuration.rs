//! Read-only projection of settings already owned by native managers.
//!
//! This module deliberately has no save/reset/activate API. It gives the UI a
//! single explainable view while mutation authority remains with the product
//! preferences, provider loadout, and local resource managers.

use crate::local_resources::{
    ExperimentalPackPhase, LocalResourceManager, LocalResourceSettings,
    LocalResourceSettingsPersistenceV1,
};
use crate::product_preferences::{
    default_effective_product_preferences, EffectiveEgressPolicyV1, EffectiveValueV1,
    ExecutionPresetV1, PreferenceMigrationStateV1, ProductPreferenceManager,
    ProductPreferenceScopeV1,
};
use crate::provider_loadouts::{
    starter_document, ProviderLoadoutManager, ProviderLoadoutPersistenceHealth,
};
use npc_provider_loadouts::{
    LoadoutContextV1, LoadoutScopeV1, ProviderLoadoutDocumentV1, ProviderModelRouteV1,
    ProviderRole, ResolvedProviderLoadoutV1, RoleOverrideV1, ValidationContextV1,
    ALL_PROVIDER_ROLES,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveConfigurationRequestV1 {
    pub scope: ProductPreferenceScopeV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfigurationSnapshotV1 {
    pub schema_version: u32,
    pub scope: ProductPreferenceScopeV1,
    pub preference_revision: u64,
    pub read_only: bool,
    pub mutation_authority_added: bool,
    pub automatic_provider_fallback: bool,
    pub entries: Vec<EffectiveConfigurationEntryV1>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveConfigurationCategoryV1 {
    Conversation,
    ProviderRoute,
    Memory,
    Presentation,
    Privacy,
    Performance,
    OptionalVisual,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveConfigurationOwnerV1 {
    ProductPreferences,
    ProviderLoadouts,
    LocalResources,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveConfigurationPersistenceSourceV1 {
    NativePersisted,
    MigratedOnboarding,
    RecoveredDefault,
    BuiltInDefault,
    FirstRunSeed,
    RecoveredLastGood,
    RecoveredSeed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfigurationPersistenceV1 {
    pub source: EffectiveConfigurationPersistenceSourceV1,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EffectiveConfigurationValueV1 {
    Boolean {
        value: bool,
    },
    Choice {
        value: String,
    },
    Integer {
        value: u64,
        unit: String,
    },
    Route {
        provider_id: String,
        model_id: String,
        voice_id: Option<String>,
        execution: String,
        egress: String,
        transmitted_data: Vec<String>,
        manual_fallback_count: usize,
        automatic_fallback: bool,
    },
    Disabled,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfigurationEntryV1 {
    pub key: String,
    pub category: EffectiveConfigurationCategoryV1,
    pub label: String,
    pub owner: EffectiveConfigurationOwnerV1,
    pub value: EffectiveConfigurationValueV1,
    pub default_value: EffectiveConfigurationValueV1,
    pub winning_scope: ProductPreferenceScopeV1,
    pub source_kind: String,
    pub persistence: EffectiveConfigurationPersistenceV1,
    pub explanation: String,
    pub runtime_consequence: String,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Error)]
pub enum EffectiveConfigurationError {
    #[error("effective configuration owner {owner} is unavailable: {detail}")]
    Owner { owner: &'static str, detail: String },
}

/// Builds a projection only. It performs no provider request, credential
/// lookup, pack activation, benchmark, or persistence mutation.
pub(crate) fn build_effective_configuration_snapshot(
    request: EffectiveConfigurationRequestV1,
    preferences: &ProductPreferenceManager,
    provider_loadouts: &ProviderLoadoutManager,
    local_resources: &LocalResourceManager,
) -> Result<EffectiveConfigurationSnapshotV1, EffectiveConfigurationError> {
    let preference = preferences
        .snapshot(
            request.scope.clone(),
            None,
            crate::product_preferences::ResourceAuthorityReferenceV1 {
                selection_id: None,
                admission_status: None,
                admission_receipt_present: false,
                exact_target_pid: None,
                activation_performed: false,
            },
        )
        .map_err(|error| owner_error("productPreferences", error))?;
    let defaults = default_effective_product_preferences(&request.scope)
        .map_err(|error| owner_error("productPreferences", error))?;
    let provider = provider_loadouts
        .snapshot()
        .map_err(|error| owner_error("providerLoadouts", error))?;
    let local_settings = local_resources
        .settings()
        .map_err(|error| owner_error("localResources", error))?;
    let local_persistence_source = local_resources
        .settings_persistence_source()
        .map_err(|error| owner_error("localResources", error))?;
    let selected = local_resources
        .selected_loadout_planner()
        .map_err(|error| owner_error("localResources", error))?;
    let visual_pack = local_resources
        .pack_state()
        .map_err(|error| owner_error("localResources", error))?;

    let product_persistence = product_persistence(
        preference.migration.state,
        preference.migration.detail.clone(),
    );
    let provider_persistence =
        provider_persistence(provider.persistence_health, provider.detail.clone());
    let local_persistence = local_persistence(local_persistence_source);
    let mut entries = Vec::new();

    macro_rules! product_choice {
        ($key:literal, $category:expr, $label:literal, $current:expr, $default:expr, $explanation:literal, $consequence:literal) => {{
            let current = &$current;
            let default = &$default;
            entries.push(product_entry(
                $key,
                $category,
                $label,
                choice(&current.value),
                choice(&default.value),
                current,
                product_persistence.clone(),
                $explanation,
                $consequence,
            ));
        }};
    }
    macro_rules! product_bool {
        ($key:literal, $category:expr, $label:literal, $current:expr, $default:expr, $explanation:literal, $consequence:literal) => {{
            let current = &$current;
            let default = &$default;
            entries.push(product_entry(
                $key,
                $category,
                $label,
                EffectiveConfigurationValueV1::Boolean {
                    value: current.value,
                },
                EffectiveConfigurationValueV1::Boolean {
                    value: default.value,
                },
                current,
                product_persistence.clone(),
                $explanation,
                $consequence,
            ));
        }};
    }

    product_choice!(
        "conversation.executionPreset",
        EffectiveConfigurationCategoryV1::Conversation,
        "Execution mode",
        preference.effective.execution_preset,
        defaults.execution_preset,
        "Selects Cloud/API-first, Hybrid, or Fully Local policy independently from quality.",
        "Controls whether provider-cloud egress is eligible; it never activates a route by itself."
    );
    product_choice!(
        "performance.preset",
        EffectiveConfigurationCategoryV1::Performance,
        "Performance preset",
        preference.effective.performance_preset,
        defaults.performance_preset,
        "Selects Competitive, Fast, Balanced, Immersive, Maximum Quality, or Custom defaults.",
        "Supplies conversation and feature defaults without changing provider-route authority."
    );
    product_choice!(
        "conversation.verbosity",
        EffectiveConfigurationCategoryV1::Conversation,
        "Verbosity",
        preference.effective.verbosity,
        defaults.verbosity,
        "Controls how compact NPC responses should be.",
        "The effective value is included in turn policy."
    );
    entries.push(product_entry(
        "conversation.creativity",
        EffectiveConfigurationCategoryV1::Conversation,
        "Creativity",
        integer(preference.effective.creativity.value.into(), "percent"),
        integer(defaults.creativity.value.into(), "percent"),
        &preference.effective.creativity,
        product_persistence.clone(),
        "Controls the requested creative latitude on a 0-100 scale.",
        "The effective value informs generation policy; adapters retain their own safe bounds.",
    ));
    product_choice!(
        "conversation.responseLength",
        EffectiveConfigurationCategoryV1::Conversation,
        "Response length",
        preference.effective.response_length,
        defaults.response_length,
        "Selects short, medium, or long response targets.",
        "The runtime uses this as a response-shaping target."
    );
    product_choice!(
        "conversation.interruption",
        EffectiveConfigurationCategoryV1::Conversation,
        "Interruption behavior",
        preference.effective.interruption_mode,
        defaults.interruption_mode,
        "Controls whether a new utterance interrupts immediately, after a sentence, or not at all.",
        "The selected policy governs turn interruption when the active input path supports it."
    );
    product_choice!(
        "input.mode",
        EffectiveConfigurationCategoryV1::Conversation,
        "Input mode",
        preference.effective.input_mode,
        defaults.input_mode,
        "Selects push-to-talk or voice-activity intent.",
        "This does not bypass native microphone admission or privacy gates."
    );
    product_bool!(
        "presentation.subtitlesEnabled",
        EffectiveConfigurationCategoryV1::Presentation,
        "Subtitles",
        preference.effective.subtitles,
        defaults.subtitles,
        "Controls whether subtitle presentation is requested.",
        "Actual presentation still requires a trusted native presentation context."
    );
    product_bool!(
        "presentation.overlayEnabled",
        EffectiveConfigurationCategoryV1::Presentation,
        "Game overlay",
        preference.effective.overlay,
        defaults.overlay,
        "Controls whether an in-game overlay is requested.",
        "Overlay delivery remains fail-closed when native capture or exclusion evidence is absent."
    );
    product_bool!(
        "memory.enabled",
        EffectiveConfigurationCategoryV1::Memory,
        "Character memory",
        preference.effective.memory,
        defaults.memory,
        "Controls user intent for scoped character memory.",
        "Memory still uses exact game/profile/character/session namespaces and receipt-backed commits."
    );
    product_bool!(
        "privacy.webcamPresenceEnabled",
        EffectiveConfigurationCategoryV1::Privacy,
        "Webcam presence",
        preference.effective.webcam_presence,
        defaults.webcam_presence,
        "Explicit opt-in intent for a future qualified local webcam-presence producer.",
        "Persisting true does not open a camera; native capture admission remains separate."
    );
    product_bool!(
        "presence.emotionEnabled",
        EffectiveConfigurationCategoryV1::Conversation,
        "NPC emotion",
        preference.effective.emotion,
        defaults.emotion,
        "Controls emotion-aware response intent.",
        "No webcam inference is implied by this NPC output setting."
    );
    product_bool!(
        "presence.visionEnabled",
        EffectiveConfigurationCategoryV1::OptionalVisual,
        "Game vision",
        preference.effective.vision,
        defaults.vision,
        "Controls whether qualified game-image context may be requested.",
        "Native capture, route, privacy, and target evidence remain mandatory."
    );
    push_egress_entries(
        &mut entries,
        &preference.effective.egress,
        &defaults.egress,
        product_persistence.clone(),
    );

    let context = loadout_context(&request.scope);
    let resolved = provider
        .document
        .resolve(&context, &ValidationContextV1::online())
        .map_err(|error| owner_error("providerLoadouts", error))?;
    let default_document = starter_document();
    let default_resolved = default_document
        .resolve(&context, &ValidationContextV1::online())
        .map_err(|error| owner_error("providerLoadouts", error))?;
    for role in ALL_PROVIDER_ROLES {
        let current = resolved.roles.get(&role);
        let default = default_resolved.roles.get(&role);
        let winning_scope = role_source_scope(&provider.document, &resolved, role)
            .unwrap_or(ProductPreferenceScopeV1::Global);
        let mut unavailable_reason = current.is_none().then(|| {
            format!(
                "The {} role is disabled in the winning provider loadout.",
                role_name(role)
            )
        });
        if let Some(route) = current {
            if preference.effective.execution_preset.value == ExecutionPresetV1::FullyLocal
                && enum_name(&route.primary.disclosure.egress) != "none"
            {
                unavailable_reason = Some(
                    "Fully Local policy denies this persisted route because it performs network egress. Select a qualified local route explicitly; no automatic fallback occurs."
                        .into(),
                );
            } else if role == ProviderRole::Vision && !preference.effective.vision.value {
                unavailable_reason = Some(
                    "The route is configured, but game vision is disabled by the effective product preference."
                        .into(),
                );
            } else if role == ProviderRole::Lipsync
                && !local_resources
                    .admits_complete_lipsync_route(
                        &route.primary.provider_id,
                        &route.primary.model_id,
                    )
                    .map_err(|error| owner_error("localResources", error))?
            {
                unavailable_reason = Some(
                    "No signed, complete, device-admitted local lip-sync pack proves this route ready. The experimental landmark pack is not a lip-sync model."
                        .into(),
                );
            }
        }
        entries.push(EffectiveConfigurationEntryV1 {
            key: format!("provider.{}", role_name(role)),
            category: EffectiveConfigurationCategoryV1::ProviderRoute,
            label: format!("{} route", role_label(role)),
            owner: EffectiveConfigurationOwnerV1::ProviderLoadouts,
            value: current.map_or(EffectiveConfigurationValueV1::Disabled, route_value),
            default_value: default.map_or(EffectiveConfigurationValueV1::Disabled, route_value),
            winning_scope,
            source_kind: "activeLoadout".into(),
            persistence: provider_persistence.clone(),
            explanation: "Resolved from the active global/game/character loadout chain. Credential values are never included in this snapshot.".into(),
            runtime_consequence: "The primary route is pinned per turn. Listed fallbacks are manual-only and never activate automatically.".into(),
            unavailable_reason,
        });
    }

    push_local_policy_entries(
        &mut entries,
        &local_settings,
        &LocalResourceSettings::default(),
        local_persistence.clone(),
    );
    entries.push(EffectiveConfigurationEntryV1 {
        key: "optionalVisual.selectedLocalLoadout".into(),
        category: EffectiveConfigurationCategoryV1::OptionalVisual,
        label: "Selected local loadout".into(),
        owner: EffectiveConfigurationOwnerV1::LocalResources,
        value: selected.selected.as_ref().map_or(
            EffectiveConfigurationValueV1::Disabled,
            |selection| EffectiveConfigurationValueV1::Choice {
                value: selection.selection_id.clone(),
            },
        ),
        default_value: EffectiveConfigurationValueV1::Disabled,
        winning_scope: ProductPreferenceScopeV1::Global,
        source_kind: "nativeSelection".into(),
        persistence: local_persistence.clone(),
        explanation: "Reports the exact selected local pack set without admitting or loading it.".into(),
        runtime_consequence: "A selected loadout remains inactive until native telemetry and signed measured-envelope admission succeed.".into(),
        unavailable_reason: (!selected.ready || selected.selected.is_none()).then_some(selected.detail),
    });
    entries.push(EffectiveConfigurationEntryV1 {
        key: "optionalVisual.experimentalSignalPack".into(),
        category: EffectiveConfigurationCategoryV1::OptionalVisual,
        label: "Experimental face-signal pack".into(),
        owner: EffectiveConfigurationOwnerV1::LocalResources,
        value: choice(&visual_pack.phase),
        default_value: choice(&ExperimentalPackPhase::NotInstalled),
        winning_scope: ProductPreferenceScopeV1::Global,
        source_kind: "nativePackLifecycle".into(),
        persistence: local_persistence,
        explanation: "Reports the optional OpenSeeFace landmark/signal dependency lifecycle.".into(),
        runtime_consequence: "This pack can provide face signals only; it is explicitly not a complete lip-sync model and cannot authorize a lip-sync route.".into(),
        unavailable_reason: (!visual_pack.complete_lip_sync_model).then_some(visual_pack.detail),
    });

    let unique = entries
        .iter()
        .map(|entry| entry.key.as_str())
        .collect::<BTreeSet<_>>();
    debug_assert_eq!(unique.len(), entries.len());

    Ok(EffectiveConfigurationSnapshotV1 {
        schema_version: SCHEMA_VERSION,
        scope: request.scope,
        preference_revision: preference.revision,
        read_only: true,
        mutation_authority_added: false,
        automatic_provider_fallback: false,
        entries,
    })
}

fn owner_error(owner: &'static str, error: impl std::fmt::Display) -> EffectiveConfigurationError {
    EffectiveConfigurationError::Owner {
        owner,
        detail: error.to_string(),
    }
}

// Each entry keeps its complete UI and runtime disclosure adjacent at construction.
#[allow(clippy::too_many_arguments)]
fn product_entry<T>(
    key: &str,
    category: EffectiveConfigurationCategoryV1,
    label: &str,
    value: EffectiveConfigurationValueV1,
    default_value: EffectiveConfigurationValueV1,
    effective: &EffectiveValueV1<T>,
    persistence: EffectiveConfigurationPersistenceV1,
    explanation: &str,
    runtime_consequence: &str,
) -> EffectiveConfigurationEntryV1 {
    EffectiveConfigurationEntryV1 {
        key: key.into(),
        category,
        label: label.into(),
        owner: EffectiveConfigurationOwnerV1::ProductPreferences,
        value,
        default_value,
        winning_scope: effective.source_scope.clone(),
        source_kind: enum_name(&effective.source_kind),
        persistence,
        explanation: explanation.into(),
        runtime_consequence: runtime_consequence.into(),
        unavailable_reason: None,
    }
}

fn push_egress_entries(
    entries: &mut Vec<EffectiveConfigurationEntryV1>,
    current: &EffectiveEgressPolicyV1,
    default: &EffectiveEgressPolicyV1,
    persistence: EffectiveConfigurationPersistenceV1,
) {
    let values = [
        (
            "privacy.egress.transcript",
            "Transcript egress",
            &current.transcript,
            &default.transcript,
        ),
        (
            "privacy.egress.microphoneAudio",
            "Microphone audio egress",
            &current.microphone_audio,
            &default.microphone_audio,
        ),
        (
            "privacy.egress.capturedGameImage",
            "Captured game image egress",
            &current.captured_game_image,
            &default.captured_game_image,
        ),
        (
            "privacy.egress.localMemoryContext",
            "Local memory context egress",
            &current.local_memory_context,
            &default.local_memory_context,
        ),
    ];
    for (key, label, value, default_value) in values {
        entries.push(product_entry(
            key,
            EffectiveConfigurationCategoryV1::Privacy,
            label,
            choice(&value.value),
            choice(&default_value.value),
            value,
            persistence.clone(),
            "Reports the explicit execution-preset egress disposition for this data class.",
            "Selected-provider-route means only an explicitly selected route may transmit it; Denied blocks egress.",
        ));
    }
}

fn push_local_policy_entries(
    entries: &mut Vec<EffectiveConfigurationEntryV1>,
    current: &LocalResourceSettings,
    default: &LocalResourceSettings,
    persistence: EffectiveConfigurationPersistenceV1,
) {
    let mut push_integer = |key: &str,
                            label: &str,
                            value: u64,
                            default_value: u64,
                            unit: &str,
                            consequence: &str| {
        entries.push(EffectiveConfigurationEntryV1 {
            key: key.into(),
            category: EffectiveConfigurationCategoryV1::Performance,
            label: label.into(),
            owner: EffectiveConfigurationOwnerV1::LocalResources,
            value: integer(value, unit),
            default_value: integer(default_value, unit),
            winning_scope: ProductPreferenceScopeV1::Global,
            source_kind: "nativeResourcePolicy".into(),
            persistence: persistence.clone(),
            explanation: "Native resource-governor policy; this inspector does not modify or admit a loadout.".into(),
            runtime_consequence: consequence.into(),
            unavailable_reason: None,
        });
    };
    push_integer(
        "performance.vramSoftCeiling",
        "VRAM soft ceiling",
        current.governor.vram_soft_ceiling_basis_points.into(),
        default.governor.vram_soft_ceiling_basis_points.into(),
        "basisPoints",
        "Constrains admitted local-model VRAM against the live OS budget.",
    );
    push_integer(
        "performance.ramSoftCeiling",
        "RAM soft ceiling",
        current.governor.ram_soft_ceiling_basis_points.into(),
        default.governor.ram_soft_ceiling_basis_points.into(),
        "basisPoints",
        "Constrains admitted local-model RAM against physical memory.",
    );
    push_integer(
        "performance.gameVramReserve",
        "Game VRAM reserve",
        current.game_reserve_vram_bytes,
        default.game_reserve_vram_bytes,
        "bytes",
        "Reserves VRAM for the selected game before local-model admission.",
    );
    push_integer(
        "performance.gameRamReserve",
        "Game RAM reserve",
        current.game_additional_reserve_ram_bytes,
        default.game_additional_reserve_ram_bytes,
        "bytes",
        "Reserves additional RAM for the game before local-model admission.",
    );
    push_integer(
        "performance.keepWarm",
        "Keep-warm window",
        current.governor.keep_warm_millis,
        default.governor.keep_warm_millis,
        "milliseconds",
        "Keeps qualified local resources warm only within the governor window.",
    );
    push_integer(
        "performance.unloadTtl",
        "Unload timeout",
        current.governor.unload_ttl_millis,
        default.governor.unload_ttl_millis,
        "milliseconds",
        "Allows cold/unload decisions after the configured idle interval.",
    );
    entries.push(EffectiveConfigurationEntryV1 {
        key: "performance.preferredResidency".into(),
        category: EffectiveConfigurationCategoryV1::Performance,
        label: "Preferred residency".into(),
        owner: EffectiveConfigurationOwnerV1::LocalResources,
        value: choice(&current.preferred_residency),
        default_value: choice(&default.preferred_residency),
        winning_scope: ProductPreferenceScopeV1::Global,
        source_kind: "nativeResourcePolicy".into(),
        persistence,
        explanation: "Preferred local-model residency before live evidence is evaluated.".into(),
        runtime_consequence:
            "The native governor may still unload or cool resources under measured contention."
                .into(),
        unavailable_reason: None,
    });
}

fn product_persistence(
    state: PreferenceMigrationStateV1,
    detail: String,
) -> EffectiveConfigurationPersistenceV1 {
    let source = match state {
        PreferenceMigrationStateV1::Current => {
            EffectiveConfigurationPersistenceSourceV1::NativePersisted
        }
        PreferenceMigrationStateV1::MigratedLegacyOnboarding => {
            EffectiveConfigurationPersistenceSourceV1::MigratedOnboarding
        }
        PreferenceMigrationStateV1::RecoveredDefaults => {
            EffectiveConfigurationPersistenceSourceV1::RecoveredDefault
        }
    };
    EffectiveConfigurationPersistenceV1 { source, detail }
}

fn provider_persistence(
    state: ProviderLoadoutPersistenceHealth,
    detail: String,
) -> EffectiveConfigurationPersistenceV1 {
    let source = match state {
        ProviderLoadoutPersistenceHealth::Healthy => {
            EffectiveConfigurationPersistenceSourceV1::NativePersisted
        }
        ProviderLoadoutPersistenceHealth::FirstRun => {
            EffectiveConfigurationPersistenceSourceV1::FirstRunSeed
        }
        ProviderLoadoutPersistenceHealth::RecoveredLastGood => {
            EffectiveConfigurationPersistenceSourceV1::RecoveredLastGood
        }
        ProviderLoadoutPersistenceHealth::RecoveredSeed => {
            EffectiveConfigurationPersistenceSourceV1::RecoveredSeed
        }
    };
    EffectiveConfigurationPersistenceV1 { source, detail }
}

fn local_persistence(
    source: LocalResourceSettingsPersistenceV1,
) -> EffectiveConfigurationPersistenceV1 {
    let (source, detail) = match source {
        LocalResourceSettingsPersistenceV1::NativePersisted => (
            EffectiveConfigurationPersistenceSourceV1::NativePersisted,
            "Local resource policy was loaded from private native application storage.",
        ),
        LocalResourceSettingsPersistenceV1::BuiltInDefault => (
            EffectiveConfigurationPersistenceSourceV1::BuiltInDefault,
            "No local resource policy file exists; built-in fail-safe defaults are effective.",
        ),
        LocalResourceSettingsPersistenceV1::RecoveredDefault => (
            EffectiveConfigurationPersistenceSourceV1::RecoveredDefault,
            "The saved local resource policy was unusable; built-in fail-safe defaults are effective.",
        ),
    };
    EffectiveConfigurationPersistenceV1 {
        source,
        detail: detail.into(),
    }
}

fn loadout_context(scope: &ProductPreferenceScopeV1) -> LoadoutContextV1 {
    match scope {
        ProductPreferenceScopeV1::Global => LoadoutContextV1::global(),
        ProductPreferenceScopeV1::Game { game_profile_id } => {
            LoadoutContextV1::game(game_profile_id.clone())
        }
        ProductPreferenceScopeV1::Character {
            game_profile_id,
            character_id,
        } => LoadoutContextV1::character(game_profile_id.clone(), character_id.clone()),
    }
}

fn role_source_scope(
    document: &ProviderLoadoutDocumentV1,
    resolved: &ResolvedProviderLoadoutV1,
    role: ProviderRole,
) -> Option<ProductPreferenceScopeV1> {
    resolved.inheritance_chain.iter().rev().find_map(|id| {
        let loadout = document.loadouts.get(id)?;
        match loadout.roles.get(&role) {
            Some(RoleOverrideV1::Inherit) | None => None,
            Some(RoleOverrideV1::Disabled | RoleOverrideV1::Route(_)) => {
                Some(preference_scope(&loadout.scope))
            }
        }
    })
}

fn preference_scope(scope: &LoadoutScopeV1) -> ProductPreferenceScopeV1 {
    match scope {
        LoadoutScopeV1::Global => ProductPreferenceScopeV1::Global,
        LoadoutScopeV1::Game { game_id } => ProductPreferenceScopeV1::Game {
            game_profile_id: game_id.clone(),
        },
        LoadoutScopeV1::Character {
            game_id,
            character_id,
        } => ProductPreferenceScopeV1::Character {
            game_profile_id: game_id.clone(),
            character_id: character_id.clone(),
        },
    }
}

fn route_value(route: &npc_provider_loadouts::RoleRouteV1) -> EffectiveConfigurationValueV1 {
    let primary: &ProviderModelRouteV1 = &route.primary;
    EffectiveConfigurationValueV1::Route {
        provider_id: primary.provider_id.clone(),
        model_id: primary.model_id.clone(),
        voice_id: primary.voice_id.clone(),
        execution: enum_name(&primary.disclosure.execution),
        egress: enum_name(&primary.disclosure.egress),
        transmitted_data: primary
            .disclosure
            .transmitted_data
            .iter()
            .map(enum_name)
            .collect(),
        manual_fallback_count: route.fallbacks.len(),
        automatic_fallback: false,
    }
}

fn role_name(role: ProviderRole) -> &'static str {
    match role {
        ProviderRole::Llm => "llm",
        ProviderRole::Stt => "stt",
        ProviderRole::Tts => "tts",
        ProviderRole::Embeddings => "embeddings",
        ProviderRole::Vision => "vision",
        ProviderRole::Lipsync => "lipsync",
    }
}

fn role_label(role: ProviderRole) -> &'static str {
    match role {
        ProviderRole::Llm => "LLM",
        ProviderRole::Stt => "STT",
        ProviderRole::Tts => "TTS",
        ProviderRole::Embeddings => "Embeddings",
        ProviderRole::Vision => "Vision",
        ProviderRole::Lipsync => "Lip-sync",
    }
}

fn choice(value: &impl Serialize) -> EffectiveConfigurationValueV1 {
    EffectiveConfigurationValueV1::Choice {
        value: enum_name(value),
    }
}

fn integer(value: u64, unit: &str) -> EffectiveConfigurationValueV1 {
    EffectiveConfigurationValueV1::Integer {
        value,
        unit: unit.into(),
    }
}

fn enum_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unavailable".into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::domain::PreferenceSnapshot;
    use crate::product_preferences::{
        ProductPreferenceOverridesV1, SaveProductPreferencesRequestV1, ScopedProductPreferencesV1,
    };
    use npc_provider_loadouts::{LoadoutId, ProviderLoadoutV1, RoleOverrideV1};
    use std::fs;
    use tempfile::tempdir;

    fn managers(
        root: &std::path::Path,
    ) -> (
        ProductPreferenceManager,
        ProviderLoadoutManager,
        LocalResourceManager,
    ) {
        (
            ProductPreferenceManager::new(root, &PreferenceSnapshot::default()).unwrap(),
            ProviderLoadoutManager::new(root.join("providers")),
            LocalResourceManager::new(root, None).unwrap(),
        )
    }

    fn snapshot(
        scope: ProductPreferenceScopeV1,
        preferences: &ProductPreferenceManager,
        providers: &ProviderLoadoutManager,
        resources: &LocalResourceManager,
    ) -> EffectiveConfigurationSnapshotV1 {
        build_effective_configuration_snapshot(
            EffectiveConfigurationRequestV1 { scope },
            preferences,
            providers,
            resources,
        )
        .unwrap()
    }

    fn entry<'a>(
        snapshot: &'a EffectiveConfigurationSnapshotV1,
        key: &str,
    ) -> &'a EffectiveConfigurationEntryV1 {
        snapshot
            .entries
            .iter()
            .find(|entry| entry.key == key)
            .unwrap()
    }

    #[test]
    fn first_run_migration_is_materialized_and_reopen_reports_native_sources() {
        let temp = tempdir().unwrap();
        let (preferences, providers, resources) = managers(temp.path());
        let first = snapshot(
            ProductPreferenceScopeV1::Global,
            &preferences,
            &providers,
            &resources,
        );
        assert_eq!(
            entry(&first, "conversation.executionPreset")
                .persistence
                .source,
            EffectiveConfigurationPersistenceSourceV1::MigratedOnboarding
        );
        assert_eq!(
            entry(&first, "provider.llm").persistence.source,
            EffectiveConfigurationPersistenceSourceV1::FirstRunSeed
        );
        assert_eq!(
            entry(&first, "performance.vramSoftCeiling")
                .persistence
                .source,
            EffectiveConfigurationPersistenceSourceV1::BuiltInDefault
        );

        resources
            .save_settings(resources.settings().unwrap())
            .unwrap();
        drop((preferences, providers, resources));
        let (preferences, providers, resources) = managers(temp.path());
        let reopened = snapshot(
            ProductPreferenceScopeV1::Global,
            &preferences,
            &providers,
            &resources,
        );
        assert_eq!(
            entry(&reopened, "conversation.executionPreset")
                .persistence
                .source,
            EffectiveConfigurationPersistenceSourceV1::NativePersisted
        );
        assert_eq!(
            entry(&reopened, "performance.vramSoftCeiling")
                .persistence
                .source,
            EffectiveConfigurationPersistenceSourceV1::NativePersisted
        );
    }

    #[test]
    fn game_and_character_overrides_report_exact_winning_scope() {
        let temp = tempdir().unwrap();
        let (preferences, providers, resources) = managers(temp.path());
        let game_scope = ProductPreferenceScopeV1::Game {
            game_profile_id: "eclipse-harbor".into(),
        };
        let character_scope = ProductPreferenceScopeV1::Character {
            game_profile_id: "eclipse-harbor".into(),
            character_id: "mara-venn".into(),
        };
        preferences
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: 1,
                    entry: ScopedProductPreferencesV1 {
                        scope: game_scope.clone(),
                        execution_preset: None,
                        performance_preset: None,
                        overrides: ProductPreferenceOverridesV1 {
                            subtitles: Some(false),
                            ..Default::default()
                        },
                    },
                },
                None,
                crate::product_preferences::ResourceAuthorityReferenceV1 {
                    selection_id: None,
                    admission_status: None,
                    admission_receipt_present: false,
                    exact_target_pid: None,
                    activation_performed: false,
                },
            )
            .unwrap();
        preferences
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: 2,
                    entry: ScopedProductPreferencesV1 {
                        scope: character_scope.clone(),
                        execution_preset: None,
                        performance_preset: None,
                        overrides: ProductPreferenceOverridesV1 {
                            memory: Some(false),
                            ..Default::default()
                        },
                    },
                },
                None,
                crate::product_preferences::ResourceAuthorityReferenceV1 {
                    selection_id: None,
                    admission_status: None,
                    admission_receipt_present: false,
                    exact_target_pid: None,
                    activation_performed: false,
                },
            )
            .unwrap();
        let actual = snapshot(
            character_scope.clone(),
            &preferences,
            &providers,
            &resources,
        );
        assert_eq!(
            entry(&actual, "presentation.subtitlesEnabled").winning_scope,
            game_scope
        );
        assert_eq!(
            entry(&actual, "memory.enabled").winning_scope,
            character_scope
        );
        assert_eq!(
            entry(&actual, "provider.llm").winning_scope,
            ProductPreferenceScopeV1::Global
        );
    }

    #[test]
    fn provider_scope_and_fully_local_unavailability_remain_truthful() {
        let temp = tempdir().unwrap();
        let provider_root = temp.path().join("providers");
        fs::create_dir_all(&provider_root).unwrap();
        let mut document = starter_document();
        let global_id = document.activation.global.clone();
        let game_id = LoadoutId::new("eclipse-game").unwrap();
        document
            .insert(ProviderLoadoutV1 {
                id: game_id.clone(),
                name: "Eclipse game".into(),
                scope: LoadoutScopeV1::Game {
                    game_id: "eclipse-harbor".into(),
                },
                parent: Some(global_id),
                roles: std::collections::BTreeMap::from([(
                    ProviderRole::Vision,
                    RoleOverrideV1::Disabled,
                )]),
            })
            .unwrap();
        document.activate(&game_id).unwrap();
        fs::write(
            provider_root.join("provider-loadouts-v1.json"),
            serde_json::to_vec_pretty(&document).unwrap(),
        )
        .unwrap();
        let preferences =
            ProductPreferenceManager::new(temp.path(), &PreferenceSnapshot::default()).unwrap();
        preferences
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: 1,
                    entry: ScopedProductPreferencesV1 {
                        scope: ProductPreferenceScopeV1::Global,
                        execution_preset: Some(ExecutionPresetV1::FullyLocal),
                        performance_preset: Some(
                            crate::product_preferences::PerformancePresetV1::Balanced,
                        ),
                        overrides: ProductPreferenceOverridesV1::default(),
                    },
                },
                None,
                crate::product_preferences::ResourceAuthorityReferenceV1 {
                    selection_id: None,
                    admission_status: None,
                    admission_receipt_present: false,
                    exact_target_pid: None,
                    activation_performed: false,
                },
            )
            .unwrap();
        let providers = ProviderLoadoutManager::new(provider_root);
        let resources = LocalResourceManager::new(temp.path(), None).unwrap();
        let actual = snapshot(
            ProductPreferenceScopeV1::Game {
                game_profile_id: "eclipse-harbor".into(),
            },
            &preferences,
            &providers,
            &resources,
        );
        assert_eq!(
            entry(&actual, "provider.vision").winning_scope,
            ProductPreferenceScopeV1::Game {
                game_profile_id: "eclipse-harbor".into()
            }
        );
        assert!(matches!(
            entry(&actual, "provider.vision").value,
            EffectiveConfigurationValueV1::Disabled
        ));
        assert!(entry(&actual, "provider.llm")
            .unavailable_reason
            .as_deref()
            .unwrap()
            .contains("Fully Local"));
        assert!(!actual.automatic_provider_fallback);
    }

    #[test]
    fn snapshot_is_read_only_unique_and_contains_no_credential_material() {
        let temp = tempdir().unwrap();
        let (preferences, providers, resources) = managers(temp.path());
        let actual = snapshot(
            ProductPreferenceScopeV1::Global,
            &preferences,
            &providers,
            &resources,
        );
        assert!(actual.read_only);
        assert!(!actual.mutation_authority_added);
        let keys = actual
            .entries
            .iter()
            .map(|entry| &entry.key)
            .collect::<BTreeSet<_>>();
        assert_eq!(keys.len(), actual.entries.len());
        let json = serde_json::to_string(&actual).unwrap();
        assert!(!json.contains("\"credential\":"));
        assert!(!json.contains("referenceId"));
        assert!(!json.contains("personal"));
        assert!(json.contains("\"providerId\":"));
        assert!(json.contains("\"modelId\":"));
        assert!(json.contains("\"voiceId\":"));
        assert!(json.contains("\"transmittedData\":"));
        assert!(json.contains("\"manualFallbackCount\":"));
        assert!(json.contains("\"automaticFallback\":"));
        assert!(!json.contains("\"provider_id\":"));
        assert!(!json.contains("\"transmitted_data\":"));
        assert!(entry(&actual, "provider.lipsync")
            .unavailable_reason
            .is_some());
        assert!(entry(&actual, "optionalVisual.experimentalSignalPack")
            .unavailable_reason
            .is_some());
    }
}
