#![allow(clippy::unwrap_used)]
// Contract tests intentionally stop at the exact fixture operation that violates an invariant.

mod support;

use std::collections::HashSet;

use npc_character_db::*;
use npc_game_profile::{
    KnowledgeAuthority, KnowledgeRecord, PromptAuthority, ProvenanceKind, ProvenanceRecord,
    ReviewStatus, StyleExample,
};
use npc_memory::{
    AuthorityScope, DeliveredTurnRecord, DerivedMemoryRecord, KnowledgeClass, MemoryContextBundle,
    Provenance, SpoilerPolicy, SpoilerScope, TurnSpeaker,
};

fn database() -> CharacterDatabase {
    CharacterDatabase::new(support::cyberpunk_profile()).expect("profile should index")
}

fn evidence() -> SelectionEvidenceV1 {
    SelectionEvidenceV1 {
        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
        explicit_character_id: None,
        addressed_alias: None,
        current_character_id: None,
        visual_candidates: vec![],
    }
}

#[test]
fn selection_is_explicit_first_sticky_and_deterministic_on_ties() {
    let database = database();
    let mut request = evidence();
    request.explicit_character_id = Some("jackie-welles".to_owned());
    request.addressed_alias = Some("Johnny".to_owned());
    assert_eq!(
        select_character(&database, &request, SelectionPolicyV1::default()).unwrap(),
        SelectionOutcomeV1::Known {
            character_id: "jackie-welles".to_owned(),
            reason: SelectionReason::Explicit,
        }
    );

    let mut request = evidence();
    request.current_character_id = Some("johnny-silverhand".to_owned());
    request.visual_candidates = vec![
        IdentityCandidateV1 {
            character_id: "jackie-welles".to_owned(),
            confidence: 0.95,
            evidence_id: "track-2".to_owned(),
        },
        IdentityCandidateV1 {
            character_id: "johnny-silverhand".to_owned(),
            confidence: 0.70,
            evidence_id: "track-1".to_owned(),
        },
    ];
    assert_eq!(
        select_character(&database, &request, SelectionPolicyV1::default()).unwrap(),
        SelectionOutcomeV1::Known {
            character_id: "johnny-silverhand".to_owned(),
            reason: SelectionReason::StickyCurrent,
        }
    );

    let mut request = evidence();
    request.visual_candidates = vec![
        IdentityCandidateV1 {
            character_id: "johnny-silverhand".to_owned(),
            confidence: 0.91,
            evidence_id: "b".to_owned(),
        },
        IdentityCandidateV1 {
            character_id: "jackie-welles".to_owned(),
            confidence: 0.91,
            evidence_id: "a".to_owned(),
        },
    ];
    assert_eq!(
        select_character(&database, &request, SelectionPolicyV1::default()).unwrap(),
        SelectionOutcomeV1::Ambiguous {
            candidate_ids: vec!["jackie-welles".to_owned(), "johnny-silverhand".to_owned()],
        }
    );
}

#[test]
fn alias_collisions_are_ambiguous_instead_of_order_dependent() {
    let mut profile = support::cyberpunk_profile();
    profile.characters[0].aliases.push("Silver".to_owned());
    profile.characters[1].aliases.push("silver".to_owned());
    let database = CharacterDatabase::new(profile).unwrap();
    let mut request = evidence();
    request.addressed_alias = Some(" SILVER ".to_owned());
    let outcome = select_character(&database, &request, SelectionPolicyV1::default()).unwrap();
    let SelectionOutcomeV1::Ambiguous { candidate_ids } = outcome else {
        panic!("alias collision must be ambiguous");
    };
    assert_eq!(candidate_ids.len(), 2);
    assert!(candidate_ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn duplicate_visual_sources_for_one_character_do_not_create_self_ambiguity() {
    let database = database();
    let mut request = evidence();
    request.visual_candidates = vec![
        IdentityCandidateV1 {
            character_id: "jackie-welles".to_owned(),
            confidence: 0.93,
            evidence_id: "face-track".to_owned(),
        },
        IdentityCandidateV1 {
            character_id: "jackie-welles".to_owned(),
            confidence: 0.90,
            evidence_id: "ocr-nameplate".to_owned(),
        },
    ];
    assert_eq!(
        select_character(&database, &request, SelectionPolicyV1::default()).unwrap(),
        SelectionOutcomeV1::Known {
            character_id: "jackie-welles".to_owned(),
            reason: SelectionReason::VisualConfidence,
        }
    );
}

#[test]
fn background_profile_and_voice_are_stable_across_input_order() {
    let database = database();
    let archetype = select_background_profile(&database, "track:market:42").unwrap();
    assert!(archetype.background_npc);
    let voices = vec![
        VoiceCandidateV1 {
            binding_id: "voice-z".to_owned(),
            adapter_id: "test".to_owned(),
            provider_voice_id: "z".to_owned(),
            locale: "en-US".to_owned(),
            traits: vec!["rough".to_owned()],
            catalog_version: Some("1".to_owned()),
            license: Some("test".to_owned()),
        },
        VoiceCandidateV1 {
            binding_id: "voice-a".to_owned(),
            adapter_id: "test".to_owned(),
            provider_voice_id: "a".to_owned(),
            locale: "en-US".to_owned(),
            traits: vec!["warm".to_owned()],
            catalog_version: Some("1".to_owned()),
            license: Some("test".to_owned()),
        },
    ];
    let first = create_stable_encounter(
        &database,
        &archetype.id,
        "track:market:42",
        &voices,
        1_000,
        61_000,
    )
    .unwrap();
    let second = create_stable_encounter(
        &database,
        &archetype.id,
        "track:market:42",
        &voices.into_iter().rev().collect::<Vec<_>>(),
        1_000,
        61_000,
    )
    .unwrap();
    assert_eq!(first.encounter_id, second.encounter_id);
    assert_eq!(first.selected_voice, second.selected_voice);
    assert_eq!(first.continuity_key_sha256.len(), 64);

    let mut expired = first;
    expired.observe(61_000).unwrap();
    assert_eq!(expired.status, EncounterStatus::Expired);
}

#[test]
fn encounter_merge_correction_and_expiry_require_explicit_events() {
    let database = database();
    let archetype = select_background_profile(&database, "encounter-lifecycle").unwrap();
    let voices = vec![VoiceCandidateV1 {
        binding_id: "voice-lifecycle".to_owned(),
        adapter_id: "test".to_owned(),
        provider_voice_id: "voice-1".to_owned(),
        locale: "en-US".to_owned(),
        traits: vec!["neutral".to_owned()],
        catalog_version: Some("1".to_owned()),
        license: Some("test".to_owned()),
    }];
    let encounter = |key: &str, expires_at_ms: i64| {
        create_stable_encounter(&database, &archetype.id, key, &voices, 10, expires_at_ms).unwrap()
    };
    let source = encounter("track:lifecycle:source", 100);
    let destination = encounter("track:lifecycle:destination", 100);
    let expiring = encounter("track:lifecycle:expiry", 20);
    let correction = encounter("track:lifecycle:correction", 100);
    let mut registry = EncounterRegistryV1::new("cyberpunk-2077").unwrap();
    for record in [
        source.clone(),
        destination.clone(),
        expiring.clone(),
        correction.clone(),
    ] {
        registry.insert(record).unwrap();
    }
    assert_eq!(registry.drain_events().len(), 4);

    assert!(registry
        .merge_unknown_encounters(source.encounter_id, destination.encounter_id, false, 30)
        .is_err());
    registry
        .merge_unknown_encounters(source.encounter_id, destination.encounter_id, true, 30)
        .unwrap();
    assert_eq!(
        registry.record(source.encounter_id).unwrap().encounter_id,
        destination.encounter_id
    );
    assert!(matches!(
        registry.drain_events().as_slice(),
        [EncounterLifecycleEventV1::MergedIntoEncounter {
            memory_migration_required: true,
            source: EncounterCorrectionSourceV1::ManualExplicit,
            ..
        }]
    ));

    registry
        .correct_to_authored_character(
            &database,
            correction.encounter_id,
            "jackie-welles",
            true,
            31,
        )
        .unwrap();
    assert!(matches!(
        registry.drain_events().as_slice(),
        [EncounterLifecycleEventV1::CorrectedToAuthoredCharacter {
            character_id,
            memory_migration_required: true,
            ..
        }] if character_id == "jackie-welles"
    ));

    assert_eq!(registry.expire_due(25), vec![expiring.encounter_id]);
    assert!(matches!(
        registry.drain_events().as_slice(),
        [EncounterLifecycleEventV1::Expired { encounter_id, .. }]
            if *encounter_id == expiring.encounter_id
    ));
    assert!(registry.observe(expiring.encounter_id, 26).is_err());
}

#[test]
fn versioned_summary_maps_to_canonical_long_term_memory() {
    let request = MemorySummaryRequestV1 {
        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
        summary_id: "session-1-rotation-1".to_owned(),
        scope: authority_scope(),
        source_turn_ids: vec!["turn-1".to_owned(), "turn-2".to_owned()],
        text: "V and Jackie agreed on the next job.".to_owned(),
        provider_id: "provider".to_owned(),
        model_id: "summarizer".to_owned(),
        model_revision: "2026-08".to_owned(),
        prompt_version: "summary-v1".to_owned(),
        created_at_ms: 20,
    };
    let input = request.to_memory_input().unwrap();
    assert_eq!(input.class, KnowledgeClass::LongTermSummary);
    assert_eq!(input.scope, authority_scope());
    assert_eq!(input.source_turn_ids, vec!["turn-1", "turn-2"]);
    assert_eq!(input.generator.unwrap().model_revision, "2026-08");
}

#[test]
fn tensor_metadata_round_trips_raw_f32_and_rejects_pickle_or_corruption() {
    let source_hash = "a".repeat(64);
    let (metadata, bytes) = encode_embedding_tensor(
        "profile-knowledge:fact-1",
        3,
        "embedder",
        "2026-08",
        "normalized-text-v1",
        &source_hash,
        &[0.25, -0.5, 1.0],
    )
    .unwrap();
    let input = decode_embedding_tensor(&metadata, &bytes).unwrap();
    assert_eq!(input.values, vec![0.25, -0.5, 1.0]);
    assert_eq!(input.model_id, "embedder@2026-08");

    let mut corrupted = bytes.clone();
    corrupted[0] ^= 0xff;
    assert!(decode_embedding_tensor(&metadata, &corrupted).is_err());

    let mut value = serde_json::to_value(&metadata).unwrap();
    value["element_format"] = serde_json::json!("pickle");
    assert!(serde_json::from_value::<EmbeddingTensorMetadataV1>(value).is_err());
}

#[test]
fn prompt_context_keeps_authorities_separate_and_pending_data_inert() {
    let mut profile = support::cyberpunk_profile();
    profile.content.provenance.extend([
        provenance("approved-source", Some(ReviewStatus::Approved)),
        provenance("pending-source", Some(ReviewStatus::Pending)),
    ]);
    profile.content.knowledge.extend([
        knowledge(
            "core-approved",
            KnowledgeAuthority::CoreCanon,
            None,
            "Core approved",
            "approved-source",
        ),
        knowledge(
            "public-approved",
            KnowledgeAuthority::GamePublic,
            None,
            "Public approved",
            "approved-source",
        ),
        knowledge(
            "public-pending",
            KnowledgeAuthority::GamePublic,
            None,
            "Never visible",
            "pending-source",
        ),
        knowledge(
            "jackie-secret",
            KnowledgeAuthority::CharacterAuthored,
            Some("jackie-welles"),
            "Jackie secret",
            "approved-source",
        ),
        knowledge(
            "johnny-secret",
            KnowledgeAuthority::CharacterAuthored,
            Some("johnny-silverhand"),
            "Johnny secret",
            "approved-source",
        ),
    ]);
    let jackie = profile
        .characters
        .iter_mut()
        .find(|character| character.id == "jackie-welles")
        .unwrap();
    jackie.style_examples.extend([
        style("style-approved", "A measured greeting", "approved-source"),
        style("style-pending", "Never visible", "pending-source"),
    ]);
    let database = CharacterDatabase::new(profile).unwrap();
    let summary = DerivedMemoryRecord {
        id: "summary:one".to_owned(),
        scope: authority_scope(),
        class: KnowledgeClass::LongTermSummary,
        spoiler_scope: SpoilerScope::CharacterPrivate,
        content: "Delivered summary".to_owned(),
        content_sha256: "b".repeat(64),
        provenance: Provenance::default(),
        confidence: 1.0,
        importance: 0.5,
        observed_at_ms: 5,
        created_at_ms: 5,
        expires_at_ms: None,
        source_turn_ids: vec!["turn-1".to_owned()],
        generator: None,
    };
    let mut foreign_summary = summary.clone();
    foreign_summary.id = "summary:foreign".to_owned();
    foreign_summary.scope.user_id = "another-user".to_owned();
    foreign_summary.content = "Cross-user leak".to_owned();
    let mut foreign_turn = delivered_turn("turn-foreign", "cross-user dialogue");
    foreign_turn.scope.user_id = "another-user".to_owned();
    let mut character_memory = summary.clone();
    character_memory.id = "character-memory:one".to_owned();
    character_memory.class = KnowledgeClass::CharacterKnowledge;
    character_memory.content = "A remembered promise".to_owned();
    let request = PromptBuildRequestV1 {
        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
        scope: authority_scope(),
        query: "What happened?".to_owned(),
        enabled_spoiler_tiers: vec!["street-level".to_owned()],
        memory_context: MemoryContextBundle {
            character_knowledge: vec![character_memory],
            long_term_summaries: vec![summary, foreign_summary],
            recent_dialogue: vec![
                delivered_turn("turn-partial", "partial"),
                delivered_turn("turn-heard", "heard"),
                foreign_turn,
            ],
            ..MemoryContextBundle::default()
        },
        memory_spoiler_policy: SpoilerPolicy {
            allow_game: false,
            allow_save: false,
            allow_character_private: true,
            allow_user_private: true,
        },
        max_style_examples: 8,
    };
    let context = build_prompt_context(&database, request).unwrap();
    let authorities = context
        .sections
        .iter()
        .map(|section| section.authority)
        .collect::<Vec<_>>();
    assert_eq!(
        authorities,
        database.profile().content.retrieval.authority_order
    );
    let all_text = context
        .sections
        .iter()
        .flat_map(|section| section.records.iter().map(|record| record.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(all_text.contains("Core approved"));
    assert!(all_text.contains("Public approved"));
    assert!(all_text.contains("Jackie secret"));
    assert!(all_text.contains("A measured greeting"));
    assert!(all_text.contains("Delivered summary"));
    assert!(all_text.contains("A remembered promise"));
    assert!(all_text.contains("Player: partial"));
    assert!(all_text.contains("Player: heard"));
    assert!(!all_text.contains("Never visible"));
    assert!(!all_text.contains("Johnny secret"));
    assert!(!all_text.contains("Cross-user leak"));
    assert!(!all_text.contains("cross-user dialogue"));

    let approved_inputs = database.approved_knowledge_memory_inputs("user-1").unwrap();
    assert!(approved_inputs
        .iter()
        .all(|input| input.content != "Never visible"));
    assert_eq!(
        context.retrieval_provenance.selected_memory_item_ids,
        vec![
            "summary:one".to_owned(),
            "character-memory:one".to_owned(),
            "turn:turn-partial".to_owned(),
            "turn:turn-heard".to_owned(),
        ]
    );
}

fn authority_scope() -> AuthorityScope {
    AuthorityScope {
        user_id: "user-1".to_owned(),
        profile_id: "cyberpunk-2077".to_owned(),
        game_id: "cyberpunk-2077".to_owned(),
        character_id: Some("jackie-welles".to_owned()),
        encounter_id: None,
        session_id: Some("session-1".to_owned()),
        save_id: None,
    }
}

#[test]
fn expanded_cyberpunk_character_context_reaches_the_runtime_prompt() {
    let database = database();
    let mut scope = authority_scope();
    scope.character_id = Some("song-so-mi".to_owned());
    let context = build_prompt_context(
        &database,
        PromptBuildRequestV1 {
            schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
            scope,
            query: "What makes this choice yours?".to_owned(),
            enabled_spoiler_tiers: vec!["dogtown".to_owned(), "relic-and-endings".to_owned()],
            memory_context: MemoryContextBundle::default(),
            memory_spoiler_policy: SpoilerPolicy {
                allow_game: true,
                allow_save: true,
                allow_character_private: true,
                allow_user_private: true,
            },
            max_style_examples: 4,
        },
    )
    .expect("expanded Songbird context should build");
    let all_text = context
        .sections
        .iter()
        .flat_map(|section| section.records.iter().map(|record| record.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(all_text.contains("Song So Mi, known as Songbird"));
    assert!(all_text.contains("FIA operations, Myers, Reed, Alex"));
    assert!(all_text.contains("every door is owned by someone else"));
    assert!(context
        .retrieval_provenance
        .selected_profile_knowledge_ids
        .contains(&"corpus-song-so-mi-context".to_owned()));
}

#[test]
fn retrieved_memory_is_typed_exactly_once_and_shares_the_global_memory_cap() {
    let mut profile = support::cyberpunk_profile();
    profile.content.retrieval.max_memory_records = 3;
    let database = CharacterDatabase::new(profile).expect("profile should index");

    let summary = derived_memory(
        "summary:cap",
        KnowledgeClass::LongTermSummary,
        SpoilerScope::CharacterPrivate,
        "CAPPED_SUMMARY",
        1.0,
    );
    let character = derived_memory(
        "memory:character",
        KnowledgeClass::CharacterKnowledge,
        SpoilerScope::CharacterPrivate,
        "CAPPED_CHARACTER_MEMORY",
        0.9,
    );
    let world = derived_memory(
        "memory:world",
        KnowledgeClass::WorldLore,
        SpoilerScope::None,
        "CAPPED_WORLD_MEMORY",
        0.8,
    );
    let biography = derived_memory(
        "memory:biography",
        KnowledgeClass::Biography,
        SpoilerScope::CharacterPrivate,
        "MUST_BE_CAPPED_OUT",
        0.7,
    );
    let locked = derived_memory(
        "memory:locked",
        KnowledgeClass::WorldLore,
        SpoilerScope::Game,
        "MUST_BE_SPOILER_FILTERED",
        0.99,
    );
    let mut foreign = derived_memory(
        "memory:foreign",
        KnowledgeClass::CharacterKnowledge,
        SpoilerScope::None,
        "MUST_BE_SCOPE_FILTERED",
        1.0,
    );
    foreign.scope.character_id = Some("johnny-silverhand".to_owned());

    let context = build_prompt_context(
        &database,
        PromptBuildRequestV1 {
            schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
            scope: authority_scope(),
            query: "What do you remember?".to_owned(),
            enabled_spoiler_tiers: Vec::new(),
            memory_context: MemoryContextBundle {
                world_lore: vec![world, locked],
                biography: vec![biography],
                character_knowledge: vec![character, foreign],
                long_term_summaries: vec![summary],
                ..MemoryContextBundle::default()
            },
            memory_spoiler_policy: SpoilerPolicy {
                allow_game: false,
                allow_save: false,
                allow_character_private: true,
                allow_user_private: true,
            },
            max_style_examples: 0,
        },
    )
    .expect("canonical prompt context");

    let memory_sections = context
        .sections
        .iter()
        .filter(|section| {
            matches!(
                section.authority,
                PromptAuthority::SessionSummary | PromptAuthority::RetrievedMemory
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(memory_sections.len(), 2);
    assert_eq!(
        memory_sections
            .iter()
            .map(|section| section.records.len())
            .sum::<usize>(),
        3
    );

    let records = memory_sections
        .iter()
        .flat_map(|section| section.records.iter())
        .collect::<Vec<_>>();
    let ids = records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["summary:cap", "memory:character", "memory:world"]);
    assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), ids.len());
    assert_eq!(
        context.retrieval_provenance.selected_memory_item_ids,
        vec![
            "summary:cap".to_owned(),
            "memory:character".to_owned(),
            "memory:world".to_owned(),
        ]
    );
    assert_eq!(
        records[0].memory_class,
        Some(KnowledgeClass::LongTermSummary)
    );
    assert_eq!(
        records[1].memory_class,
        Some(KnowledgeClass::CharacterKnowledge)
    );
    assert_eq!(records[2].memory_class, Some(KnowledgeClass::WorldLore));
    assert_eq!(
        records[0].spoiler_scope,
        Some(SpoilerScope::CharacterPrivate)
    );
    assert_eq!(records[2].spoiler_scope, Some(SpoilerScope::None));

    let serialized = serde_json::to_string(&context).expect("serialize canonical context");
    for present_once in [
        "CAPPED_SUMMARY",
        "CAPPED_CHARACTER_MEMORY",
        "CAPPED_WORLD_MEMORY",
    ] {
        assert_eq!(serialized.matches(present_once).count(), 1);
    }
    for absent in [
        "MUST_BE_CAPPED_OUT",
        "MUST_BE_SPOILER_FILTERED",
        "MUST_BE_SCOPE_FILTERED",
    ] {
        assert!(!serialized.contains(absent));
    }
}

fn derived_memory(
    id: &str,
    class: KnowledgeClass,
    spoiler_scope: SpoilerScope,
    content: &str,
    importance: f64,
) -> DerivedMemoryRecord {
    DerivedMemoryRecord {
        id: id.to_owned(),
        scope: authority_scope(),
        class,
        spoiler_scope,
        content: content.to_owned(),
        content_sha256: "c".repeat(64),
        provenance: Provenance {
            source_kind: "test_reviewed_memory".to_owned(),
            source_id: Some(format!("source:{id}")),
            source_uri: None,
            author: None,
            captured_at_ms: Some(10),
            attributes: Default::default(),
        },
        confidence: 1.0,
        importance,
        observed_at_ms: 10,
        created_at_ms: 10,
        expires_at_ms: None,
        source_turn_ids: Vec::new(),
        generator: None,
    }
}

fn delivered_turn(turn_id: &str, text: &str) -> DeliveredTurnRecord {
    DeliveredTurnRecord {
        turn_id: turn_id.to_owned(),
        scope: authority_scope(),
        speaker: TurnSpeaker::Player,
        delivered_text: text.to_owned(),
        content_sha256: "c".repeat(64),
        created_at_ms: 10,
        delivered_at_ms: 11,
        sequence: if turn_id.ends_with("partial") { 1 } else { 2 },
        cancellation_generation: 0,
        provider_id: None,
        delivery_receipt_id: None,
        provenance: Provenance::default(),
    }
}

fn provenance(id: &str, review_status: Option<ReviewStatus>) -> ProvenanceRecord {
    ProvenanceRecord {
        id: id.to_owned(),
        title: id.to_owned(),
        kind: ProvenanceKind::Original,
        source_url: None,
        license: None,
        notes: None,
        source_revision: None,
        source_path: None,
        source_sha256: None,
        review_status,
        transform_version: None,
    }
}

fn knowledge(
    id: &str,
    authority: KnowledgeAuthority,
    owner: Option<&str>,
    text: &str,
    provenance_id: &str,
) -> KnowledgeRecord {
    KnowledgeRecord {
        id: id.to_owned(),
        authority,
        owner_character_id: owner.map(str::to_owned),
        text: text.to_owned(),
        topic_tags: vec!["test".to_owned()],
        spoiler_tier: "street-level".to_owned(),
        provenance_id: provenance_id.to_owned(),
    }
}

fn style(id: &str, text: &str, provenance_id: &str) -> StyleExample {
    StyleExample {
        id: id.to_owned(),
        speaker: "Jackie Welles".to_owned(),
        text: text.to_owned(),
        situation_tags: vec![],
        tone_tags: vec![],
        weight_millis: 1_000,
        provenance_id: provenance_id.to_owned(),
    }
}
