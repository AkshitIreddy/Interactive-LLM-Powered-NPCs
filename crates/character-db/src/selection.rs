use crate::{CharacterDatabase, CharacterDbError, CHARACTER_DB_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionEvidenceV1 {
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_character_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_character_id: Option<String>,
    #[serde(default)]
    pub visual_candidates: Vec<IdentityCandidateV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityCandidateV1 {
    pub character_id: String,
    /// Normalized higher-is-better confidence in the inclusive 0..=1 range.
    pub confidence: f64,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionPolicyV1 {
    pub minimum_confidence: f64,
    pub minimum_margin: f64,
    pub sticky_confidence: f64,
}

impl Default for SelectionPolicyV1 {
    fn default() -> Self {
        Self {
            minimum_confidence: 0.80,
            minimum_margin: 0.10,
            sticky_confidence: 0.65,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    Explicit,
    AddressedAlias,
    StickyCurrent,
    VisualConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SelectionOutcomeV1 {
    Known {
        character_id: String,
        reason: SelectionReason,
    },
    Background,
    Ambiguous {
        candidate_ids: Vec<String>,
    },
}

pub fn select_character(
    database: &CharacterDatabase,
    evidence: &SelectionEvidenceV1,
    policy: SelectionPolicyV1,
) -> Result<SelectionOutcomeV1, CharacterDbError> {
    if evidence.schema_version != CHARACTER_DB_SCHEMA_VERSION {
        return Err(CharacterDbError::InvalidInput(format!(
            "unsupported selection schema `{}`",
            evidence.schema_version
        )));
    }
    validate_policy(policy)?;

    if let Some(id) = evidence.explicit_character_id.as_deref() {
        database.require_character(id)?;
        return Ok(SelectionOutcomeV1::Known {
            character_id: id.to_owned(),
            reason: SelectionReason::Explicit,
        });
    }

    if let Some(alias) = evidence.addressed_alias.as_deref() {
        let matches = database.characters_for_alias(alias);
        if matches.len() == 1 {
            return Ok(SelectionOutcomeV1::Known {
                character_id: matches[0].id.clone(),
                reason: SelectionReason::AddressedAlias,
            });
        }
        if matches.len() > 1 {
            return Ok(SelectionOutcomeV1::Ambiguous {
                candidate_ids: matches.iter().map(|profile| profile.id.clone()).collect(),
            });
        }
    }

    let mut candidates = evidence
        .visual_candidates
        .iter()
        .filter(|candidate| database.character(&candidate.character_id).is_some())
        .map(|candidate| {
            if !candidate.confidence.is_finite() || !(0.0..=1.0).contains(&candidate.confidence) {
                return Err(CharacterDbError::InvalidInput(format!(
                    "identity confidence for `{}` must be finite and in 0..=1",
                    candidate.character_id
                )));
            }
            Ok(candidate)
        })
        .collect::<Result<Vec<_>, _>>()?;
    candidates.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left.character_id.cmp(&right.character_id))
            .then_with(|| left.evidence_id.cmp(&right.evidence_id))
    });
    // Multiple detectors may report the same character. Keep only that
    // character's strongest, deterministically tie-broken observation so a
    // duplicate source cannot create a false ambiguity against itself.
    let mut seen_character_ids = BTreeSet::new();
    candidates.retain(|candidate| seen_character_ids.insert(candidate.character_id.as_str()));

    if let Some(current) = evidence.current_character_id.as_deref() {
        if database.character(current).is_some()
            && candidates.iter().any(|candidate| {
                candidate.character_id == current
                    && candidate.confidence >= policy.sticky_confidence
            })
        {
            return Ok(SelectionOutcomeV1::Known {
                character_id: current.to_owned(),
                reason: SelectionReason::StickyCurrent,
            });
        }
    }

    let Some(best) = candidates.first() else {
        return Ok(SelectionOutcomeV1::Background);
    };
    if best.confidence < policy.minimum_confidence {
        return Ok(SelectionOutcomeV1::Background);
    }
    let second_confidence = candidates
        .get(1)
        .map_or(0.0, |candidate| candidate.confidence);
    if best.confidence - second_confidence < policy.minimum_margin {
        return Ok(SelectionOutcomeV1::Ambiguous {
            candidate_ids: candidates
                .iter()
                .take_while(|candidate| {
                    best.confidence - candidate.confidence < policy.minimum_margin
                })
                .map(|candidate| candidate.character_id.clone())
                .collect(),
        });
    }
    Ok(SelectionOutcomeV1::Known {
        character_id: best.character_id.clone(),
        reason: SelectionReason::VisualConfidence,
    })
}

pub fn select_background_profile<'a>(
    database: &'a CharacterDatabase,
    continuity_key: &str,
) -> Result<&'a npc_game_profile::CharacterProfile, CharacterDbError> {
    if continuity_key.trim().is_empty() {
        return Err(CharacterDbError::InvalidInput(
            "background continuity key cannot be blank".to_owned(),
        ));
    }
    let mut profiles = database
        .profile()
        .characters
        .iter()
        .filter(|profile| profile.background_npc)
        .collect::<Vec<_>>();
    profiles.sort_by_key(|profile| profile.id.as_str());
    if profiles.is_empty() {
        return Err(CharacterDbError::InvalidInput(
            "profile has no background character archetype".to_owned(),
        ));
    }
    Ok(profiles[stable_index(continuity_key.as_bytes(), profiles.len())])
}

pub(crate) fn stable_index(seed: &[u8], count: usize) -> usize {
    debug_assert!(count > 0);
    let digest = Sha256::digest(seed);
    let value = u64::from_be_bytes(digest[0..8].try_into().expect("slice has eight bytes"));
    (value % count as u64) as usize
}

fn validate_policy(policy: SelectionPolicyV1) -> Result<(), CharacterDbError> {
    for (name, value) in [
        ("minimum_confidence", policy.minimum_confidence),
        ("minimum_margin", policy.minimum_margin),
        ("sticky_confidence", policy.sticky_confidence),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(CharacterDbError::InvalidInput(format!(
                "{name} must be finite and in 0..=1"
            )));
        }
    }
    Ok(())
}
