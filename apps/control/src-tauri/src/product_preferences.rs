use crate::domain::{ExecutionMode, PerformanceMode, PreferenceSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;
use thiserror::Error;

const SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "product-preferences-v1.json";
const MAX_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionPresetV1 {
    Cloud,
    Hybrid,
    FullyLocal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PerformancePresetV1 {
    Competitive,
    Fast,
    Balanced,
    Immersive,
    Maximum,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProductPreferenceScopeV1 {
    Global,
    Game {
        #[serde(rename = "gameProfileId")]
        game_profile_id: String,
    },
    Character {
        #[serde(rename = "gameProfileId")]
        game_profile_id: String,
        #[serde(rename = "characterId")]
        character_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VerbosityV1 {
    Concise,
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResponseLengthV1 {
    Short,
    Medium,
    Long,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InterruptionModeV1 {
    Immediate,
    FinishSentence,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InputModeV1 {
    Ptt,
    Vad,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductPreferenceOverridesV1 {
    pub verbosity: Option<VerbosityV1>,
    pub creativity: Option<u8>,
    pub response_length: Option<ResponseLengthV1>,
    pub interruption_mode: Option<InterruptionModeV1>,
    pub input_mode: Option<InputModeV1>,
    pub subtitles: Option<bool>,
    pub overlay: Option<bool>,
    pub memory: Option<bool>,
    /// Explicit consent intent for a future qualified local webcam-presence
    /// producer. It is independent of NPC output emotion and never activates a
    /// camera merely by being persisted.
    pub webcam_presence: Option<bool>,
    pub emotion: Option<bool>,
    pub vision: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedProductPreferencesV1 {
    pub scope: ProductPreferenceScopeV1,
    pub execution_preset: Option<ExecutionPresetV1>,
    pub performance_preset: Option<PerformancePresetV1>,
    pub overrides: ProductPreferenceOverridesV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProductPreferenceDocumentV1 {
    schema_version: u32,
    revision: u64,
    entries: Vec<ScopedProductPreferencesV1>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveSourceKindV1 {
    Preset,
    Override,
    Inherited,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveValueV1<T> {
    pub value: T,
    pub source_scope: ProductPreferenceScopeV1,
    pub source_kind: EffectiveSourceKindV1,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EgressDispositionV1 {
    Denied,
    SelectedProviderRoute,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveEgressPolicyV1 {
    pub transcript: EffectiveValueV1<EgressDispositionV1>,
    pub microphone_audio: EffectiveValueV1<EgressDispositionV1>,
    pub captured_game_image: EffectiveValueV1<EgressDispositionV1>,
    pub local_memory_context: EffectiveValueV1<EgressDispositionV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveProductPreferencesV1 {
    pub scope: ProductPreferenceScopeV1,
    pub execution_preset: EffectiveValueV1<ExecutionPresetV1>,
    pub performance_preset: EffectiveValueV1<PerformancePresetV1>,
    pub verbosity: EffectiveValueV1<VerbosityV1>,
    pub creativity: EffectiveValueV1<u8>,
    pub response_length: EffectiveValueV1<ResponseLengthV1>,
    pub interruption_mode: EffectiveValueV1<InterruptionModeV1>,
    pub input_mode: EffectiveValueV1<InputModeV1>,
    pub subtitles: EffectiveValueV1<bool>,
    pub overlay: EffectiveValueV1<bool>,
    pub memory: EffectiveValueV1<bool>,
    pub webcam_presence: EffectiveValueV1<bool>,
    pub emotion: EffectiveValueV1<bool>,
    pub vision: EffectiveValueV1<bool>,
    pub egress: EffectiveEgressPolicyV1,
    pub automatic_provider_fallback: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteAuthorityReferenceV1 {
    pub source_loadout_id: String,
    pub generation: Option<u64>,
    pub sha256: String,
    pub activation_performed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAuthorityReferenceV1 {
    pub selection_id: Option<String>,
    pub admission_status: Option<String>,
    pub admission_receipt_present: bool,
    pub exact_target_pid: Option<u32>,
    pub activation_performed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PreferenceMigrationStateV1 {
    Current,
    MigratedLegacyOnboarding,
    RecoveredDefaults,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceMigrationV1 {
    pub state: PreferenceMigrationStateV1,
    pub from_schema_version: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProductPreferenceSnapshotV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub entries: Vec<ScopedProductPreferencesV1>,
    pub effective: EffectiveProductPreferencesV1,
    pub migration: PreferenceMigrationV1,
    pub route_snapshot: Option<RouteAuthorityReferenceV1>,
    pub resource_snapshot: ResourceAuthorityReferenceV1,
    pub automatic_provider_fallback: bool,
    pub mutation_activated_routes_or_packs: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveProductPreferencesRequestV1 {
    pub expected_revision: u64,
    pub entry: ScopedProductPreferencesV1,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetProductPreferencesRequestV1 {
    pub expected_revision: u64,
    pub scope: ProductPreferenceScopeV1,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Error)]
pub enum ProductPreferenceError {
    #[error("product preference state is temporarily unavailable")]
    State,
    #[error("product preference request is invalid: {0}")]
    Invalid(String),
    #[error("product preference revision changed; refresh before saving")]
    RevisionConflict,
    #[error("reset requires explicit user confirmation")]
    ConfirmationRequired,
    #[error("product preference persistence failed: {0}")]
    Persistence(String),
}

struct ProductPreferenceState {
    document: ProductPreferenceDocumentV1,
    migration: PreferenceMigrationV1,
}

pub struct ProductPreferenceManager {
    path: PathBuf,
    state: Mutex<ProductPreferenceState>,
}

impl ProductPreferenceManager {
    pub fn new(
        config_directory: &Path,
        legacy: &PreferenceSnapshot,
    ) -> Result<Self, ProductPreferenceError> {
        let path = config_directory.join(FILE_NAME);
        let (document, migration) = match load_document(&path) {
            Ok(Some(document)) => (document, current_migration()),
            Ok(None) => {
                let document = legacy_document(legacy);
                // Materialize the migration immediately. Otherwise a user who
                // accepts the onboarding defaults but never edits Settings
                // would be reported as freshly migrated on every launch.
                atomic_write(&path, &document)?;
                (
                    document,
                    PreferenceMigrationV1 {
                    state: PreferenceMigrationStateV1::MigratedLegacyOnboarding,
                    from_schema_version: Some(crate::domain::ONBOARDING_SCHEMA_VERSION),
                    detail: "Native product preferences were initialized from the persisted onboarding preferences; no route or pack activation was performed.".into(),
                    },
                )
            }
            Err(error) => (
                legacy_document(&PreferenceSnapshot::default()),
                PreferenceMigrationV1 {
                    state: PreferenceMigrationStateV1::RecoveredDefaults,
                    from_schema_version: None,
                    detail: format!(
                        "Invalid saved product preferences were ignored safely: {error}"
                    ),
                },
            ),
        };
        validate_document(&document)?;
        Ok(Self {
            path,
            state: Mutex::new(ProductPreferenceState {
                document,
                migration,
            }),
        })
    }

    pub fn snapshot(
        &self,
        scope: ProductPreferenceScopeV1,
        route_snapshot: Option<RouteAuthorityReferenceV1>,
        resource_snapshot: ResourceAuthorityReferenceV1,
    ) -> Result<ProductPreferenceSnapshotV1, ProductPreferenceError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ProductPreferenceError::State)?;
        snapshot_from_state(&state, scope, route_snapshot, resource_snapshot)
    }

    pub fn save(
        &self,
        request: SaveProductPreferencesRequestV1,
        route_snapshot: Option<RouteAuthorityReferenceV1>,
        resource_snapshot: ResourceAuthorityReferenceV1,
    ) -> Result<ProductPreferenceSnapshotV1, ProductPreferenceError> {
        validate_entry(&request.entry)?;
        let scope = request.entry.scope.clone();
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProductPreferenceError::State)?;
        if state.document.revision != request.expected_revision {
            return Err(ProductPreferenceError::RevisionConflict);
        }
        let mut candidate = state.document.clone();
        if let Some(entry) = candidate
            .entries
            .iter_mut()
            .find(|entry| entry.scope == request.entry.scope)
        {
            *entry = request.entry;
        } else {
            candidate.entries.push(request.entry);
        }
        candidate.revision = candidate.revision.saturating_add(1);
        validate_document(&candidate)?;
        atomic_write(&self.path, &candidate)?;
        state.document = candidate;
        state.migration = current_migration();
        snapshot_from_state(&state, scope, route_snapshot, resource_snapshot)
    }

    pub fn reset(
        &self,
        request: ResetProductPreferencesRequestV1,
        route_snapshot: Option<RouteAuthorityReferenceV1>,
        resource_snapshot: ResourceAuthorityReferenceV1,
    ) -> Result<ProductPreferenceSnapshotV1, ProductPreferenceError> {
        if !request.explicit_user_confirmation {
            return Err(ProductPreferenceError::ConfirmationRequired);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProductPreferenceError::State)?;
        if state.document.revision != request.expected_revision {
            return Err(ProductPreferenceError::RevisionConflict);
        }
        let mut candidate = state.document.clone();
        match &request.scope {
            ProductPreferenceScopeV1::Global => {
                candidate.entries = legacy_document(&PreferenceSnapshot::default()).entries;
            }
            scope => candidate.entries.retain(|entry| &entry.scope != scope),
        }
        candidate.revision = candidate.revision.saturating_add(1);
        validate_document(&candidate)?;
        atomic_write(&self.path, &candidate)?;
        state.document = candidate;
        state.migration = current_migration();
        snapshot_from_state(&state, request.scope, route_snapshot, resource_snapshot)
    }
}

/// Canonical first-run values for read-only inspectors. Keeping this beside
/// preset resolution prevents a second settings authority from drifting away
/// from onboarding and runtime defaults.
pub(crate) fn default_effective_product_preferences(
    scope: &ProductPreferenceScopeV1,
) -> Result<EffectiveProductPreferencesV1, ProductPreferenceError> {
    resolve_effective(&legacy_document(&PreferenceSnapshot::default()), scope)
}

fn snapshot_from_state(
    state: &ProductPreferenceState,
    scope: ProductPreferenceScopeV1,
    route_snapshot: Option<RouteAuthorityReferenceV1>,
    resource_snapshot: ResourceAuthorityReferenceV1,
) -> Result<ProductPreferenceSnapshotV1, ProductPreferenceError> {
    Ok(ProductPreferenceSnapshotV1 {
        schema_version: SCHEMA_VERSION,
        revision: state.document.revision,
        entries: state.document.entries.clone(),
        effective: resolve_effective(&state.document, &scope)?,
        migration: state.migration.clone(),
        route_snapshot,
        resource_snapshot,
        automatic_provider_fallback: false,
        mutation_activated_routes_or_packs: false,
    })
}

fn resolve_effective(
    document: &ProductPreferenceDocumentV1,
    requested: &ProductPreferenceScopeV1,
) -> Result<EffectiveProductPreferencesV1, ProductPreferenceError> {
    let chain = scope_chain(requested);
    let entries = chain
        .iter()
        .filter_map(|scope| document.entries.iter().find(|entry| &entry.scope == scope))
        .collect::<Vec<_>>();
    let global = entries
        .first()
        .ok_or_else(|| ProductPreferenceError::Invalid("global preferences are missing".into()))?;

    let (execution, execution_scope) = last_preset(&entries, |entry| entry.execution_preset)
        .or_else(|| global.execution_preset.map(|value| (value, &global.scope)))
        .ok_or_else(|| {
            ProductPreferenceError::Invalid("global execution preset is missing".into())
        })?;
    let (performance, performance_scope) = last_preset(&entries, |entry| entry.performance_preset)
        .or_else(|| {
            global
                .performance_preset
                .map(|value| (value, &global.scope))
        })
        .ok_or_else(|| {
            ProductPreferenceError::Invalid("global performance preset is missing".into())
        })?;
    let defaults = preset_defaults(performance);
    let execution_value = effective_preset(execution, execution_scope, requested);
    let performance_value = effective_preset(performance, performance_scope, requested);
    let egress_value = if execution == ExecutionPresetV1::FullyLocal {
        EgressDispositionV1::Denied
    } else {
        EgressDispositionV1::SelectedProviderRoute
    };
    let egress = |value| EffectiveValueV1 {
        value,
        source_scope: execution_scope.clone(),
        source_kind: if execution_scope == requested {
            EffectiveSourceKindV1::Preset
        } else {
            EffectiveSourceKindV1::Inherited
        },
    };

    Ok(EffectiveProductPreferencesV1 {
        scope: requested.clone(),
        execution_preset: execution_value,
        performance_preset: performance_value,
        verbosity: effective_override(
            &entries,
            requested,
            |value| value.verbosity,
            defaults.verbosity,
            performance_scope,
        ),
        creativity: effective_override(
            &entries,
            requested,
            |value| value.creativity,
            defaults.creativity,
            performance_scope,
        ),
        response_length: effective_override(
            &entries,
            requested,
            |value| value.response_length,
            defaults.response_length,
            performance_scope,
        ),
        interruption_mode: effective_override(
            &entries,
            requested,
            |value| value.interruption_mode,
            defaults.interruption_mode,
            performance_scope,
        ),
        input_mode: effective_override(
            &entries,
            requested,
            |value| value.input_mode,
            defaults.input_mode,
            performance_scope,
        ),
        subtitles: effective_override(
            &entries,
            requested,
            |value| value.subtitles,
            defaults.subtitles,
            performance_scope,
        ),
        overlay: effective_override(
            &entries,
            requested,
            |value| value.overlay,
            defaults.overlay,
            performance_scope,
        ),
        memory: effective_override(
            &entries,
            requested,
            |value| value.memory,
            defaults.memory,
            performance_scope,
        ),
        webcam_presence: effective_override(
            &entries,
            requested,
            |value| value.webcam_presence,
            false,
            performance_scope,
        ),
        emotion: effective_override(
            &entries,
            requested,
            |value| value.emotion,
            defaults.emotion,
            performance_scope,
        ),
        vision: effective_override(
            &entries,
            requested,
            |value| value.vision,
            defaults.vision,
            performance_scope,
        ),
        egress: EffectiveEgressPolicyV1 {
            transcript: egress(egress_value),
            microphone_audio: egress(egress_value),
            captured_game_image: egress(if execution == ExecutionPresetV1::Cloud {
                EgressDispositionV1::Denied
            } else {
                egress_value
            }),
            local_memory_context: egress(egress_value),
        },
        automatic_provider_fallback: false,
    })
}

fn last_preset<'a, T: Copy>(
    entries: &'a [&ScopedProductPreferencesV1],
    select: impl Fn(&ScopedProductPreferencesV1) -> Option<T>,
) -> Option<(T, &'a ProductPreferenceScopeV1)> {
    entries
        .iter()
        .filter_map(|entry| select(entry).map(|value| (value, &entry.scope)))
        .last()
}

fn effective_preset<T: Copy>(
    value: T,
    source: &ProductPreferenceScopeV1,
    requested: &ProductPreferenceScopeV1,
) -> EffectiveValueV1<T> {
    EffectiveValueV1 {
        value,
        source_scope: source.clone(),
        source_kind: if source == requested {
            EffectiveSourceKindV1::Preset
        } else {
            EffectiveSourceKindV1::Inherited
        },
    }
}

fn effective_override<T: Copy>(
    entries: &[&ScopedProductPreferencesV1],
    requested: &ProductPreferenceScopeV1,
    select: impl Fn(&ProductPreferenceOverridesV1) -> Option<T>,
    default: T,
    preset_scope: &ProductPreferenceScopeV1,
) -> EffectiveValueV1<T> {
    if let Some((value, source)) = entries
        .iter()
        .filter_map(|entry| select(&entry.overrides).map(|value| (value, &entry.scope)))
        .last()
    {
        return EffectiveValueV1 {
            value,
            source_scope: source.clone(),
            source_kind: if source == requested {
                EffectiveSourceKindV1::Override
            } else {
                EffectiveSourceKindV1::Inherited
            },
        };
    }
    EffectiveValueV1 {
        value: default,
        source_scope: preset_scope.clone(),
        source_kind: if preset_scope == requested {
            EffectiveSourceKindV1::Preset
        } else {
            EffectiveSourceKindV1::Inherited
        },
    }
}

#[derive(Clone, Copy)]
struct PresetDefaults {
    verbosity: VerbosityV1,
    creativity: u8,
    response_length: ResponseLengthV1,
    interruption_mode: InterruptionModeV1,
    input_mode: InputModeV1,
    subtitles: bool,
    overlay: bool,
    memory: bool,
    emotion: bool,
    vision: bool,
}

fn preset_defaults(preset: PerformancePresetV1) -> PresetDefaults {
    match preset {
        PerformancePresetV1::Competitive => PresetDefaults {
            verbosity: VerbosityV1::Concise,
            creativity: 25,
            response_length: ResponseLengthV1::Short,
            interruption_mode: InterruptionModeV1::Immediate,
            input_mode: InputModeV1::Ptt,
            subtitles: true,
            overlay: false,
            memory: true,
            emotion: false,
            vision: false,
        },
        PerformancePresetV1::Fast => PresetDefaults {
            verbosity: VerbosityV1::Concise,
            creativity: 35,
            response_length: ResponseLengthV1::Short,
            interruption_mode: InterruptionModeV1::FinishSentence,
            input_mode: InputModeV1::Ptt,
            subtitles: true,
            overlay: false,
            memory: true,
            emotion: true,
            vision: false,
        },
        PerformancePresetV1::Balanced => PresetDefaults {
            verbosity: VerbosityV1::Standard,
            creativity: 50,
            response_length: ResponseLengthV1::Medium,
            interruption_mode: InterruptionModeV1::FinishSentence,
            input_mode: InputModeV1::Ptt,
            subtitles: true,
            overlay: false,
            memory: true,
            emotion: true,
            vision: false,
        },
        PerformancePresetV1::Immersive => PresetDefaults {
            verbosity: VerbosityV1::Detailed,
            creativity: 65,
            response_length: ResponseLengthV1::Long,
            interruption_mode: InterruptionModeV1::FinishSentence,
            input_mode: InputModeV1::Vad,
            subtitles: true,
            overlay: false,
            memory: true,
            emotion: true,
            vision: true,
        },
        PerformancePresetV1::Maximum => PresetDefaults {
            verbosity: VerbosityV1::Detailed,
            creativity: 75,
            response_length: ResponseLengthV1::Long,
            interruption_mode: InterruptionModeV1::Disabled,
            input_mode: InputModeV1::Vad,
            subtitles: true,
            overlay: false,
            memory: true,
            emotion: true,
            vision: true,
        },
        PerformancePresetV1::Custom => PresetDefaults {
            verbosity: VerbosityV1::Standard,
            creativity: 50,
            response_length: ResponseLengthV1::Medium,
            interruption_mode: InterruptionModeV1::FinishSentence,
            input_mode: InputModeV1::Ptt,
            subtitles: true,
            overlay: false,
            memory: true,
            emotion: true,
            vision: false,
        },
    }
}

fn scope_chain(scope: &ProductPreferenceScopeV1) -> Vec<ProductPreferenceScopeV1> {
    match scope {
        ProductPreferenceScopeV1::Global => vec![ProductPreferenceScopeV1::Global],
        ProductPreferenceScopeV1::Game { game_profile_id } => vec![
            ProductPreferenceScopeV1::Global,
            ProductPreferenceScopeV1::Game {
                game_profile_id: game_profile_id.clone(),
            },
        ],
        ProductPreferenceScopeV1::Character {
            game_profile_id,
            character_id,
        } => vec![
            ProductPreferenceScopeV1::Global,
            ProductPreferenceScopeV1::Game {
                game_profile_id: game_profile_id.clone(),
            },
            ProductPreferenceScopeV1::Character {
                game_profile_id: game_profile_id.clone(),
                character_id: character_id.clone(),
            },
        ],
    }
}

fn legacy_document(legacy: &PreferenceSnapshot) -> ProductPreferenceDocumentV1 {
    ProductPreferenceDocumentV1 {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        entries: vec![ScopedProductPreferencesV1 {
            scope: ProductPreferenceScopeV1::Global,
            execution_preset: Some(match legacy.execution {
                ExecutionMode::Cloud => ExecutionPresetV1::Cloud,
                ExecutionMode::Hybrid => ExecutionPresetV1::Hybrid,
                ExecutionMode::Local => ExecutionPresetV1::FullyLocal,
            }),
            performance_preset: Some(match legacy.performance {
                PerformanceMode::Competitive => PerformancePresetV1::Competitive,
                PerformanceMode::Fast => PerformancePresetV1::Fast,
                PerformanceMode::Balanced => PerformancePresetV1::Balanced,
                PerformanceMode::Immersive => PerformancePresetV1::Immersive,
                PerformanceMode::Maximum => PerformancePresetV1::Maximum,
                PerformanceMode::Custom => PerformancePresetV1::Custom,
            }),
            overrides: ProductPreferenceOverridesV1 {
                subtitles: Some(legacy.subtitles),
                input_mode: Some(if legacy.ptt {
                    InputModeV1::Ptt
                } else {
                    InputModeV1::Vad
                }),
                vision: Some(legacy.screen_presence),
                ..ProductPreferenceOverridesV1::default()
            },
        }],
    }
}

fn current_migration() -> PreferenceMigrationV1 {
    PreferenceMigrationV1 {
        state: PreferenceMigrationStateV1::Current,
        from_schema_version: None,
        detail: "Product preferences use the current native schema.".into(),
    }
}

fn validate_document(document: &ProductPreferenceDocumentV1) -> Result<(), ProductPreferenceError> {
    if document.schema_version != SCHEMA_VERSION || document.revision == 0 {
        return Err(ProductPreferenceError::Invalid(
            "unsupported schema or revision".into(),
        ));
    }
    let mut scopes = BTreeSet::new();
    for entry in &document.entries {
        validate_entry(entry)?;
        if !scopes.insert(entry.scope.clone()) {
            return Err(ProductPreferenceError::Invalid(
                "duplicate preference scope".into(),
            ));
        }
    }
    let global = document
        .entries
        .iter()
        .find(|entry| entry.scope == ProductPreferenceScopeV1::Global)
        .ok_or_else(|| {
            ProductPreferenceError::Invalid("global preference scope is required".into())
        })?;
    if global.execution_preset.is_none() || global.performance_preset.is_none() {
        return Err(ProductPreferenceError::Invalid(
            "global presets are required".into(),
        ));
    }
    Ok(())
}

fn validate_entry(entry: &ScopedProductPreferencesV1) -> Result<(), ProductPreferenceError> {
    match &entry.scope {
        ProductPreferenceScopeV1::Global => {}
        ProductPreferenceScopeV1::Game { game_profile_id } => validate_id(game_profile_id)?,
        ProductPreferenceScopeV1::Character {
            game_profile_id,
            character_id,
        } => {
            validate_id(game_profile_id)?;
            validate_id(character_id)?;
        }
    }
    if entry.overrides.creativity.is_some_and(|value| value > 100) {
        return Err(ProductPreferenceError::Invalid(
            "creativity must be from 0 through 100".into(),
        ));
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), ProductPreferenceError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ProductPreferenceError::Invalid(
            "scope identifier is invalid".into(),
        ));
    }
    Ok(())
}

fn load_document(
    path: &Path,
) -> Result<Option<ProductPreferenceDocumentV1>, ProductPreferenceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ProductPreferenceError::Persistence(error.to_string())),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_BYTES {
        return Err(ProductPreferenceError::Persistence(
            "saved preference file is not a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ProductPreferenceError::Persistence(
            "saved preference file exceeds its safety limit".into(),
        ));
    }
    let document = serde_json::from_slice(&bytes)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    validate_document(&document)?;
    Ok(Some(document))
}

fn atomic_write(
    path: &Path,
    document: &ProductPreferenceDocumentV1,
) -> Result<(), ProductPreferenceError> {
    let directory = path.parent().ok_or_else(|| {
        ProductPreferenceError::Persistence("preference directory is unavailable".into())
    })?;
    fs::create_dir_all(directory)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ProductPreferenceError::Persistence(
            "preference document exceeds its safety limit".into(),
        ));
    }
    let mut temporary = NamedTempFile::new_in(directory)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| ProductPreferenceError::Persistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| ProductPreferenceError::Persistence(error.error.to_string()))?;
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource() -> ResourceAuthorityReferenceV1 {
        ResourceAuthorityReferenceV1 {
            selection_id: None,
            admission_status: None,
            admission_receipt_present: false,
            exact_target_pid: None,
            activation_performed: false,
        }
    }

    fn route() -> RouteAuthorityReferenceV1 {
        RouteAuthorityReferenceV1 {
            source_loadout_id: "api-first-stock".into(),
            generation: Some(7),
            sha256: "a".repeat(64),
            activation_performed: false,
        }
    }

    #[test]
    fn api_first_default_is_exact_and_cannot_mint_route_or_fallback_authority() {
        let directory = tempfile::tempdir().expect("tempdir");
        let snapshot =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("manager")
                .snapshot(ProductPreferenceScopeV1::Global, None, resource())
                .expect("snapshot");

        assert_eq!(
            snapshot.migration.state,
            PreferenceMigrationStateV1::MigratedLegacyOnboarding
        );
        assert_eq!(
            snapshot.effective.execution_preset.value,
            ExecutionPresetV1::Cloud
        );
        assert_eq!(
            snapshot.effective.performance_preset.value,
            PerformancePresetV1::Balanced
        );
        assert_eq!(
            snapshot.effective.execution_preset.source_kind,
            EffectiveSourceKindV1::Preset
        );
        assert_eq!(snapshot.effective.creativity.value, 50);
        assert_eq!(snapshot.effective.input_mode.value, InputModeV1::Ptt);
        assert!(snapshot.effective.subtitles.value);
        assert!(!snapshot.effective.overlay.value);
        assert!(snapshot.effective.memory.value);
        assert!(!snapshot.effective.vision.value);
        assert!(!snapshot.effective.webcam_presence.value);
        assert_eq!(
            snapshot.effective.egress.transcript.value,
            EgressDispositionV1::SelectedProviderRoute
        );
        assert_eq!(
            snapshot.effective.egress.captured_game_image.value,
            EgressDispositionV1::Denied
        );
        assert!(snapshot.route_snapshot.is_none());
        assert!(!snapshot.resource_snapshot.admission_receipt_present);
        assert!(!snapshot.automatic_provider_fallback);
        assert!(!snapshot.effective.automatic_provider_fallback);
        assert!(!snapshot.mutation_activated_routes_or_packs);

        let reopened =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("reopen materialized migration")
                .snapshot(ProductPreferenceScopeV1::Global, None, resource())
                .expect("reopened snapshot");
        assert_eq!(
            reopened.migration.state,
            PreferenceMigrationStateV1::Current
        );
        assert_eq!(reopened.revision, snapshot.revision);
        assert_eq!(reopened.effective, snapshot.effective);
    }

    #[test]
    fn execution_and_performance_axes_resolve_independently_for_every_preset() {
        let executions = [
            ExecutionPresetV1::Cloud,
            ExecutionPresetV1::Hybrid,
            ExecutionPresetV1::FullyLocal,
        ];
        let performances = [
            (
                PerformancePresetV1::Competitive,
                VerbosityV1::Concise,
                25,
                ResponseLengthV1::Short,
                InterruptionModeV1::Immediate,
                InputModeV1::Ptt,
                false,
                false,
            ),
            (
                PerformancePresetV1::Fast,
                VerbosityV1::Concise,
                35,
                ResponseLengthV1::Short,
                InterruptionModeV1::FinishSentence,
                InputModeV1::Ptt,
                true,
                false,
            ),
            (
                PerformancePresetV1::Balanced,
                VerbosityV1::Standard,
                50,
                ResponseLengthV1::Medium,
                InterruptionModeV1::FinishSentence,
                InputModeV1::Ptt,
                true,
                false,
            ),
            (
                PerformancePresetV1::Immersive,
                VerbosityV1::Detailed,
                65,
                ResponseLengthV1::Long,
                InterruptionModeV1::FinishSentence,
                InputModeV1::Vad,
                true,
                true,
            ),
            (
                PerformancePresetV1::Maximum,
                VerbosityV1::Detailed,
                75,
                ResponseLengthV1::Long,
                InterruptionModeV1::Disabled,
                InputModeV1::Vad,
                true,
                true,
            ),
            (
                PerformancePresetV1::Custom,
                VerbosityV1::Standard,
                50,
                ResponseLengthV1::Medium,
                InterruptionModeV1::FinishSentence,
                InputModeV1::Ptt,
                true,
                false,
            ),
        ];

        for execution in executions {
            for (
                performance,
                verbosity,
                creativity,
                response_length,
                interruption_mode,
                input_mode,
                emotion,
                vision,
            ) in performances
            {
                let directory = tempfile::tempdir().expect("tempdir");
                let manager =
                    ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                        .expect("manager");
                let initial = manager
                    .snapshot(ProductPreferenceScopeV1::Global, None, resource())
                    .expect("initial");
                let snapshot = manager
                    .save(
                        SaveProductPreferencesRequestV1 {
                            expected_revision: initial.revision,
                            entry: ScopedProductPreferencesV1 {
                                scope: ProductPreferenceScopeV1::Global,
                                execution_preset: Some(execution),
                                performance_preset: Some(performance),
                                overrides: ProductPreferenceOverridesV1::default(),
                            },
                        },
                        None,
                        resource(),
                    )
                    .expect("save cross-axis preset");

                assert_eq!(snapshot.effective.execution_preset.value, execution);
                assert_eq!(snapshot.effective.performance_preset.value, performance);
                assert_eq!(snapshot.effective.verbosity.value, verbosity);
                assert_eq!(snapshot.effective.creativity.value, creativity);
                assert_eq!(snapshot.effective.response_length.value, response_length);
                assert_eq!(
                    snapshot.effective.interruption_mode.value,
                    interruption_mode
                );
                assert_eq!(snapshot.effective.input_mode.value, input_mode);
                assert!(snapshot.effective.subtitles.value);
                assert!(!snapshot.effective.overlay.value);
                assert!(snapshot.effective.memory.value);
                assert_eq!(snapshot.effective.emotion.value, emotion);
                assert_eq!(snapshot.effective.vision.value, vision);
                assert!(!snapshot.effective.webcam_presence.value);
                let expected_egress = if execution == ExecutionPresetV1::FullyLocal {
                    EgressDispositionV1::Denied
                } else {
                    EgressDispositionV1::SelectedProviderRoute
                };
                assert_eq!(snapshot.effective.egress.transcript.value, expected_egress);
                assert_eq!(
                    snapshot.effective.egress.microphone_audio.value,
                    expected_egress
                );
                assert_eq!(
                    snapshot.effective.egress.local_memory_context.value,
                    expected_egress
                );
                assert_eq!(
                    snapshot.effective.egress.captured_game_image.value,
                    if execution == ExecutionPresetV1::Hybrid {
                        EgressDispositionV1::SelectedProviderRoute
                    } else {
                        EgressDispositionV1::Denied
                    }
                );
                assert!(snapshot.route_snapshot.is_none());
                assert!(!snapshot.automatic_provider_fallback);
                assert!(!snapshot.mutation_activated_routes_or_packs);
            }
        }
    }

    #[test]
    fn route_and_resource_snapshots_are_evidence_only_and_never_activation_receipts() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("manager");
        let snapshot = manager
            .snapshot(
                ProductPreferenceScopeV1::Global,
                Some(route()),
                ResourceAuthorityReferenceV1 {
                    selection_id: Some("local-selection-intent".into()),
                    admission_status: Some("notAdmitted".into()),
                    admission_receipt_present: false,
                    exact_target_pid: None,
                    activation_performed: false,
                },
            )
            .expect("snapshot");
        assert_eq!(
            snapshot
                .route_snapshot
                .as_ref()
                .expect("route reference")
                .source_loadout_id,
            "api-first-stock"
        );
        assert!(
            !snapshot
                .route_snapshot
                .as_ref()
                .expect("route reference")
                .activation_performed
        );
        assert!(!snapshot.resource_snapshot.admission_receipt_present);
        assert!(!snapshot.resource_snapshot.activation_performed);
        assert!(!snapshot.automatic_provider_fallback);
        assert!(!snapshot.mutation_activated_routes_or_packs);
    }

    #[test]
    fn scoped_override_inherits_and_round_trips_without_activation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("manager");
        let first = manager
            .snapshot(ProductPreferenceScopeV1::Global, None, resource())
            .expect("first");
        let game = ProductPreferenceScopeV1::Game {
            game_profile_id: "skyrim-special-edition".into(),
        };
        let saved = manager
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: first.revision,
                    entry: ScopedProductPreferencesV1 {
                        scope: game.clone(),
                        execution_preset: None,
                        performance_preset: Some(PerformancePresetV1::Fast),
                        overrides: ProductPreferenceOverridesV1 {
                            subtitles: Some(false),
                            ..ProductPreferenceOverridesV1::default()
                        },
                    },
                },
                None,
                resource(),
            )
            .expect("save");
        assert_eq!(
            saved.effective.performance_preset.value,
            PerformancePresetV1::Fast
        );
        assert!(!saved.effective.subtitles.value);
        assert_eq!(
            saved.effective.execution_preset.source_kind,
            EffectiveSourceKindV1::Inherited
        );
        assert!(!saved.automatic_provider_fallback);
        assert!(!saved.mutation_activated_routes_or_packs);
        drop(manager);
        let reopened =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("reopen");
        assert_eq!(
            reopened
                .snapshot(game, None, resource())
                .expect("snapshot")
                .revision,
            saved.revision
        );
    }

    #[test]
    fn character_scope_resolves_global_game_character_sources_and_reopens_current_schema() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("manager");
        let global = manager
            .snapshot(ProductPreferenceScopeV1::Global, None, resource())
            .expect("global");
        let game_scope = ProductPreferenceScopeV1::Game {
            game_profile_id: "eclipse-harbor".into(),
        };
        let game = manager
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: global.revision,
                    entry: ScopedProductPreferencesV1 {
                        scope: game_scope.clone(),
                        execution_preset: Some(ExecutionPresetV1::Hybrid),
                        performance_preset: Some(PerformancePresetV1::Fast),
                        overrides: ProductPreferenceOverridesV1 {
                            subtitles: Some(false),
                            ..ProductPreferenceOverridesV1::default()
                        },
                    },
                },
                None,
                resource(),
            )
            .expect("game save");
        let character_scope = ProductPreferenceScopeV1::Character {
            game_profile_id: "eclipse-harbor".into(),
            character_id: "mara-venn".into(),
        };
        let character = manager
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: game.revision,
                    entry: ScopedProductPreferencesV1 {
                        scope: character_scope.clone(),
                        execution_preset: None,
                        performance_preset: Some(PerformancePresetV1::Immersive),
                        overrides: ProductPreferenceOverridesV1 {
                            response_length: Some(ResponseLengthV1::Short),
                            ..ProductPreferenceOverridesV1::default()
                        },
                    },
                },
                None,
                resource(),
            )
            .expect("character save");

        assert_eq!(
            character.effective.execution_preset.value,
            ExecutionPresetV1::Hybrid
        );
        assert_eq!(
            character.effective.execution_preset.source_scope,
            game_scope
        );
        assert_eq!(
            character.effective.execution_preset.source_kind,
            EffectiveSourceKindV1::Inherited
        );
        assert_eq!(
            character.effective.performance_preset.value,
            PerformancePresetV1::Immersive
        );
        assert_eq!(
            character.effective.performance_preset.source_scope,
            character_scope
        );
        assert!(!character.effective.subtitles.value);
        assert_eq!(character.effective.subtitles.source_scope, game_scope);
        assert_eq!(
            character.effective.subtitles.source_kind,
            EffectiveSourceKindV1::Inherited
        );
        assert_eq!(
            character.effective.response_length.value,
            ResponseLengthV1::Short
        );
        assert_eq!(
            character.effective.response_length.source_kind,
            EffectiveSourceKindV1::Override
        );

        drop(manager);
        let reopened =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("reopen");
        let reopened_snapshot = reopened
            .snapshot(character_scope, None, resource())
            .expect("reopened character snapshot");
        assert_eq!(
            reopened_snapshot.migration.state,
            PreferenceMigrationStateV1::Current
        );
        assert_eq!(reopened_snapshot.revision, character.revision);
        assert_eq!(reopened_snapshot.effective, character.effective);
    }

    #[test]
    fn invalid_saved_preferences_recover_bounded_api_first_defaults_without_activation() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(directory.path().join(FILE_NAME), b"{not-json")
            .expect("write invalid preferences");
        let snapshot = ProductPreferenceManager::new(
            directory.path(),
            &PreferenceSnapshot {
                execution: ExecutionMode::Local,
                performance: PerformanceMode::Maximum,
                local_only: true,
                ..PreferenceSnapshot::default()
            },
        )
        .expect("recover manager")
        .snapshot(ProductPreferenceScopeV1::Global, None, resource())
        .expect("recovered snapshot");
        assert_eq!(
            snapshot.migration.state,
            PreferenceMigrationStateV1::RecoveredDefaults
        );
        assert_eq!(
            snapshot.effective.execution_preset.value,
            ExecutionPresetV1::Cloud
        );
        assert_eq!(
            snapshot.effective.performance_preset.value,
            PerformancePresetV1::Balanced
        );
        assert!(snapshot.route_snapshot.is_none());
        assert!(!snapshot.resource_snapshot.admission_receipt_present);
        assert!(!snapshot.automatic_provider_fallback);
        assert!(!snapshot.mutation_activated_routes_or_packs);
    }

    #[test]
    fn reset_requires_confirmation_and_revision_match() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager =
            ProductPreferenceManager::new(directory.path(), &PreferenceSnapshot::default())
                .expect("manager");
        let first = manager
            .snapshot(ProductPreferenceScopeV1::Global, None, resource())
            .expect("first");
        assert!(matches!(
            manager.reset(
                ResetProductPreferencesRequestV1 {
                    expected_revision: first.revision,
                    scope: ProductPreferenceScopeV1::Global,
                    explicit_user_confirmation: false
                },
                None,
                resource()
            ),
            Err(ProductPreferenceError::ConfirmationRequired)
        ));
        assert!(matches!(
            manager.save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: first.revision + 1,
                    entry: ScopedProductPreferencesV1 {
                        scope: ProductPreferenceScopeV1::Global,
                        execution_preset: Some(ExecutionPresetV1::Cloud),
                        performance_preset: Some(PerformancePresetV1::Balanced),
                        overrides: ProductPreferenceOverridesV1::default()
                    }
                },
                None,
                resource()
            ),
            Err(ProductPreferenceError::RevisionConflict)
        ));
    }

    #[test]
    fn fully_local_intent_denies_all_egress_without_claiming_activation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let legacy = PreferenceSnapshot {
            execution: ExecutionMode::Local,
            local_only: true,
            ..PreferenceSnapshot::default()
        };
        let snapshot = ProductPreferenceManager::new(directory.path(), &legacy)
            .expect("manager")
            .snapshot(ProductPreferenceScopeV1::Global, None, resource())
            .expect("snapshot");
        assert_eq!(
            snapshot.effective.egress.transcript.value,
            EgressDispositionV1::Denied
        );
        assert_eq!(
            snapshot.effective.egress.microphone_audio.value,
            EgressDispositionV1::Denied
        );
        assert_eq!(
            snapshot.effective.egress.captured_game_image.value,
            EgressDispositionV1::Denied
        );
        assert_eq!(
            snapshot.effective.egress.local_memory_context.value,
            EgressDispositionV1::Denied
        );
        assert!(!snapshot.mutation_activated_routes_or_packs);
    }

    #[test]
    fn webcam_presence_is_independent_opt_in_and_legacy_vision_never_grants_consent() {
        let directory = tempfile::tempdir().expect("tempdir");
        let legacy = PreferenceSnapshot {
            screen_presence: true,
            ..PreferenceSnapshot::default()
        };
        let manager = ProductPreferenceManager::new(directory.path(), &legacy).expect("manager");
        let initial = manager
            .snapshot(ProductPreferenceScopeV1::Global, None, resource())
            .expect("snapshot");
        assert!(initial.effective.vision.value);
        assert!(!initial.effective.webcam_presence.value);

        let opted_in = manager
            .save(
                SaveProductPreferencesRequestV1 {
                    expected_revision: initial.revision,
                    entry: ScopedProductPreferencesV1 {
                        scope: ProductPreferenceScopeV1::Global,
                        execution_preset: Some(initial.effective.execution_preset.value),
                        performance_preset: Some(initial.effective.performance_preset.value),
                        overrides: ProductPreferenceOverridesV1 {
                            webcam_presence: Some(true),
                            ..ProductPreferenceOverridesV1::default()
                        },
                    },
                },
                None,
                resource(),
            )
            .expect("persist explicit opt in");
        assert!(opted_in.effective.webcam_presence.value);
        assert_eq!(
            opted_in.effective.webcam_presence.source_kind,
            EffectiveSourceKindV1::Override
        );
        assert!(!opted_in.mutation_activated_routes_or_packs);
        assert!(!opted_in.automatic_provider_fallback);
    }
}
