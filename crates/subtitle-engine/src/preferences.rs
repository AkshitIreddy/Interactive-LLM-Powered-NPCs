use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::{
    FontCatalog, FontSource, GenericFontFamily, ScriptClass, StyleManifest, SubtitleStyle,
    ASSET_LICENSES_JSON, DEFAULT_STYLES_JSON, FONT_CATALOG_JSON,
};

pub const SUBTITLE_PREFERENCE_SCHEMA_VERSION: u32 = 1;
pub const SUBTITLE_PREFERENCES_FILE_NAME: &str = "subtitle-preferences-v1.json";
pub const SUPPORTED_SUBTITLE_OVERRIDE_FIELDS: [&str; 4] =
    ["safeAreaDp", "textScale", "backplateEnabled", "opacity"];

const LEGACY_SCHEMA_VERSION: u32 = 0;
const MAX_DOCUMENT_BYTES: u64 = 128 * 1024;
const MAX_SCOPE_ID_BYTES: usize = 128;
const MIN_SAFE_AREA_DP: f32 = 0.0;
const MAX_SAFE_AREA_DP: f32 = 256.0;
const MIN_TEXT_SCALE: f32 = 0.75;
const MAX_TEXT_SCALE: f32 = 2.0;
const MIN_OPACITY: f32 = 0.25;
const MAX_OPACITY: f32 = 1.0;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SubtitlePreferenceScopeV1 {
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitlePreferenceOverridesV1 {
    /// Maps to the layout engine's safe margin and the native presenter's
    /// `BottomCenterFallback.safe_margin_px` after the trusted DPI conversion.
    pub safe_area_dp: Option<f32>,
    /// Multiplies the selected style's body and speaker sizes before native
    /// DirectWrite shaping. The actual physical-pixel size remains subject to
    /// the presenter's 6..=256 px fail-closed bound.
    pub text_scale: Option<f32>,
    /// Maps directly to `ResolvedStyle.backplate.enabled`.
    pub backplate_enabled: Option<bool>,
    /// Maps directly to `ResolvedStyle.global_alpha`.
    pub opacity: Option<f32>,
}

impl SubtitlePreferenceOverridesV1 {
    fn is_empty(&self) -> bool {
        self.safe_area_dp.is_none()
            && self.text_scale.is_none()
            && self.backplate_enabled.is_none()
            && self.opacity.is_none()
    }

    fn validate(&self) -> Result<(), SubtitlePreferenceError> {
        validate_optional_range(
            "safeAreaDp",
            self.safe_area_dp,
            MIN_SAFE_AREA_DP,
            MAX_SAFE_AREA_DP,
        )?;
        validate_optional_range("textScale", self.text_scale, MIN_TEXT_SCALE, MAX_TEXT_SCALE)?;
        validate_optional_range("opacity", self.opacity, MIN_OPACITY, MAX_OPACITY)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedSubtitlePreferencesV1 {
    pub scope: SubtitlePreferenceScopeV1,
    /// `None` means inherit from the next broader scope, ultimately falling
    /// back to the validated bundled manifest default.
    pub selected_style_id: Option<String>,
    pub overrides: SubtitlePreferenceOverridesV1,
}

impl ScopedSubtitlePreferencesV1 {
    fn is_empty(&self) -> bool {
        self.selected_style_id.is_none() && self.overrides.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubtitlePreferenceDocumentV1 {
    schema_version: u32,
    revision: u64,
    entries: Vec<ScopedSubtitlePreferencesV1>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubtitlePreferenceDocumentV0 {
    schema_version: u32,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    selected_style_id: Option<String>,
    #[serde(default)]
    overrides: SubtitlePreferenceOverridesV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleEffectiveSourceKindV1 {
    BundledManifestDefault,
    BundledStyle,
    RendererDefault,
    PersistedScope,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleEffectiveSourceV1 {
    pub kind: SubtitleEffectiveSourceKindV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<SubtitlePreferenceScopeV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleEffectiveValueV1<T> {
    pub value: T,
    pub source: SubtitleEffectiveSourceV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveSubtitlePreferencesV1 {
    pub requested_scope: SubtitlePreferenceScopeV1,
    pub selected_style_id: SubtitleEffectiveValueV1<String>,
    pub safe_area_dp: SubtitleEffectiveValueV1<f32>,
    pub text_scale: SubtitleEffectiveValueV1<f32>,
    pub backplate_enabled: SubtitleEffectiveValueV1<bool>,
    pub opacity: SubtitleEffectiveValueV1<f32>,
    /// Exact renderer-facing values after applying only the supported
    /// preference fields. Unsupported style fields never enter this DTO.
    pub renderer_parameters: SubtitleRendererParametersV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleRendererParametersV1 {
    pub style_id: String,
    pub safe_area_dp: f32,
    pub body_size_dp: f32,
    pub speaker_size_dp: f32,
    pub backplate_enabled: bool,
    pub global_opacity: f32,
}

/// Full internal state for a runtime adapter. It is intentionally not a wire
/// DTO: Tauri/UI callers receive the bounded renderer parameters above rather
/// than a mutation surface for unsupported color, font, animation, or HDR
/// fields.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSubtitlePreferences {
    pub style: SubtitleStyle,
    pub global_opacity: f32,
}

/// Exact immutable renderer authority pinned under the same preference-state
/// lock as its revision. The digest covers every field other than the digest
/// itself, including the resolved style and the provenance of every supported
/// override. It is an integrity/binding value for private native sidecars, not
/// a user-controlled signature or a remote trust claim.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleRendererAuthorityV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub requested_scope: SubtitlePreferenceScopeV1,
    pub sources: SubtitleRendererAuthoritySourcesV1,
    /// Fully resolved, validated style after safe-area, text-scale, and
    /// backplate overrides have been applied.
    pub style: SubtitleStyle,
    /// Preserved separately from the scaled type sizes so receipts can prove
    /// which accessibility control was consumed.
    pub text_scale: f32,
    /// Applied by the native renderer after all per-color alpha values.
    pub opacity: f32,
    pub authority_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleRendererAuthoritySourcesV1 {
    pub selected_style: SubtitleEffectiveSourceV1,
    pub safe_area: SubtitleEffectiveSourceV1,
    pub text_scale: SubtitleEffectiveSourceV1,
    pub backplate: SubtitleEffectiveSourceV1,
    pub opacity: SubtitleEffectiveSourceV1,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleRendererAuthorityDigestMaterial<'a> {
    schema_version: u32,
    revision: u64,
    requested_scope: &'a SubtitlePreferenceScopeV1,
    sources: &'a SubtitleRendererAuthoritySourcesV1,
    style: &'a SubtitleStyle,
    text_scale: f32,
    opacity: f32,
}

impl SubtitleRendererAuthorityV1 {
    pub fn validate(&self) -> Result<(), SubtitlePreferenceError> {
        if self.schema_version != SUBTITLE_PREFERENCE_SCHEMA_VERSION {
            return Err(SubtitlePreferenceError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        validate_scope(&self.requested_scope)?;
        self.style.validate().map_err(|error| {
            SubtitlePreferenceError::Invalid(format!(
                "renderer authority style is invalid: {error}"
            ))
        })?;
        validate_optional_range(
            "textScale",
            Some(self.text_scale),
            MIN_TEXT_SCALE,
            MAX_TEXT_SCALE,
        )?;
        validate_optional_range("opacity", Some(self.opacity), MIN_OPACITY, MAX_OPACITY)?;
        if self.authority_sha256.len() != 64
            || !self
                .authority_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.authority_sha256 != renderer_authority_digest(self)?
        {
            return Err(SubtitlePreferenceError::Invalid(
                "renderer authority digest does not match its atomic payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AvailableSubtitleStyleV1 {
    pub style_id: String,
    pub body_font_role: String,
    pub speaker_font_role: String,
    pub safe_area_dp: f32,
    pub body_size_dp: f32,
    pub speaker_size_dp: f32,
    pub backplate_enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleFontAvailabilityV1 {
    SystemLookupRequired,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleFontFamilyDisclosureV1 {
    pub family: String,
    pub platforms: Vec<String>,
    pub scripts: Vec<ScriptClass>,
    pub availability: SubtitleFontAvailabilityV1,
    pub license_id: String,
    pub binary_bundled: bool,
    pub redistribution: String,
    pub usage_basis: String,
    pub reference: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleFontRoleDisclosureV1 {
    pub role_id: String,
    pub fallback_chain: Vec<SubtitleFontFamilyDisclosureV1>,
    pub generic_fallback: GenericFontFamily,
    pub shaping_features: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleAssetDisclosureV1 {
    pub no_font_binaries_bundled: bool,
    pub generated_asset_policy: String,
    pub font_roles: Vec<SubtitleFontRoleDisclosureV1>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitlePreferenceMigrationStateV1 {
    Current,
    InitializedDefaults,
    MigratedLegacyV0,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitlePreferenceMigrationV1 {
    pub state: SubtitlePreferenceMigrationStateV1,
    pub from_schema_version: Option<u32>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitlePreferenceSnapshotV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub entries: Vec<ScopedSubtitlePreferencesV1>,
    pub effective: EffectiveSubtitlePreferencesV1,
    pub available_styles: Vec<AvailableSubtitleStyleV1>,
    pub assets: SubtitleAssetDisclosureV1,
    pub supported_override_fields: Vec<String>,
    pub migration: SubtitlePreferenceMigrationV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadSubtitlePreferencesRequestV1 {
    pub scope: SubtitlePreferenceScopeV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveSubtitlePreferencesRequestV1 {
    pub expected_revision: u64,
    pub entry: ScopedSubtitlePreferencesV1,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetSubtitlePreferencesRequestV1 {
    pub expected_revision: u64,
    pub scope: SubtitlePreferenceScopeV1,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Error)]
pub enum SubtitlePreferenceError {
    #[error("subtitle preference state is temporarily unavailable")]
    State,
    #[error("subtitle preference request is invalid: {0}")]
    Invalid(String),
    #[error("subtitle preference revision changed; refresh before saving")]
    RevisionConflict,
    #[error("subtitle preference reset requires explicit user confirmation")]
    ConfirmationRequired,
    #[error("subtitle preference schema {0} is not supported")]
    UnsupportedSchema(u32),
    #[error("subtitle preference assets are invalid: {0}")]
    InvalidAssets(String),
    #[error("subtitle preference persistence failed: {0}")]
    Persistence(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct SubtitleAssetLicenseManifest {
    schema_version: u32,
    generated_asset_policy: String,
    assets: Vec<SubtitleAssetLicenseEntry>,
    font_license_references: Vec<SubtitleFontLicenseReference>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct SubtitleAssetLicenseEntry {
    path: String,
    kind: String,
    copyright: String,
    license_expression: String,
    redistribution: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct SubtitleFontLicenseReference {
    license_id: String,
    applies_to: Vec<String>,
    binary_bundled: bool,
    redistribution: String,
    usage_basis: String,
    reference: Option<String>,
}

struct SubtitlePreferenceAssets {
    styles: StyleManifest,
    disclosure: SubtitleAssetDisclosureV1,
}

struct SubtitlePreferenceState {
    document: SubtitlePreferenceDocumentV1,
    migration: SubtitlePreferenceMigrationV1,
}

pub struct SubtitlePreferenceManager {
    path: PathBuf,
    assets: SubtitlePreferenceAssets,
    state: Mutex<SubtitlePreferenceState>,
}

impl SubtitlePreferenceManager {
    pub fn new(config_directory: &Path) -> Result<Self, SubtitlePreferenceError> {
        Self::new_with_assets(
            config_directory,
            DEFAULT_STYLES_JSON,
            FONT_CATALOG_JSON,
            ASSET_LICENSES_JSON,
        )
    }

    fn new_with_assets(
        config_directory: &Path,
        styles_json: &str,
        fonts_json: &str,
        licenses_json: &str,
    ) -> Result<Self, SubtitlePreferenceError> {
        let assets = validate_assets(styles_json, fonts_json, licenses_json)?;
        let path = config_directory.join(SUBTITLE_PREFERENCES_FILE_NAME);
        let (document, migration, persist_now) = load_document(&path)?;
        validate_document(&document, &assets.styles)?;
        if persist_now {
            atomic_write(&path, &document)?;
        }
        Ok(Self {
            path,
            assets,
            state: Mutex::new(SubtitlePreferenceState {
                document,
                migration,
            }),
        })
    }

    pub fn read(
        &self,
        request: ReadSubtitlePreferencesRequestV1,
    ) -> Result<SubtitlePreferenceSnapshotV1, SubtitlePreferenceError> {
        validate_scope(&request.scope)?;
        let state = self
            .state
            .lock()
            .map_err(|_| SubtitlePreferenceError::State)?;
        self.snapshot_from_state(&state, request.scope)
    }

    pub fn save(
        &self,
        request: SaveSubtitlePreferencesRequestV1,
    ) -> Result<SubtitlePreferenceSnapshotV1, SubtitlePreferenceError> {
        validate_entry(&request.entry, &self.assets.styles)?;
        if request.entry.is_empty() {
            return Err(SubtitlePreferenceError::Invalid(
                "empty subtitle preferences must use the explicit reset operation".into(),
            ));
        }
        let requested_scope = request.entry.scope.clone();
        let mut state = self
            .state
            .lock()
            .map_err(|_| SubtitlePreferenceError::State)?;
        if request.expected_revision != state.document.revision {
            return Err(SubtitlePreferenceError::RevisionConflict);
        }

        let mut candidate = state.document.clone();
        candidate
            .entries
            .retain(|entry| entry.scope != request.entry.scope);
        candidate.entries.push(request.entry);
        canonicalize_entries(&mut candidate.entries);
        candidate.revision = candidate.revision.saturating_add(1);
        validate_document(&candidate, &self.assets.styles)?;
        atomic_write(&self.path, &candidate)?;
        state.document = candidate;
        state.migration = current_migration();
        self.snapshot_from_state(&state, requested_scope)
    }

    pub fn reset(
        &self,
        request: ResetSubtitlePreferencesRequestV1,
    ) -> Result<SubtitlePreferenceSnapshotV1, SubtitlePreferenceError> {
        validate_scope(&request.scope)?;
        if !request.explicit_user_confirmation {
            return Err(SubtitlePreferenceError::ConfirmationRequired);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| SubtitlePreferenceError::State)?;
        if request.expected_revision != state.document.revision {
            return Err(SubtitlePreferenceError::RevisionConflict);
        }
        let mut candidate = state.document.clone();
        candidate
            .entries
            .retain(|entry| entry.scope != request.scope);
        candidate.revision = candidate.revision.saturating_add(1);
        validate_document(&candidate, &self.assets.styles)?;
        atomic_write(&self.path, &candidate)?;
        state.document = candidate;
        state.migration = current_migration();
        self.snapshot_from_state(&state, request.scope)
    }

    pub fn resolve_for_renderer(
        &self,
        scope: SubtitlePreferenceScopeV1,
    ) -> Result<ResolvedSubtitlePreferences, SubtitlePreferenceError> {
        validate_scope(&scope)?;
        let state = self
            .state
            .lock()
            .map_err(|_| SubtitlePreferenceError::State)?;
        let resolved = resolve_effective(&state.document, &scope, &self.assets.styles)?;
        Ok(ResolvedSubtitlePreferences {
            style: resolved.style,
            global_opacity: resolved.opacity.value,
        })
    }

    /// Pins the exact effective style, supported override provenance, revision,
    /// and digest in one critical section. Callers must pass this value through
    /// unchanged; resolving a snapshot and reading a revision separately would
    /// permit a save/reset race between the two operations.
    pub fn resolve_renderer_authority(
        &self,
        scope: SubtitlePreferenceScopeV1,
    ) -> Result<SubtitleRendererAuthorityV1, SubtitlePreferenceError> {
        validate_scope(&scope)?;
        let state = self
            .state
            .lock()
            .map_err(|_| SubtitlePreferenceError::State)?;
        let resolved = resolve_effective(&state.document, &scope, &self.assets.styles)?;
        build_renderer_authority(state.document.revision, scope, resolved)
    }
}

/// Resolves revision-zero bundled defaults without reading or creating user
/// preference storage. This is intended for deterministic fixtures and
/// pre-storage native bootstrap paths; ordinary product turns must use
/// [`SubtitlePreferenceManager::resolve_renderer_authority`].
pub fn bundled_default_renderer_authority(
) -> Result<SubtitleRendererAuthorityV1, SubtitlePreferenceError> {
    let assets = validate_assets(DEFAULT_STYLES_JSON, FONT_CATALOG_JSON, ASSET_LICENSES_JSON)?;
    let scope = SubtitlePreferenceScopeV1::Global;
    let document = SubtitlePreferenceDocumentV1 {
        schema_version: SUBTITLE_PREFERENCE_SCHEMA_VERSION,
        revision: 0,
        entries: Vec::new(),
    };
    let resolved = resolve_effective(&document, &scope, &assets.styles)?;
    build_renderer_authority(0, scope, resolved)
}

fn build_renderer_authority(
    revision: u64,
    scope: SubtitlePreferenceScopeV1,
    resolved: EffectiveResolution,
) -> Result<SubtitleRendererAuthorityV1, SubtitlePreferenceError> {
    let sources = SubtitleRendererAuthoritySourcesV1 {
        selected_style: resolved.style_id.source.clone(),
        safe_area: resolved.safe_area_dp.source.clone(),
        text_scale: resolved.text_scale.source.clone(),
        backplate: resolved.backplate_enabled.source.clone(),
        opacity: resolved.opacity.source.clone(),
    };
    let mut authority = SubtitleRendererAuthorityV1 {
        schema_version: SUBTITLE_PREFERENCE_SCHEMA_VERSION,
        revision,
        requested_scope: scope,
        sources,
        style: resolved.style,
        text_scale: resolved.text_scale.value,
        opacity: resolved.opacity.value,
        authority_sha256: String::new(),
    };
    authority.authority_sha256 = renderer_authority_digest(&authority)?;
    authority.validate()?;
    Ok(authority)
}

impl SubtitlePreferenceManager {
    fn snapshot_from_state(
        &self,
        state: &SubtitlePreferenceState,
        scope: SubtitlePreferenceScopeV1,
    ) -> Result<SubtitlePreferenceSnapshotV1, SubtitlePreferenceError> {
        let resolved = resolve_effective(&state.document, &scope, &self.assets.styles)?;
        let renderer_parameters = SubtitleRendererParametersV1 {
            style_id: resolved.style.id.clone(),
            safe_area_dp: resolved.style.geometry.safe_margin_dp,
            body_size_dp: resolved.style.typography.body_size_dp,
            speaker_size_dp: resolved.style.typography.speaker_size_dp,
            backplate_enabled: resolved.style.effects.backplate.enabled,
            global_opacity: resolved.opacity.value,
        };
        Ok(SubtitlePreferenceSnapshotV1 {
            schema_version: SUBTITLE_PREFERENCE_SCHEMA_VERSION,
            revision: state.document.revision,
            entries: state.document.entries.clone(),
            effective: EffectiveSubtitlePreferencesV1 {
                requested_scope: scope,
                selected_style_id: resolved.style_id,
                safe_area_dp: resolved.safe_area_dp,
                text_scale: resolved.text_scale,
                backplate_enabled: resolved.backplate_enabled,
                opacity: resolved.opacity,
                renderer_parameters,
            },
            available_styles: self
                .assets
                .styles
                .styles
                .iter()
                .map(available_style)
                .collect(),
            assets: self.assets.disclosure.clone(),
            supported_override_fields: SUPPORTED_SUBTITLE_OVERRIDE_FIELDS
                .iter()
                .map(|field| (*field).to_owned())
                .collect(),
            migration: state.migration.clone(),
        })
    }
}

fn renderer_authority_digest(
    authority: &SubtitleRendererAuthorityV1,
) -> Result<String, SubtitlePreferenceError> {
    let bytes = serde_json::to_vec(&SubtitleRendererAuthorityDigestMaterial {
        schema_version: authority.schema_version,
        revision: authority.revision,
        requested_scope: &authority.requested_scope,
        sources: &authority.sources,
        style: &authority.style,
        text_scale: authority.text_scale,
        opacity: authority.opacity,
    })
    .map_err(|error| SubtitlePreferenceError::Invalid(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

struct EffectiveResolution {
    style_id: SubtitleEffectiveValueV1<String>,
    safe_area_dp: SubtitleEffectiveValueV1<f32>,
    text_scale: SubtitleEffectiveValueV1<f32>,
    backplate_enabled: SubtitleEffectiveValueV1<bool>,
    opacity: SubtitleEffectiveValueV1<f32>,
    style: SubtitleStyle,
}

fn resolve_effective(
    document: &SubtitlePreferenceDocumentV1,
    requested_scope: &SubtitlePreferenceScopeV1,
    styles: &StyleManifest,
) -> Result<EffectiveResolution, SubtitlePreferenceError> {
    let chain = scope_chain(requested_scope);
    let entries = chain
        .iter()
        .filter_map(|scope| document.entries.iter().find(|entry| &entry.scope == scope))
        .collect::<Vec<_>>();
    let selected = entries.iter().rev().find_map(|entry| {
        entry
            .selected_style_id
            .as_ref()
            .map(|style_id| (style_id.clone(), entry.scope.clone()))
    });
    let (style_id, style_source) = match selected {
        Some((style_id, scope)) => (style_id, persisted_source(scope)),
        None => (
            styles.default_style_id.clone(),
            SubtitleEffectiveSourceV1 {
                kind: SubtitleEffectiveSourceKindV1::BundledManifestDefault,
                scope: None,
                style_id: Some(styles.default_style_id.clone()),
            },
        ),
    };
    let mut style = styles
        .style(&style_id)
        .cloned()
        .ok_or_else(|| SubtitlePreferenceError::Invalid(format!("unknown style `{style_id}`")))?;
    let safe_area_dp = effective_override(
        &entries,
        |entry| entry.overrides.safe_area_dp,
        style.geometry.safe_margin_dp,
        bundled_style_source(&style_id),
    );
    let text_scale = effective_override(
        &entries,
        |entry| entry.overrides.text_scale,
        1.0,
        renderer_default_source(),
    );
    let backplate_enabled = effective_override(
        &entries,
        |entry| entry.overrides.backplate_enabled,
        style.effects.backplate.enabled,
        bundled_style_source(&style_id),
    );
    let opacity = effective_override(
        &entries,
        |entry| entry.overrides.opacity,
        1.0,
        renderer_default_source(),
    );

    style.geometry.safe_margin_dp = safe_area_dp.value;
    style.typography.body_size_dp *= text_scale.value;
    style.typography.speaker_size_dp *= text_scale.value;
    style.effects.backplate.enabled = backplate_enabled.value;
    style.validate().map_err(|error| {
        SubtitlePreferenceError::Invalid(format!(
            "effective style is outside the renderer contract: {error}"
        ))
    })?;
    Ok(EffectiveResolution {
        style_id: SubtitleEffectiveValueV1 {
            value: style_id,
            source: style_source,
        },
        safe_area_dp,
        text_scale,
        backplate_enabled,
        opacity,
        style,
    })
}

fn effective_override<T: Copy>(
    entries: &[&ScopedSubtitlePreferencesV1],
    select: impl Fn(&ScopedSubtitlePreferencesV1) -> Option<T>,
    fallback: T,
    fallback_source: SubtitleEffectiveSourceV1,
) -> SubtitleEffectiveValueV1<T> {
    if let Some((value, scope)) = entries
        .iter()
        .rev()
        .find_map(|entry| select(entry).map(|value| (value, entry.scope.clone())))
    {
        SubtitleEffectiveValueV1 {
            value,
            source: persisted_source(scope),
        }
    } else {
        SubtitleEffectiveValueV1 {
            value: fallback,
            source: fallback_source,
        }
    }
}

fn persisted_source(scope: SubtitlePreferenceScopeV1) -> SubtitleEffectiveSourceV1 {
    SubtitleEffectiveSourceV1 {
        kind: SubtitleEffectiveSourceKindV1::PersistedScope,
        scope: Some(scope),
        style_id: None,
    }
}

fn bundled_style_source(style_id: &str) -> SubtitleEffectiveSourceV1 {
    SubtitleEffectiveSourceV1 {
        kind: SubtitleEffectiveSourceKindV1::BundledStyle,
        scope: None,
        style_id: Some(style_id.to_owned()),
    }
}

fn renderer_default_source() -> SubtitleEffectiveSourceV1 {
    SubtitleEffectiveSourceV1 {
        kind: SubtitleEffectiveSourceKindV1::RendererDefault,
        scope: None,
        style_id: None,
    }
}

fn scope_chain(scope: &SubtitlePreferenceScopeV1) -> Vec<SubtitlePreferenceScopeV1> {
    match scope {
        SubtitlePreferenceScopeV1::Global => vec![SubtitlePreferenceScopeV1::Global],
        SubtitlePreferenceScopeV1::Game { game_profile_id } => vec![
            SubtitlePreferenceScopeV1::Global,
            SubtitlePreferenceScopeV1::Game {
                game_profile_id: game_profile_id.clone(),
            },
        ],
        SubtitlePreferenceScopeV1::Character {
            game_profile_id,
            character_id,
        } => vec![
            SubtitlePreferenceScopeV1::Global,
            SubtitlePreferenceScopeV1::Game {
                game_profile_id: game_profile_id.clone(),
            },
            SubtitlePreferenceScopeV1::Character {
                game_profile_id: game_profile_id.clone(),
                character_id: character_id.clone(),
            },
        ],
    }
}

fn validate_assets(
    styles_json: &str,
    fonts_json: &str,
    licenses_json: &str,
) -> Result<SubtitlePreferenceAssets, SubtitlePreferenceError> {
    let styles: StyleManifest = serde_json::from_str(styles_json)
        .map_err(|error| SubtitlePreferenceError::InvalidAssets(error.to_string()))?;
    styles
        .validate()
        .map_err(|error| SubtitlePreferenceError::InvalidAssets(error.to_string()))?;
    let mut style_ids = BTreeSet::new();
    if styles
        .styles
        .iter()
        .any(|style| !style_ids.insert(style.id.as_str()))
    {
        return Err(SubtitlePreferenceError::InvalidAssets(
            "style ids must be unique".into(),
        ));
    }

    let fonts: FontCatalog = serde_json::from_str(fonts_json)
        .map_err(|error| SubtitlePreferenceError::InvalidAssets(error.to_string()))?;
    fonts
        .validate()
        .map_err(|error| SubtitlePreferenceError::InvalidAssets(error.to_string()))?;
    for style in &styles.styles {
        if fonts.role(&style.typography.body_font_role).is_none()
            || fonts.role(&style.typography.speaker_font_role).is_none()
        {
            return Err(SubtitlePreferenceError::InvalidAssets(format!(
                "style `{}` references an unavailable font role",
                style.id
            )));
        }
    }

    let licenses: SubtitleAssetLicenseManifest = serde_json::from_str(licenses_json)
        .map_err(|error| SubtitlePreferenceError::InvalidAssets(error.to_string()))?;
    if licenses.schema_version != 1
        || licenses.assets.len() != 3
        || licenses.generated_asset_policy.trim().is_empty()
    {
        return Err(SubtitlePreferenceError::InvalidAssets(
            "asset license manifest is incomplete".into(),
        ));
    }
    let required_assets = BTreeSet::from([
        "assets/subtitles/styles.v1.json",
        "assets/subtitles/fonts.v1.json",
        "assets/subtitles/licenses.v1.json",
    ]);
    let declared_assets = licenses
        .assets
        .iter()
        .map(|asset| asset.path.as_str())
        .collect::<BTreeSet<_>>();
    if required_assets != declared_assets
        || licenses.assets.iter().any(|asset| {
            asset.kind.trim().is_empty()
                || asset.copyright.trim().is_empty()
                || asset.license_expression != "MIT"
                || asset.redistribution != "included"
        })
    {
        return Err(SubtitlePreferenceError::InvalidAssets(
            "subtitle asset license entries do not match the bundled metadata".into(),
        ));
    }

    let mut license_by_id = BTreeMap::new();
    for disclosure in licenses.font_license_references {
        if disclosure.license_id.trim().is_empty()
            || disclosure.binary_bundled
            || disclosure.redistribution.trim().is_empty()
            || disclosure.usage_basis.trim().is_empty()
            || license_by_id
                .insert(disclosure.license_id.clone(), disclosure)
                .is_some()
        {
            return Err(SubtitlePreferenceError::InvalidAssets(
                "font license references must be unique and non-bundled".into(),
            ));
        }
    }

    let mut role_disclosures = Vec::with_capacity(fonts.roles.len());
    for role in &fonts.roles {
        let mut fallback_chain = Vec::with_capacity(role.system_families.len());
        for family in &role.system_families {
            if family.source != FontSource::OperatingSystem || family.redistribute {
                return Err(SubtitlePreferenceError::InvalidAssets(format!(
                    "font `{}` is not a system-only fallback",
                    family.family
                )));
            }
            let license = license_by_id.get(&family.license_id).ok_or_else(|| {
                SubtitlePreferenceError::InvalidAssets(format!(
                    "font `{}` has no license disclosure",
                    family.family
                ))
            })?;
            if !license.applies_to.iter().any(|name| name == &family.family) {
                return Err(SubtitlePreferenceError::InvalidAssets(format!(
                    "font `{}` is absent from its license disclosure",
                    family.family
                )));
            }
            fallback_chain.push(SubtitleFontFamilyDisclosureV1 {
                family: family.family.clone(),
                platforms: family.platforms.clone(),
                scripts: family.scripts.clone(),
                availability: SubtitleFontAvailabilityV1::SystemLookupRequired,
                license_id: family.license_id.clone(),
                binary_bundled: false,
                redistribution: license.redistribution.clone(),
                usage_basis: license.usage_basis.clone(),
                reference: license.reference.clone(),
            });
        }
        role_disclosures.push(SubtitleFontRoleDisclosureV1 {
            role_id: role.id.clone(),
            fallback_chain,
            generic_fallback: role.generic_fallback,
            shaping_features: role.shaping_features.clone(),
        });
    }

    Ok(SubtitlePreferenceAssets {
        styles,
        disclosure: SubtitleAssetDisclosureV1 {
            no_font_binaries_bundled: true,
            generated_asset_policy: licenses.generated_asset_policy,
            font_roles: role_disclosures,
        },
    })
}

fn validate_document(
    document: &SubtitlePreferenceDocumentV1,
    styles: &StyleManifest,
) -> Result<(), SubtitlePreferenceError> {
    if document.schema_version != SUBTITLE_PREFERENCE_SCHEMA_VERSION {
        return Err(SubtitlePreferenceError::UnsupportedSchema(
            document.schema_version,
        ));
    }
    if document.entries.len() > 512 {
        return Err(SubtitlePreferenceError::Invalid(
            "too many subtitle preference scopes".into(),
        ));
    }
    let mut scopes = BTreeSet::new();
    for entry in &document.entries {
        validate_entry(entry, styles)?;
        if entry.is_empty() {
            return Err(SubtitlePreferenceError::Invalid(
                "empty subtitle preference entries are not persisted".into(),
            ));
        }
        if !scopes.insert(entry.scope.clone()) {
            return Err(SubtitlePreferenceError::Invalid(
                "subtitle preference scopes must be unique".into(),
            ));
        }
    }
    Ok(())
}

fn validate_entry(
    entry: &ScopedSubtitlePreferencesV1,
    styles: &StyleManifest,
) -> Result<(), SubtitlePreferenceError> {
    validate_scope(&entry.scope)?;
    if let Some(style_id) = &entry.selected_style_id {
        if styles.style(style_id).is_none() {
            return Err(SubtitlePreferenceError::Invalid(format!(
                "unknown subtitle style `{style_id}`"
            )));
        }
    }
    entry.overrides.validate()
}

fn validate_scope(scope: &SubtitlePreferenceScopeV1) -> Result<(), SubtitlePreferenceError> {
    match scope {
        SubtitlePreferenceScopeV1::Global => Ok(()),
        SubtitlePreferenceScopeV1::Game { game_profile_id } => {
            validate_scope_id("gameProfileId", game_profile_id)
        }
        SubtitlePreferenceScopeV1::Character {
            game_profile_id,
            character_id,
        } => {
            validate_scope_id("gameProfileId", game_profile_id)?;
            validate_scope_id("characterId", character_id)
        }
    }
}

fn validate_scope_id(field: &str, value: &str) -> Result<(), SubtitlePreferenceError> {
    if value.is_empty()
        || value.len() > MAX_SCOPE_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(SubtitlePreferenceError::Invalid(format!(
            "{field} is not a bounded identifier"
        )));
    }
    Ok(())
}

fn validate_optional_range(
    field: &str,
    value: Option<f32>,
    minimum: f32,
    maximum: f32,
) -> Result<(), SubtitlePreferenceError> {
    if let Some(value) = value {
        if !value.is_finite() || !(minimum..=maximum).contains(&value) {
            return Err(SubtitlePreferenceError::Invalid(format!(
                "{field} must be finite and between {minimum} and {maximum}"
            )));
        }
    }
    Ok(())
}

fn canonicalize_entries(entries: &mut [ScopedSubtitlePreferencesV1]) {
    entries.sort_by(|left, right| left.scope.cmp(&right.scope));
}

fn available_style(style: &SubtitleStyle) -> AvailableSubtitleStyleV1 {
    AvailableSubtitleStyleV1 {
        style_id: style.id.clone(),
        body_font_role: style.typography.body_font_role.clone(),
        speaker_font_role: style.typography.speaker_font_role.clone(),
        safe_area_dp: style.geometry.safe_margin_dp,
        body_size_dp: style.typography.body_size_dp,
        speaker_size_dp: style.typography.speaker_size_dp,
        backplate_enabled: style.effects.backplate.enabled,
    }
}

fn load_document(
    path: &Path,
) -> Result<
    (
        SubtitlePreferenceDocumentV1,
        SubtitlePreferenceMigrationV1,
        bool,
    ),
    SubtitlePreferenceError,
> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                SubtitlePreferenceDocumentV1 {
                    schema_version: SUBTITLE_PREFERENCE_SCHEMA_VERSION,
                    revision: 0,
                    entries: Vec::new(),
                },
                SubtitlePreferenceMigrationV1 {
                    state: SubtitlePreferenceMigrationStateV1::InitializedDefaults,
                    from_schema_version: None,
                    detail: "Initialized native subtitle preferences from the validated bundled manifest; no font binaries were installed or downloaded.".into(),
                },
                true,
            ));
        }
        Err(error) => return Err(SubtitlePreferenceError::Persistence(error.to_string())),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_DOCUMENT_BYTES
    {
        return Err(SubtitlePreferenceError::Persistence(
            "subtitle preference file is not a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|file| file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(SubtitlePreferenceError::Persistence(
            "subtitle preference file exceeds the byte limit".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            SubtitlePreferenceError::Persistence(
                "subtitle preference schemaVersion is missing".into(),
            )
        })?;
    let version =
        u32::try_from(version).map_err(|_| SubtitlePreferenceError::UnsupportedSchema(u32::MAX))?;
    match version {
        SUBTITLE_PREFERENCE_SCHEMA_VERSION => {
            let document: SubtitlePreferenceDocumentV1 = serde_json::from_value(value)
                .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
            Ok((document, current_migration(), false))
        }
        LEGACY_SCHEMA_VERSION => {
            let legacy: SubtitlePreferenceDocumentV0 = serde_json::from_value(value)
                .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
            if legacy.schema_version != LEGACY_SCHEMA_VERSION {
                return Err(SubtitlePreferenceError::UnsupportedSchema(
                    legacy.schema_version,
                ));
            }
            let entry = ScopedSubtitlePreferencesV1 {
                scope: SubtitlePreferenceScopeV1::Global,
                selected_style_id: legacy.selected_style_id,
                overrides: legacy.overrides,
            };
            let entries = if entry.is_empty() {
                Vec::new()
            } else {
                vec![entry]
            };
            Ok((
                SubtitlePreferenceDocumentV1 {
                    schema_version: SUBTITLE_PREFERENCE_SCHEMA_VERSION,
                    revision: legacy.revision.saturating_add(1),
                    entries,
                },
                SubtitlePreferenceMigrationV1 {
                    state: SubtitlePreferenceMigrationStateV1::MigratedLegacyV0,
                    from_schema_version: Some(LEGACY_SCHEMA_VERSION),
                    detail: "Atomically migrated the legacy global subtitle style and renderer-supported overrides to scoped schema v1.".into(),
                },
                true,
            ))
        }
        unsupported => Err(SubtitlePreferenceError::UnsupportedSchema(unsupported)),
    }
}

fn current_migration() -> SubtitlePreferenceMigrationV1 {
    SubtitlePreferenceMigrationV1 {
        state: SubtitlePreferenceMigrationStateV1::Current,
        from_schema_version: None,
        detail: "Native subtitle preferences use the current scoped schema v1.".into(),
    }
}

fn atomic_write(
    path: &Path,
    document: &SubtitlePreferenceDocumentV1,
) -> Result<(), SubtitlePreferenceError> {
    let parent = path.parent().ok_or_else(|| {
        SubtitlePreferenceError::Persistence("preference path has no parent".into())
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(SubtitlePreferenceError::Persistence(
                "refusing to replace a subtitle preference symlink".into(),
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(SubtitlePreferenceError::Persistence(
                "refusing to replace a non-file subtitle preference path".into(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(SubtitlePreferenceError::Persistence(error.to_string())),
    }
    let mut bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(SubtitlePreferenceError::Persistence(
            "subtitle preferences exceed the byte limit".into(),
        ));
    }
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    }
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| SubtitlePreferenceError::Persistence(error.error.to_string()))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| SubtitlePreferenceError::Persistence(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_validator_rejects_any_bundled_font_claim() {
        let tampered = FONT_CATALOG_JSON.replacen(
            "\"source\": \"operating_system\"",
            "\"source\": \"bundled\"",
            1,
        );
        let result = validate_assets(DEFAULT_STYLES_JSON, &tampered, ASSET_LICENSES_JSON);
        assert!(matches!(
            result,
            Err(SubtitlePreferenceError::InvalidAssets(_))
        ));
    }

    #[test]
    fn duplicate_style_ids_fail_closed() {
        let mut value: serde_json::Value =
            serde_json::from_str(DEFAULT_STYLES_JSON).expect("style manifest");
        let duplicate = value["styles"][0].clone();
        value["styles"]
            .as_array_mut()
            .expect("styles")
            .push(duplicate);
        let result = validate_assets(
            &serde_json::to_string(&value).expect("serialize tamper"),
            FONT_CATALOG_JSON,
            ASSET_LICENSES_JSON,
        );
        assert!(matches!(
            result,
            Err(SubtitlePreferenceError::InvalidAssets(_))
        ));
    }
}
