use crate::{
    CharacterDatabase, CharacterDbError, RetrievalProvenanceV1, CHARACTER_DB_SCHEMA_VERSION,
};
use npc_game_profile::{KnowledgeAuthority, PromptAuthority, ReviewStatus, StyleExample};
use npc_memory::{
    AuthorityScope, DeliveredTurnRecord, DerivedMemoryRecord, KnowledgeClass, MemoryContextBundle,
    SpoilerPolicy, SpoilerScope, TurnSpeaker,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptBuildRequestV1 {
    pub schema_version: String,
    pub scope: AuthorityScope,
    pub query: String,
    #[serde(default)]
    pub enabled_spoiler_tiers: Vec<String>,
    #[serde(default)]
    pub memory_context: MemoryContextBundle,
    #[serde(default)]
    pub memory_spoiler_policy: SpoilerPolicy,
    #[serde(default = "default_style_examples")]
    pub max_style_examples: usize,
}

fn default_style_examples() -> usize {
    4
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptRecordV1 {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_class: Option<KnowledgeClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spoiler_scope: Option<SpoilerScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptSectionV1 {
    pub authority: PromptAuthority,
    pub records: Vec<PromptRecordV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptContextV1 {
    pub schema_version: String,
    pub profile_id: String,
    pub character_id: String,
    pub sections: Vec<PromptSectionV1>,
    pub retrieval_provenance: RetrievalProvenanceV1,
}

/// Assemble a typed prompt context. Authority lanes remain separate so callers
/// cannot accidentally flatten authored canon, character secrets, summaries,
/// and delivered transcript into one indistinguishable string.
pub fn build_prompt_context(
    database: &CharacterDatabase,
    mut request: PromptBuildRequestV1,
) -> Result<PromptContextV1, CharacterDbError> {
    if request.schema_version != CHARACTER_DB_SCHEMA_VERSION {
        return Err(CharacterDbError::InvalidInput(format!(
            "unsupported prompt request schema `{}`",
            request.schema_version
        )));
    }
    request
        .scope
        .validate()
        .map_err(|error| CharacterDbError::InvalidInput(error.to_string()))?;
    if request.query.trim().is_empty() || request.scope.session_id.is_none() {
        return Err(CharacterDbError::InvalidInput(
            "prompt query and a session-scoped authority boundary are required".to_owned(),
        ));
    }
    if request.max_style_examples > 64 {
        return Err(CharacterDbError::InvalidInput(
            "max_style_examples cannot exceed 64".to_owned(),
        ));
    }
    let profile = database.profile();
    if request.scope.profile_id != profile.id || request.scope.game_id != profile.id {
        return Err(CharacterDbError::InvalidInput(
            "prompt authority scope does not match the active game profile".to_owned(),
        ));
    }
    let character_id = request.scope.character_id.as_deref().ok_or_else(|| {
        CharacterDbError::InvalidInput("prompt authority scope requires a character ID".to_owned())
    })?;
    let character = database.require_character(character_id)?;
    let enabled_tiers = request
        .enabled_spoiler_tiers
        .drain(..)
        .collect::<HashSet<_>>();
    let policy = &profile.content.retrieval;
    let mut sections = Vec::new();
    let mut remaining_memory_records = policy.max_memory_records as usize;

    for authority in &policy.authority_order {
        let records = match authority {
            PromptAuthority::CoreCanon => {
                let mut records = vec![PromptRecordV1 {
                    id: "profile-world-lore".to_owned(),
                    text: profile.content.world_lore.clone(),
                    provenance_id: None,
                    memory_class: None,
                    spoiler_scope: None,
                }];
                records.extend(
                    profile
                        .content
                        .knowledge
                        .iter()
                        .filter(|record| record.authority == KnowledgeAuthority::CoreCanon)
                        .filter(|record| provenance_available(&record.provenance_id, profile))
                        .filter(|record| {
                            spoiler_allowed(&record.spoiler_tier, &enabled_tiers, profile)
                        })
                        .map(profile_knowledge_record),
                );
                records.sort_by(|left, right| left.id.cmp(&right.id));
                records.truncate(policy.max_core_records as usize);
                records
            }
            PromptAuthority::CharacterProfile => {
                let mut records = vec![
                    PromptRecordV1 {
                        id: format!("{}-biography", character.id),
                        text: character.biography.clone(),
                        provenance_id: None,
                        memory_class: None,
                        spoiler_scope: None,
                    },
                    PromptRecordV1 {
                        id: format!("{}-personality", character.id),
                        text: character.personality.clone(),
                        provenance_id: None,
                        memory_class: None,
                        spoiler_scope: None,
                    },
                    PromptRecordV1 {
                        id: format!("{}-dialogue-style", character.id),
                        text: character.dialogue_style.clone(),
                        provenance_id: None,
                        memory_class: None,
                        spoiler_scope: None,
                    },
                ];
                let available_examples = character
                    .style_examples
                    .iter()
                    .filter(|example| provenance_available(&example.provenance_id, profile))
                    .cloned()
                    .collect::<Vec<_>>();
                records.extend(
                    select_style_examples(
                        &available_examples,
                        request.query.as_bytes(),
                        request.max_style_examples,
                    )
                    .into_iter()
                    .map(|example| PromptRecordV1 {
                        id: example.id.clone(),
                        text: format!("{}: {}", example.speaker, example.text),
                        provenance_id: Some(example.provenance_id.clone()),
                        memory_class: None,
                        spoiler_scope: None,
                    }),
                );
                records
            }
            PromptAuthority::GamePublic => {
                let mut records = profile
                    .content
                    .knowledge
                    .iter()
                    .filter(|record| record.authority == KnowledgeAuthority::GamePublic)
                    .filter(|record| provenance_available(&record.provenance_id, profile))
                    .filter(|record| spoiler_allowed(&record.spoiler_tier, &enabled_tiers, profile))
                    .map(profile_knowledge_record)
                    .collect::<Vec<_>>();
                records.sort_by(|left, right| left.id.cmp(&right.id));
                records.truncate(policy.max_public_records as usize);
                records
            }
            PromptAuthority::CharacterAuthored => {
                let mut records = profile
                    .content
                    .knowledge
                    .iter()
                    .filter(|record| {
                        record.authority == KnowledgeAuthority::CharacterAuthored
                            && record.owner_character_id.as_deref() == Some(character.id.as_str())
                    })
                    .filter(|record| provenance_available(&record.provenance_id, profile))
                    .filter(|record| spoiler_allowed(&record.spoiler_tier, &enabled_tiers, profile))
                    .map(profile_knowledge_record)
                    .collect::<Vec<_>>();
                records.sort_by(|left, right| left.id.cmp(&right.id));
                records.truncate(policy.max_character_records as usize);
                records
            }
            PromptAuthority::RetrievedMemory => {
                let mut candidates = request
                    .memory_context
                    .world_lore
                    .iter()
                    .chain(&request.memory_context.biography)
                    .chain(&request.memory_context.character_knowledge)
                    .chain(&request.memory_context.uncertain_public_info)
                    .filter(|record| record.scope == request.scope)
                    .filter(|record| request.memory_spoiler_policy.allows(record.spoiler_scope))
                    .collect::<Vec<_>>();
                candidates.sort_by(|left, right| {
                    right
                        .importance
                        .total_cmp(&left.importance)
                        .then_with(|| right.observed_at_ms.cmp(&left.observed_at_ms))
                        .then_with(|| left.id.cmp(&right.id))
                });
                let records = candidates
                    .into_iter()
                    .take(remaining_memory_records)
                    .map(derived_memory_record)
                    .collect::<Vec<_>>();
                remaining_memory_records = remaining_memory_records.saturating_sub(records.len());
                records
            }
            PromptAuthority::SessionSummary => {
                request
                    .memory_context
                    .long_term_summaries
                    .sort_by(|left, right| {
                        right
                            .importance
                            .total_cmp(&left.importance)
                            .then_with(|| right.observed_at_ms.cmp(&left.observed_at_ms))
                            .then_with(|| left.id.cmp(&right.id))
                    });
                let records = request
                    .memory_context
                    .long_term_summaries
                    .iter()
                    .filter(|record| record.scope == request.scope)
                    .filter(|record| request.memory_spoiler_policy.allows(record.spoiler_scope))
                    .take(remaining_memory_records)
                    .map(derived_memory_record)
                    .collect::<Vec<_>>();
                remaining_memory_records = remaining_memory_records.saturating_sub(records.len());
                records
            }
            PromptAuthority::RecentDeliveredTurns => {
                request
                    .memory_context
                    .recent_dialogue
                    .sort_by(|left, right| {
                        left.sequence
                            .cmp(&right.sequence)
                            .then_with(|| left.turn_id.cmp(&right.turn_id))
                    });
                let mut records = request
                    .memory_context
                    .recent_dialogue
                    .iter()
                    .filter(|turn| turn.scope == request.scope)
                    .map(delivered_turn_record)
                    .collect::<Vec<_>>();
                let keep = policy.query_window_delivered_turns as usize;
                if records.len() > keep {
                    records.drain(0..records.len() - keep);
                }
                records
            }
        };
        if !records.is_empty() {
            sections.push(PromptSectionV1 {
                authority: *authority,
                records,
            });
        }
    }

    let policy_json = serde_json::to_vec(policy)
        .map_err(|error| CharacterDbError::InvalidInput(error.to_string()))?;
    let mut retrieval_provenance = RetrievalProvenanceV1::for_query(&request.query, &policy_json);
    retrieval_provenance.selected_profile_knowledge_ids = sections
        .iter()
        .filter(|section| {
            matches!(
                section.authority,
                PromptAuthority::CoreCanon
                    | PromptAuthority::GamePublic
                    | PromptAuthority::CharacterAuthored
            )
        })
        .flat_map(|section| section.records.iter().map(|record| record.id.clone()))
        .collect();
    retrieval_provenance.selected_memory_item_ids = sections
        .iter()
        .filter(|section| {
            matches!(
                section.authority,
                PromptAuthority::SessionSummary
                    | PromptAuthority::RecentDeliveredTurns
                    | PromptAuthority::RetrievedMemory
            )
        })
        .flat_map(|section| section.records.iter().map(|record| record.id.clone()))
        .collect();

    Ok(PromptContextV1 {
        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
        profile_id: profile.id.clone(),
        character_id: character.id.clone(),
        sections,
        retrieval_provenance,
    })
}

fn profile_knowledge_record(record: &npc_game_profile::KnowledgeRecord) -> PromptRecordV1 {
    PromptRecordV1 {
        id: record.id.clone(),
        text: record.text.clone(),
        provenance_id: Some(record.provenance_id.clone()),
        memory_class: None,
        spoiler_scope: None,
    }
}

fn derived_memory_record(record: &DerivedMemoryRecord) -> PromptRecordV1 {
    PromptRecordV1 {
        id: record.id.clone(),
        text: record.content.clone(),
        provenance_id: record
            .provenance
            .source_id
            .clone()
            .or_else(|| Some(record.id.clone())),
        memory_class: Some(record.class),
        spoiler_scope: Some(record.spoiler_scope),
    }
}

fn spoiler_allowed(
    tier: &str,
    enabled: &HashSet<String>,
    profile: &npc_game_profile::GameProfileV2,
) -> bool {
    enabled.contains(tier)
        || profile
            .content
            .spoiler_tiers
            .iter()
            .any(|candidate| candidate.id == tier && candidate.default_enabled)
}

fn provenance_available(id: &str, profile: &npc_game_profile::GameProfileV2) -> bool {
    profile
        .content
        .provenance
        .iter()
        .find(|record| record.id == id)
        .is_some_and(|record| matches!(record.review_status, None | Some(ReviewStatus::Approved)))
}

fn select_style_examples<'a>(
    examples: &'a [StyleExample],
    seed: &[u8],
    limit: usize,
) -> Vec<&'a StyleExample> {
    let mut ranked = examples
        .iter()
        .map(|example| {
            let mut hasher = Sha256::new();
            hasher.update(seed);
            hasher.update([0]);
            hasher.update(example.id.as_bytes());
            let digest = hasher.finalize();
            (
                digest.to_vec(),
                std::cmp::Reverse(example.weight_millis),
                example,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.id.cmp(&right.2.id))
    });
    ranked.into_iter().take(limit).map(|item| item.2).collect()
}

fn delivered_turn_record(turn: &DeliveredTurnRecord) -> PromptRecordV1 {
    let speaker = match turn.speaker {
        TurnSpeaker::Player => "Player",
        TurnSpeaker::Npc => "NPC",
    };
    PromptRecordV1 {
        id: format!("turn:{}", turn.turn_id),
        text: format!("{speaker}: {}", turn.delivered_text),
        provenance_id: Some(turn.turn_id.clone()),
        memory_class: None,
        spoiler_scope: None,
    }
}
