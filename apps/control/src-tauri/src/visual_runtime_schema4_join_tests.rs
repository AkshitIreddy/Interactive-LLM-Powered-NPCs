use super::*;
use crate::character_mouth_packs::{
    CharacterMouthPackFilesV1, CharacterMouthPackManagerV1, CurrentCharacterMouthEnableAuthorityV1,
    EnableCharacterMouthPackRequestV1, ImportCharacterMouthPackRequestV1,
    SelectedCharacterMouthAuthorityV1,
};
use crate::identity_runtime::NativeFullSourceRoiV1;

/// Opt-in cross-language join proof for the schema-four current-pixel atlas.
///
/// The test deliberately stops below production capture/presentation authority:
/// it runs the real test game in its no-window self-test mode, launches the real
/// GUI-subsystem mouth worker under the app Job Object, sends the Rust-produced
/// schema-four atlas wire, and runs the native owned-texture D3D service smoke.
/// Provider command 10 is exercised separately by the activation proof whose
/// exact receipt is required as an input here. An isolated registry uses a
/// labeled test-only actor lock to exercise the exact character equality gate;
/// it is never stored as runtime authority. No target-PID loadout admission,
/// capture lease, audio lease, or production actor lock is minted.
#[cfg(all(windows, debug_assertions))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires real built native processes, a schema-four review fixture, and a provider-activation receipt"]
async fn real_schema_four_worker_join_is_headless_and_fail_closed() {
    use std::os::windows::process::CommandExt;

    fn required_path(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("{name} is required"))
    }

    fn hidden_child(
        executable: &Path,
        arguments: &[&std::ffi::OsStr],
    ) -> (u32, std::process::Output) {
        let mut command = std::process::Command::new(executable);
        command.args(arguments);
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let child = command.spawn().unwrap_or_else(|error| {
            panic!("launch hidden process {}: {error}", executable.display())
        });
        let process_id = child.id();
        let output = child.wait_with_output().unwrap_or_else(|error| {
            panic!("wait for hidden process {}: {error}", executable.display())
        });
        (process_id, output)
    }

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let a = chunk[0];
            let b = chunk.get(1).copied().unwrap_or_default();
            let c = chunk.get(2).copied().unwrap_or_default();
            encoded.push(ALPHABET[(a >> 2) as usize] as char);
            encoded.push(ALPHABET[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
            encoded.push(if chunk.len() > 1 {
                ALPHABET[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char
            } else {
                '='
            });
            encoded.push(if chunk.len() > 2 {
                ALPHABET[(c & 0x3f) as usize] as char
            } else {
                '='
            });
        }
        encoded
    }

    fn stage_exact_reviewed_fixture(source: &Path, destination: &Path) -> serde_json::Value {
        std::fs::create_dir_all(destination).expect("fixture directory");
        let manifest_path = source.join("atlas.json");
        let manifest_bytes = std::fs::read(&manifest_path).expect("schema-four atlas manifest");
        let manifest: serde_json::Value =
            serde_json::from_slice(&manifest_bytes).expect("schema-four atlas JSON");
        assert_eq!(
            manifest["schemaVersion"],
            serde_json::json!(CURRENT_PIXEL_ORAL_STRIP_ATLAS_SCHEMA)
        );
        assert_eq!(
            manifest["texture"]["representation"],
            serde_json::json!("normalized-oral-strip-v1")
        );
        assert_eq!(
            manifest["enrollmentBinding"]["gameProfileId"],
            serde_json::json!("eclipse-harbor")
        );
        assert_eq!(
            manifest["enrollmentBinding"]["characterId"],
            serde_json::json!("mara-venn")
        );
        assert_eq!(
            manifest["enrollmentBinding"]["reviewStatus"],
            serde_json::json!("reviewed-private")
        );
        assert!(manifest["enrollmentBinding"]["reviewEvidenceSha256"]
            .as_str()
            .is_some_and(is_lowercase_sha256));
        let texture_name = manifest["texture"]["file"]
            .as_str()
            .expect("texture file name");
        std::fs::copy(source.join(texture_name), destination.join(texture_name))
            .expect("copy exact schema-four texture");
        std::fs::write(destination.join("atlas.json"), manifest_bytes)
            .expect("write exact reviewed manifest");
        manifest
    }

    let worker = required_path("NPC_REAL_SCHEMA4_MOUTH_WORKER");
    let atlas_source = required_path("NPC_REAL_SCHEMA4_ATLAS_ROOT");
    let test_game = required_path("NPC_REAL_SCHEMA4_TEST_GAME");
    let native_smoke = required_path("NPC_REAL_SCHEMA4_NATIVE_SERVICE_SMOKE");
    let provider_evidence = required_path("NPC_REAL_SCHEMA4_PROVIDER_EVIDENCE");
    let evidence_root = required_path("NPC_REAL_SCHEMA4_JOIN_EVIDENCE_ROOT");
    assert!(
        worker.is_absolute() && worker.is_file(),
        "real worker is missing"
    );
    assert!(
        atlas_source.is_absolute() && atlas_source.is_dir(),
        "schema-four atlas source is missing"
    );
    assert!(
        test_game.is_absolute() && test_game.is_file(),
        "headless test game is missing"
    );
    assert!(
        native_smoke.is_absolute() && native_smoke.is_file(),
        "native D3D service smoke is missing"
    );
    assert!(
        provider_evidence.is_absolute() && provider_evidence.is_file(),
        "provider command-10 evidence is missing"
    );
    assert!(
        evidence_root.is_absolute() && !evidence_root.exists(),
        "join evidence root must be fresh"
    );
    std::fs::create_dir_all(&evidence_root).expect("fresh join evidence root");

    let provider_evidence_bytes =
        std::fs::read(&provider_evidence).expect("provider activation evidence");
    let provider_receipt: serde_json::Value =
        serde_json::from_slice(&provider_evidence_bytes).expect("provider evidence JSON");
    assert_eq!(provider_receipt["schemaVersion"], serde_json::json!(1));
    assert!(provider_receipt["receipt"]["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("provider load only")));
    assert_eq!(
        provider_receipt["receipt"]["installedContentTreeSha256"],
        provider_receipt["activeInventory"]["contentTreeSha256"]
    );
    assert!(provider_receipt["retryRejected"]
        .as_str()
        .is_some_and(|detail| detail.contains("not awaiting")));

    let test_game_report = evidence_root.join("test-game-self-test.json");
    let test_game_frames = evidence_root.join("test-game-frames");
    let test_game_arguments = [
        std::ffi::OsStr::new("--self-test-report"),
        test_game_report.as_os_str(),
        std::ffi::OsStr::new("--self-test-frame-directory"),
        test_game_frames.as_os_str(),
        std::ffi::OsStr::new("--mute"),
    ];
    let (test_game_process_id, test_game_output) = hidden_child(&test_game, &test_game_arguments);
    assert!(
        test_game_output.status.success(),
        "headless test game failed: stdout={} stderr={}",
        String::from_utf8_lossy(&test_game_output.stdout),
        String::from_utf8_lossy(&test_game_output.stderr)
    );
    let test_game_receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&test_game_report).expect("test-game self-test receipt"),
    )
    .expect("test-game self-test JSON");
    assert_eq!(test_game_receipt["status"], serde_json::json!("passed"));
    assert_eq!(
        test_game_receipt["source_mouth_articulation"],
        serde_json::json!(false)
    );
    assert_eq!(
        test_game_receipt["product_lip_sync"],
        serde_json::json!(false)
    );
    assert_eq!(
        test_game_receipt["third_party_binaries_loaded"],
        serde_json::json!(false)
    );

    let reviewed_atlas_root = evidence_root.join("reviewed-wire-fixture");
    let manifest = stage_exact_reviewed_fixture(&atlas_source, &reviewed_atlas_root);
    let game_profile_id = manifest["enrollmentBinding"]["gameProfileId"]
        .as_str()
        .expect("game profile id")
        .to_owned();
    let character_id = manifest["enrollmentBinding"]["characterId"]
        .as_str()
        .expect("character id")
        .to_owned();
    let actor_id = stable_nonzero_id(&format!("{game_profile_id}:{character_id}"));
    let requested = RequestedCharacterMouthAtlasIdentity {
        game_profile_id: &game_profile_id,
        character_id: &character_id,
        actor_id,
    };

    let catalog = crate::catalog::ResourceCatalog::new(None);
    let profile = catalog
        .load_debug_synthetic_review_profile()
        .expect("bundled canonical hidden review profile");
    assert_eq!(profile.id, game_profile_id);
    let selected =
        SelectedCharacterMouthAuthorityV1::from_persisted_selection(&profile, Some(&character_id))
            .expect("canonical persisted character selection");
    let atlas_manifest_text =
        std::fs::read_to_string(reviewed_atlas_root.join("atlas.json")).expect("atlas text");
    let texture_file_name = manifest["texture"]["file"]
        .as_str()
        .expect("texture file name")
        .to_owned();
    let texture_bytes =
        std::fs::read(reviewed_atlas_root.join(&texture_file_name)).expect("atlas pixels");
    let pack_files = CharacterMouthPackFilesV1 {
        game_profile_id: game_profile_id.clone(),
        atlas_json_text: atlas_manifest_text,
        texture_file_name,
        texture_base64: encode_base64(&texture_bytes),
    };
    let registry_root = evidence_root.join("isolated-mouth-pack-registry");
    let mouth_packs = CharacterMouthPackManagerV1::new(&registry_root)
        .expect("isolated character mouth-pack manager");
    let preview = mouth_packs
        .inspect(&selected, &pack_files)
        .expect("canonical reviewed pack preview");
    assert_eq!(
        preview.atlas_schema_version,
        CURRENT_PIXEL_ORAL_STRIP_ATLAS_SCHEMA
    );
    let installed = mouth_packs
        .import(
            &selected,
            ImportCharacterMouthPackRequestV1 {
                files: pack_files,
                expected_content_sha256: preview.content_sha256.clone(),
            },
            41,
        )
        .expect("isolated exact reviewed pack import");
    let actor_lock = NativeSelectedActorLockV1 {
        schema_version: 1,
        lock_generation: 1,
        provenance: NativeActorLockProvenanceV1::QualifiedIdentity {
            qualification_id: "schema4-join-test-only".into(),
            catalog_admission_sha256: "a".repeat(64),
            admission_receipt_sha256: "b".repeat(64),
        },
        game_profile_id: game_profile_id.clone(),
        capture_session_id: "schema4-join-test-only".into(),
        selected_process_id: test_game_process_id,
        selected_window_handle: 1,
        selected_executable_name: test_game
            .file_name()
            .expect("test game file name")
            .to_string_lossy()
            .into_owned(),
        character_id: character_id.clone(),
        selection_authority: NativeActorSelectionAuthorityV1::Explicit,
        actor_id: "schema4-join-test-only".into(),
        runtime_actor_id: actor_id,
        track_id: 1,
        track_epoch: 1,
        full_source_roi: NativeFullSourceRoiV1 {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            source_width: 960,
            source_height: 600,
        },
        appearance_hysteresis_latched: true,
        appearance_descriptor_revision: 1,
        expected_appearance_digest_high: 1,
        expected_appearance_digest_low: 2,
        observed_appearance_digest_high: 1,
        observed_appearance_digest_low: 2,
        appearance_similarity: 1.0,
        temporal_iou: 1.0,
        blocker_coverage: 0.0,
        scene_transition_detected: false,
        identity_confidence: 1.0,
        identity_margin: 1.0,
        source_content_sha256: "c".repeat(64),
        source_frame_sequence: 1,
        source_frame_qpc: 1,
        qpc_frequency: 1,
        captured_at_unix_ms: 41,
        device_generation: 1,
        geometry_epoch: 1,
        expires_at_unix_ms: 43,
        cancellation_generation: 41,
    };
    let enable_authority =
        CurrentCharacterMouthEnableAuthorityV1::from_current_actor(selected, &actor_lock, 42)
            .expect("exact selected-character lock match");
    let enabled = mouth_packs
        .enable(
            &enable_authority,
            EnableCharacterMouthPackRequestV1 {
                game_profile_id: game_profile_id.clone(),
                expected_content_sha256: installed.content_sha256.clone(),
            },
            42,
        )
        .expect("isolated exact pack enable");
    let resolved = mouth_packs
        .resolve_enabled(&game_profile_id, &character_id)
        .expect("isolated enabled pack lookup")
        .expect("isolated enabled pack");
    assert_eq!(resolved.content_sha256, enabled.content_sha256);
    assert_eq!(resolved.identity_revision, installed.identity_revision);
    assert_eq!(
        resolved.enrollment_binding_sha256,
        installed.enrollment_binding_sha256
    );
    let atlas = resolve_character_mouth_atlas(Some(&resolved.root), 41, &requested)
        .expect("registry-resolved reviewed schema-four atlas");
    assert_eq!(atlas.schema_version, CURRENT_PIXEL_ORAL_STRIP_ATLAS_SCHEMA);
    let exact_wire = encode_character_mouth_atlas(&atlas).expect("schema-four worker wire");
    let exact_wire_sha256 = sha256_hex(&exact_wire);

    let foreign_identity = RequestedCharacterMouthAtlasIdentity {
        game_profile_id: &game_profile_id,
        character_id: "foreign-character",
        actor_id: stable_nonzero_id("foreign-character"),
    };
    let foreign_identity_error =
        resolve_character_mouth_atlas(Some(&reviewed_atlas_root), 41, &foreign_identity)
            .expect_err("foreign semantic identity must fail before worker wire")
            .to_string();
    assert!(foreign_identity_error.contains("does not match"));

    let corrupt_root = evidence_root.join("corrupt-hash-fixture");
    let corrupt_manifest = stage_exact_reviewed_fixture(&atlas_source, &corrupt_root);
    let corrupt_texture = corrupt_root.join(
        corrupt_manifest["texture"]["file"]
            .as_str()
            .expect("corrupt texture name"),
    );
    let mut corrupt_bytes = std::fs::read(&corrupt_texture).expect("corrupt texture bytes");
    corrupt_bytes[0] ^= 1;
    std::fs::write(&corrupt_texture, corrupt_bytes).expect("write corrupt texture");
    let content_hash_error = resolve_character_mouth_atlas(Some(&corrupt_root), 41, &requested)
        .expect_err("changed texture must fail its manifest hash")
        .to_string();
    assert!(content_hash_error.contains("hash mismatch"));

    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root");
    let current = std::env::current_exe().expect("test executable");
    let parent = RuntimeSupervisor::try_new(crate::sidecar_supervisor::RuntimeLaunchConfig {
        executable: current.clone(),
        resource_root: repository,
        app_data: evidence_root.join("runtime-host-data"),
        development_fixture_allowed: true,
        application_namespace: interactive_npcs_credential_vault::PRODUCTION_APPLICATION_NAMESPACE
            .into(),
    })
    .expect("kill-on-close parent job");
    let broker = MediaBrokerSupervisor::new(
        crate::media_broker::MediaBrokerLaunchConfig {
            executable: current,
            development_fixture_allowed: false,
            audio_output_selection_path: evidence_root.join("audio-output-selection-v1.json"),
            #[cfg(debug_assertions)]
            debug_synthetic_metadata_path: evidence_root.join("unused-target.json"),
        },
        parent.clone(),
    );
    let visual = MouthWorkerSupervisor::new(
        MouthWorkerLaunchConfig {
            executable: worker.clone(),
            development_fixture_allowed: false,
            #[cfg(debug_assertions)]
            review_openseeface_root: None,
            #[cfg(debug_assertions)]
            review_mouth_atlas_root: None,
        },
        parent,
        broker,
    );
    let (client, worker_identity) = visual
        .ensure_ready(41)
        .await
        .expect("authenticated hidden mouth worker");
    client
        .install_atlas(41, exact_wire.clone())
        .await
        .expect("real worker accepts Rust schema-four atlas wire");

    let mut malformed_wire = exact_wire.clone();
    malformed_wire.pop();
    let malformed_wire_error = client
        .install_atlas(41, malformed_wire)
        .await
        .expect_err("real worker must reject truncated schema-four wire")
        .to_string();
    client
        .cancel_to(41, 42)
        .await
        .expect("authenticated generation cancellation");
    let stale_generation_error = client
        .install_atlas(41, exact_wire)
        .await
        .expect_err("cancelled generation must reject atlas replay")
        .to_string();
    let mut next_atlas = atlas.clone();
    next_atlas.cancellation_generation = 42;
    client
        .install_atlas(
            42,
            encode_character_mouth_atlas(&next_atlas).expect("next-generation schema-four wire"),
        )
        .await
        .expect("real worker accepts exact atlas at current generation");
    visual.shutdown().await;

    let native_smoke_arguments = [worker.as_os_str()];
    let (native_smoke_process_id, native_smoke_output) =
        hidden_child(&native_smoke, &native_smoke_arguments);
    let native_smoke_stdout = String::from_utf8_lossy(&native_smoke_output.stdout);
    let native_smoke_stderr = String::from_utf8_lossy(&native_smoke_output.stderr);
    assert!(
        native_smoke_output.status.success(),
        "native owned-texture service smoke failed: stdout={native_smoke_stdout} stderr={native_smoke_stderr}"
    );
    assert!(native_smoke_stdout
        .contains("PASS: authenticated cross-process D3D mouth-worker service lifecycle"));

    let receipt_path = evidence_root.join("schema4-native-join-receipt.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "status": "passed",
            "scope": "provider-receipt-plus-rust-schema4-wire-plus-owned-d3d-process-join",
            "providerActivation": {
                "evidencePath": provider_evidence,
                "evidenceSha256": sha256_hex(&provider_evidence_bytes),
                "attestationSha256": provider_receipt["receipt"]["attestationSha256"],
                "installedContentTreeSha256": provider_receipt["receipt"]["installedContentTreeSha256"],
                "providerLoadDurationMillis": provider_receipt["receipt"]["providerLoadDurationMillis"],
                "command10LoadAndUnloadProven": true
            },
            "testGame": {
                "processId": test_game_process_id,
                "receiptPath": test_game_report,
                "status": test_game_receipt["status"],
                "sourceMouthArticulation": false,
                "productLipSync": false,
                "visibleWindowCreated": false,
                "audioPlayed": false
            },
            "schema4Atlas": {
                "sourceRoot": atlas_source,
                "fixtureRoot": reviewed_atlas_root,
                "fixtureAuthority": "exact-reviewed-private-input-byte-for-byte",
                "gameProfileId": game_profile_id,
                "characterId": character_id,
                "actorId": actor_id,
                "identityRevision": atlas.identity_revision,
                "manifestSha256": atlas.manifest_sha256,
                "textureFileName": atlas.texture_file_name,
                "contentSha256": atlas.content_sha256,
                "wireSha256": exact_wire_sha256,
                "wireBytes": encode_character_mouth_atlas(&atlas).expect("receipt wire").len(),
                "refineSourceEdges": atlas.states.first().map(|state| state.refine_source_edges),
                "workerProcessId": worker_identity.process_id,
                "workerProcessCreationTime": worker_identity.process_creation_time,
                "exactInstallAccepted": true,
                "nextGenerationInstallAccepted": true,
                "malformedWireRejected": malformed_wire_error,
                "cancelledGenerationReplayRejected": stale_generation_error,
                "foreignSemanticIdentityRejected": foreign_identity_error,
                "changedTextureHashRejected": content_hash_error
            },
            "isolatedMouthPackRegistry": {
                "root": registry_root,
                "canonicalBundledProfileResolved": true,
                "persistedSelectionMatched": true,
                "testOnlyActorLockMatched": true,
                "imported": true,
                "enabled": true,
                "resolvedExactInstalledRevision": true,
                "contentSha256": installed.content_sha256,
                "enrollmentBindingSha256": installed.enrollment_binding_sha256,
                "productConfigurationChanged": false
            },
            "nativeOwnedTextureSmoke": {
                "processId": native_smoke_process_id,
                "status": "passed",
                "workerPath": worker,
                "smokePath": native_smoke,
                "d3dSourceWasTestOwned": true,
                "desktopCaptureUsed": false
            },
            "notProven": [
                "production selected-character actor-lock qualification",
                "fresh target-PID runtime loadout admission",
                "commercial-game WGC capture",
                "WASAPI playback envelope",
                "broker overlay presentation",
                "live-game lip-sync quality",
                "persistent product-config pack enablement"
            ]
        }))
        .expect("join receipt JSON"),
    )
    .expect("write join receipt");
    assert!(receipt_path.is_file());
}
