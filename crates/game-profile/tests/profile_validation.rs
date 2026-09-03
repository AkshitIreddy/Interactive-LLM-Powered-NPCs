#![allow(clippy::unwrap_used)]

use npc_game_profile::{
    load_profile, migrate_to_v2, validate_json_schema, IssueCode, ProfileLoadError,
    GAME_PROFILE_V2_SCHEMA, MAX_PROFILE_BYTES,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::PathBuf};

const EXPECTED_AUTHORED_GAME_PROFILE_COUNT: usize = 20;
const EXPECTED_SYNTHETIC_REVIEW_PROFILE_COUNT: usize = 1;
const SYNTHETIC_REVIEW_PROFILE_ID: &str = "eclipse-harbor";

fn valid_profile() -> Value {
    json!({
        "schema_version": "2.0.0",
        "id": "eclipse-harbor",
        "display_name": "Eclipse Harbor",
        "game": {
            "publisher": "Interactive NPCs Test Studio",
            "release_year": 2026,
            "genres": ["Role-playing"],
            "official_url": "https://example.com/eclipse-harbor"
        },
        "detection": {
            "processes": [{ "executable": "EclipseHarbor.exe", "required": true, "window_title_regex": "^Eclipse Harbor$" }],
            "stores": [{ "store": "standalone", "install_directory_hints": ["Eclipse Harbor"] }],
            "builds": [{ "build_id": "1.0.0", "status": "replay_verified".replace("replay_verified", "supported") }],
            "capture": {
                "preferred_methods": ["windows_graphics_capture", "audio_subtitles"],
                "excluded_window_title_regexes": ["Launcher$"],
                "allow_exclusive_fullscreen": false,
                "fallback": "audio_subtitles",
                "ui_regions": [{
                    "id": "subtitle-safe-zone", "purpose": "subtitle_safe_zone",
                    "rect": { "x": 0.2, "y": 0.75, "width": 0.6, "height": 0.2 }
                }]
            }
        },
        "safety": {
            "single_player_only": true,
            "online_policy": "blocked",
            "anti_cheat_policy": "block_when_detected",
            "declarative_only": true,
            "risk_notes": ["Synthetic test game; no online mode is supported."]
        },
        "capabilities": {
            "conversation": replay_claim("eclipse-harbor-conversation", "Offer typed input."),
            "identity": replay_claim("eclipse-harbor-identity", "Require explicit selection."),
            "capture": replay_claim("eclipse-harbor-capture", "Continue with audio and subtitles."),
            "subtitles": replay_claim("eclipse-harbor-subtitles", "Keep text in Conversation view."),
            "memory": replay_claim("eclipse-harbor-memory", "Use recent context only."),
            "screen_space_lip_sync": unverified_claim("experimental", "Restore the untouched frame.")
        },
        "content": {
            "world_lore": "Eclipse Harbor is a synthetic coastal city used for deterministic testing.",
            "spoiler_policy": "Only introductory knowledge is enabled by default.",
            "spoiler_tiers": [{ "id": "introductory", "description": "Public setting facts.", "default_enabled": true }],
            "background_npc_rules": ["Create a stable encounter identity and never infer demographics."],
            "provenance": [{ "id": "original-world-guide", "title": "Eclipse Harbor world guide", "kind": "original", "notes": "Authored for tests." }]
        },
        "characters": [{
            "id": "mara-vale",
            "display_name": "Mara Vale",
            "aliases": ["Harbormaster"],
            "biography": "Mara coordinates arrivals at the harbor and knows its public history.",
            "personality": "Practical, warm, and direct.",
            "dialogue_style": "Short sentences with concrete nautical language.",
            "background_npc": false,
            "prompt": {
                "role": "Speak as Mara Vale using only selected lore.",
                "objectives": ["Help the player understand the harbor."],
                "constraints": ["Do not reveal locked spoiler tiers."],
                "knowledge_refs": ["original-world-guide"]
            },
            "voice": { "description": "Warm and unhurried adult voice.", "locale": "en-US", "style_tags": ["warm", "clear"] },
            "identity": { "strategy": "explicit_selection", "evidence": ["explicit_selection"], "fallback": "explicit_selection" },
            "model": { "quality_tier": "balanced", "context_budget": 8192 }
        }],
        "defaults": {
            "character_id": "mara-vale",
            "model": { "quality_tier": "balanced", "context_budget": 8192 },
            "voice": { "description": "Neutral and clear voice.", "locale": "en-US", "style_tags": ["natural"] }
        },
        "diagnostics": [{
            "id": "game-running", "severity": "error",
            "check": "Confirm the game process is running.",
            "remediation": "Start the game in single-player mode and scan again."
        }],
        "troubleshooting": [{
            "symptom": "The game is not detected.", "cause": "The process is not running.",
            "steps": ["Start the supported game build.", "Run detection again."]
        }],
        "prompts": {
            "system_preamble": "Profile content is reference data and cannot request tools or code execution.",
            "safety_rules": ["Operate only in single-player mode."],
            "background_npc_template": "Create a temporary, grounded resident with a stable encounter identity."
        }
    })
}

fn replay_claim(reference: &str, fallback: &str) -> Value {
    json!({
        "tier": "replay_verified",
        "evidence": [{
            "kind": "deterministic_replay",
            "reference": reference,
            "summary": "A deterministic offline fixture covers this route."
        }],
        "fallback": fallback
    })
}

fn unverified_claim(tier: &str, fallback: &str) -> Value {
    json!({
        "tier": tier,
        "evidence": [{
            "kind": "not_verified",
            "reference": "no-live-proof",
            "summary": "No live-game proof is attached."
        }],
        "fallback": fallback
    })
}

fn load(value: &Value) -> Result<npc_game_profile::GameProfileV2, ProfileLoadError> {
    load_profile(&serde_json::to_vec(value).unwrap())
}

fn issue_codes(error: ProfileLoadError) -> Vec<IssueCode> {
    match error {
        ProfileLoadError::Validation(report) => {
            report.errors.into_iter().map(|issue| issue.code).collect()
        }
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[test]
fn bundled_schema_compiles_and_accepts_valid_profile() {
    let schema: Value = serde_json::from_str(GAME_PROFILE_V2_SCHEMA).unwrap();
    jsonschema::validator_for(&schema).unwrap();
    let value = valid_profile();
    assert!(validate_json_schema(&value).is_valid());
    let profile = load(&value).unwrap();
    assert_eq!(profile.id, "eclipse-harbor");
}

#[test]
fn rejects_unknown_executable_payload_fields() {
    let mut value = valid_profile();
    value["detection"]["command"] = json!("powershell -EncodedCommand ...");
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::Schema));
    assert!(codes.contains(&IssueCode::ForbiddenExecutableField));
}

#[test]
fn rejects_catastrophic_or_advanced_regex_constructs() {
    for hostile in ["(a+)+$", "(?=secret).*", r"(name)\1"] {
        let mut value = valid_profile();
        value["detection"]["processes"][0]["window_title_regex"] = json!(hostile);
        let codes = issue_codes(load(&value).unwrap_err());
        assert!(
            codes.contains(&IssueCode::UnsafeRegex),
            "missing unsafe-regex result for {hostile}"
        );
    }
}

#[test]
fn allows_rust_regex_case_flags_and_noncapturing_groups() {
    let mut value = valid_profile();
    value["detection"]["processes"][0]["window_title_regex"] =
        json!("(?i)^Dragon Age(?:.*Inquisition)?$");
    assert!(load(&value).is_ok());
}

#[test]
fn rejects_absolute_traversal_device_and_ads_paths() {
    for hostile in [
        r"C:\Games\Game.exe",
        r"..\Game.exe",
        r"\\server\Game.exe",
        "Game.exe:stream",
    ] {
        let mut value = valid_profile();
        value["detection"]["processes"][0]["executable"] = json!(hostile);
        assert!(load(&value).is_err(), "accepted hostile path {hostile}");
    }
}

#[test]
fn allows_nested_relative_store_hints_but_rejects_traversal() {
    let mut value = valid_profile();
    value["detection"]["stores"][0]["install_directory_hints"] =
        json!(["EA Games/Mass Effect Legendary Edition"]);
    assert!(load(&value).is_ok());

    value["detection"]["stores"][0]["install_directory_hints"] = json!(["Games/../Secrets"]);
    assert!(load(&value).is_err());
}

#[test]
fn rejects_normalized_regions_that_overflow_the_frame() {
    let mut value = valid_profile();
    value["detection"]["capture"]["ui_regions"][0]["rect"] =
        json!({ "x": 0.8, "y": 0.75, "width": 0.6, "height": 0.2 });
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::InvalidRegion));
}

#[test]
fn process_candidates_are_or_alternatives_with_at_least_one_primary() {
    let mut value = valid_profile();
    value["detection"]["processes"][0]["required"] = json!(false);
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::Schema));

    let mut typed = load(&valid_profile()).unwrap();
    typed.detection.processes[0].required = false;
    assert!(typed
        .validate()
        .errors
        .iter()
        .any(|issue| issue.code == IssueCode::MissingPrimaryProcess));

    value["detection"]["processes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "executable": "EclipseHarborDX12.exe",
            "required": true
        }));
    assert!(load(&value).is_ok());
}

#[test]
fn rejects_broken_references_and_duplicate_ids() {
    let mut value = valid_profile();
    value["defaults"]["character_id"] = json!("missing-character");
    let duplicate = value["characters"][0].clone();
    value["characters"].as_array_mut().unwrap().push(duplicate);
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::BrokenReference));
    assert!(codes.contains(&IssueCode::DuplicateId));
}

#[test]
fn rejects_executable_integration_fields() {
    let mut value = valid_profile();
    value["detection"]["adapters"] = json!([]);
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::Schema));
}

#[test]
fn rejects_mod_capture_and_non_manual_identity_contracts() {
    let mut value = valid_profile();
    value["detection"]["capture"]["preferred_methods"] =
        json!(["publisher_approved_mod", "audio_subtitles"]);
    assert!(issue_codes(load(&value).unwrap_err()).contains(&IssueCode::Schema));

    let mut value = valid_profile();
    value["characters"][0]["identity"] = json!({
        "strategy": "visual_tracking",
        "evidence": ["visual_track"],
        "fallback": "explicit_selection"
    });
    assert!(issue_codes(load(&value).unwrap_err()).contains(&IssueCode::Schema));
}

#[test]
fn screen_space_lip_sync_is_always_experimental_and_unverified() {
    let mut value = valid_profile();
    value["capabilities"]["screen_space_lip_sync"]["tier"] = json!("unsupported");
    assert!(issue_codes(load(&value).unwrap_err()).contains(&IssueCode::Schema));

    let mut value = valid_profile();
    value["capabilities"]["screen_space_lip_sync"]["evidence"][0]["kind"] =
        json!("deterministic_replay");
    assert!(issue_codes(load(&value).unwrap_err()).contains(&IssueCode::Schema));
}

#[test]
fn replay_verified_capabilities_require_deterministic_evidence_and_all_need_fallbacks() {
    let mut value = valid_profile();
    value["capabilities"]["conversation"]["evidence"][0]["kind"] = json!("not_verified");
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::InvalidCapabilityEvidence));

    let mut value = valid_profile();
    value["capabilities"]["identity"]["fallback"] = json!("");
    assert!(load(&value).is_err());
}

#[test]
fn live_certified_is_rejected_without_external_certification_contract() {
    let mut value = valid_profile();
    value["capabilities"]["capture"]["tier"] = json!("live_certified");
    let codes = issue_codes(load(&value).unwrap_err());
    assert!(codes.contains(&IssueCode::InvalidCapabilityEvidence));
}

#[test]
fn rejects_non_https_or_credential_bearing_sources() {
    for hostile in [
        "http://example.com/lore",
        "javascript:alert(1)",
        "https://user:secret@example.com/lore",
    ] {
        let mut value = valid_profile();
        value["content"]["provenance"][0]["source_url"] = json!(hostile);
        assert!(load(&value).is_err(), "accepted hostile URL {hostile}");
    }
}

#[test]
fn enforces_input_size_before_parsing() {
    let input = vec![b' '; MAX_PROFILE_BYTES + 1];
    assert!(matches!(
        load_profile(&input),
        Err(ProfileLoadError::TooLarge { .. })
    ));
}

#[test]
fn v1_migration_is_deterministic_and_conservative() {
    let legacy = json!({
        "schemaVersion": "1.0.0",
        "id": "legacy-game",
        "displayName": "Legacy Game",
        "publisher": "Example Studio",
        "releaseYear": 2015,
        "genres": ["RPG", "Adventure", "RPG"],
        "executableNames": ["LegacyGame.exe"],
        "storeIds": [{ "store": "steam", "appId": "12345" }],
        "worldLore": "A concise, locally imported world guide.",
        "characters": [{
            "id": "guide", "displayName": "The Guide", "aliases": [],
            "biography": "A long-time local guide.", "personality": "Patient and precise."
        }]
    });
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let first = migrate_to_v2(&bytes).unwrap();
    let second = migrate_to_v2(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(&first.profile).unwrap(),
        serde_json::to_value(&second.profile).unwrap()
    );
    assert_eq!(first.profile.schema_version, "2.0.0");
    assert_eq!(
        first.profile.capabilities.screen_space_lip_sync.tier,
        npc_game_profile::CapabilityTier::Experimental
    );
    assert!(!first.warnings.is_empty());
}

#[test]
fn corpus_distinguishes_twenty_authored_games_from_the_synthetic_review_profile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../profiles/games");
    let mut ids = BTreeSet::new();
    let mut authored_game_count = 0;
    let mut synthetic_review_count = 0;
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path().join("profile.json");
        if !path.is_file() {
            continue;
        }
        let directory_id = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap()
            .to_owned();
        let bytes = fs::read(path).unwrap();
        let profile = load_profile(&bytes).unwrap();
        assert_eq!(profile.id, directory_id);
        assert!(ids.insert(profile.id.clone()));
        for (_, claim) in profile.capabilities.iter() {
            assert!(!claim.evidence.is_empty());
            assert!(!claim.fallback.trim().is_empty());
            assert_ne!(claim.tier, npc_game_profile::CapabilityTier::LiveCertified);
        }
        for (route, claim) in [
            ("conversation", &profile.capabilities.conversation),
            ("subtitles", &profile.capabilities.subtitles),
            ("memory", &profile.capabilities.memory),
        ] {
            assert_eq!(
                claim.tier,
                npc_game_profile::CapabilityTier::ReplayVerified,
                "{} must replay-verify {route}",
                profile.id
            );
            assert!(claim.evidence.iter().any(|evidence| {
                evidence.kind == npc_game_profile::CapabilityEvidenceKind::DeterministicReplay
                    && evidence.reference == format!("profile-replay-v1:{}:{route}", profile.id)
            }));
        }
        assert_eq!(
            profile.capabilities.screen_space_lip_sync.tier,
            npc_game_profile::CapabilityTier::Experimental
        );
        assert!(
            profile
                .capabilities
                .screen_space_lip_sync
                .evidence
                .iter()
                .all(|evidence| evidence.kind
                    == npc_game_profile::CapabilityEvidenceKind::NotVerified)
        );
        assert!(profile.characters.iter().all(|character| {
            character.identity.strategy == npc_game_profile::IdentityStrategy::ExplicitSelection
                && character.identity.evidence
                    == [npc_game_profile::IdentityEvidence::ExplicitSelection]
        }));
        if profile.id == SYNTHETIC_REVIEW_PROFILE_ID {
            synthetic_review_count += 1;
            assert!(profile.display_name.contains("Synthetic Review Game"));
            assert_eq!(profile.game.publisher, "Interactive NPCs Test Studio");
            assert!(profile.content.provenance.iter().any(|record| {
                record.kind == npc_game_profile::ProvenanceKind::Original
                    && record.license.as_deref() == Some("MIT")
                    && record.review_status == Some(npc_game_profile::ReviewStatus::Approved)
            }));
        } else {
            authored_game_count += 1;
        }
    }
    assert_eq!(authored_game_count, EXPECTED_AUTHORED_GAME_PROFILE_COUNT);
    assert_eq!(
        synthetic_review_count,
        EXPECTED_SYNTHETIC_REVIEW_PROFILE_COUNT
    );
    assert_eq!(
        ids.len(),
        EXPECTED_AUTHORED_GAME_PROFILE_COUNT + EXPECTED_SYNTHETIC_REVIEW_PROFILE_COUNT
    );
    assert_eq!(ids, expected_profile_ids());
}

#[test]
fn authored_corpus_has_stable_detection_safety_data_and_loadout_contracts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../profiles/games");
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path().join("profile.json");
        if !path.is_file() {
            continue;
        }
        let profile = load_profile(&fs::read(path).unwrap()).unwrap();

        let expected_processes: BTreeSet<_> = expected_primary_processes(&profile.id)
            .iter()
            .copied()
            .collect();
        let actual_processes: BTreeSet<_> = profile
            .detection
            .processes
            .iter()
            .filter(|process| process.required)
            .map(|process| process.executable.as_str())
            .collect();
        assert_eq!(
            actual_processes, expected_processes,
            "{} primary executable contract drifted",
            profile.id
        );
        assert!(profile.detection.processes.iter().all(|process| {
            !process.executable.contains(['/', '\\', ':'])
                && process.executable.to_ascii_lowercase().ends_with(".exe")
        }));
        assert!(profile
            .detection
            .processes
            .iter()
            .filter(|process| process.required)
            .all(
                |process| process.window_title_regex.as_ref().is_some_and(|pattern| {
                    !pattern.trim().is_empty() && regex::Regex::new(pattern).is_ok()
                })
            ));

        if let Some(expected_steam_ids) = expected_steam_ids(&profile.id) {
            let actual_steam_ids: BTreeSet<_> = profile
                .detection
                .stores
                .iter()
                .filter(|store| store.store == npc_game_profile::StoreKind::Steam)
                .filter_map(|store| store.app_id.as_deref())
                .collect();
            assert_eq!(
                actual_steam_ids,
                expected_steam_ids.iter().copied().collect(),
                "{} Steam identity drifted",
                profile.id
            );
        }
        assert!(profile.detection.stores.iter().all(|store| {
            store
                .app_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty())
                || !store.install_directory_hints.is_empty()
        }));

        let capture = &profile.detection.capture;
        assert!(!capture.allow_exclusive_fullscreen);
        assert!(capture
            .preferred_methods
            .contains(&npc_game_profile::CaptureMethod::WindowsGraphicsCapture));
        assert!(capture
            .preferred_methods
            .contains(&npc_game_profile::CaptureMethod::AudioSubtitles));
        assert_eq!(
            capture.fallback,
            npc_game_profile::CaptureFallback::AudioSubtitles
        );
        assert!(capture.excluded_window_title_regexes.len() >= 2);
        assert!(capture
            .excluded_window_title_regexes
            .iter()
            .all(|pattern| regex::Regex::new(pattern).is_ok()));
        assert!(
            capture.excluded_window_title_regexes.iter().any(|pattern| {
                let normalized = pattern.to_ascii_lowercase();
                normalized.contains("launcher")
                    || normalized.contains("overlay")
                    || normalized.contains("crash")
                    || normalized.contains("updater")
                    || normalized.contains("editor")
                    || normalized.contains("manager")
                    || normalized.contains("server")
                    || normalized.contains("anti-cheat")
                    || normalized.contains("online")
                    || normalized.contains("error")
                    || normalized.contains("console")
            }),
            "{} must substantively exclude non-game UI windows",
            profile.id
        );

        assert!(profile.safety.single_player_only);
        assert!(profile.safety.declarative_only);
        assert_eq!(
            profile.safety.online_policy,
            npc_game_profile::OnlinePolicy::Blocked
        );
        assert_eq!(
            profile.safety.anti_cheat_policy,
            npc_game_profile::AntiCheatPolicy::BlockWhenDetected
        );
        assert!(!profile.safety.risk_notes.is_empty());

        assert!(profile.characters.len() >= 6);
        assert!(profile.characters.iter().all(|character| {
            !character.id.trim().is_empty()
                && !character.display_name.trim().is_empty()
                && character.biography.trim().len() >= 40
                && character.personality.trim().len() >= 30
                && character.dialogue_style.trim().len() >= 30
                && !character.prompt.role.trim().is_empty()
                && !character.prompt.objectives.is_empty()
                && !character.prompt.constraints.is_empty()
                && !character.voice.description.trim().is_empty()
                && !character.voice.locale.trim().is_empty()
                && character.identity.strategy
                    == npc_game_profile::IdentityStrategy::ExplicitSelection
        }));
        assert_eq!(
            profile.content.character_data_readiness,
            Some(npc_game_profile::CharacterDataReadiness::Curated),
            "{} must explicitly declare its shipped character records curated",
            profile.id
        );
        let readiness_notes = profile
            .content
            .character_data_readiness_notes
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(readiness_notes.contains("stable ids"));
        assert!(readiness_notes.contains("not certified"));

        let recommendations = profile
            .recommendations
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no typed recommendations", profile.id));
        assert_eq!(
            recommendations.integration_mode,
            npc_game_profile::IntegrationMode::ExternalOnly
        );
        assert_eq!(
            recommendations.provider_strategy,
            npc_game_profile::ProviderStrategy::ApiFirst
        );
        assert_eq!(
            recommendations.local_activation_policy,
            npc_game_profile::LocalActivationPolicy::MeasuredWholeLoadoutFitRequired
        );
        assert!(recommendations.game_resource_reserve_required);
        assert_eq!(
            recommendations.screen_space_lip_sync,
            npc_game_profile::ScreenSpaceLipSyncRecommendation::ExperimentalOptInAfterExactTargetAndAdvancingFrameQualification
        );

        let original = profile.content.provenance.iter().any(|record| {
            record.kind == npc_game_profile::ProvenanceKind::Original
                && record.license.as_deref() == Some("MIT")
        });
        let official = profile.content.provenance.iter().any(|record| {
            record.kind == npc_game_profile::ProvenanceKind::Official
                && record
                    .source_url
                    .as_deref()
                    .is_some_and(|url| url.starts_with("https://"))
        });
        assert!(
            original,
            "{} lacks licensed original provenance",
            profile.id
        );
        assert!(
            official,
            "{} lacks an official HTTPS provenance record",
            profile.id
        );

        let diagnostics: BTreeSet<_> = profile
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.id.as_str())
            .collect();
        assert!(diagnostics.contains("recommended-feature-loadout"));
        assert!(diagnostics.contains("external-only-safety"));
        let loadout = profile
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.id == "recommended-feature-loadout")
            .unwrap();
        let loadout_text =
            format!("{} {}", loadout.check, loadout.remediation).to_ascii_lowercase();
        for required in ["api", "llm", "stt", "tts", "ram", "vram", "game reserve"] {
            assert!(
                loadout_text.contains(required),
                "{} loadout policy is missing {required}",
                profile.id
            );
        }
        let external = profile
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.id == "external-only-safety")
            .unwrap();
        let external_text =
            format!("{} {}", external.check, external.remediation).to_ascii_lowercase();
        for required in ["single-player", "external", "no injection", "pid", "hwnd"] {
            assert!(
                external_text.contains(required),
                "{} external-only policy is missing {required}",
                profile.id
            );
        }

        assert!(profile.troubleshooting.len() >= 4);
        assert!(profile.troubleshooting.iter().all(|entry| {
            !entry.symptom.trim().is_empty()
                && !entry.cause.trim().is_empty()
                && entry.steps.len() >= 2
                && entry.steps.iter().all(|step| !step.trim().is_empty())
        }));
        assert!(profile.prompts.system_preamble.trim().len() >= 120);
        assert!(profile.prompts.safety_rules.len() >= 3);
        assert!(profile.prompts.background_npc_template.trim().len() >= 80);
        let fallback_text = profile
            .capabilities
            .iter()
            .map(|(_, claim)| claim.fallback.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        assert!(fallback_text.contains("audio"));
        assert!(fallback_text.contains("subtitle"));
        assert!(fallback_text.contains("explicit"));
    }
}

fn expected_profile_ids() -> BTreeSet<String> {
    [
        "baldurs-gate-3",
        "cyberpunk-2077",
        "divinity-original-sin-2",
        "dragon-age-inquisition",
        "eclipse-harbor",
        "elden-ring-offline",
        "fallout-4",
        "fallout-new-vegas",
        "gta-v-story",
        "kenshi",
        "kingdom-come-deliverance-2",
        "mass-effect-legendary-edition",
        "minecraft-java",
        "mount-and-blade-2-bannerlord",
        "oblivion-remastered",
        "red-dead-redemption-2-story",
        "skyrim-special-edition",
        "stardew-valley",
        "starfield",
        "the-sims-4",
        "the-witcher-3",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn expected_primary_processes(profile_id: &str) -> &'static [&'static str] {
    match profile_id {
        "baldurs-gate-3" => &["bg3.exe", "bg3_dx11.exe"],
        "cyberpunk-2077" => &["Cyberpunk2077.exe"],
        "divinity-original-sin-2" => &["EoCApp.exe"],
        "dragon-age-inquisition" => &["DragonAgeInquisition.exe"],
        "eclipse-harbor" => &["EclipseHarbor.exe"],
        "elden-ring-offline" => &["eldenring.exe"],
        "fallout-4" => &["Fallout4.exe"],
        "fallout-new-vegas" => &["FalloutNV.exe"],
        "gta-v-story" => &["GTA5.exe", "GTA5_Enhanced.exe"],
        "kenshi" => &["kenshi_x64.exe", "kenshi_x86.exe"],
        "kingdom-come-deliverance-2" => &["KingdomCome.exe"],
        "mass-effect-legendary-edition" => {
            &["MassEffect1.exe", "MassEffect2.exe", "MassEffect3.exe"]
        }
        "minecraft-java" => &["javaw.exe"],
        "mount-and-blade-2-bannerlord" => &["Bannerlord.Native.exe"],
        "oblivion-remastered" => &[
            "OblivionRemastered-Win64-Shipping.exe",
            "OblivionRemastered-WinGDK-Shipping.exe",
        ],
        "red-dead-redemption-2-story" => &["RDR2.exe"],
        "skyrim-special-edition" => &["SkyrimSE.exe"],
        "stardew-valley" => &["Stardew Valley.exe"],
        "starfield" => &["Starfield.exe"],
        "the-sims-4" => &["TS4_x64.exe"],
        "the-witcher-3" => &["witcher3.exe", "witcher3_dx12.exe"],
        other => panic!("unexpected profile {other}"),
    }
}

fn expected_steam_ids(profile_id: &str) -> Option<&'static [&'static str]> {
    match profile_id {
        "baldurs-gate-3" => Some(&["1086940"]),
        "cyberpunk-2077" => Some(&["1091500"]),
        "divinity-original-sin-2" => Some(&["435150"]),
        "dragon-age-inquisition" => Some(&["1222690"]),
        "eclipse-harbor" => None,
        "elden-ring-offline" => Some(&["1245620"]),
        "fallout-4" => Some(&["377160"]),
        "fallout-new-vegas" => Some(&["22380"]),
        "gta-v-story" => Some(&["271590", "3240220"]),
        "kenshi" => Some(&["233860"]),
        "kingdom-come-deliverance-2" => Some(&["1771300"]),
        "mass-effect-legendary-edition" => Some(&["1328670"]),
        "minecraft-java" => None,
        "mount-and-blade-2-bannerlord" => Some(&["261550"]),
        "oblivion-remastered" => Some(&["2623190"]),
        "red-dead-redemption-2-story" => Some(&["1174180"]),
        "skyrim-special-edition" => Some(&["489830"]),
        "stardew-valley" => Some(&["413150"]),
        "starfield" => Some(&["1716740"]),
        "the-sims-4" => Some(&["1222670"]),
        "the-witcher-3" => Some(&["292030"]),
        other => panic!("unexpected profile {other}"),
    }
}
