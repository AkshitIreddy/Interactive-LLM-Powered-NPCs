use crate::{
    selection::stable_index, CharacterDatabase, CharacterDbError, CHARACTER_DB_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

const ENCOUNTER_NAMESPACE: Uuid = Uuid::from_bytes([
    0x4a, 0xc4, 0x3b, 0x8e, 0x2d, 0x32, 0x4c, 0x49, 0x91, 0x6b, 0x95, 0x25, 0x7e, 0x8d, 0x43, 0x31,
]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceCandidateV1 {
    pub binding_id: String,
    pub adapter_id: String,
    pub provider_voice_id: String,
    pub locale: String,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterStatus {
    Active,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterRecordV1 {
    pub schema_version: String,
    pub encounter_id: Uuid,
    pub game_profile_id: String,
    pub archetype_character_id: String,
    /// A one-way digest is persisted instead of raw OCR, face, or tracking data.
    pub continuity_key_sha256: String,
    pub selected_voice: VoiceCandidateV1,
    pub created_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub expires_at_ms: i64,
    pub status: EncounterStatus,
}

impl EncounterRecordV1 {
    pub fn observe(&mut self, observed_at_ms: i64) -> Result<(), CharacterDbError> {
        if observed_at_ms < self.created_at_ms {
            return Err(CharacterDbError::InvalidInput(
                "encounter observation predates creation".to_owned(),
            ));
        }
        self.last_seen_at_ms = self.last_seen_at_ms.max(observed_at_ms);
        if observed_at_ms >= self.expires_at_ms {
            self.status = EncounterStatus::Expired;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterCorrectionSourceV1 {
    ManualExplicit,
}

/// Auditable encounter lifecycle. Correction and merge events express native
/// user intent, but never silently rewrite memory: consumers must perform an
/// explicit, receipt-backed migration under the destination authority scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EncounterLifecycleEventV1 {
    Created {
        encounter_id: Uuid,
        game_profile_id: String,
        occurred_at_ms: i64,
    },
    Observed {
        encounter_id: Uuid,
        occurred_at_ms: i64,
    },
    CorrectedToAuthoredCharacter {
        encounter_id: Uuid,
        character_id: String,
        source: EncounterCorrectionSourceV1,
        occurred_at_ms: i64,
        memory_migration_required: bool,
    },
    MergedIntoEncounter {
        source_encounter_id: Uuid,
        destination_encounter_id: Uuid,
        source: EncounterCorrectionSourceV1,
        occurred_at_ms: i64,
        memory_migration_required: bool,
    },
    Expired {
        encounter_id: Uuid,
        occurred_at_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterRegistryV1 {
    pub schema_version: String,
    pub game_profile_id: String,
    records: BTreeMap<Uuid, EncounterRecordV1>,
    redirects: BTreeMap<Uuid, Uuid>,
    events: Vec<EncounterLifecycleEventV1>,
}

impl EncounterRegistryV1 {
    pub fn new(game_profile_id: impl Into<String>) -> Result<Self, CharacterDbError> {
        let game_profile_id = game_profile_id.into();
        if game_profile_id.trim().is_empty() {
            return Err(CharacterDbError::InvalidInput(
                "encounter registry game profile cannot be blank".to_owned(),
            ));
        }
        Ok(Self {
            schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
            game_profile_id,
            records: BTreeMap::new(),
            redirects: BTreeMap::new(),
            events: Vec::new(),
        })
    }

    pub fn insert(&mut self, record: EncounterRecordV1) -> Result<(), CharacterDbError> {
        if record.schema_version != CHARACTER_DB_SCHEMA_VERSION
            || record.game_profile_id != self.game_profile_id
            || self.records.contains_key(&record.encounter_id)
        {
            return Err(CharacterDbError::InvalidInput(
                "encounter does not belong to this registry or already exists".to_owned(),
            ));
        }
        let encounter_id = record.encounter_id;
        let occurred_at_ms = record.created_at_ms;
        self.records.insert(encounter_id, record);
        self.events.push(EncounterLifecycleEventV1::Created {
            encounter_id,
            game_profile_id: self.game_profile_id.clone(),
            occurred_at_ms,
        });
        Ok(())
    }

    pub fn record(&self, encounter_id: Uuid) -> Option<&EncounterRecordV1> {
        let resolved = self
            .redirects
            .get(&encounter_id)
            .copied()
            .unwrap_or(encounter_id);
        self.records.get(&resolved)
    }

    pub fn observe(
        &mut self,
        encounter_id: Uuid,
        observed_at_ms: i64,
    ) -> Result<(), CharacterDbError> {
        let resolved = self
            .redirects
            .get(&encounter_id)
            .copied()
            .unwrap_or(encounter_id);
        let record = self
            .records
            .get_mut(&resolved)
            .ok_or_else(|| CharacterDbError::InvalidInput("unknown encounter ID".to_owned()))?;
        if record.status != EncounterStatus::Active || observed_at_ms >= record.expires_at_ms {
            return Err(CharacterDbError::InvalidInput(
                "expired encounter cannot be observed".to_owned(),
            ));
        }
        record.observe(observed_at_ms)?;
        self.events.push(EncounterLifecycleEventV1::Observed {
            encounter_id: resolved,
            occurred_at_ms: observed_at_ms,
        });
        Ok(())
    }

    pub fn correct_to_authored_character(
        &mut self,
        database: &CharacterDatabase,
        encounter_id: Uuid,
        character_id: &str,
        explicitly_confirmed: bool,
        occurred_at_ms: i64,
    ) -> Result<(), CharacterDbError> {
        if !explicitly_confirmed || database.profile().id != self.game_profile_id {
            return Err(CharacterDbError::InvalidInput(
                "encounter correction requires explicit native confirmation in the same game"
                    .to_owned(),
            ));
        }
        let character = database.require_character(character_id)?;
        if character.background_npc {
            return Err(CharacterDbError::InvalidInput(
                "authored correction target must be a named character".to_owned(),
            ));
        }
        let record = self
            .records
            .get_mut(&encounter_id)
            .ok_or_else(|| CharacterDbError::InvalidInput("unknown encounter ID".to_owned()))?;
        require_active_at(record, occurred_at_ms)?;
        record.status = EncounterStatus::Expired;
        record.last_seen_at_ms = record.last_seen_at_ms.max(occurred_at_ms);
        self.events
            .push(EncounterLifecycleEventV1::CorrectedToAuthoredCharacter {
                encounter_id,
                character_id: character.id.clone(),
                source: EncounterCorrectionSourceV1::ManualExplicit,
                occurred_at_ms,
                memory_migration_required: true,
            });
        Ok(())
    }

    pub fn merge_unknown_encounters(
        &mut self,
        source_encounter_id: Uuid,
        destination_encounter_id: Uuid,
        explicitly_confirmed: bool,
        occurred_at_ms: i64,
    ) -> Result<(), CharacterDbError> {
        if !explicitly_confirmed || source_encounter_id == destination_encounter_id {
            return Err(CharacterDbError::InvalidInput(
                "encounter merge requires two distinct explicitly confirmed encounters".to_owned(),
            ));
        }
        let source = self.records.get(&source_encounter_id).ok_or_else(|| {
            CharacterDbError::InvalidInput("unknown source encounter ID".to_owned())
        })?;
        let destination = self.records.get(&destination_encounter_id).ok_or_else(|| {
            CharacterDbError::InvalidInput("unknown destination encounter ID".to_owned())
        })?;
        require_active_at(source, occurred_at_ms)?;
        require_active_at(destination, occurred_at_ms)?;
        let source = self.records.get_mut(&source_encounter_id).ok_or_else(|| {
            CharacterDbError::InvalidInput("unknown source encounter ID".to_owned())
        })?;
        source.status = EncounterStatus::Expired;
        source.last_seen_at_ms = source.last_seen_at_ms.max(occurred_at_ms);
        self.redirects
            .insert(source_encounter_id, destination_encounter_id);
        self.events
            .push(EncounterLifecycleEventV1::MergedIntoEncounter {
                source_encounter_id,
                destination_encounter_id,
                source: EncounterCorrectionSourceV1::ManualExplicit,
                occurred_at_ms,
                memory_migration_required: true,
            });
        Ok(())
    }

    pub fn expire_due(&mut self, now_ms: i64) -> Vec<Uuid> {
        let mut expired = Vec::new();
        for record in self.records.values_mut() {
            if record.status == EncounterStatus::Active && now_ms >= record.expires_at_ms {
                record.status = EncounterStatus::Expired;
                expired.push(record.encounter_id);
                self.events.push(EncounterLifecycleEventV1::Expired {
                    encounter_id: record.encounter_id,
                    occurred_at_ms: now_ms,
                });
            }
        }
        expired
    }

    pub fn drain_events(&mut self) -> Vec<EncounterLifecycleEventV1> {
        std::mem::take(&mut self.events)
    }
}

fn require_active_at(
    record: &EncounterRecordV1,
    occurred_at_ms: i64,
) -> Result<(), CharacterDbError> {
    if record.status != EncounterStatus::Active
        || occurred_at_ms < record.created_at_ms
        || occurred_at_ms >= record.expires_at_ms
    {
        return Err(CharacterDbError::InvalidInput(
            "encounter is expired or event time is outside its active lifetime".to_owned(),
        ));
    }
    Ok(())
}

pub fn create_stable_encounter(
    database: &CharacterDatabase,
    archetype_character_id: &str,
    continuity_key: &str,
    voice_candidates: &[VoiceCandidateV1],
    created_at_ms: i64,
    expires_at_ms: i64,
) -> Result<EncounterRecordV1, CharacterDbError> {
    let archetype = database.require_character(archetype_character_id)?;
    if !archetype.background_npc {
        return Err(CharacterDbError::InvalidInput(format!(
            "character `{archetype_character_id}` is not a background archetype"
        )));
    }
    if continuity_key.trim().is_empty() {
        return Err(CharacterDbError::InvalidInput(
            "encounter continuity key cannot be blank".to_owned(),
        ));
    }
    if expires_at_ms <= created_at_ms {
        return Err(CharacterDbError::InvalidInput(
            "encounter expiry must follow creation".to_owned(),
        ));
    }
    let mut voices = voice_candidates.to_vec();
    voices.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
    voices.dedup_by(|left, right| left.binding_id == right.binding_id);
    for voice in &voices {
        validate_voice(voice)?;
    }
    let selected_voice = if let Some(provider_voice_id) = &archetype.voice.provider_voice_id {
        VoiceCandidateV1 {
            binding_id: format!("profile:{}", archetype.id),
            adapter_id: archetype
                .voice
                .adapter_id
                .clone()
                .unwrap_or_else(|| "profile-default".to_owned()),
            provider_voice_id: provider_voice_id.clone(),
            locale: archetype.voice.locale.clone(),
            traits: archetype.voice.style_tags.clone(),
            catalog_version: archetype.voice.catalog_version.clone(),
            license: archetype.voice.license.clone(),
        }
    } else {
        if voices.is_empty() {
            return Err(CharacterDbError::InvalidInput(
                "background encounter requires a profile voice or a non-empty voice pool"
                    .to_owned(),
            ));
        }
        voices[stable_index(continuity_key.as_bytes(), voices.len())].clone()
    };

    let identity_material = format!(
        "{}\0{}\0{}\0{}",
        CHARACTER_DB_SCHEMA_VERSION,
        database.profile().id,
        archetype_character_id,
        continuity_key
    );
    let encounter_id = Uuid::new_v5(&ENCOUNTER_NAMESPACE, identity_material.as_bytes());
    let continuity_key_sha256 = hex_sha256(continuity_key.as_bytes());
    Ok(EncounterRecordV1 {
        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
        encounter_id,
        game_profile_id: database.profile().id.clone(),
        archetype_character_id: archetype_character_id.to_owned(),
        continuity_key_sha256,
        selected_voice,
        created_at_ms,
        last_seen_at_ms: created_at_ms,
        expires_at_ms,
        status: EncounterStatus::Active,
    })
}

fn validate_voice(voice: &VoiceCandidateV1) -> Result<(), CharacterDbError> {
    if [
        voice.binding_id.as_str(),
        voice.adapter_id.as_str(),
        voice.provider_voice_id.as_str(),
        voice.locale.as_str(),
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err(CharacterDbError::InvalidInput(
            "voice candidate IDs and locale cannot be blank".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
