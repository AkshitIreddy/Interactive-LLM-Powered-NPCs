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
