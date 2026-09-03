#![allow(clippy::unwrap_used)]
// Test setup treats every fixture construction failure as an immediate assertion failure.

mod support;

use npc_character_db::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn importer_reads_only_allowlisted_data_and_keeps_pending_content_inert() {
    let temp = TempDir::new().unwrap();
    populate_source(temp.path(), "Jackie Welles: Safe style", false);
    fs::write(
        temp.path().join("characters/Jackie_Welles/images/bad.png"),
        b"not png",
    )
    .unwrap();
    fs::write(
        temp.path()
            .join("characters/Jackie_Welles/images/cache.pkl"),
        b"pickle",
    )
    .unwrap();
    fs::write(
        temp.path().join("characters/Jackie_Welles/vectordb.bin"),
        b"index",
    )
    .unwrap();
    fs::write(
        temp.path().join("characters/Jackie_Welles/vectors.parquet"),
        b"index",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("characters/Jackie_Welles/voice")).unwrap();
    fs::write(
        temp.path().join("characters/Jackie_Welles/voice/voice.py"),
        b"print('never read')",
    )
    .unwrap();
    fs::write(
        temp.path().join("apikeys.json"),
        b"{\"secret\":\"never read\"}",
    )
    .unwrap();

    let plan = scan_legacy_sources(
        "cyberpunk-2077",
        &[LegacySource {
            source_id: "root-tree".to_owned(),
            revision: "503ef3b".to_owned(),
            root: temp.path().to_owned(),
        }],
        ImportLimits::default(),
    )
    .unwrap();
    let manifest = plan.manifest();
    assert!(manifest.conflicts.is_empty());
    assert!(manifest
        .accepted
        .iter()
        .any(|item| item.logical_path == "world.txt"));
    assert!(manifest.accepted.iter().any(|item| {
        item.logical_path == "characters/Jackie_Welles/images/1.jpg"
            && item.kind == ImportedArtifactKind::IdentityImage
    }));
    for kind in [
        RejectionKind::UnsafeDerived,
        RejectionKind::Executable,
        RejectionKind::NotAllowlisted,
        RejectionKind::InvalidImage,
        RejectionKind::MutableRuntimeState,
    ] {
        assert!(
            manifest.rejected.iter().any(|item| item.kind == kind),
            "missing rejection {kind:?}"
        );
    }

    let materialized = plan
        .materialize(&support::cyberpunk_profile(), &[])
        .unwrap();
    assert!(!materialized.activatable);
    assert!(!materialized.staged_profile.content.knowledge.is_empty());
    assert!(materialized
        .staged_profile
        .content
        .provenance
        .iter()
        .filter(|item| item.kind == npc_game_profile::ProvenanceKind::LegacyImport)
        .all(|item| item.review_status == Some(npc_game_profile::ReviewStatus::Pending)));
    assert_eq!(materialized.identity_assets.len(), 1);
    assert!(materialized.identity_assets[0].local_only);
    assert_eq!(
        materialized.identity_assets[0].review_status,
        npc_game_profile::ReviewStatus::Pending
    );

    let database = CharacterDatabase::new(materialized.staged_profile).unwrap();
    assert!(database
        .approved_knowledge_memory_inputs("user-1")
        .unwrap()
        .iter()
        .all(|input| !input.content.contains("Night City\nCore rules")));
}

#[test]
fn normalized_duplicates_merge_but_substantive_conflicts_require_resolution() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    populate_source(first.path(), "Jackie Welles: First candidate", true);
    populate_source(second.path(), "Jackie Welles: Second candidate", false);

    let plan = scan_legacy_sources(
        "cyberpunk-2077",
        &[
            LegacySource {
                source_id: "active-root".to_owned(),
                revision: "503ef3b".to_owned(),
                root: first.path().to_owned(),
            },
            LegacySource {
                source_id: "games-copy".to_owned(),
                revision: "503ef3b".to_owned(),
                root: second.path().to_owned(),
            },
        ],
        ImportLimits::default(),
    )
    .unwrap();
    assert!(!plan
        .manifest()
        .conflicts
        .iter()
        .any(|conflict| conflict.logical_path == "world.txt"));
    assert_eq!(
        plan.manifest()
            .accepted
            .iter()
            .find(|item| item.logical_path == "world.txt")
            .unwrap()
            .candidates
            .len(),
        2
    );
    assert_eq!(plan.manifest().conflicts.len(), 1);
    assert_eq!(
        plan.manifest().conflicts[0].logical_path,
        "characters/Jackie_Welles/pre_conversation.json"
    );

    let unresolved = plan.materialize(&support::cyberpunk_profile(), &[]);
    assert!(matches!(
        unresolved,
        Err(LegacyImportError::UnresolvedConflicts(paths))
            if paths == vec!["characters/Jackie_Welles/pre_conversation.json"]
    ));
    let invalid = plan.materialize(
        &support::cyberpunk_profile(),
        &[ConflictResolutionV1 {
            logical_path: "characters/Jackie_Welles/pre_conversation.json".to_owned(),
            selected_source_id: "missing".to_owned(),
        }],
    );
    assert!(matches!(
        invalid,
        Err(LegacyImportError::InvalidResolution { .. })
    ));

    let materialized = plan
        .materialize(
            &support::cyberpunk_profile(),
            &[ConflictResolutionV1 {
                logical_path: "characters/Jackie_Welles/pre_conversation.json".to_owned(),
                selected_source_id: "games-copy".to_owned(),
            }],
        )
        .unwrap();
    let jackie = materialized
        .staged_profile
        .characters
        .iter()
        .find(|character| character.id == "jackie-welles")
        .unwrap();
    assert!(jackie
        .style_examples
        .iter()
        .any(|example| example.text == "Second candidate"));
    assert!(!jackie
        .style_examples
        .iter()
        .any(|example| example.text == "First candidate"));
    assert_eq!(
        materialized
            .manifest
            .accepted
            .iter()
            .find(|item| { item.logical_path == "characters/Jackie_Welles/pre_conversation.json" })
            .unwrap()
            .selected_source_id
            .as_deref(),
        Some("games-copy")
    );
}

#[test]
fn oversized_allowed_files_are_rejected_before_reading() {
    let temp = TempDir::new().unwrap();
    populate_source(temp.path(), "Jackie Welles: Style", false);
    fs::write(temp.path().join("public_info.txt"), vec![b'x'; 65]).unwrap();
    let plan = scan_legacy_sources(
        "cyberpunk-2077",
        &[LegacySource {
            source_id: "oversized".to_owned(),
            revision: "503ef3b".to_owned(),
            root: temp.path().to_owned(),
        }],
        ImportLimits {
            max_text_or_json_bytes: 64,
            ..ImportLimits::default()
        },
    )
    .unwrap();
    assert!(plan.manifest().rejected.iter().any(|item| {
        item.relative_path == "public_info.txt" && item.kind == RejectionKind::Oversized
    }));
}

fn populate_source(root: &Path, style_line: &str, crlf_world: bool) {
    fs::create_dir_all(root.join("characters/Jackie_Welles/images")).unwrap();
    fs::create_dir_all(root.join("characters/default")).unwrap();
    fs::write(
        root.join("world.txt"),
        if crlf_world {
            b"Night City\r\nCore rules\r\n".as_slice()
        } else {
            b"Night City\nCore rules\n".as_slice()
        },
    )
    .unwrap();
    fs::write(
        root.join("public_info.txt"),
        b"Public fact one.\n\nPublic fact two.",
    )
    .unwrap();
    fs::write(
        root.join("characters/Jackie_Welles/bio.txt"),
        b"A concise reviewed biography candidate.",
    )
    .unwrap();
    fs::write(
        root.join("characters/Jackie_Welles/character_knowledge.txt"),
        b"Character-only fact.",
    )
    .unwrap();
    fs::write(
        root.join("characters/Jackie_Welles/pre_conversation.json"),
        format!(
            "{{\"pre_conversation\":[{{\"line\":{}}}]}}",
            serde_json::to_string(style_line).unwrap()
        ),
    )
    .unwrap();
    fs::write(
        root.join("characters/Jackie_Welles/conversation.json"),
        b"{\"conversation\":[{\"sender\":\"V\",\"message\":\"private history\"}]}",
    )
    .unwrap();
    fs::write(
        root.join("characters/default/pre_conversation.json"),
        b"{\"pre_conversation\":[{\"line\":\"Common city dialect\"}]}",
    )
    .unwrap();
    fs::write(root.join("characters/default/name.txt"), b"mutable").unwrap();
    fs::write(
        root.join("characters/Jackie_Welles/images/1.jpg"),
        [0xff, 0xd8, 0xff, 0x00, 0xff, 0xd9],
    )
    .unwrap();
}
