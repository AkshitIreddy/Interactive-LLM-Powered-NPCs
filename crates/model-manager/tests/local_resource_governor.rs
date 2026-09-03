#![allow(clippy::unwrap_used)]

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use model_manager::*;
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct AcceptFixtureSignatures;

impl CatalogSignatureVerifier for AcceptFixtureSignatures {
    fn is_trusted_key(&self, key_id: &str) -> bool {
        matches!(key_id, "fixture-a" | "fixture-b")
    }

    fn verify(&self, _key_id: &str, algorithm: &str, _message: &[u8], signature: &str) -> bool {
        algorithm == "fixture" && signature == "valid"
    }
}

fn digest(label: &str) -> Sha256Digest {
    Sha256Digest::of_bytes(label.as_bytes())
}

fn identity(pack: &str) -> PackRevision {
    PackRevision {
        pack_id: PackId::parse(pack).unwrap(),
        revision: Revision::parse("r1").unwrap(),
    }
}

fn measurement(
    identity: PackRevision,
    manifest_sha256: Sha256Digest,
    device: Sha256Digest,
    capability: ModelPackKindV1,
    sequence: u64,
    placements: BTreeMap<ResidencyModeV1, PlacementMeasurementV1>,
) -> SignedMeasuredResourceEnvelopeV1 {
    SignedMeasuredResourceEnvelopeV1 {
        signed: MeasuredResourceEnvelopePayloadV1 {
            schema: MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1.to_owned(),
            report_id: format!("measurement-{sequence:04}"),
            sequence,
            measured_unix_seconds: 100,
            expires_unix_seconds: 1_000,
            device_fingerprint_sha256: device,
            identity,
            manifest_sha256,
            capability,
            benchmark_suite_revision: "windows-game-contention-v1".to_owned(),
            runtime: "fixture-runtime".to_owned(),
            runtime_revision: "r1".to_owned(),
            backend: "cpu-cuda".to_owned(),
            sample_count: 32,
            placements,
        },
        signatures: vec![CatalogSignatureV1 {
            key_id: "fixture-a".to_owned(),
            algorithm: "fixture".to_owned(),
            signature: "valid".to_owned(),
        }],
    }
}

fn cpu_measurement(ram: u64) -> PlacementMeasurementV1 {
    PlacementMeasurementV1 {
        resident_ram_bytes: ram,
        p99_total_ram_bytes: ram + 100,
        resident_vram_bytes: 0,
        p99_workspace_vram_bytes: 0,
        p99_load_millis: 100,
        p99_reload_millis: 80,
        p99_operation_millis: 40,
    }
}

fn gpu_measurement(ram: u64, resident_vram: u64, workspace_vram: u64) -> PlacementMeasurementV1 {
    PlacementMeasurementV1 {
        resident_ram_bytes: ram,
        p99_total_ram_bytes: ram + 100,
        resident_vram_bytes: resident_vram,
        p99_workspace_vram_bytes: workspace_vram,
        p99_load_millis: 300,
        p99_reload_millis: 240,
        p99_operation_millis: 20,
    }
}

fn verify_fixture_measurement(
    envelope: &SignedMeasuredResourceEnvelopeV1,
    prior: &MeasurementTrustStateV1,
) -> Result<(QualifiedResourceEnvelopeV1, MeasurementTrustStateV1), ResourceEnvelopeError> {
    verify_measured_resource_envelope(
        envelope,
        &AcceptFixtureSignatures,
        &MeasurementTrustPolicyV1 {
            signature_threshold: 1,
            minimum_samples: 20,
            maximum_lifetime_seconds: 2_000,
            maximum_clock_skew_seconds: 10,
        },
        prior,
        200,
    )
}

const LOCAL_REVIEW_TEST_PRIVATE_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

fn sign_local_review_payload(payload: LocalReviewCatalogPayloadV1) -> SignedLocalReviewCatalogV1 {
    let signing = SigningKey::from_bytes(&LOCAL_REVIEW_TEST_PRIVATE_SEED);
    assert_eq!(
        signing.verifying_key().to_bytes(),
        LOCAL_REVIEW_TEST_PUBLIC_KEY_V1
    );
    let bytes = canonical_local_review_catalog_bytes(&payload).unwrap();
    SignedLocalReviewCatalogV1 {
        signed: payload,
        signatures: vec![CatalogSignatureV1 {
            key_id: LOCAL_REVIEW_TEST_KEY_ID_V1.to_owned(),
            algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
            signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(signing.sign(&bytes).to_bytes()),
        }],
    }
}

fn signed_openseeface_local_review_catalog() -> SignedLocalReviewCatalogV1 {
    let manifest = openseeface_visual_signal_manifest_v1().unwrap();
    let contract = openseeface_visual_signal_contract_v1();
    sign_local_review_payload(LocalReviewCatalogPayloadV1 {
        schema: LOCAL_REVIEW_CATALOG_SCHEMA_V1.to_owned(),
        review_id: "openseeface-visual-pack-qualification-2026-08-30".to_owned(),
        review_sequence: 1,
        generated_unix_seconds: 100,
        expires_unix_seconds: 1_000,
        manifest_sha256: manifest.digest().unwrap(),
        manifest,
        contract_sha256: contract.digest().unwrap(),
        contract,
    })
}

#[test]
fn built_in_candidates_are_concrete_but_never_self_qualifying() {
    let candidates = built_in_local_model_candidates_v1();
    assert_eq!(candidates.len(), 7);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.capability.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ModelPackKindV1::LanguageModel,
            ModelPackKindV1::SpeechRecognition,
            ModelPackKindV1::SpeechSynthesis,
            ModelPackKindV1::Embedding,
            ModelPackKindV1::Vision,
            ModelPackKindV1::LipSync,
        ])
    );
    for candidate in &candidates {
        candidate.validate().unwrap();
        assert!(candidate.expected_license.transitive_assets_review_required);
        assert!(!candidate.qualification_gates.is_empty());
    }

    let sface = candidates
        .iter()
        .find(|candidate| candidate.candidate_id == "opencv-sface-2021dec")
        .unwrap();
    assert_eq!(
        sface.disposition,
        CandidateDispositionV1::PrivateEvaluationOnly
    );
    assert!(!sface.expected_license.redistributable_signal);
    assert!(sface
        .qualification_gates
        .contains(&QualificationGateV1::PretrainedWeightsAndTrainingDataRights));

    let lipsync = candidates
        .iter()
        .find(|candidate| candidate.capability == ModelPackKindV1::LipSync)
        .unwrap();
    assert_eq!(lipsync.install.preferred_device, PreferredDeviceV1::Cpu);
    assert_eq!(lipsync.install.model_format, "model-free-reference-code");

    let openseeface = candidates
        .iter()
        .find(|candidate| candidate.candidate_id == OPENSEEFACE_VISUAL_SIGNAL_PACK_ID)
        .unwrap();
    assert!(openseeface.install.explicit_download_required);
    assert_eq!(
        openseeface.install.dependency_of_candidate_ids,
        BTreeSet::from(["npc-causal-viseme-mouth-warp-v1".to_owned()])
    );
    assert_eq!(
        openseeface.install.required_self_test_kind.as_deref(),
        Some("openseeface-temporal-guard")
    );
    assert_eq!(
        openseeface
            .install
            .required_self_test_suite_revision
            .as_deref(),
        Some("eclipse-harbor-guard-2026-08-30")
    );
    assert_eq!(
        openseeface.install.required_self_test_output_sha256,
        Some(openseeface_guarded_report_sha256_v1())
    );

    let mut dishonest = candidates[0].clone();
    dishonest.planning_estimate = Some(PlanningResourceEstimateV1 {
        schema: PLANNING_RESOURCE_ESTIMATE_SCHEMA_V1.to_owned(),
        provenance_url: "https://example.test/research".to_owned(),
        assumptions: "A planning range, not a local measurement.".to_owned(),
        estimated_installed_bytes: EstimateRangeV1 {
            minimum: 1,
            maximum: 2,
        },
        estimated_resident_ram_bytes: EstimateRangeV1 {
            minimum: 1,
            maximum: 2,
        },
        estimated_resident_vram_bytes: EstimateRangeV1 {
            minimum: 1,
            maximum: 2,
        },
        estimated_peak_vram_bytes: EstimateRangeV1 {
            minimum: 1,
            maximum: 2,
        },
        estimated_load_millis: EstimateRangeV1 {
            minimum: 1,
            maximum: 2,
        },
        not_for_admission: false,
    });
    assert!(matches!(
        dishonest.validate(),
        Err(LocalCatalogError::EstimateClaimedAsMeasurement)
    ));
}

#[test]
fn openseeface_review_evidence_is_exact_crypto_testable_and_not_release_trust() {
    let manifest = openseeface_visual_signal_manifest_v1().unwrap();
    assert_eq!(
        manifest.revision.as_str(),
        OPENSEEFACE_VISUAL_SIGNAL_REVISION
    );
    assert_eq!(
        manifest
            .artifacts
            .iter()
            .map(|item| item.size_bytes)
            .sum::<u64>(),
        OPENSEEFACE_VISUAL_SIGNAL_INSTALLED_BYTES
    );
    assert_eq!(
        manifest
            .artifacts
            .iter()
            .map(|item| (
                item.destination.as_str(),
                item.size_bytes,
                item.sha256.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "models/mnv3_detection_opt.onnx",
                568_302,
                "0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809",
            ),
            (
                "models/lm_model1_opt.onnx",
                4_842_329,
                "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f",
            ),
            (
                "LICENSE",
                1_364,
                "28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146",
            ),
        ]
    );
    assert_eq!(
        manifest.license.spdx_expression.as_deref(),
        Some("BSD-2-Clause")
    );
    assert!(manifest.license.redistributable);
    assert_eq!(manifest.resources.measured_ram_bytes, Some(93_265_592));
    assert_eq!(manifest.resources.measured_vram_bytes, Some(0));
    let contract = openseeface_visual_signal_contract_v1();
    contract.validate().unwrap();
    assert_eq!(contract.maximum_signal_rate_hz, 15);
    assert_eq!(contract.measurement.peak_rss_delta_mib_x1000, 88_945);
    assert_eq!(contract.measurement.full_frame_nanos.p99, 92_424_500);
    assert!(
        contract
            .temporal_guard
            .appearance_and_occlusion_latch_required
    );
    assert!(!contract.is_complete_lip_sync_model);
    assert!(!contract.is_identity_recognition_model);
    assert!(!contract.is_talking_head_generator);

    let signed = signed_openseeface_local_review_catalog();
    let verified = verify_local_review_catalog(&signed, 200).unwrap();
    assert_eq!(
        verified.install_binding().trust_domain,
        CatalogTrustDomainV1::LocalReviewDevOnly
    );

    let mut tampered = signed;
    tampered.signed.contract.maximum_signal_rate_hz = 30;
    assert!(matches!(
        verify_local_review_catalog(&tampered, 200),
        Err(ExperimentalVisualPackError::EvidenceMismatch)
            | Err(ExperimentalVisualPackError::ContractMismatch)
    ));

    // Even if the public dev key signs a production-shaped payload, the
    // production verifier rejects the dev-only schema before signature trust.
    let signing = SigningKey::from_bytes(&LOCAL_REVIEW_TEST_PRIVATE_SEED);
    let release_shaped_payload = CatalogPayloadV1 {
        schema: LOCAL_REVIEW_CATALOG_SCHEMA_V1.to_owned(),
        version: 1,
        generated_unix_seconds: 100,
        expires_unix_seconds: 1_000,
        entries: vec![CatalogEntryV1 {
            manifest_sha256: manifest.digest().unwrap(),
            manifest,
            channels: BTreeSet::from([OPTIONAL_LOCAL_CHANNEL_V1.to_owned()]),
            published_unix_seconds: 100,
            revoked: false,
            revocation_reason: None,
        }],
    };
    let bytes = canonical_catalog_payload_bytes(&release_shaped_payload).unwrap();
    let release_shaped = SignedCatalogV1 {
        signed: release_shaped_payload,
        signatures: vec![CatalogSignatureV1 {
            key_id: LOCAL_REVIEW_TEST_KEY_ID_V1.to_owned(),
            algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
            signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(signing.sign(&bytes).to_bytes()),
        }],
    };
    let verifier = Ed25519CatalogVerifier::new([(
        LOCAL_REVIEW_TEST_KEY_ID_V1.to_owned(),
        LOCAL_REVIEW_TEST_PUBLIC_KEY_V1,
    )])
    .unwrap();
    assert!(matches!(
        verify_catalog(
            &release_shaped,
            &verifier,
            &CatalogTrustPolicy::default(),
            &CatalogTrustState::default(),
            200,
        ),
        Err(CatalogError::UnsupportedSchema(_))
    ));
}

#[test]
fn sface_generic_apache_claim_cannot_cross_the_private_evaluation_gate() {
    let candidate = built_in_local_model_candidates_v1()
        .into_iter()
        .find(|candidate| candidate.candidate_id == "opencv-sface-2021dec")
        .unwrap();
    let mut manifest = manifest_for(&candidate);
    // Simulate an overly broad interpretation of the repository license. The
    // candidate gate must win even if a manifest calls this redistributable.
    manifest.license.redistributable = true;
    let manifest_sha256 = manifest.digest().unwrap();
    let catalog = SignedCatalogV1 {
        signed: CatalogPayloadV1 {
            schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
            version: 1,
            generated_unix_seconds: 100,
            expires_unix_seconds: 1_000,
            entries: vec![CatalogEntryV1 {
                manifest: manifest.clone(),
                manifest_sha256: manifest_sha256.clone(),
                channels: BTreeSet::from([OPTIONAL_LOCAL_CHANNEL_V1.to_owned()]),
                published_unix_seconds: 100,
                revoked: false,
                revocation_reason: None,
            }],
        },
        signatures: vec![
            CatalogSignatureV1 {
                key_id: "fixture-a".to_owned(),
                algorithm: "fixture".to_owned(),
                signature: "valid".to_owned(),
            },
            CatalogSignatureV1 {
                key_id: "fixture-b".to_owned(),
                algorithm: "fixture".to_owned(),
                signature: "valid".to_owned(),
            },
        ],
    };
    let (trusted, _) = verify_catalog(
        &catalog,
        &AcceptFixtureSignatures,
        &CatalogTrustPolicy::default(),
        &CatalogTrustState::default(),
        200,
    )
    .unwrap();
    let signed_measurement = measurement(
        manifest.identity(),
        manifest_sha256,
        digest("sface-private-device"),
        ModelPackKindV1::Vision,
        1,
        BTreeMap::from([(ResidencyModeV1::CpuResident, cpu_measurement(500))]),
    );
    let measured =
        verify_fixture_measurement(&signed_measurement, &MeasurementTrustStateV1::default())
            .unwrap()
            .0;
    assert!(matches!(
        qualify_local_model(&candidate, &trusted, &manifest.identity(), measured),
        Err(LocalCatalogError::CandidatePrivateEvaluationOnly)
    ));
}

#[test]
fn measured_envelope_import_is_signed_bound_monotonic_and_fail_closed() {
    let model = identity("fixture.llm");
    let manifest_sha256 = digest("manifest");
    let device = digest("device-driver-runtime");
    let placements = BTreeMap::from([
        (ResidencyModeV1::CpuResident, cpu_measurement(1_000)),
        (
            ResidencyModeV1::GpuResident,
            gpu_measurement(700, 2_000, 500),
        ),
        (ResidencyModeV1::CpuResidentGpuCold, cpu_measurement(1_200)),
    ]);
    let envelope = measurement(
        model.clone(),
        manifest_sha256,
        device,
        ModelPackKindV1::LanguageModel,
        1,
        placements,
    );
    let (verified, state) =
        verify_fixture_measurement(&envelope, &MeasurementTrustStateV1::default()).unwrap();
    assert_eq!(verified.identity(), &model);
    assert_eq!(
        verified
            .placement(&ResidencyModeV1::GpuResident)
            .unwrap()
            .resident_vram_bytes,
        2_000
    );

    let mut rollback = envelope.clone();
    rollback.signed.sequence = 0;
    assert!(verify_fixture_measurement(&rollback, &state).is_err());

    let mut equivocation = envelope.clone();
    equivocation.signed.report_id = "measurement-other".to_owned();
    assert!(matches!(
        verify_fixture_measurement(&equivocation, &state),
        Err(ResourceEnvelopeError::SequenceEquivocation(1))
    ));

    let mut unsigned = envelope.clone();
    unsigned.signatures.clear();
    assert!(matches!(
        verify_fixture_measurement(&unsigned, &MeasurementTrustStateV1::default()),
        Err(ResourceEnvelopeError::SignatureThreshold { .. })
    ));

    let mut too_few_samples = envelope.clone();
    too_few_samples.signed.sample_count = 19;
    assert!(matches!(
        verify_fixture_measurement(&too_few_samples, &MeasurementTrustStateV1::default()),
        Err(ResourceEnvelopeError::IncompleteMeasurement)
    ));

    let mut invalid_cpu = envelope;
    invalid_cpu
        .signed
        .placements
        .get_mut(&ResidencyModeV1::CpuResident)
        .unwrap()
        .resident_vram_bytes = 1;
    assert!(matches!(
        verify_fixture_measurement(&invalid_cpu, &MeasurementTrustStateV1::default()),
        Err(ResourceEnvelopeError::InvalidPlacementMeasurement(
            ResidencyModeV1::CpuResident
        ))
    ));

    let mut expired = measurement(
        model,
        digest("manifest"),
        digest("device-driver-runtime"),
        ModelPackKindV1::LanguageModel,
        2,
        BTreeMap::from([(ResidencyModeV1::CpuResident, cpu_measurement(1_000))]),
    );
    expired.signed.expires_unix_seconds = 199;
    assert!(matches!(
        verify_fixture_measurement(&expired, &MeasurementTrustStateV1::default()),
        Err(ResourceEnvelopeError::Expired)
    ));
}

fn governor_policy() -> ResourceGovernorPolicyV1 {
    ResourceGovernorPolicyV1 {
        schema: RESOURCE_GOVERNOR_POLICY_SCHEMA_V1.to_owned(),
        vram_soft_ceiling_basis_points: 9_000,
        ram_soft_ceiling_basis_points: 9_000,
        minimum_vram_safety_bytes: 1_000,
        proportional_vram_safety_basis_points: 1_000,
        minimum_ram_safety_bytes: 500,
        maximum_snapshot_age_millis: 100,
        keep_warm_millis: 20,
        unload_ttl_millis: 100,
    }
}

fn snapshot(device: Sha256Digest) -> LiveResourceSnapshotV1 {
    LiveResourceSnapshotV1 {
        captured_monotonic_millis: 1_000,
        device_fingerprint_sha256: device,
        physical_vram_bytes: 12_000,
        os_vram_budget_bytes: 11_000,
        desktop_resident_vram_bytes: 500,
        game_resident_vram_bytes: 3_000,
        game_reserve_vram_bytes: 5_000,
        physical_ram_bytes: 32_000,
        available_ram_bytes: 16_000,
        game_additional_reserve_ram_bytes: 1_000,
    }
}

fn verified_for_governor(
    pack: &str,
    role_index: u64,
    device: &Sha256Digest,
    capability: ModelPackKindV1,
    cpu_ram: u64,
    gpu: Option<(u64, u64)>,
) -> QualifiedResourceEnvelopeV1 {
    let mut placements = BTreeMap::from([
        (ResidencyModeV1::CpuResident, cpu_measurement(cpu_ram)),
        (
            ResidencyModeV1::CpuResidentGpuCold,
            cpu_measurement(cpu_ram + 50),
        ),
    ]);
    if let Some((resident, workspace)) = gpu {
        placements.insert(
            ResidencyModeV1::GpuResident,
            gpu_measurement(cpu_ram / 2, resident, workspace),
        );
    }
    let envelope = measurement(
        identity(pack),
        digest(&format!("manifest-{role_index}")),
        device.clone(),
        capability,
        1,
        placements,
    );
    verify_fixture_measurement(&envelope, &MeasurementTrustStateV1::default())
        .unwrap()
        .0
}

#[test]
fn governor_admits_whole_loadout_against_live_game_reserve_and_soft_ceiling() {
    let device = digest("gaming-device");
    let llm = verified_for_governor(
        "loadout.llm",
        1,
        &device,
        ModelPackKindV1::LanguageModel,
        2_000,
        Some((1_500, 300)),
    );
    let stt = verified_for_governor(
        "loadout.stt",
        2,
        &device,
        ModelPackKindV1::SpeechRecognition,
        500,
        None,
    );
    let tts = verified_for_governor(
        "loadout.tts",
        3,
        &device,
        ModelPackKindV1::SpeechSynthesis,
        600,
        None,
    );
    let embed = verified_for_governor(
        "loadout.embed",
        4,
        &device,
        ModelPackKindV1::Embedding,
        200,
        None,
    );
    let governor = ResourceGovernorV1::new(governor_policy()).unwrap();
    let admission = governor
        .admit(
            &snapshot(device.clone()),
            1_050,
            &[
                LoadoutModelRequestV1 {
                    role: ModelPackKindV1::LanguageModel,
                    mode: ResidencyModeV1::GpuResident,
                    envelope: &llm,
                },
                LoadoutModelRequestV1 {
                    role: ModelPackKindV1::SpeechRecognition,
                    mode: ResidencyModeV1::CpuResident,
                    envelope: &stt,
                },
                LoadoutModelRequestV1 {
                    role: ModelPackKindV1::SpeechSynthesis,
                    mode: ResidencyModeV1::CpuResident,
                    envelope: &tts,
                },
                LoadoutModelRequestV1 {
                    role: ModelPackKindV1::Embedding,
                    mode: ResidencyModeV1::CpuResident,
                    envelope: &embed,
                },
            ],
        )
        .unwrap();
    assert_eq!(admission.protected_desktop_and_game_vram_bytes(), 5_500);
    assert_eq!(admission.selected_peak_vram_bytes(), 1_800);
    assert_eq!(admission.vram_safety_bytes(), 1_100);
    assert_eq!(admission.projected_total_vram_bytes(), 8_400);
    assert_eq!(admission.models().len(), 4);
    assert_ne!(admission.digest().unwrap(), digest("not-the-receipt"));

    let mut pressure = snapshot(device.clone());
    pressure.game_reserve_vram_bytes = 8_000;
    assert!(matches!(
        governor.admit(
            &pressure,
            1_050,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::LanguageModel,
                mode: ResidencyModeV1::GpuResident,
                envelope: &llm,
            }]
        ),
        Err(ResourceAdmissionError::VramSoftCeilingExceeded { .. })
    ));

    let cpu_only = governor
        .admit(
            &pressure,
            1_050,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::LanguageModel,
                mode: ResidencyModeV1::CpuResidentGpuCold,
                envelope: &llm,
            }],
        )
        .unwrap();
    assert_eq!(cpu_only.selected_peak_vram_bytes(), 0);
}

#[test]
fn governor_rejects_unknown_duplicate_stale_and_wrong_device_evidence() {
    let device = digest("device-a");
    let llm = verified_for_governor(
        "guard.llm",
        1,
        &device,
        ModelPackKindV1::LanguageModel,
        2_000,
        Some((1_000, 200)),
    );
    let governor = ResourceGovernorV1::new(governor_policy()).unwrap();
    let live = snapshot(device.clone());

    assert!(matches!(
        governor.admit(
            &live,
            1_050,
            &[
                LoadoutModelRequestV1 {
                    role: ModelPackKindV1::LanguageModel,
                    mode: ResidencyModeV1::GpuResident,
                    envelope: &llm,
                },
                LoadoutModelRequestV1 {
                    role: ModelPackKindV1::LanguageModel,
                    mode: ResidencyModeV1::CpuResident,
                    envelope: &llm,
                }
            ]
        ),
        Err(ResourceAdmissionError::DuplicateRole(
            ModelPackKindV1::LanguageModel
        ))
    ));
    assert!(matches!(
        governor.admit(
            &live,
            1_050,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::LanguageModel,
                mode: ResidencyModeV1::GpuResident,
                envelope: &verified_for_governor(
                    "guard.cpu",
                    2,
                    &device,
                    ModelPackKindV1::LanguageModel,
                    100,
                    None,
                ),
            }]
        ),
        Err(ResourceAdmissionError::UnknownEnvelope { .. })
    ));
    assert!(matches!(
        governor.admit(
            &live,
            1_101,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::LanguageModel,
                mode: ResidencyModeV1::GpuResident,
                envelope: &llm,
            }]
        ),
        Err(ResourceAdmissionError::StaleSnapshot { .. })
    ));
    let wrong_device = snapshot(digest("device-b"));
    assert_eq!(
        governor.admit(
            &wrong_device,
            1_050,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::LanguageModel,
                mode: ResidencyModeV1::GpuResident,
                envelope: &llm,
            }]
        ),
        Err(ResourceAdmissionError::DeviceFingerprintMismatch)
    );
    assert!(matches!(
        governor.admit(
            &live,
            1_050,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::SpeechRecognition,
                mode: ResidencyModeV1::CpuResident,
                envelope: &llm,
            }]
        ),
        Err(ResourceAdmissionError::CapabilityMismatch { .. })
    ));

    let mut ram_pressure = live;
    ram_pressure.available_ram_bytes = 1_000;
    assert!(matches!(
        governor.admit(
            &ram_pressure,
            1_050,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::LanguageModel,
                mode: ResidencyModeV1::CpuResident,
                envelope: &llm,
            }]
        ),
        Err(ResourceAdmissionError::RamSoftCeilingExceeded { .. })
    ));

    let mut invalid_policy = governor_policy();
    invalid_policy.keep_warm_millis = invalid_policy.unload_ttl_millis + 1;
    assert!(matches!(
        ResourceGovernorV1::new(invalid_policy),
        Err(ResourceAdmissionError::InvalidPolicy)
    ));
}

#[test]
fn residency_tracker_keeps_warm_then_drops_gpu_then_unloads() {
    let model = identity("resident.llm");
    let policy = governor_policy();
    let mut tracker = ResidencyTrackerV1::new(&policy).unwrap();
    tracker
        .record_loaded(model.clone(), RuntimeResidencyStateV1::GpuWarm, true, 100)
        .unwrap();
    assert!(tracker.maintenance_actions(119).unwrap().is_empty());
    assert_eq!(
        tracker.maintenance_actions(120).unwrap(),
        vec![ResidencyActionV1::DropGpuKeepCpu {
            identity: model.clone()
        }]
    );
    assert_eq!(
        tracker.record(&model).unwrap().state,
        RuntimeResidencyStateV1::CpuResidentGpuCold
    );
    assert!(tracker.maintenance_actions(150).unwrap().is_empty());
    assert_eq!(
        tracker.maintenance_actions(200).unwrap(),
        vec![ResidencyActionV1::Unload {
            identity: model.clone()
        }]
    );
    assert!(tracker.record(&model).is_none());
}

fn work(id: &str, kind: WorkKindV1, visual: Option<(u64, u64)>, deadline: u64) -> WorkItemV1 {
    WorkItemV1 {
        work_id: id.to_owned(),
        kind,
        submitted_monotonic_millis: 100,
        deadline_monotonic_millis: deadline,
        visual: visual.map(|(generation_id, frame_id)| VisualWorkAddressV1 {
            generation_id,
            frame_id,
        }),
    }
}

#[test]
fn scheduler_preempts_background_and_cancels_stale_visual_work() {
    let mut scheduler = WorkSchedulerV1::default();
    scheduler
        .submit(work("embedding-001", WorkKindV1::Embedding, None, 500), 100)
        .unwrap();
    scheduler
        .submit(
            work("vision-frame-001", WorkKindV1::Vision, Some((1, 1)), 500),
            100,
        )
        .unwrap();
    let result = scheduler
        .submit(
            work("dialogue-001", WorkKindV1::LanguageModel, None, 500),
            100,
        )
        .unwrap();
    assert_eq!(result.cancellations.len(), 2);
    assert!(result
        .cancellations
        .iter()
        .all(|item| item.reason == CancellationReasonV1::PreemptedByInteractive));

    scheduler
        .submit(
            work("lipsync-frame-002", WorkKindV1::LipSync, Some((1, 2)), 500),
            100,
        )
        .unwrap();
    let cancellations = scheduler.advance_visual_clock(1, 3, 101).unwrap();
    assert_eq!(
        cancellations,
        vec![WorkCancellationV1 {
            work_id: "lipsync-frame-002".to_owned(),
            reason: CancellationReasonV1::StaleFrame,
        }]
    );

    let stale = scheduler
        .submit(
            work("vision-frame-old", WorkKindV1::Vision, Some((1, 2)), 500),
            101,
        )
        .unwrap();
    assert!(!stale.accepted);
    assert_eq!(
        stale.cancellations[0].reason,
        CancellationReasonV1::StaleFrame
    );

    scheduler
        .submit(work("embedding-002", WorkKindV1::Embedding, None, 500), 101)
        .unwrap();
    scheduler
        .submit(
            work("speech-001", WorkKindV1::SpeechRecognition, None, 500),
            101,
        )
        .unwrap();
    assert_eq!(
        scheduler.pop_next(102).unwrap().kind,
        WorkKindV1::SpeechRecognition
    );

    scheduler
        .submit(
            work("vision-newer", WorkKindV1::Vision, Some((2, 10)), 500),
            102,
        )
        .unwrap();
    let superseded = scheduler
        .submit(
            work("vision-newest", WorkKindV1::Vision, Some((2, 11)), 500),
            102,
        )
        .unwrap();
    assert_eq!(
        superseded.cancellations,
        vec![WorkCancellationV1 {
            work_id: "vision-newer".to_owned(),
            reason: CancellationReasonV1::StaleFrame,
        }]
    );

    scheduler
        .submit(
            work("late-tts", WorkKindV1::SpeechSynthesis, None, 103),
            102,
        )
        .unwrap();
    let expired = scheduler.advance_visual_clock(2, 11, 103).unwrap();
    assert_eq!(
        expired,
        vec![WorkCancellationV1 {
            work_id: "late-tts".to_owned(),
            reason: CancellationReasonV1::DeadlineExpired,
        }]
    );

    scheduler
        .submit(
            work("lipsync-old-gen", WorkKindV1::LipSync, Some((3, 1)), 600),
            103,
        )
        .unwrap();
    let cross_kind_stale = scheduler
        .submit(
            work("vision-new-gen", WorkKindV1::Vision, Some((4, 1)), 600),
            103,
        )
        .unwrap();
    assert_eq!(
        cross_kind_stale.cancellations,
        vec![WorkCancellationV1 {
            work_id: "lipsync-old-gen".to_owned(),
            reason: CancellationReasonV1::StaleGeneration,
        }]
    );
    let same_frame_replacement = scheduler
        .submit(
            work(
                "vision-same-frame-retry",
                WorkKindV1::Vision,
                Some((4, 1)),
                600,
            ),
            103,
        )
        .unwrap();
    assert_eq!(
        same_frame_replacement.cancellations,
        vec![WorkCancellationV1 {
            work_id: "vision-new-gen".to_owned(),
            reason: CancellationReasonV1::SupersededVisualWork,
        }]
    );
}

struct DamageableMemoryStorage {
    inner: InMemoryPackStorage,
    damaged: Arc<AtomicBool>,
}

impl DamageableMemoryStorage {
    fn new(damaged: Arc<AtomicBool>) -> Self {
        Self {
            inner: InMemoryPackStorage::default(),
            damaged,
        }
    }
}

impl PackStorage for DamageableMemoryStorage {
    fn stage(
        &mut self,
        manifest: &ModelPackManifestV1,
        evidence: &[ArtifactEvidence],
    ) -> Result<StagedPack, StorageError> {
        self.inner.stage(manifest, evidence)
    }

    fn staged_content_binding(
        &self,
        staged: &StagedPack,
    ) -> Result<StagedContentBindingV1, StorageError> {
        self.inner.staged_content_binding(staged)
    }

    fn consume_attestation_replay_key(
        &mut self,
        replay_key: &Sha256Digest,
    ) -> Result<bool, StorageError> {
        self.inner.consume_attestation_replay_key(replay_key)
    }

    fn activate(
        &mut self,
        staged: StagedPack,
        authorization: &ActivationAuthorization,
    ) -> Result<(), StorageError> {
        self.inner.activate(staged, authorization)
    }

    fn activate_existing(&mut self, target: &PackRevision) -> Result<(), StorageError> {
        self.inner.activate_existing(target)
    }

    fn inspect(&self, target: &PackRevision) -> Result<StorageInspection, StorageError> {
        let mut inspection = self.inner.inspect(target)?;
        if self.damaged.load(Ordering::SeqCst) {
            inspection.content_healthy = false;
        }
        Ok(inspection)
    }

    fn remove_revision(&mut self, target: &PackRevision) -> Result<(), StorageError> {
        self.inner.remove_revision(target)
    }

    fn add_reference(&mut self, pack_id: &PackId, consumer: &str) -> Result<(), StorageError> {
        self.inner.add_reference(pack_id, consumer)
    }

    fn remove_reference(&mut self, pack_id: &PackId, consumer: &str) -> Result<bool, StorageError> {
        self.inner.remove_reference(pack_id, consumer)
    }

    fn reference_count(&self, pack_id: &PackId) -> Result<usize, StorageError> {
        self.inner.reference_count(pack_id)
    }
}

struct LocalReviewAttestationVerifier;

impl SelfTestAttestationVerifier for LocalReviewAttestationVerifier {
    fn verify(
        &self,
        claimed_runner_id: &str,
        proof: &AttestationProofV1,
        _canonical_payload: &[u8],
    ) -> Result<VerifiedRunnerIdentity, AttestationVerificationError> {
        if claimed_runner_id != "local-review-runner" {
            return Err(AttestationVerificationError::UnknownRunner);
        }
        if proof.algorithm != "fixture-proof" || proof.value != "valid" {
            return Err(AttestationVerificationError::InvalidProof);
        }
        Ok(VerifiedRunnerIdentity {
            runner_id: claimed_runner_id.to_owned(),
            trust_revision: "local-review-runner-v1".to_owned(),
        })
    }
}

fn complete_visual_pack_download_fixture<S: PackStorage>(
    manager: &mut ModelPackManager<S>,
    identity: &PackRevision,
    manifest: &ModelPackManifestV1,
) {
    for artifact in &manifest.artifacts {
        manager
            .download_journal_mut(identity, &artifact.id)
            .unwrap()
            .accept_response(ResumeResponse {
                status: 200,
                range_start: None,
                total_size: artifact.size_bytes,
                validator: Some(format!("\"{}\"", artifact.sha256)),
            })
            .unwrap();
        manager
            .download_journal_mut(identity, &artifact.id)
            .unwrap()
            .record_bytes(artifact.size_bytes)
            .unwrap();
        manager
            .accept_verified_artifact(
                identity,
                ArtifactEvidence {
                    artifact_id: artifact.id.clone(),
                    size_bytes: artifact.size_bytes,
                    sha256: artifact.sha256.clone(),
                },
            )
            .unwrap();
    }
}

fn attest_and_activate_visual_pack<S: PackStorage>(
    manager: &mut ModelPackManager<S>,
    identity: &PackRevision,
    now: u64,
) {
    if manager.record(identity).unwrap().state == InstallState::Staging {
        manager.stage(identity).unwrap();
    }
    let challenge = manager
        .issue_self_test_challenge(identity, "onnxruntime-1.22.1-cpu-1thread", now, 60)
        .unwrap();
    let attestation = SelfTestAttestationV1 {
        signed: SelfTestAttestationPayloadV1 {
            schema: SELF_TEST_ATTESTATION_SCHEMA_V1.to_owned(),
            challenge,
            runner_id: "local-review-runner".to_owned(),
            outcome: AttestedSelfTestOutcomeV1 {
                passed: true,
                duration_millis: 92,
                output_sha256: Some(
                    Sha256Digest::parse(
                        "7bbd78e9c2aae7b760fb6258878437009cf6f31c7bf13df34ae5867f6be8cf97",
                    )
                    .unwrap(),
                ),
                diagnostic_code: None,
            },
        },
        proof: AttestationProofV1 {
            algorithm: "fixture-proof".to_owned(),
            value: "valid".to_owned(),
        },
    };
    manager
        .record_self_test_attestation(
            identity,
            attestation,
            &LocalReviewAttestationVerifier,
            now + 1,
        )
        .unwrap();
    manager.activate(identity).unwrap();
}

#[test]
fn openseeface_dev_fixture_installs_activates_repairs_and_removes() {
    let manifest = openseeface_visual_signal_manifest_v1().unwrap();
    let identity = manifest.identity();
    let local_review =
        verify_local_review_catalog(&signed_openseeface_local_review_catalog(), 200).unwrap();

    let device = digest("openseeface-fixture-device");
    let mut measured = measurement(
        identity.clone(),
        manifest.digest().unwrap(),
        device.clone(),
        ModelPackKindV1::Vision,
        1,
        BTreeMap::from([(
            ResidencyModeV1::CpuResident,
            PlacementMeasurementV1 {
                resident_ram_bytes: 15_868_101,
                p99_total_ram_bytes: 93_265_592,
                resident_vram_bytes: 0,
                p99_workspace_vram_bytes: 0,
                p99_load_millis: 72,
                p99_reload_millis: 60,
                p99_operation_millis: 93,
            },
        )]),
    );
    let signing = SigningKey::from_bytes(&LOCAL_REVIEW_TEST_PRIVATE_SEED);
    let measured_bytes = canonical_measured_resource_envelope_bytes(&measured.signed).unwrap();
    measured.signatures = vec![CatalogSignatureV1 {
        key_id: LOCAL_REVIEW_TEST_KEY_ID_V1.to_owned(),
        algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
        signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signing.sign(&measured_bytes).to_bytes()),
    }];
    let verifier = Ed25519CatalogVerifier::new([(
        LOCAL_REVIEW_TEST_KEY_ID_V1.to_owned(),
        LOCAL_REVIEW_TEST_PUBLIC_KEY_V1,
    )])
    .unwrap();
    let (envelope, _) = verify_measured_resource_envelope(
        &measured,
        &verifier,
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
    let admission = ResourceGovernorV1::new(ResourceGovernorPolicyV1::default())
        .unwrap()
        .admit(
            &LiveResourceSnapshotV1 {
                captured_monotonic_millis: 1_000,
                device_fingerprint_sha256: device,
                physical_vram_bytes: 12_884_901_888,
                os_vram_budget_bytes: 12_000_000_000,
                desktop_resident_vram_bytes: 600_000_000,
                game_resident_vram_bytes: 7_000_000_000,
                game_reserve_vram_bytes: 8_000_000_000,
                physical_ram_bytes: 34_359_738_368,
                available_ram_bytes: 12_000_000_000,
                game_additional_reserve_ram_bytes: 2_000_000_000,
            },
            1_001,
            &[LoadoutModelRequestV1 {
                role: ModelPackKindV1::Vision,
                mode: ResidencyModeV1::CpuResident,
                envelope: &envelope,
            }],
        )
        .unwrap();
    let install_selection = MeasuredLocalPackSelectionPolicyV1
        .authorize(
            &manifest,
            PackSelectionRequestV1 {
                selection_id: "openseeface-explicit-install-001".to_owned(),
                origin: PackSelectionOriginV1::ExplicitUser,
                action: PackSelectionActionV1::InstallOnly,
                selected_unix_seconds: 199,
            },
            None,
        )
        .unwrap();
    let activation_selection = MeasuredLocalPackSelectionPolicyV1
        .authorize(
            &manifest,
            PackSelectionRequestV1 {
                selection_id: "openseeface-explicit-selection-001".to_owned(),
                origin: PackSelectionOriginV1::ExplicitUser,
                action: PackSelectionActionV1::InstallAndActivate,
                selected_unix_seconds: 200,
            },
            Some(&admission),
        )
        .unwrap();

    let damaged = Arc::new(AtomicBool::new(false));
    let mut manager = ModelPackManager::new(DamageableMemoryStorage::new(damaged.clone()));
    manager
        .begin_install(
            manifest.clone(),
            local_review.install_binding(),
            install_selection,
            false,
        )
        .unwrap();
    complete_visual_pack_download_fixture(&mut manager, &identity, &manifest);
    manager.stage(&identity).unwrap();
    manager
        .authorize_activation(&identity, activation_selection)
        .unwrap();
    attest_and_activate_visual_pack(&mut manager, &identity, 300);
    assert_eq!(
        manager.record(&identity).unwrap().state,
        InstallState::Active
    );
    assert!(manager.audit(&identity).unwrap().healthy);

    damaged.store(true, Ordering::SeqCst);
    let repair = manager.begin_repair(&identity).unwrap();
    assert!(!repair.healthy);
    assert!(repair
        .issues
        .contains(&RepairIssue::InstalledContentMismatch));
    manager.restart_repair_downloads(&identity).unwrap();
    damaged.store(false, Ordering::SeqCst);
    complete_visual_pack_download_fixture(&mut manager, &identity, &manifest);
    attest_and_activate_visual_pack(&mut manager, &identity, 400);
    assert!(manager.audit(&identity).unwrap().healthy);

    assert_eq!(
        manager
            .remove(&identity.pack_id, RemovalPolicy::RequireUnreferenced)
            .unwrap(),
        1
    );
    assert!(manager.record(&identity).is_none());

    assert_eq!(
        UI_COMMAND_INSTALL_EXPERIMENTAL_PACK,
        "install_experimental_model_pack"
    );
    assert_eq!(
        UI_COMMAND_ACTIVATE_EXPERIMENTAL_PACK,
        "activate_experimental_model_pack"
    );
    assert_eq!(UI_COMMAND_REPAIR_MODEL_PACK, "repair_model_pack");
    assert_eq!(UI_COMMAND_REMOVE_MODEL_PACK, "remove_model_pack");
    let handoff = openseeface_ui_command_handoff_v1().unwrap();
    assert_eq!(handoff.len(), 4);
    assert_eq!(
        handoff
            .iter()
            .map(|command| serde_json::to_value(command).unwrap()["command"]
                .as_str()
                .unwrap()
                .to_owned())
            .collect::<Vec<_>>(),
        vec![
            UI_COMMAND_INSTALL_EXPERIMENTAL_PACK,
            UI_COMMAND_ACTIVATE_EXPERIMENTAL_PACK,
            UI_COMMAND_REPAIR_MODEL_PACK,
            UI_COMMAND_REMOVE_MODEL_PACK,
        ]
    );
}

fn manifest_for(candidate: &LocalModelCandidateV1) -> ModelPackManifestV1 {
    let bytes = b"fixture-model";
    ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: PackId::parse("qualified.fixture").unwrap(),
        revision: Revision::parse("r1").unwrap(),
        display_name: candidate.display_name.clone(),
        description: "Exact signed fixture manifest for local qualification.".to_owned(),
        source_project: candidate.source.project_url.clone(),
        source_revision: candidate.source.expected_source_revision.clone(),
        capability: ModelPackCapabilityV1 {
            kind: candidate.capability.clone(),
            scope: ModelPackScopeV1::Generic,
        },
        artifacts: vec![ArtifactV1 {
            id: "weights".to_owned(),
            kind: ArtifactKind::File,
            archive_format: None,
            source_urls: vec!["https://models.example.test/weights.bin".to_owned()],
            size_bytes: bytes.len() as u64,
            sha256: Sha256Digest::of_bytes(bytes),
            destination: "models/weights.bin".to_owned(),
            strip_prefix: None,
            required_paths: vec![],
        }],
        runtime: RuntimeCompatibilityV1 {
            runtime: candidate.install.runtime.clone(),
            abi: candidate.install.abi.clone(),
            minimum_runtime_revision: Some("r1".to_owned()),
            supported_platforms: BTreeSet::from([Platform::Windows10, Platform::Windows11]),
            supported_architectures: BTreeSet::from([Architecture::X86_64]),
        },
        hardware: HardwareRequirementsV1 {
            minimum_ram_bytes: 1,
            recommended_ram_bytes: 2,
            minimum_vram_bytes: None,
            recommended_vram_bytes: None,
            minimum_cpu_threads: 1,
            accelerators: BTreeSet::from([Accelerator::Cpu]),
            required_cpu_features: BTreeSet::new(),
        },
        resources: ResourceEnvelopeV1 {
            storage_bytes: bytes.len() as u64,
            peak_install_bytes: bytes.len() as u64,
            // Legacy manifest values are descriptive only; governor admission
            // consumes the separate signed measured envelope.
            measured_ram_bytes: None,
            measured_vram_bytes: None,
            measured_load_millis: None,
            benchmark_hardware: None,
            quality_tier: QualityTier::Fast,
            languages: BTreeSet::from(["en".to_owned()]),
        },
        license: LicenseMetadataV1 {
            spdx_expression: candidate.expected_license.spdx_expression.clone(),
            license_name: candidate.expected_license.license_name.clone(),
            license_url: candidate.expected_license.license_url.clone(),
            attribution: "Fixture upstream authors".to_owned(),
            redistributable: candidate.expected_license.redistributable_signal,
            commercial_use: LicensePermission::Allowed,
            derivative_use: LicensePermission::Allowed,
            acceptance_required: candidate.expected_license.acceptance_required,
            notices: vec!["Review all transitive assets.".to_owned()],
        },
        self_test: SelfTestV1 {
            kind: "fixture".to_owned(),
            suite_revision: "fixture-v1".to_owned(),
            allowed_runtime_backends: BTreeSet::from(["fixture-runtime".to_owned()]),
            input_fixture: "tests/input.json".to_owned(),
            expected_output_sha256: Some(digest("output")),
            timeout_millis: 1_000,
        },
    }
}

#[test]
fn qualification_binds_candidate_signed_catalog_manifest_and_measurement() {
    let candidate = built_in_local_model_candidates_v1()
        .into_iter()
        .find(|candidate| candidate.capability == ModelPackKindV1::SpeechRecognition)
        .unwrap();
    let manifest = manifest_for(&candidate);
    let manifest_sha256 = manifest.digest().unwrap();
    let catalog = SignedCatalogV1 {
        signed: CatalogPayloadV1 {
            schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
            version: 1,
            generated_unix_seconds: 100,
            expires_unix_seconds: 1_000,
            entries: vec![CatalogEntryV1 {
                manifest: manifest.clone(),
                manifest_sha256: manifest_sha256.clone(),
                channels: BTreeSet::from([OPTIONAL_LOCAL_CHANNEL_V1.to_owned()]),
                published_unix_seconds: 100,
                revoked: false,
                revocation_reason: None,
            }],
        },
        signatures: vec![
            CatalogSignatureV1 {
                key_id: "fixture-a".to_owned(),
                algorithm: "fixture".to_owned(),
                signature: "valid".to_owned(),
            },
            CatalogSignatureV1 {
                key_id: "fixture-b".to_owned(),
                algorithm: "fixture".to_owned(),
                signature: "valid".to_owned(),
            },
        ],
    };
    let (trusted, _) = verify_catalog(
        &catalog,
        &AcceptFixtureSignatures,
        &CatalogTrustPolicy::default(),
        &CatalogTrustState::default(),
        200,
    )
    .unwrap();
    let device = digest("qualified-device");
    let signed_measurement = measurement(
        manifest.identity(),
        manifest_sha256.clone(),
        device,
        candidate.capability.clone(),
        1,
        BTreeMap::from([(ResidencyModeV1::CpuResident, cpu_measurement(500))]),
    );
    let measured =
        verify_fixture_measurement(&signed_measurement, &MeasurementTrustStateV1::default())
            .unwrap()
            .0;
    let qualified = qualify_local_model(&candidate, &trusted, &manifest.identity(), measured)
        .expect("all signed/hash/measurement bindings match");
    assert_eq!(qualified.manifest_sha256(), &manifest_sha256);
    assert_eq!(qualified.catalog_version(), 1);

    let mut mismatched = candidate;
    mismatched.source.expected_source_revision = "different-release".to_owned();
    let measured =
        verify_fixture_measurement(&signed_measurement, &MeasurementTrustStateV1::default())
            .unwrap()
            .0;
    assert!(matches!(
        qualify_local_model(&mismatched, &trusted, &manifest.identity(), measured),
        Err(LocalCatalogError::CandidateManifestMismatch)
    ));

    let mut blocked_catalog = catalog;
    blocked_catalog.signed.version = 2;
    blocked_catalog.signed.entries[0].revoked = true;
    blocked_catalog.signed.entries[0].revocation_reason = Some("fixture revocation".to_owned());
    let (blocked, _) = verify_catalog(
        &blocked_catalog,
        &AcceptFixtureSignatures,
        &CatalogTrustPolicy::default(),
        &CatalogTrustState::default(),
        200,
    )
    .unwrap();
    let measured =
        verify_fixture_measurement(&signed_measurement, &MeasurementTrustStateV1::default())
            .unwrap()
            .0;
    assert!(matches!(
        qualify_local_model(
            &built_in_local_model_candidates_v1()
                .into_iter()
                .find(|candidate| candidate.capability == ModelPackKindV1::SpeechRecognition)
                .unwrap(),
            &blocked,
            &manifest.identity(),
            measured
        ),
        Err(LocalCatalogError::Catalog(CatalogError::Revoked(_)))
    ));
}
