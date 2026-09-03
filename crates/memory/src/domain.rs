use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

/// Semantic partitions are stored in one table so cross-namespace retrieval is
/// transactional while still remaining explicitly filterable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryNamespace {
    Event,
    Episode,
    Fact,
    Relationship,
    Summary,
    Lore,
    Profile,
}

impl MemoryNamespace {
    pub const ALL: [Self; 7] = [
        Self::Event,
        Self::Episode,
        Self::Fact,
        Self::Relationship,
        Self::Summary,
        Self::Lore,
        Self::Profile,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Episode => "episode",
            Self::Fact => "fact",
            Self::Relationship => "relationship",
            Self::Summary => "summary",
            Self::Lore => "lore",
            Self::Profile => "profile",
        }
    }
}

impl Display for MemoryNamespace {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MemoryNamespace {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "event" => Ok(Self::Event),
            "episode" => Ok(Self::Episode),
            "fact" => Ok(Self::Fact),
            "relationship" => Ok(Self::Relationship),
            "summary" => Ok(Self::Summary),
            "lore" => Ok(Self::Lore),
            "profile" => Ok(Self::Profile),
            other => Err(MemoryError::InvalidData(format!(
                "unknown memory namespace: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Private,
    Character,
    Game,
    Profile,
    Global,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Character => "character",
            Self::Game => "game",
            Self::Profile => "profile",
            Self::Global => "global",
        }
    }
}

impl Display for Visibility {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Visibility {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "private" => Ok(Self::Private),
            "character" => Ok(Self::Character),
            "game" => Ok(Self::Game),
            "profile" => Ok(Self::Profile),
            "global" => Ok(Self::Global),
            other => Err(MemoryError::InvalidData(format!(
                "unknown visibility: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoryScope {
    pub profile_id: Option<String>,
    pub game_id: Option<String>,
    pub character_id: Option<String>,
    pub session_id: Option<String>,
    pub save_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub source_kind: String,
    pub source_id: Option<String>,
    pub source_uri: Option<String>,
    pub author: Option<String>,
    pub captured_at_ms: Option<i64>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            source_kind: "runtime".to_owned(),
            source_id: None,
            source_uri: None,
            author: None,
            captured_at_ms: None,
            attributes: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryInput {
    /// Omit to generate a random UUID. Imports should supply stable IDs.
    pub id: Option<String>,
    pub namespace: MemoryNamespace,
    #[serde(default)]
    pub scope: MemoryScope,
    pub visibility: Visibility,
    pub content: String,
    #[serde(default)]
    pub provenance: Provenance,
    /// Confidence and importance must be finite and in the inclusive 0..=1 range.
    pub confidence: f64,
    pub importance: f64,
    pub observed_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    /// Optional optimistic-concurrency token. A mismatch rejects the write.
    pub expected_revision: Option<i64>,
}

impl MemoryInput {
    pub fn validate(&self) -> Result<(), MemoryError> {
        if self.content.trim().is_empty() {
            return Err(MemoryError::InvalidData(
                "memory content cannot be blank".to_owned(),
            ));
        }
        if self.content.len() > 1_048_576 {
            return Err(MemoryError::InvalidData(
                "memory content exceeds 1 MiB".to_owned(),
            ));
        }
        for (name, value) in [
            ("confidence", self.confidence),
            ("importance", self.importance),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(MemoryError::InvalidData(format!(
                    "{name} must be finite and between 0 and 1"
                )));
            }
        }
        if let Some(expires) = self.expires_at_ms {
            if expires <= self.observed_at_ms {
                return Err(MemoryError::InvalidData(
                    "expires_at_ms must be after observed_at_ms".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub namespace: MemoryNamespace,
    pub scope: MemoryScope,
    pub visibility: Visibility,
    pub content: String,
    pub content_sha256: String,
    pub provenance: Provenance,
    pub confidence: f64,
    pub importance: f64,
    pub observed_at_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub embedding_generation: Option<i64>,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingInput {
    pub item_id: String,
    pub generation: i64,
    pub model_id: String,
    pub values: Vec<f32>,
}

impl EmbeddingInput {
    pub fn validate(&self) -> Result<(), MemoryError> {
        if self.generation < 0 {
            return Err(MemoryError::InvalidData(
                "embedding generation cannot be negative".to_owned(),
            ));
        }
        if self.model_id.trim().is_empty() || self.values.is_empty() {
            return Err(MemoryError::InvalidData(
                "embedding model and vector are required".to_owned(),
            ));
        }
        if self.values.len() > 65_536 || self.values.iter().any(|v| !v.is_finite()) {
            return Err(MemoryError::InvalidData(
                "embedding is too large or contains a non-finite value".to_owned(),
            ));
        }
        let norm = self
            .values
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            .sqrt();
        if norm <= f64::EPSILON {
            return Err(MemoryError::InvalidData(
                "zero-length embedding is not searchable".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalFilter {
    #[serde(default = "default_namespaces")]
    pub namespaces: Vec<MemoryNamespace>,
    #[serde(default)]
    pub visibility: Vec<Visibility>,
    pub profile_id: Option<String>,
    pub game_id: Option<String>,
    pub character_id: Option<String>,
    pub session_id: Option<String>,
    pub save_id: Option<String>,
    #[serde(default = "default_true")]
    pub include_global_scope: bool,
}

fn default_namespaces() -> Vec<MemoryNamespace> {
    MemoryNamespace::ALL.to_vec()
}

fn default_true() -> bool {
    true
}

impl Default for RetrievalFilter {
    fn default() -> Self {
        Self {
            namespaces: default_namespaces(),
            visibility: vec![
                Visibility::Private,
                Visibility::Character,
                Visibility::Game,
                Visibility::Profile,
                Visibility::Global,
            ],
            profile_id: None,
            game_id: None,
            character_id: None,
            session_id: None,
            save_id: None,
            include_global_scope: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalQuery {
    pub text: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub embedding_generation: Option<i64>,
    pub filter: RetrievalFilter,
    pub limit: usize,
    pub candidate_limit: usize,
    pub now_ms: i64,
}

impl RetrievalQuery {
    pub fn validate(&self) -> Result<(), MemoryError> {
        if self.limit == 0 || self.limit > 1_000 {
            return Err(MemoryError::InvalidData(
                "retrieval limit must be in 1..=1000".to_owned(),
            ));
        }
        if self.candidate_limit < self.limit || self.candidate_limit > 50_000 {
            return Err(MemoryError::InvalidData(
                "candidate_limit must be >= limit and <= 50000".to_owned(),
            ));
        }
        if self.filter.namespaces.is_empty() || self.filter.visibility.is_empty() {
            return Err(MemoryError::InvalidData(
                "at least one namespace and visibility are required".to_owned(),
            ));
        }
        if let Some(vector) = &self.embedding {
            if vector.is_empty() || vector.iter().any(|v| !v.is_finite()) {
                return Err(MemoryError::InvalidData(
                    "query embedding must be non-empty and finite".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub record: MemoryRecord,
    pub score: f64,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorSearchBackend {
    ExactCosine,
    SqliteVec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexRequest {
    pub target_generation: i64,
    pub model_id: String,
    pub batch_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexReport {
    pub fts_rebuilt: bool,
    pub embedding_jobs_enqueued: usize,
    pub target_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboxJobInput {
    pub kind: String,
    pub aggregate_id: Option<String>,
    pub payload: Value,
    pub available_at_ms: i64,
    pub max_attempts: u32,
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    Pending,
    Processing,
    Completed,
    Dead,
}

impl OutboxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Dead => "dead",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboxJob {
    pub id: i64,
    pub kind: String,
    pub aggregate_id: Option<String>,
    pub payload: Value,
    pub status: OutboxStatus,
    pub attempts: u32,
    pub max_attempts: u32,
    pub available_at_ms: i64,
    pub lease_owner: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportConflictPolicy {
    Reject,
    KeepExisting,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub inserted: usize,
    pub replaced: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupReport {
    pub destination: PathBuf,
    pub pages_copied: i32,
}

/// Native-owned, deliberately narrow physical-erasure request. Optional
/// encounter/session/save values further narrow the mandatory user/profile/
/// game/character boundary; missing optional values never widen character_id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterMemoryErasureRequest {
    pub scope: AuthorityScope,
    pub erased_at_ms: i64,
}

impl CharacterMemoryErasureRequest {
    pub fn validate(&self) -> Result<(), MemoryError> {
        self.scope.validate()?;
        if self.scope.character_id.is_none() {
            return Err(MemoryError::InvalidData(
                "character memory erasure requires an exact character ID".to_owned(),
            ));
        }
        if self.erased_at_ms < 0 {
            return Err(MemoryError::InvalidData(
                "memory erasure timestamp cannot be negative".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterMemoryErasureReport {
    pub erasure_id: String,
    pub scope_sha256: String,
    pub delivered_turns_deleted: usize,
    pub structured_memories_deleted: usize,
    pub legacy_items_deleted: usize,
    pub outbox_jobs_deleted: usize,
    pub erased_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterMemoryStatusReport {
    pub delivered_turns: usize,
    pub structured_memories: usize,
    pub legacy_items: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid memory data: {0}")]
    InvalidData(String),
    #[error("record not found: {0}")]
    NotFound(String),
    #[error("revision conflict for {id}: expected {expected}, found {actual}")]
    RevisionConflict {
        id: String,
        expected: i64,
        actual: i64,
    },
    #[error("writer is unavailable")]
    WriterUnavailable,
    #[error("writer queue is full; caller should retry with backoff")]
    WriterBackpressure,
    #[error("background task failed: {0}")]
    BackgroundTask(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Stable high-level storage failure categories suitable for UI recovery
/// guidance. Callers must not parse SQLite's platform-dependent error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFailureKind {
    DiskFull,
    Corrupt,
    Busy,
    ReadOnly,
    Io,
    Unknown,
}

impl MemoryError {
    pub fn storage_failure_kind(&self) -> Option<StorageFailureKind> {
        let code = match self {
            Self::Database(rusqlite::Error::SqliteFailure(error, _)) => error.extended_code & 0xff,
            Self::Io(_) => return Some(StorageFailureKind::Io),
            _ => return None,
        };
        Some(match code {
            rusqlite::ffi::SQLITE_FULL => StorageFailureKind::DiskFull,
            rusqlite::ffi::SQLITE_CORRUPT | rusqlite::ffi::SQLITE_NOTADB => {
                StorageFailureKind::Corrupt
            }
            rusqlite::ffi::SQLITE_BUSY | rusqlite::ffi::SQLITE_LOCKED => StorageFailureKind::Busy,
            rusqlite::ffi::SQLITE_READONLY => StorageFailureKind::ReadOnly,
            rusqlite::ffi::SQLITE_IOERR | rusqlite::ffi::SQLITE_CANTOPEN => StorageFailureKind::Io,
            _ => StorageFailureKind::Unknown,
        })
    }
}

/// Complete authority boundary used by the 2.0 memory APIs. Unlike the legacy
/// optional `MemoryScope`, user/profile/game identity is mandatory. Queries
/// against these APIs never widen a missing identifier into another user's or
/// another game's data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityScope {
    pub user_id: String,
    pub profile_id: String,
    pub game_id: String,
    pub character_id: Option<String>,
    pub encounter_id: Option<String>,
    pub session_id: Option<String>,
    pub save_id: Option<String>,
}

impl AuthorityScope {
    pub fn validate(&self) -> Result<(), MemoryError> {
        for (name, value) in [
            ("user_id", Some(self.user_id.as_str())),
            ("profile_id", Some(self.profile_id.as_str())),
            ("game_id", Some(self.game_id.as_str())),
            ("character_id", self.character_id.as_deref()),
            ("encounter_id", self.encounter_id.as_deref()),
            ("session_id", self.session_id.as_deref()),
            ("save_id", self.save_id.as_deref()),
        ] {
            if let Some(value) = value {
                if value.trim().is_empty() || value.len() > 512 {
                    return Err(MemoryError::InvalidData(format!(
                        "{name} must be non-blank and at most 512 bytes"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnSpeaker {
    Player,
    Npc,
}

impl TurnSpeaker {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Npc => "npc",
        }
    }
}

impl FromStr for TurnSpeaker {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "player" => Ok(Self::Player),
            "npc" => Ok(Self::Npc),
            other => Err(MemoryError::InvalidData(format!(
                "unknown turn speaker: {other}"
            ))),
        }
    }
}

/// A producer may submit its final lifecycle state directly. Only `Delivered`
/// and the validated prefix of `PartiallyDelivered` are eligible for storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeliveryDisposition {
    Queued,
    Delivered {
        delivered_at_ms: i64,
    },
    PartiallyDelivered {
        delivered_bytes: usize,
        delivered_at_ms: i64,
    },
    Cancelled,
    Failed,
}

impl DeliveryDisposition {
    pub(crate) fn eligible_text<'a>(
        &self,
        text: &'a str,
    ) -> Result<Option<(&'a str, i64)>, MemoryError> {
        match self {
            Self::Delivered { delivered_at_ms } => Ok(Some((text, *delivered_at_ms))),
            Self::PartiallyDelivered {
                delivered_bytes,
                delivered_at_ms,
            } => {
                if *delivered_bytes == 0
                    || *delivered_bytes >= text.len()
                    || !text.is_char_boundary(*delivered_bytes)
                {
                    return Err(MemoryError::InvalidData(
                        "partial delivery must be a non-empty, proper UTF-8 byte prefix".to_owned(),
                    ));
                }
                Ok(Some((&text[..*delivered_bytes], *delivered_at_ms)))
            }
            Self::Queued | Self::Cancelled | Self::Failed => Ok(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnCommitInput {
    pub turn_id: String,
    pub scope: AuthorityScope,
    pub speaker: TurnSpeaker,
    pub text: String,
    pub delivery: DeliveryDisposition,
    pub created_at_ms: i64,
    pub sequence: u64,
    pub cancellation_generation: u64,
    pub provider_id: Option<String>,
    pub delivery_receipt_id: Option<String>,
    #[serde(default)]
    pub provenance: Provenance,
}

impl TurnCommitInput {
    pub fn validate(&self) -> Result<(), MemoryError> {
        self.scope.validate()?;
        validate_bounded("turn_id", &self.turn_id, 512)?;
        if self.text.trim().is_empty() || self.text.len() > 1_048_576 {
            return Err(MemoryError::InvalidData(
                "turn text must be non-blank and at most 1 MiB".to_owned(),
            ));
        }
        if let Some(value) = self.provider_id.as_deref() {
            validate_bounded("provider_id", value, 512)?;
        }
        if let Some(value) = self.delivery_receipt_id.as_deref() {
            validate_bounded("delivery_receipt_id", value, 512)?;
        }
        if let Some((text, delivered_at_ms)) = self.delivery.eligible_text(&self.text)? {
            if text.trim().is_empty() || delivered_at_ms < self.created_at_ms {
                return Err(MemoryError::InvalidData(
                    "delivered text cannot be blank or predate turn creation".to_owned(),
                ));
            }
            if self.speaker == TurnSpeaker::Npc && self.delivery_receipt_id.is_none() {
                return Err(MemoryError::InvalidData(
                    "delivered NPC text requires an audio or subtitle delivery receipt".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveredTurnRecord {
    pub turn_id: String,
    pub scope: AuthorityScope,
    pub speaker: TurnSpeaker,
    pub delivered_text: String,
    pub content_sha256: String,
    pub created_at_ms: i64,
    pub delivered_at_ms: i64,
    pub sequence: u64,
    pub cancellation_generation: u64,
    pub provider_id: Option<String>,
    pub delivery_receipt_id: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeClass {
    WorldLore,
    Biography,
    CharacterKnowledge,
    UncertainPublicInfo,
    LongTermSummary,
}

impl KnowledgeClass {
    pub const ALL: [Self; 5] = [
        Self::WorldLore,
        Self::Biography,
        Self::CharacterKnowledge,
        Self::UncertainPublicInfo,
        Self::LongTermSummary,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WorldLore => "world_lore",
            Self::Biography => "biography",
            Self::CharacterKnowledge => "character_knowledge",
            Self::UncertainPublicInfo => "uncertain_public_info",
            Self::LongTermSummary => "long_term_summary",
        }
    }
}

impl FromStr for KnowledgeClass {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "world_lore" => Ok(Self::WorldLore),
            "biography" => Ok(Self::Biography),
            "character_knowledge" => Ok(Self::CharacterKnowledge),
            "uncertain_public_info" => Ok(Self::UncertainPublicInfo),
            "long_term_summary" => Ok(Self::LongTermSummary),
            other => Err(MemoryError::InvalidData(format!(
                "unknown knowledge class: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoilerScope {
    None,
    Game,
    Save,
    CharacterPrivate,
    UserPrivate,
}

impl SpoilerScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Game => "game",
            Self::Save => "save",
            Self::CharacterPrivate => "character_private",
            Self::UserPrivate => "user_private",
        }
    }
}

impl FromStr for SpoilerScope {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "game" => Ok(Self::Game),
            "save" => Ok(Self::Save),
            "character_private" => Ok(Self::CharacterPrivate),
            "user_private" => Ok(Self::UserPrivate),
            other => Err(MemoryError::InvalidData(format!(
                "unknown spoiler scope: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorProvenance {
    pub provider_id: String,
    pub model_id: String,
    pub model_revision: String,
    pub prompt_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedMemoryInput {
    pub id: Option<String>,
    pub scope: AuthorityScope,
    pub class: KnowledgeClass,
    pub spoiler_scope: SpoilerScope,
    pub content: String,
    #[serde(default)]
    pub provenance: Provenance,
    pub confidence: f64,
    pub importance: f64,
    pub observed_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    #[serde(default)]
    pub source_turn_ids: Vec<String>,
    pub generator: Option<GeneratorProvenance>,
}

impl DerivedMemoryInput {
    pub fn validate(&self) -> Result<(), MemoryError> {
        self.scope.validate()?;
        if let Some(id) = self.id.as_deref() {
            validate_bounded("derived memory id", id, 512)?;
        }
        if self.content.trim().is_empty() || self.content.len() > 1_048_576 {
            return Err(MemoryError::InvalidData(
                "derived memory must be non-blank and at most 1 MiB".to_owned(),
            ));
        }
        for (name, value) in [
            ("confidence", self.confidence),
            ("importance", self.importance),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(MemoryError::InvalidData(format!(
                    "{name} must be finite and between 0 and 1"
                )));
            }
        }
        if self
            .expires_at_ms
            .is_some_and(|expiry| expiry <= self.observed_at_ms)
        {
            return Err(MemoryError::InvalidData(
                "derived memory expiry must follow observation".to_owned(),
            ));
        }
        if self.source_turn_ids.len() > 10_000 {
            return Err(MemoryError::InvalidData(
                "derived memory references too many source turns".to_owned(),
            ));
        }
        let mut unique = std::collections::BTreeSet::new();
        for id in &self.source_turn_ids {
            validate_bounded("source turn id", id, 512)?;
            if !unique.insert(id) {
                return Err(MemoryError::InvalidData(
                    "derived memory source turn IDs must be unique".to_owned(),
                ));
            }
        }
        if self.class == KnowledgeClass::LongTermSummary
            && (self.source_turn_ids.is_empty() || self.generator.is_none())
        {
            return Err(MemoryError::InvalidData(
                "long-term summaries require source turns and generator provenance".to_owned(),
            ));
        }
        if let Some(generator) = &self.generator {
            for (name, value) in [
                ("generator provider", generator.provider_id.as_str()),
                ("generator model", generator.model_id.as_str()),
                ("generator revision", generator.model_revision.as_str()),
                ("prompt version", generator.prompt_version.as_str()),
            ] {
                validate_bounded(name, value, 512)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedMemoryRecord {
    pub id: String,
    pub scope: AuthorityScope,
    pub class: KnowledgeClass,
    pub spoiler_scope: SpoilerScope,
    pub content: String,
    pub content_sha256: String,
    pub provenance: Provenance,
    pub confidence: f64,
    pub importance: f64,
    pub observed_at_ms: i64,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub source_turn_ids: Vec<String>,
    pub generator: Option<GeneratorProvenance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCommitBatch {
    #[serde(default)]
    pub turns: Vec<TurnCommitInput>,
    #[serde(default)]
    pub derived: Vec<DerivedMemoryInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedTurnReason {
    Queued,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkippedTurn {
    pub turn_id: String,
    pub reason: SkippedTurnReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MemoryCommitReport {
    pub stored_turns: Vec<DeliveredTurnRecord>,
    pub skipped_turns: Vec<SkippedTurn>,
    pub derived_memories: Vec<DerivedMemoryRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpoilerPolicy {
    pub allow_game: bool,
    pub allow_save: bool,
    pub allow_character_private: bool,
    pub allow_user_private: bool,
}

impl Default for SpoilerPolicy {
    fn default() -> Self {
        Self {
            allow_game: true,
            allow_save: true,
            allow_character_private: true,
            allow_user_private: true,
        }
    }
}

impl SpoilerPolicy {
    pub fn allows(self, scope: SpoilerScope) -> bool {
        match scope {
            SpoilerScope::None => true,
            SpoilerScope::Game => self.allow_game,
            SpoilerScope::Save => self.allow_save,
            SpoilerScope::CharacterPrivate => self.allow_character_private,
            SpoilerScope::UserPrivate => self.allow_user_private,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextQuery {
    pub scope: AuthorityScope,
    pub text: Option<String>,
    #[serde(default)]
    pub spoiler_policy: SpoilerPolicy,
    pub recent_turn_limit: usize,
    pub per_class_limit: usize,
    pub now_ms: i64,
}

impl ContextQuery {
    pub fn validate(&self) -> Result<(), MemoryError> {
        self.scope.validate()?;
        if self.recent_turn_limit > 1_000 || self.per_class_limit > 1_000 {
            return Err(MemoryError::InvalidData(
                "context limits must be at most 1000".to_owned(),
            ));
        }
        if self.text.as_ref().is_some_and(|text| text.len() > 65_536) {
            return Err(MemoryError::InvalidData(
                "context query text is too large".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MemoryContextBundle {
    pub world_lore: Vec<DerivedMemoryRecord>,
    pub biography: Vec<DerivedMemoryRecord>,
    pub character_knowledge: Vec<DerivedMemoryRecord>,
    pub uncertain_public_info: Vec<DerivedMemoryRecord>,
    pub recent_dialogue: Vec<DeliveredTurnRecord>,
    pub long_term_summaries: Vec<DerivedMemoryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub ok: bool,
    pub messages: Vec<String>,
    pub schema_version: i64,
    pub delivered_turns: u64,
    pub derived_memories: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurabilityReport {
    pub wal_frames: i64,
    pub checkpointed_frames: i64,
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub destination: PathBuf,
    pub quarantined_database: Option<PathBuf>,
    pub integrity: IntegrityReport,
}

fn validate_bounded(name: &str, value: &str, maximum: usize) -> Result<(), MemoryError> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(MemoryError::InvalidData(format!(
            "{name} must be non-blank and at most {maximum} bytes"
        )));
    }
    Ok(())
}
