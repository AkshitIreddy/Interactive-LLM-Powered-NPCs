use std::fs;

use npc_subtitle_engine::{
    plan_shaping, FontCatalog, ReadSubtitlePreferencesRequestV1, ResetSubtitlePreferencesRequestV1,
    SaveSubtitlePreferencesRequestV1, ScopedSubtitlePreferencesV1, SubtitleEffectiveSourceKindV1,
    SubtitleFontAvailabilityV1, SubtitlePreferenceError, SubtitlePreferenceManager,
    SubtitlePreferenceMigrationStateV1, SubtitlePreferenceOverridesV1, SubtitlePreferenceScopeV1,
    TextDirection, FONT_CATALOG_JSON, SUBTITLE_PREFERENCES_FILE_NAME,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

fn global() -> SubtitlePreferenceScopeV1 {
    SubtitlePreferenceScopeV1::Global
}

fn game() -> SubtitlePreferenceScopeV1 {
    SubtitlePreferenceScopeV1::Game {
        game_profile_id: "skyrim-special-edition".into(),
    }
}

fn character() -> SubtitlePreferenceScopeV1 {
    SubtitlePreferenceScopeV1::Character {
        game_profile_id: "skyrim-special-edition".into(),
        character_id: "whiterun:balgruuf".into(),
    }
}

fn read(
    manager: &SubtitlePreferenceManager,
    scope: SubtitlePreferenceScopeV1,
) -> npc_subtitle_engine::SubtitlePreferenceSnapshotV1 {
    manager
        .read(ReadSubtitlePreferencesRequestV1 { scope })
        .expect("read preferences")
}

fn save(
    manager: &SubtitlePreferenceManager,
    revision: u64,
    scope: SubtitlePreferenceScopeV1,
    selected_style_id: Option<&str>,
    overrides: SubtitlePreferenceOverridesV1,
) -> npc_subtitle_engine::SubtitlePreferenceSnapshotV1 {
    manager
        .save(SaveSubtitlePreferencesRequestV1 {
            expected_revision: revision,
            entry: ScopedSubtitlePreferencesV1 {
                scope,
                selected_style_id: selected_style_id.map(str::to_owned),
                overrides,
            },
        })
        .expect("save preferences")
}

#[test]
fn scoped_selection_and_supported_overrides_report_exact_sources_and_persist_atomically() {
    let directory = tempdir().expect("temporary preferences");
    let manager = SubtitlePreferenceManager::new(directory.path()).expect("manager");
    let initialized = read(&manager, character());
    assert_eq!(initialized.revision, 0);
    assert_eq!(
        initialized.migration.state,
        SubtitlePreferenceMigrationStateV1::InitializedDefaults
    );
    assert_eq!(
        initialized.effective.selected_style_id.value,
        "cinematic_glass"
    );
    assert_eq!(
        initialized.effective.selected_style_id.source.kind,
        SubtitleEffectiveSourceKindV1::BundledManifestDefault
    );
    assert_eq!(
        initialized.supported_override_fields,
        ["safeAreaDp", "textScale", "backplateEnabled", "opacity"]
    );

    let global_snapshot = save(
        &manager,
        0,
        global(),
        Some("accessibility_high_contrast"),
        SubtitlePreferenceOverridesV1 {
            safe_area_dp: Some(50.0),
            opacity: Some(0.8),
            ..SubtitlePreferenceOverridesV1::default()
        },
    );
    let game_snapshot = save(
        &manager,
        global_snapshot.revision,
        game(),
        None,
        SubtitlePreferenceOverridesV1 {
            text_scale: Some(1.25),
            backplate_enabled: Some(false),
            ..SubtitlePreferenceOverridesV1::default()
        },
    );
    let character_snapshot = save(
        &manager,
        game_snapshot.revision,
        character(),
        None,
        SubtitlePreferenceOverridesV1 {
            safe_area_dp: Some(80.0),
            ..SubtitlePreferenceOverridesV1::default()
        },
    );

    assert_eq!(character_snapshot.revision, 3);
    assert_eq!(
        character_snapshot.effective.selected_style_id.source.scope,
        Some(global())
    );
    assert_eq!(
        character_snapshot.effective.safe_area_dp.source.scope,
        Some(character())
    );
    assert_eq!(
        character_snapshot.effective.text_scale.source.scope,
        Some(game())
    );
    assert_eq!(
        character_snapshot.effective.backplate_enabled.source.scope,
        Some(game())
    );
    assert_eq!(
        character_snapshot.effective.opacity.source.scope,
        Some(global())
    );
    assert_eq!(
        character_snapshot
            .effective
            .renderer_parameters
            .safe_area_dp,
        80.0
    );
    assert_eq!(
        character_snapshot
            .effective
            .renderer_parameters
            .body_size_dp,
        32.5
    );
    assert_eq!(
        character_snapshot
            .effective
            .renderer_parameters
            .speaker_size_dp,
        21.25
    );
    assert!(
        !character_snapshot
            .effective
            .renderer_parameters
            .backplate_enabled
    );
    assert_eq!(
        character_snapshot
            .effective
            .renderer_parameters
            .global_opacity,
        0.8
    );
    let runtime_state = manager
        .resolve_for_renderer(character())
        .expect("resolve full renderer state");
    assert_eq!(runtime_state.style.id, "accessibility_high_contrast");
    assert_eq!(runtime_state.style.geometry.safe_margin_dp, 80.0);
    assert_eq!(runtime_state.style.typography.body_size_dp, 32.5);
    assert!(!runtime_state.style.effects.backplate.enabled);
    assert_eq!(runtime_state.global_opacity, 0.8);

    let persisted_path = directory.path().join(SUBTITLE_PREFERENCES_FILE_NAME);
    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(&persisted_path).expect("persisted preference bytes"))
            .expect("atomic file is complete JSON");
    assert_eq!(persisted["schemaVersion"], 1);
    assert_eq!(persisted["revision"], 3);
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("preference directory")
            .count(),
        1,
        "atomic temporary files must not remain"
    );

    let reopened = SubtitlePreferenceManager::new(directory.path()).expect("reopen manager");
    let reopened_snapshot = read(&reopened, character());
    assert_eq!(reopened_snapshot.revision, 3);
    assert_eq!(
        reopened_snapshot.migration.state,
        SubtitlePreferenceMigrationStateV1::Current
    );
    assert_eq!(reopened_snapshot.effective, character_snapshot.effective);
}

#[test]
fn reset_is_confirmation_and_revision_guarded_and_removes_only_the_requested_scope() {
    let directory = tempdir().expect("temporary preferences");
    let manager = SubtitlePreferenceManager::new(directory.path()).expect("manager");
    let global_snapshot = save(
        &manager,
        0,
        global(),
        Some("accessibility_high_contrast"),
        SubtitlePreferenceOverridesV1 {
            safe_area_dp: Some(48.0),
            ..SubtitlePreferenceOverridesV1::default()
        },
    );
    let game_snapshot = save(
        &manager,
        global_snapshot.revision,
        game(),
        None,
        SubtitlePreferenceOverridesV1 {
            text_scale: Some(1.4),
            ..SubtitlePreferenceOverridesV1::default()
        },
    );
    let character_snapshot = save(
        &manager,
        game_snapshot.revision,
        character(),
        None,
        SubtitlePreferenceOverridesV1 {
            opacity: Some(0.6),
            ..SubtitlePreferenceOverridesV1::default()
        },
    );

    let unconfirmed = manager.reset(ResetSubtitlePreferencesRequestV1 {
        expected_revision: character_snapshot.revision,
        scope: character(),
        explicit_user_confirmation: false,
    });
    assert!(matches!(
        unconfirmed,
        Err(SubtitlePreferenceError::ConfirmationRequired)
    ));
    let stale = manager.reset(ResetSubtitlePreferencesRequestV1 {
        expected_revision: 1,
        scope: character(),
        explicit_user_confirmation: true,
    });
    assert!(matches!(
        stale,
        Err(SubtitlePreferenceError::RevisionConflict)
    ));

    let reset_character = manager
        .reset(ResetSubtitlePreferencesRequestV1 {
            expected_revision: character_snapshot.revision,
            scope: character(),
            explicit_user_confirmation: true,
        })
        .expect("reset character");
    assert_eq!(reset_character.revision, 4);
    assert_eq!(reset_character.entries.len(), 2);
    assert_eq!(reset_character.effective.opacity.value, 1.0);
    assert_eq!(
        reset_character.effective.opacity.source.kind,
        SubtitleEffectiveSourceKindV1::RendererDefault
    );
    assert_eq!(reset_character.effective.text_scale.value, 1.4);
    assert_eq!(reset_character.effective.safe_area_dp.value, 48.0);

    let reset_global = manager
        .reset(ResetSubtitlePreferencesRequestV1 {
            expected_revision: reset_character.revision,
            scope: global(),
            explicit_user_confirmation: true,
        })
        .expect("reset global");
    assert_eq!(reset_global.entries.len(), 1, "game scope remains intact");
    assert_eq!(
        reset_global.effective.selected_style_id.value,
        "cinematic_glass"
    );
    assert_eq!(
        reset_global.effective.selected_style_id.source.kind,
        SubtitleEffectiveSourceKindV1::BundledManifestDefault
    );
}

#[test]
fn legacy_v0_migrates_once_to_versioned_scoped_state_and_future_versions_fail_closed() {
    let legacy_directory = tempdir().expect("legacy directory");
    let path = legacy_directory.path().join(SUBTITLE_PREFERENCES_FILE_NAME);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 0,
            "revision": 9,
            "selectedStyleId": "accessibility_high_contrast",
            "overrides": {
                "safeAreaDp": 64.0,
                "textScale": 1.2,
                "backplateEnabled": true,
                "opacity": 0.9
            }
        }))
        .expect("legacy JSON"),
    )
    .expect("write legacy fixture");
    let manager = SubtitlePreferenceManager::new(legacy_directory.path()).expect("migrate v0");
    let migrated = read(&manager, global());
    assert_eq!(migrated.revision, 10);
    assert_eq!(
        migrated.migration.state,
        SubtitlePreferenceMigrationStateV1::MigratedLegacyV0
    );
    assert_eq!(migrated.migration.from_schema_version, Some(0));
    assert_eq!(migrated.effective.safe_area_dp.value, 64.0);
    let migrated_file: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("migrated file")).expect("migrated JSON");
    assert_eq!(migrated_file["schemaVersion"], 1);
    assert!(migrated_file.get("selectedStyleId").is_none());
    assert!(migrated_file["entries"].is_array());

    let future_directory = tempdir().expect("future directory");
    let future_path = future_directory.path().join(SUBTITLE_PREFERENCES_FILE_NAME);
    let future_bytes = br#"{"schemaVersion":2,"revision":44,"entries":[]}"#;
    fs::write(&future_path, future_bytes).expect("write future fixture");
    let result = SubtitlePreferenceManager::new(future_directory.path());
    assert!(matches!(
        result,
        Err(SubtitlePreferenceError::UnsupportedSchema(2))
    ));
    assert_eq!(
        fs::read(&future_path).expect("future file preserved"),
        future_bytes,
        "unsupported future state must never be overwritten"
    );
}

#[test]
fn corrupt_documents_fail_closed_without_destroying_recovery_evidence() {
    let directory = tempdir().expect("corrupt directory");
    let path = directory.path().join(SUBTITLE_PREFERENCES_FILE_NAME);
    let corrupt = br#"{"schemaVersion":1,"revision":"torn""#;
    fs::write(&path, corrupt).expect("write corrupt fixture");
    let result = SubtitlePreferenceManager::new(directory.path());
    assert!(matches!(
        result,
        Err(SubtitlePreferenceError::Persistence(_))
    ));
    assert_eq!(
        fs::read(&path).expect("corrupt evidence preserved"),
        corrupt
    );
}

#[test]
fn unsupported_fields_invalid_ranges_and_unknown_styles_cannot_mutate_persistence() {
    let unsupported = serde_json::from_value::<SaveSubtitlePreferencesRequestV1>(json!({
        "expectedRevision": 0,
        "entry": {
            "scope": { "kind": "global" },
            "selectedStyleId": "cinematic_glass",
            "overrides": {
                "safeAreaDp": 32.0,
                "fontFamily": "Downloaded Font",
                "animation": true,
                "hdrPeakNits": 1000
            }
        }
    }));
    assert!(
        unsupported.is_err(),
        "unknown renderer fields must fail closed"
    );

    let directory = tempdir().expect("temporary preferences");
    let manager = SubtitlePreferenceManager::new(directory.path()).expect("manager");
    let path = directory.path().join(SUBTITLE_PREFERENCES_FILE_NAME);
    let before = fs::read(&path).expect("initial persisted state");
    let invalid_opacity = manager.save(SaveSubtitlePreferencesRequestV1 {
        expected_revision: 0,
        entry: ScopedSubtitlePreferencesV1 {
            scope: global(),
            selected_style_id: None,
            overrides: SubtitlePreferenceOverridesV1 {
                opacity: Some(0.0),
                ..SubtitlePreferenceOverridesV1::default()
            },
        },
    });
    assert!(matches!(
        invalid_opacity,
        Err(SubtitlePreferenceError::Invalid(_))
    ));
    let unknown_style = manager.save(SaveSubtitlePreferencesRequestV1 {
        expected_revision: 0,
        entry: ScopedSubtitlePreferencesV1 {
            scope: global(),
            selected_style_id: Some("remote_unvalidated_style".into()),
            overrides: SubtitlePreferenceOverridesV1::default(),
        },
    });
    assert!(matches!(
        unknown_style,
        Err(SubtitlePreferenceError::Invalid(_))
    ));
    let empty_save = manager.save(SaveSubtitlePreferencesRequestV1 {
        expected_revision: 0,
        entry: ScopedSubtitlePreferencesV1 {
            scope: global(),
            selected_style_id: None,
            overrides: SubtitlePreferenceOverridesV1::default(),
        },
    });
    assert!(matches!(
        empty_save,
        Err(SubtitlePreferenceError::Invalid(_))
    ));
    assert_eq!(read(&manager, global()).revision, 0);
    assert_eq!(fs::read(&path).expect("unchanged state"), before);
}

#[test]
fn font_disclosure_and_shaping_cover_rtl_and_cjk_without_claiming_bundled_or_installed_fonts() {
    let directory = tempdir().expect("temporary preferences");
    let manager = SubtitlePreferenceManager::new(directory.path()).expect("manager");
    let snapshot = read(&manager, global());
    assert!(snapshot.assets.no_font_binaries_bundled);
    assert!(snapshot
        .assets
        .generated_asset_policy
        .contains("No generated or remotely downloaded font binaries"));
    let families = snapshot
        .assets
        .font_roles
        .iter()
        .flat_map(|role| &role.fallback_chain)
        .collect::<Vec<_>>();
    assert!(families.iter().all(|family| {
        !family.binary_bundled
            && family.availability == SubtitleFontAvailabilityV1::SystemLookupRequired
            && !family.license_id.is_empty()
            && !family.usage_basis.is_empty()
    }));
    assert!(families
        .iter()
        .any(|family| family.family == "Noto Sans Arabic"));
    assert!(families
        .iter()
        .any(|family| family.family == "Yu Gothic UI"));
    assert!(families
        .iter()
        .any(|family| family.family == "Noto Sans CJK KR"));

    let fonts: FontCatalog = serde_json::from_str(FONT_CATALOG_JSON).expect("font catalog");
    let arabic = plan_shaping("مرحبا بالعالم", Some("ar"), "subtitle_sans", &fonts)
        .expect("Arabic shaping plan");
    assert_eq!(arabic.direction, TextDirection::RightToLeft);
    assert!(arabic.require_bidi_reordering);
    assert!(arabic.require_complex_shaping);
    assert!(arabic
        .family_fallback_chain
        .iter()
        .any(|family| family == "Noto Sans Arabic"));

    let japanese = plan_shaping("東京で会いましょう", Some("ja-JP"), "subtitle_sans", &fonts)
        .expect("Japanese shaping plan");
    assert_eq!(japanese.direction, TextDirection::LeftToRight);
    assert!(!japanese.require_bidi_reordering);
    assert!(japanese
        .family_fallback_chain
        .iter()
        .any(|family| family == "Yu Gothic UI"));
}

#[test]
fn read_save_reset_dtos_use_camel_case_and_reject_unbounded_scope_ids() {
    let save_value = serde_json::to_value(SaveSubtitlePreferencesRequestV1 {
        expected_revision: 7,
        entry: ScopedSubtitlePreferencesV1 {
            scope: character(),
            selected_style_id: Some("cinematic_glass".into()),
            overrides: SubtitlePreferenceOverridesV1 {
                text_scale: Some(1.1),
                ..SubtitlePreferenceOverridesV1::default()
            },
        },
    })
    .expect("serialize save DTO");
    assert_eq!(save_value["expectedRevision"], 7);
    assert_eq!(save_value["entry"]["scope"]["kind"], "character");
    assert_eq!(
        save_value["entry"]["scope"]["gameProfileId"],
        "skyrim-special-edition"
    );
    let serialized_scale = save_value["entry"]["overrides"]["textScale"]
        .as_f64()
        .expect("serialized text scale");
    assert!((serialized_scale - 1.1).abs() < 0.000_001);
    assert!(save_value["entry"]["overrides"].get("bodyColor").is_none());

    let directory = tempdir().expect("temporary preferences");
    let manager = SubtitlePreferenceManager::new(directory.path()).expect("manager");
    let invalid = manager.read(ReadSubtitlePreferencesRequestV1 {
        scope: SubtitlePreferenceScopeV1::Game {
            game_profile_id: "../untrusted/path".into(),
        },
    });
    assert!(matches!(invalid, Err(SubtitlePreferenceError::Invalid(_))));
}

#[test]
fn renderer_authority_is_atomic_digest_bound_and_preserves_effective_sources() {
    let directory = tempdir().expect("temporary preferences");
    let manager = SubtitlePreferenceManager::new(directory.path()).expect("manager");
    let global_snapshot = save(
        &manager,
        0,
        global(),
        Some("accessibility_high_contrast"),
        SubtitlePreferenceOverridesV1 {
            safe_area_dp: Some(48.0),
            text_scale: Some(1.25),
            backplate_enabled: Some(false),
            opacity: Some(0.8),
        },
    );
    let authority = manager
        .resolve_renderer_authority(character())
        .expect("atomic authority");

    assert_eq!(authority.revision, global_snapshot.revision);
    assert_eq!(authority.style.id, "accessibility_high_contrast");
    assert_eq!(authority.style.geometry.safe_margin_dp, 48.0);
    assert_eq!(authority.style.typography.body_size_dp, 32.5);
    assert_eq!(authority.style.typography.speaker_size_dp, 21.25);
    assert!(!authority.style.effects.backplate.enabled);
    assert_eq!(authority.text_scale, 1.25);
    assert_eq!(authority.opacity, 0.8);
    assert_eq!(authority.authority_sha256.len(), 64);
    authority.validate().expect("self-consistent authority");
    assert_eq!(
        authority.sources.selected_style.kind,
        SubtitleEffectiveSourceKindV1::PersistedScope
    );
    assert_eq!(
        authority.sources.text_scale.kind,
        SubtitleEffectiveSourceKindV1::PersistedScope
    );

    let mut style_tamper = authority.clone();
    style_tamper.style.typography.body_size_dp += 1.0;
    assert!(style_tamper.validate().is_err());
    let mut opacity_tamper = authority.clone();
    opacity_tamper.opacity = 0.7;
    assert!(opacity_tamper.validate().is_err());
    let mut revision_tamper = authority.clone();
    revision_tamper.revision += 1;
    assert!(revision_tamper.validate().is_err());
    let mut source_tamper = authority;
    source_tamper.sources.opacity.kind = SubtitleEffectiveSourceKindV1::RendererDefault;
    assert!(source_tamper.validate().is_err());
}
