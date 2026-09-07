use crate::catalog::ResourceCatalog;
use npc_game_profile::{load_profile, GameProfileV2};
use npc_provider_catalog::{CatalogDocument, Modality, RouteAvailability};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;

const CONTENT_PACK_FORMAT: &str = "npc.content-pack";
const CONTENT_PACK_SCHEMA_VERSION: u32 = 1;
const ACTIVE_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_PACK_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECOMMENDATIONS: usize = 24;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const BUNDLED_PROVIDER_CATALOG: &[u8] = include_bytes!("../../../../catalog/v1/catalog.json");

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentPackKindV1 {
    GameBase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentPackProviderRoleV1 {
    Llm,
    Stt,
    Tts,
    Embeddings,
}

impl ContentPackProviderRoleV1 {
    fn modality(self) -> Modality {
        match self {
            Self::Llm => Modality::Llm,
            Self::Stt => Modality::Stt,
            Self::Tts => Modality::Tts,
            Self::Embeddings => Modality::Embedding,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContentPackProviderRecommendationV1 {
    pub role: ContentPackProviderRoleV1,
    pub provider_id: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentPackRightsReviewV1 {
    Approved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContentPackRightsV1 {
    pub license_name: String,
    pub source_summary: String,
    pub redistributable: bool,
    pub commercial_use: bool,
    pub derivative_use: bool,
    pub review_status: ContentPackRightsReviewV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContentPackEnvelopeV1 {
    pub format: String,
    pub schema_version: u32,
    pub namespace: String,
    pub pack_id: String,
    pub version: String,
    pub kind: ContentPackKindV1,
    pub game_profile_id: String,
    pub title: String,
    pub summary: String,
    pub rights: ContentPackRightsV1,
    #[serde(default)]
    pub provider_recommendations: Vec<ContentPackProviderRecommendationV1>,
    pub profile: GameProfileV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveProfileDocumentV1 {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub namespace: String,
    pub pack_id: String,
    pub version: String,
    pub content_sha256: String,
    pub title: String,
    pub summary: String,
    pub activated_at_unix_millis: u64,
    pub rights: ContentPackRightsV1,
    pub provider_recommendations: Vec<ContentPackProviderRecommendationV1>,
    pub profile: GameProfileV2,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContentPackRecommendationPreviewV1 {
    pub role: ContentPackProviderRoleV1,
    pub provider_id: String,
    pub model_id: String,
    pub character_id: Option<String>,
    pub voice_id: Option<String>,
    pub rationale: String,
    pub catalog_available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContentPackPreviewV1 {
    pub schema_version: u32,
    pub namespace: String,
    pub pack_id: String,
    pub version: String,
    pub game_profile_id: String,
    pub title: String,
    pub summary: String,
    pub content_sha256: String,
    pub display_name: String,
    pub character_count: usize,
    pub knowledge_count: usize,
    pub provider_recommendations: Vec<ContentPackRecommendationPreviewV1>,
    pub rights: ContentPackRightsV1,
    pub apply_detail: String,
    pub network_request_performed: bool,
    pub provider_routes_changed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveContentPackV1 {
    pub namespace: String,
    pub pack_id: String,
    pub version: String,
    pub game_profile_id: String,
    pub title: String,
    pub content_sha256: String,
    pub activated_at_unix_millis: u64,
    pub character_count: usize,
    pub knowledge_count: usize,
    pub provider_recommendation_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContentPackStateV1 {
    pub schema_version: u32,
    pub active: Vec<ActiveContentPackV1>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectContentPackRequestV1 {
    pub json_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivateContentPackRequestV1 {
    pub json_text: String,
    pub expected_content_sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ContentPackError {
    #[error("content pack is empty or exceeds the 2 MiB local import limit")]
    Size,
    #[error("content pack JSON is invalid: {0}")]
    Json(serde_json::Error),
    #[error("content pack envelope is invalid: {0}")]
    Invalid(String),
    #[error("content pack game profile is invalid: {0}")]
    Profile(String),
    #[error("content pack no longer matches the reviewed digest")]
    DigestMismatch,
    #[error("content pack state could not be persisted: {0}")]
    Persistence(String),
}

#[derive(Debug)]
pub struct ContentPackManagerV1 {
    active_profile_root: PathBuf,
    mutation: Mutex<()>,
}

impl ContentPackManagerV1 {
    pub fn new(config_directory: &Path) -> Self {
        Self {
            active_profile_root: config_directory
                .join("content-packs-v1")
                .join("active-profiles"),
            mutation: Mutex::new(()),
        }
    }

    pub fn active_profile_root(&self) -> PathBuf {
        self.active_profile_root.clone()
    }

    pub fn inspect(
        &self,
        request: InspectContentPackRequestV1,
        resources: &ResourceCatalog,
    ) -> Result<ContentPackPreviewV1, ContentPackError> {
        let (pack, digest) = parse_and_validate_pack(&request.json_text, resources)?;
        preview(pack, digest)
    }

    pub fn activate(
        &self,
        request: ActivateContentPackRequestV1,
        resources: &ResourceCatalog,
    ) -> Result<ActiveContentPackV1, ContentPackError> {
        let (pack, digest) = parse_and_validate_pack(&request.json_text, resources)?;
        if !is_sha256(&request.expected_content_sha256)
            || !digest.eq_ignore_ascii_case(&request.expected_content_sha256)
        {
            return Err(ContentPackError::DigestMismatch);
        }
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| ContentPackError::Persistence("activation lock is unavailable".into()))?;
        let activated_at_unix_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| ContentPackError::Persistence(error.to_string()))?
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let active = ActiveProfileDocumentV1 {
            schema_version: ACTIVE_PROFILE_SCHEMA_VERSION,
            game_profile_id: pack.game_profile_id.clone(),
            namespace: pack.namespace.clone(),
            pack_id: pack.pack_id.clone(),
            version: pack.version.clone(),
            content_sha256: digest,
            title: pack.title,
            summary: pack.summary,
            activated_at_unix_millis,
            rights: pack.rights,
            provider_recommendations: pack.provider_recommendations,
            profile: pack.profile,
        };
        atomic_write_json(&self.active_path(&active.game_profile_id), &active)?;
        Ok(active_summary(&active))
    }

    pub fn state(&self) -> Result<ContentPackStateV1, ContentPackError> {
        if !self.active_profile_root.exists() {
            return Ok(ContentPackStateV1 {
                schema_version: 1,
                active: Vec::new(),
                detail: "No optional content pack is active. Bundled game profiles remain in use."
                    .into(),
            });
        }
        let metadata = fs::symlink_metadata(&self.active_profile_root)
            .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ContentPackError::Persistence(
                "active content-pack directory is not a regular directory".into(),
            ));
        }
        let mut active = Vec::new();
        for entry in fs::read_dir(&self.active_profile_root)
            .map_err(|error| ContentPackError::Persistence(error.to_string()))?
        {
            let entry = entry.map_err(|error| ContentPackError::Persistence(error.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > MAX_PACK_BYTES as u64 * 2
            {
                return Err(ContentPackError::Persistence(
                    "active content-pack record is linked, oversized, or not a file".into(),
                ));
            }
            let bytes = fs::read(&path)
                .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
            let document: ActiveProfileDocumentV1 = serde_json::from_slice(&bytes)
                .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
            validate_active_document(&document, &path)?;
            active.push(active_summary(&document));
        }
        active.sort_by(|left, right| left.game_profile_id.cmp(&right.game_profile_id));
        Ok(ContentPackStateV1 {
            schema_version: 1,
            detail: if active.is_empty() {
                "No optional content pack is active. Bundled game profiles remain in use.".into()
            } else {
                format!(
                    "{} optional content pack{} active. Saved character, memory, and provider choices were left untouched.",
                    active.len(),
                    if active.len() == 1 { " is" } else { "s are" }
                )
            },
            active,
        })
    }

    fn active_path(&self, game_profile_id: &str) -> PathBuf {
        self.active_profile_root
            .join(format!("{game_profile_id}.json"))
    }
}

fn parse_and_validate_pack(
    json_text: &str,
    resources: &ResourceCatalog,
) -> Result<(ContentPackEnvelopeV1, String), ContentPackError> {
    if json_text.is_empty() || json_text.len() > MAX_PACK_BYTES {
        return Err(ContentPackError::Size);
    }
    let pack: ContentPackEnvelopeV1 =
        serde_json::from_str(json_text).map_err(ContentPackError::Json)?;
    if pack.format != CONTENT_PACK_FORMAT || pack.schema_version != CONTENT_PACK_SCHEMA_VERSION {
        return Err(ContentPackError::Invalid(
            "expected npc.content-pack schema version 1".into(),
        ));
    }
    validate_identifier("namespace", &pack.namespace)?;
    validate_identifier("pack_id", &pack.pack_id)?;
    validate_semver(&pack.version)?;
    validate_text("title", &pack.title, 120)?;
    validate_text("summary", &pack.summary, MAX_TEXT_BYTES)?;
    validate_text("license_name", &pack.rights.license_name, 160)?;
    validate_text("rights source_summary", &pack.rights.source_summary, 2_048)?;
    if pack.provider_recommendations.len() > MAX_RECOMMENDATIONS {
        return Err(ContentPackError::Invalid(format!(
            "at most {MAX_RECOMMENDATIONS} provider recommendations are allowed"
        )));
    }
    let profile_bytes = serde_json::to_vec(&pack.profile).map_err(ContentPackError::Json)?;
    if profile_bytes.len() > crate::character_content_overrides::MAX_EFFECTIVE_PROFILE_BYTES {
        return Err(ContentPackError::Invalid(
            "embedded profile exceeds the authenticated runtime-message limit".into(),
        ));
    }
    let profile = load_profile(&profile_bytes)
        .map_err(|error| ContentPackError::Profile(error.to_string()))?;
    if profile.id != pack.game_profile_id {
        return Err(ContentPackError::Invalid(
            "game_profile_id does not match the embedded profile".into(),
        ));
    }
    validate_identifier("game_profile_id", &pack.game_profile_id)?;
    let bundled = resources
        .load_bundled_catalog_profile(&pack.game_profile_id)
        .map_err(|error| ContentPackError::Invalid(error.to_string()))?;
    if profile.game != bundled.game
        || profile.detection != bundled.detection
        || profile.safety != bundled.safety
        || profile.capabilities != bundled.capabilities
    {
        return Err(ContentPackError::Invalid(
            "local content packs may add authored content but cannot change game metadata, detection, safety, or capability claims"
                .into(),
        ));
    }
    let bundled_characters = bundled
        .characters
        .iter()
        .map(|character| character.id.as_str())
        .collect::<BTreeSet<_>>();
    let pack_characters = profile
        .characters
        .iter()
        .map(|character| character.id.as_str())
        .collect::<BTreeSet<_>>();
    if !bundled_characters.is_subset(&pack_characters) {
        return Err(ContentPackError::Invalid(
            "a base content pack must retain every bundled stable character ID".into(),
        ));
    }
    if !profile.defaults.voice.user_override_allowed
        || profile
            .characters
            .iter()
            .any(|character| !character.voice.user_override_allowed)
    {
        return Err(ContentPackError::Invalid(
            "all pack voice defaults must allow player overrides".into(),
        ));
    }
    validate_recommendations(&pack, bundled_provider_catalog()?)?;
    let digest = hex::encode(Sha256::digest(json_text.as_bytes()));
    Ok((pack, digest))
}

fn validate_recommendations(
    pack: &ContentPackEnvelopeV1,
    catalog: CatalogDocument,
) -> Result<(), ContentPackError> {
    let character_ids = pack
        .profile
        .characters
        .iter()
        .map(|character| character.id.as_str())
        .collect::<BTreeSet<_>>();
    for recommendation in &pack.provider_recommendations {
        validate_identifier("provider_id", &recommendation.provider_id)?;
        validate_text("model_id", &recommendation.model_id, 160)?;
        validate_text("recommendation rationale", &recommendation.rationale, 2_048)?;
        if let Some(character_id) = &recommendation.character_id {
            validate_identifier("character_id", character_id)?;
            if !character_ids.contains(character_id.as_str()) {
                return Err(ContentPackError::Invalid(format!(
                    "provider recommendation references unknown character `{character_id}`"
                )));
            }
        }
        if let Some(voice_id) = &recommendation.voice_id {
            validate_text("voice_id", voice_id, 160)?;
        }
        if recommendation.role == ContentPackProviderRoleV1::Tts
            && recommendation.voice_id.is_none()
        {
            return Err(ContentPackError::Invalid(
                "TTS recommendations must name an exact stock voice for later account validation"
                    .into(),
            ));
        }
        let route_exists = catalog.content.models.iter().any(|model| {
            model.provider_id == recommendation.provider_id
                && (model.upstream_id == recommendation.model_id
                    || model.id == recommendation.model_id)
                && model.modality == recommendation.role.modality()
                && matches!(
                    model.availability,
                    RouteAvailability::ImplementedAdapter | RouteAvailability::InstalledQualified
                )
        });
        if !route_exists {
            return Err(ContentPackError::Invalid(format!(
                "provider recommendation `{}/{}` is not selectable in the current catalog",
                recommendation.provider_id, recommendation.model_id
            )));
        }
    }
    Ok(())
}

fn bundled_provider_catalog() -> Result<CatalogDocument, ContentPackError> {
    CatalogDocument::parse(BUNDLED_PROVIDER_CATALOG)
        .map_err(|error| ContentPackError::Invalid(format!("provider catalog is invalid: {error}")))
}

fn preview(
    pack: ContentPackEnvelopeV1,
    digest: String,
) -> Result<ContentPackPreviewV1, ContentPackError> {
    let catalog = bundled_provider_catalog()?;
    let provider_recommendations = pack
        .provider_recommendations
        .iter()
        .map(|recommendation| {
            let model = catalog.content.models.iter().find(|model| {
                model.provider_id == recommendation.provider_id
                    && (model.upstream_id == recommendation.model_id
                        || model.id == recommendation.model_id)
                    && model.modality == recommendation.role.modality()
            });
            let needs_voice_check = recommendation.role == ContentPackProviderRoleV1::Tts;
            ContentPackRecommendationPreviewV1 {
                role: recommendation.role,
                provider_id: recommendation.provider_id.clone(),
                model_id: recommendation.model_id.clone(),
                character_id: recommendation.character_id.clone(),
                voice_id: recommendation.voice_id.clone(),
                rationale: recommendation.rationale.clone(),
                catalog_available: model.is_some_and(|model| {
                    matches!(
                        model.availability,
                        RouteAvailability::ImplementedAdapter
                            | RouteAvailability::InstalledQualified
                    )
                }),
                detail: if needs_voice_check {
                    "The model is selectable. The exact stock voice still requires the player's account and current provider inventory check in Voice & models."
                        .into()
                } else {
                    "The exact provider/model route is selectable in the bundled catalog. It remains a suggestion until the player chooses it in Voice & models."
                        .into()
                },
            }
        })
        .collect();
    Ok(ContentPackPreviewV1 {
        schema_version: 1,
        namespace: pack.namespace,
        pack_id: pack.pack_id,
        version: pack.version,
        game_profile_id: pack.game_profile_id,
        title: pack.title,
        summary: pack.summary,
        content_sha256: digest,
        display_name: pack.profile.display_name,
        character_count: pack.profile.characters.len(),
        knowledge_count: pack.profile.content.knowledge.len(),
        provider_recommendations,
        rights: pack.rights,
        apply_detail: "Activation replaces only the authored profile layer for this game. Saved character selection, delivered memory, credentials, and provider loadouts remain player-owned and unchanged."
            .into(),
        network_request_performed: false,
        provider_routes_changed: false,
    })
}

fn validate_active_document(
    document: &ActiveProfileDocumentV1,
    path: &Path,
) -> Result<(), ContentPackError> {
    if document.schema_version != ACTIVE_PROFILE_SCHEMA_VERSION
        || document.game_profile_id != document.profile.id
        || !is_sha256(&document.content_sha256)
        || path.file_stem().and_then(|value| value.to_str())
            != Some(document.game_profile_id.as_str())
    {
        return Err(ContentPackError::Persistence(
            "active content-pack record failed identity validation".into(),
        ));
    }
    let report = document.profile.validate();
    if !report.is_valid() {
        return Err(ContentPackError::Persistence(
            "active content-pack profile failed validation".into(),
        ));
    }
    Ok(())
}

fn active_summary(document: &ActiveProfileDocumentV1) -> ActiveContentPackV1 {
    ActiveContentPackV1 {
        namespace: document.namespace.clone(),
        pack_id: document.pack_id.clone(),
        version: document.version.clone(),
        game_profile_id: document.game_profile_id.clone(),
        title: document.title.clone(),
        content_sha256: document.content_sha256.clone(),
        activated_at_unix_millis: document.activated_at_unix_millis,
        character_count: document.profile.characters.len(),
        knowledge_count: document.profile.content.knowledge.len(),
        provider_recommendation_count: document.provider_recommendations.len(),
    }
}

fn validate_identifier(field: &str, value: &str) -> Result<(), ContentPackError> {
    let valid = !value.is_empty()
        && value.len() <= 96
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        Err(ContentPackError::Invalid(format!(
            "{field} must be a lowercase data identifier"
        )))
    }
}

fn validate_semver(value: &str) -> Result<(), ContentPackError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
    {
        return Err(ContentPackError::Invalid(
            "version must be a bounded semantic version".into(),
        ));
    }
    let core = value.split(['-', '+']).next().unwrap_or_default();
    let segments = core.split('.').collect::<Vec<_>>();
    if segments.len() != 3
        || segments
            .iter()
            .any(|segment| segment.is_empty() || segment.parse::<u64>().is_err())
    {
        return Err(ContentPackError::Invalid(
            "version must use major.minor.patch semantic-version form".into(),
        ));
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, maximum_bytes: usize) -> Result<(), ContentPackError> {
    if value.trim().is_empty() || value.len() > maximum_bytes || value.contains('\0') {
        return Err(ContentPackError::Invalid(format!(
            "{field} is empty or exceeds its safety limit"
        )));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ContentPackError> {
    let directory = path.parent().ok_or_else(|| {
        ContentPackError::Persistence("active content-pack directory is unavailable".into())
    })?;
    fs::create_dir_all(directory)
        .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
    let directory_metadata = fs::symlink_metadata(directory)
        .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
    if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
        return Err(ContentPackError::Persistence(
            "active content-pack directory is linked or not a directory".into(),
        ));
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(ContentPackError::Json)?;
    if bytes.len() > MAX_PACK_BYTES * 2 {
        return Err(ContentPackError::Persistence(
            "materialized content pack exceeds the 4 MiB limit".into(),
        ));
    }
    let mut temporary = NamedTempFile::new_in(directory)
        .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| ContentPackError::Persistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| ContentPackError::Persistence(error.error.to_string()))?;
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_json() -> String {
        let resources = ResourceCatalog::new(None);
        let mut profile = resources
            .load_builtin_game_profile("cyberpunk-2077")
            .expect("profile");
        profile.defaults.voice.user_override_allowed = true;
        for character in &mut profile.characters {
            character.voice.user_override_allowed = true;
        }
        serde_json::to_string(&ContentPackEnvelopeV1 {
            format: CONTENT_PACK_FORMAT.into(),
            schema_version: 1,
            namespace: "org.example".into(),
            pack_id: "night-city-authored".into(),
            version: "1.0.0".into(),
            kind: ContentPackKindV1::GameBase,
            game_profile_id: "cyberpunk-2077".into(),
            title: "Night City authored context".into(),
            summary: "Original data-only lore and character defaults.".into(),
            rights: ContentPackRightsV1 {
                license_name: "CC-BY-4.0".into(),
                source_summary: "Original authored text; no publisher media.".into(),
                redistributable: true,
                commercial_use: true,
                derivative_use: true,
                review_status: ContentPackRightsReviewV1::Approved,
            },
            provider_recommendations: vec![ContentPackProviderRecommendationV1 {
                role: ContentPackProviderRoleV1::Llm,
                provider_id: "groq".into(),
                model_id: "qwen/qwen3.6-27b".into(),
                character_id: None,
                voice_id: None,
                rationale: "Fast structured dialogue candidate.".into(),
            }],
            profile,
        })
        .expect("json")
    }

    #[test]
    fn preview_and_activation_are_persistent_and_do_not_mutate_player_state() {
        let directory = tempfile::tempdir().expect("tempdir");
        let character_selection = directory.path().join("selected-characters-v1.json");
        let provider_loadouts = directory.path().join("provider-loadouts-v1.json");
        fs::write(&character_selection, b"player-character-selection").expect("selection");
        fs::write(&provider_loadouts, b"player-provider-loadout").expect("loadout");
        let manager = ContentPackManagerV1::new(directory.path());
        let resources =
            ResourceCatalog::with_active_content_profiles(None, manager.active_profile_root());
        let json_text = pack_json();
        let preview = manager
            .inspect(
                InspectContentPackRequestV1 {
                    json_text: json_text.clone(),
                },
                &resources,
            )
            .expect("preview");
        assert_eq!(preview.game_profile_id, "cyberpunk-2077");
        assert!(!preview.network_request_performed);
        assert!(!preview.provider_routes_changed);
        let active = manager
            .activate(
                ActivateContentPackRequestV1 {
                    json_text,
                    expected_content_sha256: preview.content_sha256,
                },
                &resources,
            )
            .expect("activate");
        assert_eq!(active.pack_id, "night-city-authored");
        let reloaded = ContentPackManagerV1::new(directory.path())
            .state()
            .expect("state");
        assert_eq!(reloaded.active, vec![active]);
        assert_eq!(
            resources
                .load_game_profile("cyberpunk-2077")
                .expect("active profile")
                .id,
            "cyberpunk-2077"
        );
        assert_eq!(
            fs::read(&character_selection).expect("selection"),
            b"player-character-selection"
        );
        assert_eq!(
            fs::read(&provider_loadouts).expect("loadout"),
            b"player-provider-loadout"
        );
    }

    #[test]
    fn rejects_unknown_fields_and_credential_material() {
        let resources = ResourceCatalog::new(None);
        let mut value: serde_json::Value = serde_json::from_str(&pack_json()).expect("value");
        value["api_key"] = serde_json::Value::String("should-never-enter-a-pack".into());
        let error = parse_and_validate_pack(&value.to_string(), &resources).expect_err("reject");
        assert!(matches!(error, ContentPackError::Json(_)));
    }

    #[test]
    fn rejects_detection_changes_and_disabled_player_voice_overrides() {
        let resources = ResourceCatalog::new(None);
        let mut value: serde_json::Value = serde_json::from_str(&pack_json()).expect("value");
        value["profile"]["detection"]["processes"][0]["executable"] =
            serde_json::Value::String("unsafe-replacement.exe".into());
        let error = parse_and_validate_pack(&value.to_string(), &resources).expect_err("reject");
        assert!(error.to_string().contains("cannot change game metadata"));

        let mut value: serde_json::Value = serde_json::from_str(&pack_json()).expect("value");
        value["profile"]["characters"][0]["voice"]["user_override_allowed"] =
            serde_json::Value::Bool(false);
        let error = parse_and_validate_pack(&value.to_string(), &resources).expect_err("reject");
        assert!(error.to_string().contains("allow player overrides"));
    }

    #[test]
    fn activation_rechecks_the_exact_reviewed_bytes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = ContentPackManagerV1::new(directory.path());
        let resources = ResourceCatalog::new(None);
        let json_text = pack_json();
        let error = manager
            .activate(
                ActivateContentPackRequestV1 {
                    json_text,
                    expected_content_sha256: "0".repeat(64),
                },
                &resources,
            )
            .expect_err("digest mismatch");
        assert!(matches!(error, ContentPackError::DigestMismatch));
        assert!(manager.state().expect("state").active.is_empty());
    }

    #[test]
    fn rejects_catalog_stale_provider_recommendations() {
        let resources = ResourceCatalog::new(None);
        let mut value: serde_json::Value = serde_json::from_str(&pack_json()).expect("value");
        value["provider_recommendations"][0]["provider_id"] =
            serde_json::Value::String("made-up-provider".into());
        let error = parse_and_validate_pack(&value.to_string(), &resources).expect_err("reject");
        assert!(error.to_string().contains("not selectable"));
    }

    #[test]
    fn checked_in_local_review_pack_passes_the_real_import_boundary() {
        let resources = ResourceCatalog::new(None);
        let json_text = include_str!(
            "../../../../profiles/content-packs/local-review/cyberpunk-2077-authored-context-v1.pack.json"
        );
        let (pack, digest) = parse_and_validate_pack(json_text, &resources).expect("valid pack");
        assert_eq!(pack.profile.content.knowledge.len(), 7);
        assert_eq!(pack.provider_recommendations.len(), 2);
        assert!(is_sha256(&digest));
    }
}
