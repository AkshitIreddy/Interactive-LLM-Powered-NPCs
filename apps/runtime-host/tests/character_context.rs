#![allow(clippy::unwrap_used)]

use std::{collections::BTreeMap, path::PathBuf};

use npc_character_db::PromptSectionV1;
use npc_game_profile::PromptAuthority;
use npc_identity_engine::{IdentityDecisionV1, TrackEpoch};
use npc_memory::{
    AuthorityScope, DeliveryDisposition, DerivedMemoryInput, GeneratorProvenance, KnowledgeClass,
    MemoryCommitBatch, Provenance, SpoilerScope, TurnCommitInput, TurnSpeaker,
};
use npc_runtime_core::TurnEvent;
use npc_runtime_host::{
    simulation::{SimulationError, SimulationProfilePolicy, SimulationSafetyContext},
    HostConfig, HostState, ManualFallbackActivation, ManualFallbackRoute, RouteExecution,
    SelectedProviderRoute, SelectedRoleRoute, SelectedRouteRoles, SelectedRouteSnapshot,
    SelectedRouteState, SimulationRequest, TurnDeliveryRequest, TurnInputSnapshot,
    TURN_ROUTE_SCHEMA_VERSION,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("runtime-host is nested under apps")
        .to_path_buf()
}

async fn host() -> HostState {
    let app_data = tempfile::tempdir().expect("temporary app data").keep();
    HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data,
    })
    .await
    .expect("initialize runtime host")
}

fn request(turn_id: &str) -> SimulationRequest {
    SimulationRequest {
        session_id: "character-context-session".into(),
        turn_id: turn_id.into(),
        game_id: "skyrim-special-edition".into(),
        character_id: None,
        native_identity_decision: None,
        enabled_spoiler_tiers: Vec::new(),
        generic_selection: None,
        safety_context: SimulationSafetyContext::verified_safe(),
        application_namespace: None,
        transcript: "Tell me what you remember about this place.".into(),
        locale: "en-US".into(),
        execution_mode: None,
        dev_live_tts: None,
        route_snapshot: None,
        input: TurnInputSnapshot::default(),
        delivery: TurnDeliveryRequest::default(),
        audio_playback_leases: Vec::new(),
        private_evaluation_acknowledgements: Vec::new(),
        subtitle_presentation_context: None,
    }
}

fn disabled_role() -> SelectedRoleRoute {
    SelectedRoleRoute {
        state: SelectedRouteState::Disabled,
        primary: None,
        fallbacks: Vec::new(),
        degradation: None,
    }
}

fn fixture_route() -> SelectedRouteSnapshot {
    SelectedRouteSnapshot {
        schema_version: TURN_ROUTE_SCHEMA_VERSION,
        source_loadout_id: "character-context-loadout".into(),
        inheritance_chain: Vec::new(),
        generation: 1,
        roles: SelectedRouteRoles {
            llm: SelectedRoleRoute {
                state: SelectedRouteState::Ready,
                primary: Some(SelectedProviderRoute {
                    provider_id: "mock-llm".into(),
                    model_id: "mock-stream-v1".into(),
                    voice_id: None,
                    execution: RouteExecution::Local,
                    egress: "conversation_text".into(),
                    credential_reference: None,
                }),
                fallbacks: Vec::new(),
                degradation: None,
            },
            stt: disabled_role(),
            tts: SelectedRoleRoute {
                state: SelectedRouteState::Ready,
                primary: Some(SelectedProviderRoute {
                    provider_id: "mock-tts".into(),
                    model_id: "mock-pcm-v1".into(),
                    voice_id: Some("mock-stock-voice-1".into()),
                    execution: RouteExecution::Local,
                    egress: "conversation_audio".into(),
                    credential_reference: None,
                }),
                fallbacks: Vec::new(),
                degradation: None,
            },
            embeddings: disabled_role(),
            vision: disabled_role(),
            lip_sync: disabled_role(),
        },
    }
}

#[test]
fn webview_style_identity_field_is_rejected_but_native_contract_is_typed() {
    let base = serde_json::json!({
        "sessionId": "native-owned-session",
        "turnId": "native-owned-turn",
        "gameId": "skyrim-special-edition",
        "characterId": null,
        "enabledSpoilerTiers": [],
        "genericSelection": null,
        "safetyContext": {
            "evidenceState": "verifiedSafe",
            "profilePolicy": "singlePlayerOnly",
            "visualsAllowed": false,
            "protectedOnlineDetected": false,
            "antiCheatDetected": false
        },
        "transcript": "Typed dialogue",
        "locale": "en-US",
        "delivery": {"audio": true, "subtitles": true}
    });
    let mut untrusted = base.clone();
    untrusted["identityDecision"] = serde_json::json!({
        "state": "explicit",
        "encounter_id": "webview-minted",
        "subject_id": "lydia"
    });
    assert!(serde_json::from_value::<SimulationRequest>(untrusted).is_err());

    let mut native = base;
    native["nativeIdentityDecision"] = serde_json::json!({
        "state": "offscreen",
        "encounter_id": "native-track-1",
        "last_confirmed_subject_id": "lydia",
        "track_epoch": 1
    });
    let request = serde_json::from_value::<SimulationRequest>(native)
        .expect("authenticated native sidecar contract");
    assert!(matches!(
        request.native_identity_decision,
        Some(IdentityDecisionV1::Offscreen {
            last_confirmed_subject_id: Some(ref subject_id),
            ..
        }) if subject_id == "lydia"
    ));
}

#[test]
fn console_isolated_policy_has_an_exact_native_wire_value() {
    let wire = serde_json::json!({
        "sessionId": "native-console-session",
        "turnId": "native-console-turn",
        "gameId": "eclipse-harbor",
        "characterId": "mara-venn",
        "enabledSpoilerTiers": [],
        "genericSelection": null,
        "safetyContext": {
            "evidenceState": "consoleIsolated",
            "profilePolicy": "consoleIsolatedNoGameInteraction",
            "visualsAllowed": false,
            "protectedOnlineDetected": false,
            "antiCheatDetected": false
        },
        "transcript": "Typed dialogue",
        "locale": "en-US",
        "delivery": {"audio": true, "subtitles": true}
    });
    let parsed: SimulationRequest = serde_json::from_value(wire).unwrap();
    assert_eq!(
        parsed.safety_context.profile_policy,
        SimulationProfilePolicy::ConsoleIsolatedNoGameInteraction
    );
}

#[tokio::test]
async fn console_isolated_mara_turn_is_profile_backed_and_rejects_visual_authority() {
    let state = host().await;
    let mut turn = request("console-isolated-mara");
    turn.game_id = "eclipse-harbor".into();
    turn.character_id = Some("mara-venn".into());
    turn.route_snapshot = Some(fixture_route());
    turn.safety_context = SimulationSafetyContext::verified_console_isolated();
    let result = state
        .simulate_turn(turn.clone())
        .await
        .expect("isolated authored console turn");
    let character = result.character_context.expect("character evidence");
    assert_eq!(character.profile_id, "eclipse-harbor");
    assert_eq!(character.character_id, "mara-venn");
    assert!(character.explicit_selection);
    let prompt = character.prompt.expect("canonical prompt evidence");
    assert_eq!(prompt.profile_id, "eclipse-harbor");
    assert_eq!(prompt.character_id, "mara-venn");

    turn.turn_id = "console-isolated-visual-reject".into();
    turn.safety_context.visuals_allowed = true;
    assert!(matches!(
        state.simulate_turn(turn).await,
        Err(SimulationError::SafetyEvidenceUnverified)
    ));

    let mut fallback_turn = request("console-isolated-visual-fallback-reject");
    fallback_turn.game_id = "eclipse-harbor".into();
    fallback_turn.character_id = Some("mara-venn".into());
    fallback_turn.safety_context = SimulationSafetyContext::verified_console_isolated();
    let mut route = fixture_route();
    route.roles.vision.fallbacks.push(ManualFallbackRoute {
        route: SelectedProviderRoute {
            provider_id: "must-not-be-authorized".into(),
            model_id: "visual-fallback".into(),
            voice_id: None,
            execution: RouteExecution::Local,
            egress: "visual_frame".into(),
            credential_reference: None,
        },
        activation: ManualFallbackActivation::ManualOnly,
        user_authorized: true,
    });
    fallback_turn.route_snapshot = Some(route);
    assert!(matches!(
        state.simulate_turn(fallback_turn).await,
        Err(SimulationError::InvalidRequest)
    ));
}

#[tokio::test]
async fn explicit_known_character_uses_canonical_database_and_prompt() {
    let state = host().await;
    let mut turn = request("explicit-known");
    turn.character_id = Some("lydia".into());
    turn.native_identity_decision = Some(IdentityDecisionV1::Ambiguous {
        encounter_id: "track:explicit-wins".into(),
        top_subject_id: "serana".into(),
        runner_up_subject_id: Some("lydia".into()),
        top_similarity: 0.86,
        runner_up_similarity: Some(0.84),
        top1_top2_margin: 0.02,
        supporting_frames: 4,
        window_frames: 5,
    });

    let result = state
        .simulate_turn(turn)
        .await
        .expect("known character turn");
    let evidence = result
        .character_context
        .expect("character context evidence");
    assert_eq!(evidence.profile_id, "skyrim-special-edition");
    assert_eq!(evidence.character_id, "lydia");
    assert_eq!(evidence.identity_source, "manual_explicit_selection");
    assert!(evidence.explicit_selection);
    assert!(matches!(
        evidence.selection,
        Some(npc_character_db::SelectionOutcomeV1::Known {
            character_id,
            reason: npc_character_db::SelectionReason::Explicit,
        }) if character_id == "lydia"
    ));
    let prompt = evidence.prompt.expect("prompt assembly evidence");
    assert!(prompt.authorities.contains(&PromptAuthority::CoreCanon));
    assert!(prompt
        .authorities
        .contains(&PromptAuthority::CharacterProfile));
    assert!(prompt.record_count >= 4);

    let resolved = result.events.iter().find_map(|event| match event {
        TurnEvent::CharacterResolved { character, .. } => Some(character),
        _ => None,
    });
    let resolved = resolved.expect("character resolved event");
    assert_eq!(resolved.character_id.as_deref(), Some("lydia"));
    assert_eq!(resolved.display_name, "Lydia");
    assert!(resolved.explicit_selection);
    assert!(resolved
        .evidence
        .iter()
        .any(|value| value == "explicit_selection"));
    assert!(resolved
        .evidence
        .iter()
        .any(|value| value == "manual_explicit_selection"));
}

#[tokio::test]
async fn ordinary_selected_route_requires_character_or_typed_identity_evidence() {
    let state = host().await;
    let mut turn = request("selected-route-needs-identity");
    turn.route_snapshot = Some(fixture_route());

    assert!(matches!(
        state.simulate_turn(turn).await,
        Err(SimulationError::ExplicitCharacterSelectionRequired)
    ));
}

#[tokio::test]
async fn ambiguous_track_evidence_requires_explicit_selection_without_switching() {
    let state = host().await;
    let mut turn = request("ambiguous");
    turn.native_identity_decision = Some(IdentityDecisionV1::Ambiguous {
        encounter_id: "track:44".into(),
        top_subject_id: "lydia".into(),
        runner_up_subject_id: Some("serana".into()),
        top_similarity: 0.88,
        runner_up_similarity: Some(0.84),
        top1_top2_margin: 0.04,
        supporting_frames: 4,
        window_frames: 5,
    });

    let error = state
        .simulate_turn(turn)
        .await
        .expect_err("ambiguous evidence must fail closed");
    assert!(matches!(error, SimulationError::IdentityAmbiguous));
}

#[tokio::test]
async fn offscreen_evidence_keeps_last_confirmed_character_without_visual_claim() {
    let state = host().await;
    let mut turn = request("offscreen");
    turn.native_identity_decision = Some(IdentityDecisionV1::Offscreen {
        encounter_id: "track:lydia:7".into(),
        last_confirmed_subject_id: Some("lydia".into()),
        track_epoch: TrackEpoch(7),
    });

    let result = state
        .simulate_turn(turn)
        .await
        .expect("offscreen continuity");
    let evidence = result
        .character_context
        .expect("character context evidence");
    assert_eq!(evidence.character_id, "lydia");
    assert_eq!(
        evidence.identity_source,
        "trusted_native_identity_offscreen_continuity"
    );
    assert!(!evidence.explicit_selection);
    let resolved = result.events.iter().find_map(|event| match event {
        TurnEvent::CharacterResolved { character, .. } => Some(character),
        _ => None,
    });
    let resolved = resolved.expect("character resolved event");
    assert_eq!(resolved.confidence, 0.0);
    assert!(resolved
        .evidence
        .iter()
        .any(|item| item == "trusted_native_identity_offscreen_last_confirmed"));
    assert!(!resolved.evidence.iter().any(|item| item.contains("face")));
}

#[tokio::test]
async fn hysteresis_match_keeps_the_current_character_instead_of_silently_switching() {
    let state = host().await;
    let mut turn = request("sticky-match");
    turn.native_identity_decision = Some(IdentityDecisionV1::Matched {
        encounter_id: "track:sticky:12".into(),
        subject_id: "lydia".into(),
        subject_similarity: 0.70,
        top_candidate_subject_id: "serana".into(),
        top_candidate_similarity: 0.95,
        runner_up_similarity: Some(0.70),
        top1_top2_margin: 0.25,
        supporting_frames: 4,
        window_frames: 5,
        held_by_hysteresis: true,
    });

    let result = state
        .simulate_turn(turn)
        .await
        .expect("sticky identity turn");
    let evidence = result.character_context.expect("character evidence");
    assert_eq!(evidence.character_id, "lydia");
    assert_eq!(
        evidence.identity_source,
        "trusted_native_identity_sticky_match"
    );
    assert!(matches!(
        evidence.selection,
        Some(npc_character_db::SelectionOutcomeV1::Known {
            character_id,
            reason: npc_character_db::SelectionReason::StickyCurrent,
        }) if character_id == "lydia"
    ));
}

#[tokio::test]
async fn unknown_actor_gets_stable_background_encounter_and_voice_binding() {
    let state = host().await;
    let execute = |turn_id: &str| {
        let mut turn = request(turn_id);
        turn.route_snapshot = Some(fixture_route());
        turn.native_identity_decision = Some(IdentityDecisionV1::NoMatch {
            encounter_id: "wgc:target-8:track-19".into(),
            best_subject_id: None,
            best_similarity: Some(0.41),
            observed_frames: 5,
        });
        turn
    };

    let first = state
        .simulate_turn(execute("background-1"))
        .await
        .expect("first background turn");
    let second = state
        .simulate_turn(execute("background-2"))
        .await
        .expect("second background turn");
    let first = first.character_context.expect("first character evidence");
    let second = second.character_context.expect("second character evidence");
    assert_eq!(first.character_id, "hold-resident");
    assert_eq!(first.character_id, second.character_id);
    assert!(matches!(
        first.selection,
        Some(npc_character_db::SelectionOutcomeV1::Background)
    ));
    let first_encounter = first.encounter.as_ref().expect("stable encounter");
    let second_encounter = second.encounter.as_ref().expect("stable encounter");
    assert_eq!(first_encounter.encounter_id, second_encounter.encounter_id);
    assert_eq!(
        first_encounter.selected_voice,
        second_encounter.selected_voice
    );
    assert_eq!(
        first_encounter.selected_voice.provider_voice_id,
        "mock-stock-voice-1"
    );
    assert!(second
        .prompt
        .expect("second prompt evidence")
        .scoped_memory_classes
        .iter()
        .any(|class| class == "recent_delivered_turn"));
}

#[tokio::test]
async fn identity_evidence_cannot_cross_the_active_game_scope() {
    let state = host().await;
    let mut turn = request("wrong-game");
    turn.native_identity_decision = Some(IdentityDecisionV1::Explicit {
        encounter_id: "track:cyberpunk".into(),
        subject_id: "judy-alvarez".into(),
    });

    let error = state
        .simulate_turn(turn)
        .await
        .expect_err("cross-game identity must be rejected");
    assert!(matches!(error, SimulationError::IdentityEvidenceWrongGame));
}

#[tokio::test]
async fn manual_retry_still_returns_the_resolved_identity_and_prompt_provenance() {
    let state = host().await;
    let mut route = fixture_route();
    route.roles.llm = disabled_role();
    let mut turn = request("manual-retry-character-evidence");
    turn.character_id = Some("lydia".into());
    turn.route_snapshot = Some(route);

    let result = state
        .simulate_turn(turn)
        .await
        .expect("typed manual-retry result");
    let evidence = result
        .character_context
        .expect("resolved context survives provider failure");
    assert_eq!(evidence.character_id, "lydia");
    assert_eq!(evidence.identity_source, "manual_explicit_selection");
    assert!(evidence.prompt.is_some());
    assert!(result
        .turn_execution
        .expect("turn evidence")
        .degradations
        .iter()
        .any(|degradation| matches!(
            degradation,
            npc_runtime_host::TurnExecutionDegradation::ManualRetryRequired { .. }
        )));
}

#[tokio::test]
async fn provenance_scoped_character_memory_reaches_the_generation_context() {
    let state = host().await;
    let scope = AuthorityScope {
        user_id: "local-user".into(),
        profile_id: "skyrim-special-edition".into(),
        game_id: "skyrim-special-edition".into(),
        character_id: Some("lydia".into()),
        encounter_id: None,
        session_id: Some("character-context-session".into()),
        save_id: None,
    };
    let other_scope = AuthorityScope {
        character_id: Some("serana".into()),
        ..scope.clone()
    };
    let provenance = Provenance {
        source_kind: "runtime_test_reviewed_context".into(),
        source_id: Some("approved-runtime-test".into()),
        source_uri: None,
        author: Some("runtime-test".into()),
        captured_at_ms: Some(10),
        attributes: BTreeMap::new(),
    };
    state
        .memory
        .commit_batch(MemoryCommitBatch {
            turns: vec![TurnCommitInput {
                turn_id: "delivered-before-current-turn".into(),
                scope: scope.clone(),
                speaker: TurnSpeaker::Player,
                text: "We agreed to meet at the western watchtower.".into(),
                delivery: DeliveryDisposition::Delivered {
                    delivered_at_ms: 20,
                },
                created_at_ms: 10,
                sequence: 1,
                cancellation_generation: 0,
                provider_id: None,
                delivery_receipt_id: None,
                provenance: provenance.clone(),
            }],
            derived: vec![
                DerivedMemoryInput {
                    id: Some("lydia-approved-knowledge".into()),
                    scope: scope.clone(),
                    class: KnowledgeClass::CharacterKnowledge,
                    spoiler_scope: SpoilerScope::CharacterPrivate,
                    content: "Lydia privately remembers the western watchtower agreement.".into(),
                    provenance: provenance.clone(),
                    confidence: 1.0,
                    importance: 0.9,
                    observed_at_ms: 10,
                    expires_at_ms: None,
                    source_turn_ids: Vec::new(),
                    generator: None,
                },
                DerivedMemoryInput {
                    id: Some("lydia-session-summary".into()),
                    scope: scope.clone(),
                    class: KnowledgeClass::LongTermSummary,
                    spoiler_scope: SpoilerScope::CharacterPrivate,
                    content: "The player and Lydia planned a western watchtower meeting.".into(),
                    provenance: provenance.clone(),
                    confidence: 0.9,
                    importance: 0.8,
                    observed_at_ms: 10,
                    expires_at_ms: None,
                    source_turn_ids: vec!["delivered-before-current-turn".into()],
                    generator: Some(GeneratorProvenance {
                        provider_id: "fixture-summary-provider".into(),
                        model_id: "fixture-summary-model".into(),
                        model_revision: "1".into(),
                        prompt_version: "summary-v1".into(),
                    }),
                },
                DerivedMemoryInput {
                    id: Some("serana-private-secret".into()),
                    scope: other_scope,
                    class: KnowledgeClass::CharacterKnowledge,
                    spoiler_scope: SpoilerScope::CharacterPrivate,
                    content: "SERANA_SCOPE_MUST_NOT_REACH_LYDIA".into(),
                    provenance,
                    confidence: 1.0,
                    importance: 1.0,
                    observed_at_ms: 10,
                    expires_at_ms: None,
                    source_turn_ids: Vec::new(),
                    generator: None,
                },
            ],
        })
        .await
        .expect("seed exact scoped memory");

    let mut turn = request("provenance");
    turn.character_id = Some("lydia".into());
    let result = state.simulate_turn(turn).await.expect("scoped prompt turn");
    let memory = result.events.iter().find_map(|event| match event {
        TurnEvent::MemoryReady { context, .. } => Some(context),
        _ => None,
    });
    let memory = memory.expect("memory-ready event");
    let generation_context = serde_json::to_string(memory).expect("serialize memory evidence");
    let lydia_biography = state
        .profiles
        .profile("skyrim-special-edition")
        .and_then(|profile| {
            profile
                .characters
                .iter()
                .find(|character| character.id == "lydia")
        })
        .map(|character| character.biography.as_str())
        .expect("Lydia biography");
    assert!(generation_context.contains(lydia_biography));
    assert!(generation_context.contains("western watchtower agreement"));
    assert!(generation_context.contains("western watchtower meeting"));
    assert!(generation_context.contains("We agreed to meet at the western watchtower"));
    assert!(!generation_context.contains("SERANA_SCOPE_MUST_NOT_REACH_LYDIA"));
    for exact_record in [
        "Lydia privately remembers the western watchtower agreement.",
        "The player and Lydia planned a western watchtower meeting.",
        "Player: We agreed to meet at the western watchtower.",
    ] {
        assert_eq!(generation_context.matches(exact_record).count(), 1);
    }
    let payload_sections = memory
        .working_context
        .iter()
        .chain(&memory.canon_facts)
        .chain(&memory.episodic_memories)
        .map(|section| {
            serde_json::from_str::<PromptSectionV1>(section)
                .expect("every generation memory entry is a typed prompt section")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        payload_sections
            .iter()
            .filter(|section| section.authority == PromptAuthority::RetrievedMemory)
            .count(),
        1
    );
    assert_eq!(
        payload_sections
            .iter()
            .filter(|section| section.authority == PromptAuthority::SessionSummary)
            .count(),
        1
    );
    assert!(memory.canon_facts.iter().all(|section| {
        let section = serde_json::from_str::<PromptSectionV1>(section).unwrap();
        matches!(
            section.authority,
            PromptAuthority::CoreCanon
                | PromptAuthority::GamePublic
                | PromptAuthority::CharacterAuthored
        )
    }));

    let prompt = result
        .character_context
        .and_then(|evidence| evidence.prompt)
        .expect("prompt evidence");
    assert_eq!(payload_sections.len(), prompt.authorities.len());
    assert_eq!(
        payload_sections
            .iter()
            .map(|section| section.records.len())
            .sum::<usize>(),
        prompt.record_count
    );
    assert!(prompt
        .authorities
        .contains(&PromptAuthority::CharacterProfile));
    assert!(prompt
        .authorities
        .contains(&PromptAuthority::SessionSummary));
    assert!(prompt
        .authorities
        .contains(&PromptAuthority::RetrievedMemory));
    assert!(prompt
        .authorities
        .contains(&PromptAuthority::RecentDeliveredTurns));
    assert!(prompt
        .scoped_memory_item_ids
        .iter()
        .any(|id| id == "lydia-approved-knowledge"));
    assert!(!prompt
        .scoped_memory_item_ids
        .iter()
        .any(|id| id == "serana-private-secret"));
    assert!(prompt
        .scoped_memory_classes
        .iter()
        .any(|class| class == "character_knowledge"));
}

#[tokio::test]
async fn game_spoiler_memory_requires_an_explicit_non_default_tier() {
    let state = host().await;
    let scope = AuthorityScope {
        user_id: "local-user".into(),
        profile_id: "skyrim-special-edition".into(),
        game_id: "skyrim-special-edition".into(),
        character_id: Some("lydia".into()),
        encounter_id: None,
        session_id: Some("character-context-session".into()),
        save_id: None,
    };
    state
        .memory
        .derive_memory(DerivedMemoryInput {
            id: Some("locked-main-quest-memory".into()),
            scope,
            class: KnowledgeClass::WorldLore,
            spoiler_scope: SpoilerScope::Game,
            content: "LOCKED_MAIN_QUEST_MEMORY".into(),
            provenance: Provenance {
                source_kind: "runtime_test_reviewed_context".into(),
                source_id: Some("main-quest-evidence".into()),
                source_uri: None,
                author: None,
                captured_at_ms: Some(10),
                attributes: BTreeMap::new(),
            },
            confidence: 1.0,
            importance: 1.0,
            observed_at_ms: 10,
            expires_at_ms: None,
            source_turn_ids: Vec::new(),
            generator: None,
        })
        .await
        .expect("seed spoiler-scoped memory");

    let mut locked = request("spoiler-locked");
    locked.character_id = Some("lydia".into());
    let locked = state.simulate_turn(locked).await.expect("locked turn");
    let locked_context = locked.events.iter().find_map(|event| match event {
        TurnEvent::MemoryReady { context, .. } => Some(context),
        _ => None,
    });
    assert!(
        !serde_json::to_string(locked_context.expect("locked memory event"))
            .unwrap()
            .contains("LOCKED_MAIN_QUEST_MEMORY")
    );

    let mut enabled = request("spoiler-enabled");
    enabled.character_id = Some("lydia".into());
    enabled.enabled_spoiler_tiers = vec!["main-quest".into()];
    let enabled = state.simulate_turn(enabled).await.expect("enabled turn");
    let enabled_context = enabled.events.iter().find_map(|event| match event {
        TurnEvent::MemoryReady { context, .. } => Some(context),
        _ => None,
    });
    assert!(
        serde_json::to_string(enabled_context.expect("enabled memory event"))
            .unwrap()
            .contains("LOCKED_MAIN_QUEST_MEMORY")
    );

    let mut unknown = request("spoiler-invalid");
    unknown.character_id = Some("lydia".into());
    unknown.enabled_spoiler_tiers = vec!["not-a-profile-tier".into()];
    assert!(matches!(
        state.simulate_turn(unknown).await,
        Err(SimulationError::InvalidRequest)
    ));
}
