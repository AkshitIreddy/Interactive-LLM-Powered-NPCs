#![allow(clippy::unwrap_used)]

use model_manager::*;
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;
use std::io::Cursor;

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::of_bytes(bytes)
}

fn catalog_binding(manifest: &ModelPackManifestV1) -> CatalogInstallBindingV1 {
    CatalogInstallBindingV1 {
        catalog_payload_sha256: digest(
            format!("fixture-catalog:{}", manifest.digest().unwrap()).as_bytes(),
        ),
        catalog_version: 1,
        trust_domain: CatalogTrustDomainV1::ReleaseThreshold,
    }
}

fn selection_authorization(manifest: &ModelPackManifestV1) -> PackSelectionAuthorizationV1 {
    selection_authorization_with_action(manifest, PackSelectionActionV1::InstallAndActivate)
}

fn selection_authorization_with_action(
    manifest: &ModelPackManifestV1,
    action: PackSelectionActionV1,
) -> PackSelectionAuthorizationV1 {
    ApiFirstPackSelectionPolicyV1
        .authorize(
            manifest,
            PackSelectionRequestV1 {
                selection_id: format!("test-selection-{}-{}", manifest.pack_id, manifest.revision),
                origin: PackSelectionOriginV1::ExplicitUser,
                action,
                selected_unix_seconds: 1,
            },
        )
        .unwrap()
}

struct FixtureAttestationVerifier;

impl SelfTestAttestationVerifier for FixtureAttestationVerifier {
    fn verify(
        &self,
        claimed_runner_id: &str,
        proof: &AttestationProofV1,
        _canonical_payload: &[u8],
    ) -> Result<VerifiedRunnerIdentity, AttestationVerificationError> {
        if claimed_runner_id != "fixture-runner" {
            return Err(AttestationVerificationError::UnknownRunner);
        }
        if proof.algorithm != "fixture-proof" || proof.value != "valid" {
            return Err(AttestationVerificationError::InvalidProof);
        }
        Ok(VerifiedRunnerIdentity {
            runner_id: claimed_runner_id.to_owned(),
            trust_revision: "fixture-trust-v1".to_owned(),
        })
    }
}

fn fixture_attestation(
    challenge: SelfTestChallengeV1,
    outcome: AttestedSelfTestOutcomeV1,
) -> SelfTestAttestationV1 {
    SelfTestAttestationV1 {
        signed: SelfTestAttestationPayloadV1 {
            schema: SELF_TEST_ATTESTATION_SCHEMA_V1.to_owned(),
            challenge,
            runner_id: "fixture-runner".to_owned(),
            outcome,
        },
        proof: AttestationProofV1 {
            algorithm: "fixture-proof".to_owned(),
            value: "valid".to_owned(),
        },
    }
}

fn sample_manifest(pack: &str, revision: &str, bytes: &[u8]) -> ModelPackManifestV1 {
    ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: PackId::parse(pack).unwrap(),
        revision: Revision::parse(revision).unwrap(),
        display_name: "Test speech model".to_owned(),
        description: "A deterministic fixture pack for lifecycle tests.".to_owned(),
        source_project: "https://example.test/upstream".to_owned(),
        source_revision: format!("sha256-{}", &digest(bytes).as_str()[..16]),
        capability: ModelPackCapabilityV1 {
            kind: ModelPackKindV1::LipSync,
            scope: ModelPackScopeV1::Generic,
        },
        artifacts: vec![ArtifactV1 {
            id: "weights".to_owned(),
            kind: ArtifactKind::File,
            archive_format: None,
            source_urls: vec!["https://models.example.test/weights.bin".to_owned()],
            size_bytes: bytes.len() as u64,
            sha256: digest(bytes),
            destination: "models/weights.bin".to_owned(),
            strip_prefix: None,
            required_paths: vec![],
        }],
        runtime: RuntimeCompatibilityV1 {
            runtime: "onnxruntime".to_owned(),
            abi: "npc-onnx-v1".to_owned(),
            minimum_runtime_revision: Some("1.20.0".to_owned()),
            supported_platforms: BTreeSet::from([Platform::Windows10, Platform::Windows11]),
            supported_architectures: BTreeSet::from([Architecture::X86_64]),
        },
        hardware: HardwareRequirementsV1 {
            minimum_ram_bytes: 2_000_000_000,
            recommended_ram_bytes: 4_000_000_000,
            minimum_vram_bytes: None,
            recommended_vram_bytes: None,
            minimum_cpu_threads: 2,
            accelerators: BTreeSet::from([Accelerator::Cpu]),
            required_cpu_features: BTreeSet::new(),
        },
        resources: ResourceEnvelopeV1 {
            storage_bytes: bytes.len() as u64,
            peak_install_bytes: bytes.len() as u64 * 2,
            measured_ram_bytes: Some(700_000_000),
            measured_vram_bytes: None,
            measured_load_millis: Some(100),
            benchmark_hardware: Some("fixture".to_owned()),
            quality_tier: QualityTier::Fast,
            languages: BTreeSet::from(["en".to_owned()]),
        },
        license: LicenseMetadataV1 {
            spdx_expression: Some("Apache-2.0".to_owned()),
            license_name: "Apache License 2.0".to_owned(),
            license_url: "https://www.apache.org/licenses/LICENSE-2.0".to_owned(),
            attribution: "Fixture Authors".to_owned(),
            redistributable: true,
            commercial_use: LicensePermission::Allowed,
            derivative_use: LicensePermission::Allowed,
            acceptance_required: false,
            notices: vec![],
        },
        self_test: SelfTestV1 {
            kind: "sha256_fixture".to_owned(),
            suite_revision: "fixture-v1".to_owned(),
            allowed_runtime_backends: BTreeSet::from(["mock-runtime".to_owned()]),
            input_fixture: "tests/input.json".to_owned(),
            expected_output_sha256: Some(digest(b"self-test-ok")),
            timeout_millis: 10_000,
        },
    }
}

struct FixtureMeasurementVerifier;

impl CatalogSignatureVerifier for FixtureMeasurementVerifier {
    fn is_trusted_key(&self, key_id: &str) -> bool {
        key_id == "fixture-measurement-key"
    }

    fn verify(&self, _key_id: &str, algorithm: &str, _message: &[u8], signature: &str) -> bool {
        algorithm == "fixture" && signature == "valid"
    }
}

fn measured_admission(manifest: &ModelPackManifestV1) -> LoadoutAdmissionV1 {
    let device = digest(b"fixture-local-device");
    let envelope = SignedMeasuredResourceEnvelopeV1 {
        signed: MeasuredResourceEnvelopePayloadV1 {
            schema: MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1.to_owned(),
            report_id: "fixture-loadout-measurement".to_owned(),
            sequence: 1,
            measured_unix_seconds: 100,
            expires_unix_seconds: 1_000,
            device_fingerprint_sha256: device.clone(),
            identity: manifest.identity(),
            manifest_sha256: manifest.digest().unwrap(),
            capability: manifest.capability.kind.clone(),
            benchmark_suite_revision: "fixture-suite-v1".to_owned(),
            runtime: "onnxruntime".to_owned(),
            runtime_revision: "r1".to_owned(),
            backend: "cpu".to_owned(),
            sample_count: 20,
            placements: std::collections::BTreeMap::from([(
                ResidencyModeV1::CpuResident,
                PlacementMeasurementV1 {
                    resident_ram_bytes: 700_000_000,
                    p99_total_ram_bytes: 800_000_000,
                    resident_vram_bytes: 0,
                    p99_workspace_vram_bytes: 0,
                    p99_load_millis: 100,
                    p99_reload_millis: 80,
                    p99_operation_millis: 20,
                },
            )]),
        },
        signatures: vec![CatalogSignatureV1 {
            key_id: "fixture-measurement-key".to_owned(),
            algorithm: "fixture".to_owned(),
            signature: "valid".to_owned(),
        }],
    };
    let (verified, _) = verify_measured_resource_envelope(
        &envelope,
        &FixtureMeasurementVerifier,
        &MeasurementTrustPolicyV1 {
            signature_threshold: 1,
            minimum_samples: 20,
            maximum_lifetime_seconds: 2_000,
            maximum_clock_skew_seconds: 10,
        },
        &MeasurementTrustStateV1::default(),
        200,
    )
    .unwrap();
    let governor = ResourceGovernorV1::new(ResourceGovernorPolicyV1 {
        schema: RESOURCE_GOVERNOR_POLICY_SCHEMA_V1.to_owned(),
        vram_soft_ceiling_basis_points: 9_000,
        ram_soft_ceiling_basis_points: 9_000,
        minimum_vram_safety_bytes: 1_000_000_000,
        proportional_vram_safety_basis_points: 1_000,
        minimum_ram_safety_bytes: 2_000_000_000,
        maximum_snapshot_age_millis: 100,
        keep_warm_millis: 30_000,
        unload_ttl_millis: 120_000,
    })
    .unwrap();
    governor
        .admit(
            &LiveResourceSnapshotV1 {
                captured_monotonic_millis: 1_000,
                device_fingerprint_sha256: device,
                physical_vram_bytes: 12_000_000_000,
                os_vram_budget_bytes: 12_000_000_000,
                desktop_resident_vram_bytes: 500_000_000,
                game_resident_vram_bytes: 5_000_000_000,
                game_reserve_vram_bytes: 7_000_000_000,
                physical_ram_bytes: 32_000_000_000,
                available_ram_bytes: 20_000_000_000,
                game_additional_reserve_ram_bytes: 2_000_000_000,
            },
            1_050,
            &[LoadoutModelRequestV1 {
                role: manifest.capability.kind.clone(),
                mode: ResidencyModeV1::CpuResident,
                envelope: &verified,
            }],
        )
        .unwrap()
}

fn finish_install(
    manager: &mut ModelPackManager<InMemoryPackStorage>,
    manifest: ModelPackManifestV1,
    bytes: &[u8],
) -> PackRevision {
    let identity = if manager.active_revision(&manifest.pack_id).is_some() {
        manager
            .begin_update(
                manifest.clone(),
                catalog_binding(&manifest),
                selection_authorization(&manifest),
                true,
            )
            .unwrap()
    } else {
        manager
            .begin_install(
                manifest.clone(),
                catalog_binding(&manifest),
                selection_authorization(&manifest),
                true,
            )
            .unwrap()
    };
    {
        let journal = manager.download_journal_mut(&identity, "weights").unwrap();
        journal
            .accept_response(ResumeResponse {
                status: 200,
                range_start: None,
                total_size: bytes.len() as u64,
                validator: Some("\"immutable-etag\"".to_owned()),
            })
            .unwrap();
        journal.record_bytes(bytes.len() as u64).unwrap();
    }
    let evidence = verify_artifact(
        "weights",
        bytes.len() as u64,
        &digest(bytes),
        Cursor::new(bytes),
    )
    .unwrap();
    manager
        .accept_verified_artifact(&identity, evidence)
        .unwrap();
    manager.stage(&identity).unwrap();
    let challenge = manager
        .issue_self_test_challenge(&identity, "mock-runtime", 100, 60)
        .unwrap();
    manager
        .record_self_test_attestation(
            &identity,
            fixture_attestation(
                challenge,
                AttestedSelfTestOutcomeV1 {
                    passed: true,
                    duration_millis: 20,
                    output_sha256: Some(digest(b"self-test-ok")),
                    diagnostic_code: None,
                },
            ),
            &FixtureAttestationVerifier,
            120,
        )
        .unwrap();
    manager.activate(&identity).unwrap();
    identity
}

fn prepare_staged_manager() -> (ModelPackManager<InMemoryPackStorage>, PackRevision) {
    prepare_staged_manager_with_action(PackSelectionActionV1::InstallAndActivate)
}

fn prepare_staged_manager_with_action(
    action: PackSelectionActionV1,
) -> (ModelPackManager<InMemoryPackStorage>, PackRevision) {
    let bytes = b"attestation-weights";
    let manifest = sample_manifest("attestation.fixture", "r1", bytes);
    let mut manager = ModelPackManager::new(InMemoryPackStorage::default());
    let identity = manager
        .begin_install(
            manifest.clone(),
            catalog_binding(&manifest),
            selection_authorization_with_action(&manifest, action),
            true,
        )
        .unwrap();
    let journal = manager.download_journal_mut(&identity, "weights").unwrap();
    journal
        .accept_response(ResumeResponse {
            status: 200,
            range_start: None,
            total_size: bytes.len() as u64,
            validator: Some("\"attestation-etag\"".to_owned()),
        })
        .unwrap();
    journal.record_bytes(bytes.len() as u64).unwrap();
    manager
        .accept_verified_artifact(
            &identity,
            ArtifactEvidence {
                artifact_id: "weights".to_owned(),
                size_bytes: bytes.len() as u64,
                sha256: digest(bytes),
            },
        )
        .unwrap();
    manager.stage(&identity).unwrap();
    (manager, identity)
}

#[test]
fn manifest_rejects_moving_revision_insecure_urls_and_underreported_size() {
    let mut manifest = sample_manifest("speech.fixture", "r1", b"weights");
    manifest.source_revision = "latest".to_owned();
    assert!(matches!(
        manifest.validate(),
        Err(ManifestError::MutableSourceRevision(_))
    ));

    let mut manifest = sample_manifest("speech.fixture", "r1", b"weights");
    manifest.artifacts[0].source_urls[0] = "http://models.example.test/file".to_owned();
    assert!(matches!(
        manifest.validate(),
        Err(ManifestError::InsecureUrl(_))
    ));

    let mut manifest = sample_manifest("speech.fixture", "r1", b"weights");
    manifest.resources.storage_bytes = 1;
    assert!(matches!(
        manifest.validate(),
        Err(ManifestError::StorageUnderreported { .. })
    ));
}

#[test]
fn api_first_policy_allows_only_explicit_generic_lip_sync_selection() {
    let policy = ApiFirstPackSelectionPolicyV1;
    let manifest = sample_manifest("selection.policy", "r1", b"weights");
    let explicit = PackSelectionRequestV1 {
        selection_id: "selection-explicit-001".to_owned(),
        origin: PackSelectionOriginV1::ExplicitUser,
        action: PackSelectionActionV1::InstallAndActivate,
        selected_unix_seconds: 1,
    };
    assert!(policy.authorize(&manifest, explicit.clone()).is_ok());

    for origin in [
        PackSelectionOriginV1::AutomaticRecommendation,
        PackSelectionOriginV1::DefaultSelection,
        PackSelectionOriginV1::DependencyResolution,
        PackSelectionOriginV1::Migration,
    ] {
        let mut request = explicit.clone();
        request.origin = origin.clone();
        assert_eq!(
            policy.authorize(&manifest, request),
            Err(PackSelectionError::ExplicitUserSelectionRequired(origin))
        );
    }

    for kind in [
        ModelPackKindV1::LanguageModel,
        ModelPackKindV1::SpeechRecognition,
        ModelPackKindV1::SpeechSynthesis,
        ModelPackKindV1::Embedding,
        ModelPackKindV1::Vision,
        ModelPackKindV1::Animation,
        ModelPackKindV1::Other,
    ] {
        let mut blocked = manifest.clone();
        blocked.capability.kind = kind.clone();
        assert_eq!(
            policy.authorize(&blocked, explicit.clone()),
            Err(PackSelectionError::LocalInferenceKindBlocked(kind))
        );
    }

    let mut game_specific = manifest;
    game_specific.capability.scope = ModelPackScopeV1::GameSpecific;
    assert_eq!(
        policy.authorize(&game_specific, explicit),
        Err(PackSelectionError::GenericLipSyncRequired)
    );
}

#[test]
fn measured_local_policy_requires_complete_loadout_wide_fit_before_activation() {
    let policy = MeasuredLocalPackSelectionPolicyV1;
    let manifest = sample_manifest("local.language", "r1", b"weights");
    let request = PackSelectionRequestV1 {
        selection_id: "selection-local-001".to_owned(),
        origin: PackSelectionOriginV1::ExplicitUser,
        action: PackSelectionActionV1::InstallAndActivate,
        selected_unix_seconds: 10,
    };
    for kind in [
        ModelPackKindV1::LanguageModel,
        ModelPackKindV1::SpeechRecognition,
        ModelPackKindV1::SpeechSynthesis,
        ModelPackKindV1::Embedding,
        ModelPackKindV1::Vision,
        ModelPackKindV1::LipSync,
    ] {
        let mut selected = manifest.clone();
        selected.capability.kind = kind;
        let admission = measured_admission(&selected);
        let authorization = policy
            .authorize(&selected, request.clone(), Some(&admission))
            .expect("measured local pack should be authorized");
        assert!(authorization.activation_allowed());
        authorization
            .validate_for_manifest(&selected)
            .expect("authorization remains bound to the manifest and fit policy");
    }

    assert_eq!(
        policy.authorize(&manifest, request.clone(), None),
        Err(PackSelectionError::IncompleteLoadoutFitEvidence)
    );

    let different = sample_manifest("local.different", "r1", b"weights");
    let unrelated_admission = measured_admission(&different);
    assert_eq!(
        policy.authorize(&manifest, request.clone(), Some(&unrelated_admission)),
        Err(PackSelectionError::LoadoutAdmissionDoesNotCoverManifest)
    );

    let mut install_only = request;
    install_only.action = PackSelectionActionV1::InstallOnly;
    let authorization = policy
        .authorize(&manifest, install_only, None)
        .expect("an explicit install may precede device qualification");
    assert!(!authorization.activation_allowed());
}

#[test]
fn identifiers_are_deliberately_narrow() {
    for bad in ["UPPER", "../escape", "x", "a..b", "a/b", "space id"] {
        assert!(PackId::parse(bad).is_err(), "accepted pack id {bad:?}");
    }
    for bad in ["", "../r", "latest/r", "r 1"] {
        assert!(Revision::parse(bad).is_err(), "accepted revision {bad:?}");
    }
}

#[test]
fn archive_paths_reject_windows_and_posix_escape_techniques() {
    let attacks = [
        "../evil",
        "a/../../evil",
        r"a\..\evil",
        "/absolute",
        r"C:\evil",
        r"\\server\share",
        "file.txt:payload",
        "CON",
        "aux.txt",
        "folder/NUL.bin",
        "trailing. ",
        "a//b",
        "a/./b",
        "question?.bin",
        "pipe|name",
    ];
    for attack in attacks {
        assert!(
            validate_relative_archive_path(attack).is_err(),
            "accepted hostile path {attack:?}"
        );
    }
    assert_eq!(
        validate_relative_archive_path(r"models\voice\weights.onnx")
            .unwrap()
            .as_str(),
        "models/voice/weights.onnx"
    );
}

#[test]
fn archive_preflight_blocks_links_bombs_case_collisions_and_file_parents() {
    let policy = ArchivePolicy {
        max_entries: 5,
        max_uncompressed_bytes: 100,
        max_single_file_bytes: 80,
    };
    assert!(matches!(
        validate_archive_entries(
            [ArchiveEntry {
                path: "link",
                kind: ArchiveEntryKind::Symlink,
                uncompressed_size: 0
            }],
            &policy,
        ),
        Err(ArchiveValidationError::UnsupportedEntryType { .. })
    ));
    assert!(matches!(
        validate_archive_entries(
            [
                ArchiveEntry {
                    path: "A.bin",
                    kind: ArchiveEntryKind::File,
                    uncompressed_size: 1
                },
                ArchiveEntry {
                    path: "a.BIN",
                    kind: ArchiveEntryKind::File,
                    uncompressed_size: 1
                },
            ],
            &policy,
        ),
        Err(ArchiveValidationError::CaseFoldCollision(_))
    ));
    assert!(matches!(
        validate_archive_entries(
            [
                ArchiveEntry {
                    path: "model",
                    kind: ArchiveEntryKind::File,
                    uncompressed_size: 1
                },
                ArchiveEntry {
                    path: "model/weights",
                    kind: ArchiveEntryKind::File,
                    uncompressed_size: 1
                },
            ],
            &policy,
        ),
        Err(ArchiveValidationError::FileAsParent(_))
    ));
    assert!(matches!(
        validate_archive_entries(
            [
                ArchiveEntry {
                    path: "one",
                    kind: ArchiveEntryKind::File,
                    uncompressed_size: 60
                },
                ArchiveEntry {
                    path: "two",
                    kind: ArchiveEntryKind::File,
                    uncompressed_size: 60
                },
            ],
            &policy,
        ),
        Err(ArchiveValidationError::SizeLimit)
    ));
}

#[test]
fn verification_is_streaming_and_checks_size_before_digest() {
    let bytes = b"trusted model bytes";
    assert!(verify_artifact("a", bytes.len() as u64, &digest(bytes), Cursor::new(bytes)).is_ok());
    assert!(matches!(
        verify_artifact("a", 3, &digest(bytes), Cursor::new(bytes)),
        Err(VerificationError::SizeMismatch { .. })
    ));
    assert!(matches!(
        verify_artifact(
            "a",
            bytes.len() as u64,
            &digest(b"other model bytes"),
            Cursor::new(bytes)
        ),
        Err(VerificationError::DigestMismatch { .. })
    ));
}

#[test]
fn resume_requires_exact_range_stable_validator_and_size() {
    let mut journal = DownloadJournal::new("weights".to_owned(), 10, digest(b"0123456789"));
    journal
        .accept_response(ResumeResponse {
            status: 200,
            range_start: None,
            total_size: 10,
            validator: Some("v1".to_owned()),
        })
        .unwrap();
    journal.record_bytes(4).unwrap();
    assert_eq!(
        journal.resume_request(),
        ResumeRequest {
            offset: 4,
            if_range: Some("v1".to_owned())
        }
    );
    assert_eq!(
        journal.accept_response(ResumeResponse {
            status: 206,
            range_start: Some(4),
            total_size: 10,
            validator: Some("v2".to_owned()),
        }),
        Err(ResumeError::ValidatorChanged)
    );
    assert_eq!(
        journal.accept_response(ResumeResponse {
            status: 200,
            range_start: None,
            total_size: 10,
            validator: Some("v1".to_owned()),
        }),
        Err(ResumeError::RangeNotHonored)
    );
    assert_eq!(
        journal.accept_response(ResumeResponse {
            status: 206,
            range_start: Some(4),
            total_size: 11,
            validator: Some("v1".to_owned()),
        }),
        Err(ResumeError::RemoteSizeChanged {
            expected: 10,
            actual: 11
        })
    );
}

#[test]
fn complete_lifecycle_updates_rolls_back_and_protects_references() {
    let mut manager = ModelPackManager::new(InMemoryPackStorage::default());
    let r1 = finish_install(
        &mut manager,
        sample_manifest("speech.fixture", "r1", b"weights-v1"),
        b"weights-v1",
    );
    assert_eq!(manager.record(&r1).unwrap().state, InstallState::Active);

    let r2 = finish_install(
        &mut manager,
        sample_manifest("speech.fixture", "r2", b"weights-v2"),
        b"weights-v2",
    );
    assert_eq!(manager.active_revision(&r2.pack_id), Some(&r2.revision));
    manager
        .add_reference(&r2.pack_id, "game-session:42")
        .unwrap();
    assert!(matches!(
        manager.rollback(&r2.pack_id),
        Err(ManagerError::PackInUse(1))
    ));
    assert!(matches!(
        manager.remove(&r2.pack_id, RemovalPolicy::RequireUnreferenced),
        Err(ManagerError::PackInUse(1))
    ));
    assert!(manager
        .release_reference(&r2.pack_id, "game-session:42")
        .unwrap());
    assert_eq!(manager.rollback(&r2.pack_id).unwrap(), r1);
    assert_eq!(manager.record(&r2).unwrap().state, InstallState::RolledBack);
    assert_eq!(manager.record(&r1).unwrap().state, InstallState::Active);
    assert_eq!(
        manager
            .remove(&r1.pack_id, RemovalPolicy::RequireUnreferenced)
            .unwrap(),
        2
    );
}

#[test]
fn failed_self_test_quarantines_and_never_activates() {
    let bytes = b"weights";
    let manifest = sample_manifest("speech.fixture", "r1", bytes);
    let mut manager = ModelPackManager::new(InMemoryPackStorage::default());
    let binding = catalog_binding(&manifest);
    let selection = selection_authorization(&manifest);
    let id = manager
        .begin_install(manifest, binding, selection, true)
        .unwrap();
    let journal = manager.download_journal_mut(&id, "weights").unwrap();
    journal
        .accept_response(ResumeResponse {
            status: 200,
            range_start: None,
            total_size: bytes.len() as u64,
            validator: Some("v1".to_owned()),
        })
        .unwrap();
    journal.record_bytes(bytes.len() as u64).unwrap();
    let evidence = verify_artifact(
        "weights",
        bytes.len() as u64,
        &digest(bytes),
        Cursor::new(bytes),
    )
    .unwrap();
    manager.accept_verified_artifact(&id, evidence).unwrap();
    manager.stage(&id).unwrap();
    let challenge = manager
        .issue_self_test_challenge(&id, "mock-runtime", 100, 60)
        .unwrap();
    assert!(matches!(
        manager.record_self_test_attestation(
            &id,
            fixture_attestation(
                challenge,
                AttestedSelfTestOutcomeV1 {
                    passed: true,
                    duration_millis: 1,
                    output_sha256: Some(digest(b"tampered output")),
                    diagnostic_code: None,
                },
            ),
            &FixtureAttestationVerifier,
            120,
        ),
        Err(ManagerError::SelfTestFailed)
    ));
    assert!(matches!(
        manager.record(&id).unwrap().state,
        InstallState::Quarantined {
            phase: InstallPhase::SelfTest,
            ..
        }
    ));
    assert!(manager.active_revision(&id.pack_id).is_none());
}

#[test]
fn activation_requires_a_verified_attestation_and_rejects_replay() {
    let (mut manager, identity) = prepare_staged_manager();
    assert!(matches!(
        manager.activate(&identity),
        Err(ManagerError::InvalidState(InstallState::AwaitingSelfTest))
    ));
    let challenge = manager
        .issue_self_test_challenge(&identity, "mock-runtime", 100, 60)
        .unwrap();
    let attestation = fixture_attestation(
        challenge,
        AttestedSelfTestOutcomeV1 {
            passed: true,
            duration_millis: 1,
            output_sha256: Some(digest(b"self-test-ok")),
            diagnostic_code: None,
        },
    );
    manager
        .record_self_test_attestation(
            &identity,
            attestation.clone(),
            &FixtureAttestationVerifier,
            120,
        )
        .unwrap();
    assert!(matches!(
        manager.record_self_test_attestation(
            &identity,
            attestation,
            &FixtureAttestationVerifier,
            120,
        ),
        Err(ManagerError::AttestationReplay)
    ));
    manager.activate(&identity).unwrap();
}

#[test]
fn install_only_selection_cannot_be_promoted_to_default_activation() {
    let (mut manager, identity) =
        prepare_staged_manager_with_action(PackSelectionActionV1::InstallOnly);
    let challenge = manager
        .issue_self_test_challenge(&identity, "mock-runtime", 100, 60)
        .unwrap();
    manager
        .record_self_test_attestation(
            &identity,
            fixture_attestation(
                challenge,
                AttestedSelfTestOutcomeV1 {
                    passed: true,
                    duration_millis: 1,
                    output_sha256: Some(digest(b"self-test-ok")),
                    diagnostic_code: None,
                },
            ),
            &FixtureAttestationVerifier,
            120,
        )
        .unwrap();
    assert!(matches!(
        manager.activate(&identity),
        Err(ManagerError::Selection(
            PackSelectionError::ActivationNotAuthorized
        ))
    ));
    assert!(manager.active_revision(&identity.pack_id).is_none());
}

#[test]
fn attestation_rejects_expiry_unknown_runner_invalid_proof_and_binding_mismatch() {
    let (mut manager, identity) = prepare_staged_manager();
    let challenge = manager
        .issue_self_test_challenge(&identity, "mock-runtime", 100, 60)
        .unwrap();
    let valid_outcome = AttestedSelfTestOutcomeV1 {
        passed: true,
        duration_millis: 1,
        output_sha256: Some(digest(b"self-test-ok")),
        diagnostic_code: None,
    };
    assert!(matches!(
        manager.record_self_test_attestation(
            &identity,
            fixture_attestation(challenge.clone(), valid_outcome.clone()),
            &FixtureAttestationVerifier,
            161,
        ),
        Err(ManagerError::AttestationExpired)
    ));

    let mut unknown_runner = fixture_attestation(challenge.clone(), valid_outcome.clone());
    unknown_runner.signed.runner_id = "unknown-runner".to_owned();
    assert!(matches!(
        manager.record_self_test_attestation(
            &identity,
            unknown_runner,
            &FixtureAttestationVerifier,
            120,
        ),
        Err(ManagerError::AttestationVerification(
            AttestationVerificationError::UnknownRunner
        ))
    ));

    let mut unsigned = fixture_attestation(challenge.clone(), valid_outcome.clone());
    unsigned.proof.value = "forged".to_owned();
    assert!(matches!(
        manager
            .record_self_test_attestation(&identity, unsigned, &FixtureAttestationVerifier, 120,),
        Err(ManagerError::AttestationVerification(
            AttestationVerificationError::InvalidProof
        ))
    ));

    let mut mismatched = fixture_attestation(challenge, valid_outcome);
    mismatched.signed.challenge.catalog.catalog_version = 999;
    assert!(matches!(
        manager.record_self_test_attestation(
            &identity,
            mismatched,
            &FixtureAttestationVerifier,
            120,
        ),
        Err(ManagerError::AttestationChallengeMismatch)
    ));
}

#[test]
fn explicit_license_acceptance_is_enforced() {
    let mut manifest = sample_manifest("speech.fixture", "r1", b"weights");
    manifest.license.acceptance_required = true;
    let mut manager = ModelPackManager::new(InMemoryPackStorage::default());
    assert!(matches!(
        manager.begin_install(
            manifest.clone(),
            catalog_binding(&manifest),
            selection_authorization(&manifest),
            false,
        ),
        Err(ManagerError::LicenseAcceptanceRequired)
    ));
}

struct FixtureVerifier;

impl CatalogSignatureVerifier for FixtureVerifier {
    fn is_trusted_key(&self, key_id: &str) -> bool {
        matches!(key_id, "root-a" | "root-b")
    }
    fn verify(&self, _key_id: &str, algorithm: &str, _message: &[u8], signature: &str) -> bool {
        algorithm == "fixture-sha256" && signature == "valid"
    }
}

fn signed_catalog(version: u64, manifest: ModelPackManifestV1) -> SignedCatalogV1 {
    let entry = CatalogEntryV1 {
        manifest_sha256: manifest.digest().unwrap(),
        manifest,
        channels: BTreeSet::from(["stable".to_owned()]),
        published_unix_seconds: 1_000,
        revoked: false,
        revocation_reason: None,
    };
    SignedCatalogV1 {
        signed: CatalogPayloadV1 {
            schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
            version,
            generated_unix_seconds: 1_000,
            expires_unix_seconds: 2_000,
            entries: vec![entry],
        },
        signatures: vec![
            CatalogSignatureV1 {
                key_id: "root-a".to_owned(),
                algorithm: "fixture-sha256".to_owned(),
                signature: "valid".to_owned(),
            },
            CatalogSignatureV1 {
                key_id: "root-b".to_owned(),
                algorithm: "fixture-sha256".to_owned(),
                signature: "valid".to_owned(),
            },
        ],
    }
}

#[test]
fn catalog_enforces_threshold_expiration_and_rollback_protection() {
    let manifest = sample_manifest("speech.fixture", "r1", b"weights");
    let policy = CatalogTrustPolicy {
        signature_threshold: 2,
        maximum_lifetime_seconds: 2_000,
        maximum_clock_skew_seconds: 10,
    };
    let catalog = signed_catalog(5, manifest.clone());
    let (trusted, state) = verify_catalog(
        &catalog,
        &FixtureVerifier,
        &policy,
        &CatalogTrustState::default(),
        1_500,
    )
    .unwrap();
    assert!(trusted.installable_entry(&manifest.identity()).is_ok());

    let mut too_few = signed_catalog(6, sample_manifest("speech.fixture", "r2", b"new"));
    too_few.signatures.truncate(1);
    assert!(matches!(
        verify_catalog(&too_few, &FixtureVerifier, &policy, &state, 1_500),
        Err(CatalogError::SignatureThreshold { .. })
    ));
    assert!(matches!(
        verify_catalog(
            &signed_catalog(4, manifest.clone()),
            &FixtureVerifier,
            &policy,
            &state,
            1_500
        ),
        Err(CatalogError::Rollback { .. })
    ));
    assert!(matches!(
        verify_catalog(
            &catalog,
            &FixtureVerifier,
            &policy,
            &CatalogTrustState::default(),
            2_001
        ),
        Err(CatalogError::Expired)
    ));
}

#[test]
fn catalog_rejects_equivocation_and_changed_immutable_revision() {
    let policy = CatalogTrustPolicy {
        signature_threshold: 2,
        maximum_lifetime_seconds: 2_000,
        maximum_clock_skew_seconds: 10,
    };
    let original = sample_manifest("speech.fixture", "r1", b"weights");
    let first = signed_catalog(5, original.clone());
    let (_, state) = verify_catalog(
        &first,
        &FixtureVerifier,
        &policy,
        &CatalogTrustState::default(),
        1_500,
    )
    .unwrap();

    let same_version_other_payload =
        signed_catalog(5, sample_manifest("speech.fixture", "r2", b"new"));
    assert!(matches!(
        verify_catalog(
            &same_version_other_payload,
            &FixtureVerifier,
            &policy,
            &state,
            1_500
        ),
        Err(CatalogError::VersionEquivocation(5))
    ));

    let changed = sample_manifest("speech.fixture", "r1", b"tampered weights");
    let changed_catalog = signed_catalog(6, changed);
    assert!(matches!(
        verify_catalog(&changed_catalog, &FixtureVerifier, &policy, &state, 1_500),
        Err(CatalogError::ImmutableRevisionChanged(_))
    ));
}

#[test]
fn revoked_catalog_entry_cannot_be_installed() {
    let manifest = sample_manifest("speech.fixture", "r1", b"weights");
    let mut catalog = signed_catalog(1, manifest.clone());
    catalog.signed.entries[0].revoked = true;
    catalog.signed.entries[0].revocation_reason = Some("upstream compromise".to_owned());
    let policy = CatalogTrustPolicy {
        signature_threshold: 2,
        maximum_lifetime_seconds: 2_000,
        maximum_clock_skew_seconds: 10,
    };
    let (trusted, _) = verify_catalog(
        &catalog,
        &FixtureVerifier,
        &policy,
        &CatalogTrustState::default(),
        1_500,
    )
    .unwrap();
    assert!(matches!(
        trusted.installable_entry(&manifest.identity()),
        Err(CatalogError::Revoked(_))
    ));
}
