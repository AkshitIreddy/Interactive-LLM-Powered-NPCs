use npc_game_profile::{
    CharacterProfile, GameProfileV2, KnowledgeAuthority, ReviewStatus, ValidationReport,
};
use npc_memory::{AuthorityScope, DerivedMemoryInput, KnowledgeClass, Provenance, SpoilerScope};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CharacterDbError {
    #[error("game profile is invalid")]
    InvalidProfile(ValidationReport),
    #[error("unknown character `{0}`")]
    UnknownCharacter(String),
    #[error("invalid character database input: {0}")]
    InvalidInput(String),
}

/// Read-only index over the canonical game-profile schema. Runtime memory stays
/// in `npc-memory`; encounters and prompt views are created by sibling modules.
#[derive(Debug, Clone)]
pub struct CharacterDatabase {
    profile: GameProfileV2,
    character_indexes: HashMap<String, usize>,
    alias_indexes: HashMap<String, Vec<usize>>,
}

impl CharacterDatabase {
    pub fn new(profile: GameProfileV2) -> Result<Self, CharacterDbError> {
        let report = profile.validate();
        if !report.is_valid() {
            return Err(CharacterDbError::InvalidProfile(report));
        }
        let mut character_indexes = HashMap::new();
        let mut alias_indexes: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, character) in profile.characters.iter().enumerate() {
            character_indexes.insert(character.id.clone(), index);
            for alias in std::iter::once(character.display_name.as_str())
                .chain(character.aliases.iter().map(String::as_str))
            {
                alias_indexes
                    .entry(normalize_alias(alias))
                    .or_default()
                    .push(index);
            }
        }
        for indexes in alias_indexes.values_mut() {
            indexes.sort_by_key(|index| profile.characters[*index].id.as_str());
            indexes.dedup();
        }
        Ok(Self {
            profile,
            character_indexes,
            alias_indexes,
        })
    }

    pub fn profile(&self) -> &GameProfileV2 {
        &self.profile
    }

    pub fn character(&self, id: &str) -> Option<&CharacterProfile> {
        self.character_indexes
            .get(id)
            .map(|index| &self.profile.characters[*index])
    }

    pub fn require_character(&self, id: &str) -> Result<&CharacterProfile, CharacterDbError> {
        self.character(id)
            .ok_or_else(|| CharacterDbError::UnknownCharacter(id.to_owned()))
    }

    pub(crate) fn characters_for_alias(&self, alias: &str) -> Vec<&CharacterProfile> {
        self.alias_indexes
            .get(&normalize_alias(alias))
            .into_iter()
            .flatten()
            .map(|index| &self.profile.characters[*index])
            .collect()
    }

    /// Convert approved authored knowledge into the canonical memory-store input
    /// type. Pending/rejected imports remain inert and are never indexed.
    pub fn approved_knowledge_memory_inputs(
        &self,
        user_id: &str,
    ) -> Result<Vec<DerivedMemoryInput>, CharacterDbError> {
        if user_id.trim().is_empty() {
            return Err(CharacterDbError::InvalidInput(
                "knowledge indexing requires a non-blank user ID".to_owned(),
            ));
        }
        let inputs = self
            .profile
            .content
            .knowledge
            .iter()
            .filter(|record| {
                self.profile
                    .content
                    .provenance
                    .iter()
                    .find(|provenance| provenance.id == record.provenance_id)
                    .is_some_and(|provenance| {
                        matches!(
                            provenance.review_status,
                            None | Some(ReviewStatus::Approved)
                        )
                    })
            })
            .map(|record| {
                let (class, spoiler_scope) = match record.authority {
                    KnowledgeAuthority::CoreCanon | KnowledgeAuthority::GamePublic => {
                        (KnowledgeClass::WorldLore, SpoilerScope::Game)
                    }
                    KnowledgeAuthority::CharacterAuthored => (
                        KnowledgeClass::CharacterKnowledge,
                        SpoilerScope::CharacterPrivate,
                    ),
                };
                DerivedMemoryInput {
                    id: Some(format!("profile-knowledge:{}", record.id)),
                    scope: AuthorityScope {
                        user_id: user_id.to_owned(),
                        profile_id: self.profile.id.clone(),
                        game_id: self.profile.id.clone(),
                        character_id: record.owner_character_id.clone(),
                        encounter_id: None,
                        session_id: None,
                        save_id: None,
                    },
                    class,
                    spoiler_scope,
                    content: record.text.clone(),
                    provenance: Provenance {
                        source_kind: "game_profile_knowledge".to_owned(),
                        source_id: Some(record.provenance_id.clone()),
                        source_uri: None,
                        author: None,
                        captured_at_ms: None,
                        attributes: BTreeMap::from([
                            ("knowledge_id".to_owned(), json!(record.id)),
                            ("authority".to_owned(), json!(record.authority)),
                            ("spoiler_tier".to_owned(), json!(record.spoiler_tier)),
                        ]),
                    },
                    confidence: 1.0,
                    importance: 0.7,
                    observed_at_ms: 0,
                    expires_at_ms: None,
                    source_turn_ids: vec![],
                    generator: None,
                }
            })
            .collect::<Vec<_>>();
        for input in &inputs {
            input
                .validate()
                .map_err(|error| CharacterDbError::InvalidInput(error.to_string()))?;
        }
        Ok(inputs)
    }
}

pub(crate) fn normalize_alias(value: &str) -> String {
    value
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}
