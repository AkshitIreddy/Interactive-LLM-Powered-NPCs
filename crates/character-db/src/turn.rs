use crate::{encounter::hex_sha256, CharacterDbError, CHARACTER_DB_SCHEMA_VERSION};
use npc_memory::{
    AuthorityScope, DerivedMemoryInput, GeneratorProvenance, KnowledgeClass, Provenance,
    SpoilerScope,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

/// Versioned orchestration request for creating a long-term summary. The
/// authoritative stored representation remains `npc_memory::DerivedMemoryInput`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySummaryRequestV1 {
    pub schema_version: String,
    pub summary_id: String,
    pub scope: AuthorityScope,
    pub source_turn_ids: Vec<String>,
    pub text: String,
    pub provider_id: String,
    pub model_id: String,
    pub model_revision: String,
    pub prompt_version: String,
    pub created_at_ms: i64,
}

impl MemorySummaryRequestV1 {
    pub fn to_memory_input(&self) -> Result<DerivedMemoryInput, CharacterDbError> {
        if self.schema_version != CHARACTER_DB_SCHEMA_VERSION {
            return Err(CharacterDbError::InvalidInput(format!(
                "unsupported memory summary schema `{}`",
                self.schema_version
            )));
        }
        let input = DerivedMemoryInput {
            id: Some(format!("summary:{}", self.summary_id)),
            scope: self.scope.clone(),
            class: KnowledgeClass::LongTermSummary,
            spoiler_scope: SpoilerScope::CharacterPrivate,
            content: self.text.clone(),
            provenance: Provenance {
                source_kind: "versioned_turn_summary".to_owned(),
                source_id: Some(self.summary_id.clone()),
                source_uri: None,
                author: None,
                captured_at_ms: Some(self.created_at_ms),
                attributes: BTreeMap::from([(
                    "source_turn_ids_sha256".to_owned(),
                    json!(hex_sha256(self.source_turn_ids.join("\0").as_bytes())),
                )]),
            },
            confidence: 0.8,
            importance: 0.6,
            observed_at_ms: self.created_at_ms,
            expires_at_ms: None,
            source_turn_ids: self.source_turn_ids.clone(),
            generator: Some(GeneratorProvenance {
                provider_id: self.provider_id.clone(),
                model_id: self.model_id.clone(),
                model_revision: self.model_revision.clone(),
                prompt_version: self.prompt_version.clone(),
            }),
        };
        input
            .validate()
            .map_err(|error| CharacterDbError::InvalidInput(error.to_string()))?;
        Ok(input)
    }
}

/// Audit evidence for one assembled prompt. Raw user queries are represented
/// only by a digest; selected canonical record IDs remain replayable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalProvenanceV1 {
    pub schema_version: String,
    pub policy_fingerprint_sha256: String,
    pub query_sha256: String,
    #[serde(default)]
    pub selected_profile_knowledge_ids: Vec<String>,
    #[serde(default)]
    pub selected_memory_item_ids: Vec<String>,
    #[serde(default)]
    pub embedding_metadata_ids: Vec<String>,
}

impl RetrievalProvenanceV1 {
    pub fn for_query(query: &str, policy_json: &[u8]) -> Self {
        Self {
            schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
            policy_fingerprint_sha256: hex_sha256(policy_json),
            query_sha256: hex_sha256(query.as_bytes()),
            selected_profile_knowledge_ids: vec![],
            selected_memory_item_ids: vec![],
            embedding_metadata_ids: vec![],
        }
    }
}
