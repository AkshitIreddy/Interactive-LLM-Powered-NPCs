#![allow(clippy::unwrap_used)]

use model_manager::*;
use npc_system_telemetry::{
    AdapterLuid, GraphicsAdapterIdentity, Observation, ObservationProvenance,
    ResourceTelemetrySnapshotV1, TelemetrySource, UnavailableReason, RESOURCE_TELEMETRY_SCHEMA_V1,
};
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;

struct FixtureVerifier;

impl CatalogSignatureVerifier for FixtureVerifier {
    fn is_trusted_key(&self, key_id: &str) -> bool {
        key_id == "fixture-key"
    }

    fn verify(&self, _key_id: &str, algorithm: &str, _message: &[u8], signature: &str) -> bool {
        algorithm == "fixture" && signature == "valid"
    }
}

#[derive(Default)]
struct FixtureEnvelopeSource {
    envelopes: BTreeMap<PackRevision, SignedMeasuredResourceEnvelopeV1>,
}

impl SignedResourceEnvelopeSourceV1 for FixtureEnvelopeSource {
    type Error = Infallible;

    fn load_signed_envelope(
        &self,
        identity: &PackRevision,
        _device_fingerprint_sha256: &Sha256Digest,
        _placement: &ResidencyModeV1,
    ) -> Result<Option<SignedMeasuredResourceEnvelopeV1>, Self::Error> {
        Ok(self.envelopes.get(identity).cloned())
    }
}

fn digest(label: &str) -> Sha256Digest {
    Sha256Digest::of_bytes(label.as_bytes())
}

fn manifest(pack: &str, role: ModelPackKindV1) -> ModelPackManifestV1 {
    let artifact = format!("fixture-{pack}").into_bytes();
    ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: PackId::parse(pack).unwrap(),
        revision: Revision::parse("r1").unwrap(),
        display_name: format!("Fixture {pack}"),
        description: "Deterministic selected-loadout admission fixture.".to_owned(),
        source_project: "https://example.test/model".to_owned(),
        source_revision: format!("commit-{}", &digest(pack).as_str()[..16]),
        capability: ModelPackCapabilityV1 {
            kind: role,
            scope: ModelPackScopeV1::Generic,
        },
        artifacts: vec![ArtifactV1 {
            id: "weights".to_owned(),
            kind: ArtifactKind::File,
            archive_format: None,
            source_urls: vec!["https://example.test/weights.bin".to_owned()],
            size_bytes: artifact.len() as u64,
            sha256: Sha256Digest::of_bytes(&artifact),
            destination: "models/weights.bin".to_owned(),
            strip_prefix: None,
            required_paths: vec![],
        }],
        runtime: RuntimeCompatibilityV1 {
            runtime: "fixture-runtime".to_owned(),
            abi: "npc-fixture-v1".to_owned(),
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
        // These values are intentionally irrelevant to admission. Tests use
        // only separately signed device-bound measurements below.
        resources: ResourceEnvelopeV1 {
            storage_bytes: artifact.len() as u64,
            peak_install_bytes: artifact.len() as u64 * 2,
            measured_ram_bytes: None,
            measured_vram_bytes: None,
            measured_load_millis: None,
            benchmark_hardware: None,
            quality_tier: QualityTier::Experimental,
            languages: BTreeSet::from(["en".to_owned()]),
        },
        license: LicenseMetadataV1 {
            spdx_expression: Some("MIT".to_owned()),
            license_name: "MIT".to_owned(),
            license_url: "https://example.test/license".to_owned(),
            attribution: "Fixture only".to_owned(),
            redistributable: true,
            commercial_use: LicensePermission::Allowed,
            derivative_use: LicensePermission::Allowed,
            acceptance_required: false,
            notices: Vec::new(),
        },
        self_test: SelfTestV1 {
            kind: "fixture".to_owned(),
            suite_revision: "fixture-suite-r1".to_owned(),
            allowed_runtime_backends: BTreeSet::from(["fixture-cpu".to_owned()]),
            input_fixture: "fixtures/input.json".to_owned(),
            expected_output_sha256: Some(digest("expected-output")),
            timeout_millis: 1_000,
        },
    }
}

fn trusted_catalog(manifests: &[ModelPackManifestV1]) -> TrustedCatalog {
    let mut entries: Vec<_> = manifests
        .iter()
        .cloned()
        .map(|manifest| CatalogEntryV1 {
            manifest_sha256: manifest.digest().unwrap(),
            manifest,
            channels: BTreeSet::from(["optional-local".to_owned()]),
            published_unix_seconds: 100,
            revoked: false,
            revocation_reason: None,
        })
        .collect();
    entries.sort_by_key(|entry| entry.manifest.identity());
    let signed = SignedCatalogV1 {
        signed: CatalogPayloadV1 {
            schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
            version: 1,
            generated_unix_seconds: 100,
            expires_unix_seconds: 10_000,
            entries,
        },
        signatures: vec![CatalogSignatureV1 {
            key_id: "fixture-key".to_owned(),
            algorithm: "fixture".to_owned(),
            signature: "valid".to_owned(),
        }],
    };
    verify_catalog(
        &signed,
        &FixtureVerifier,
        &CatalogTrustPolicy {
            signature_threshold: 1,
            maximum_lifetime_seconds: 20_000,
            maximum_clock_skew_seconds: 10,
        },
        &CatalogTrustState::default(),
        200,
    )
    .unwrap()
    .0
}

fn placement(
    ram: u64,
    resident_vram: u64,
    workspace_vram: u64,
    reload: u64,
) -> PlacementMeasurementV1 {
    PlacementMeasurementV1 {
        resident_ram_bytes: ram,
        p99_total_ram_bytes: ram + 100,
        resident_vram_bytes: resident_vram,
        p99_workspace_vram_bytes: workspace_vram,
        p99_load_millis: reload + 10,
        p99_reload_millis: reload,
        p99_operation_millis: 25,
    }
}

fn signed_measurement(
    manifest: &ModelPackManifestV1,
    device: &Sha256Digest,
    placements: BTreeMap<ResidencyModeV1, PlacementMeasurementV1>,
) -> SignedMeasuredResourceEnvelopeV1 {
    SignedMeasuredResourceEnvelopeV1 {
        signed: MeasuredResourceEnvelopePayloadV1 {
            schema: MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1.to_owned(),
            report_id: format!("measure-{}", manifest.pack_id),
            sequence: 1,
            measured_unix_seconds: 100,
            expires_unix_seconds: 10_000,
            device_fingerprint_sha256: device.clone(),
            identity: manifest.identity(),
            manifest_sha256: manifest.digest().unwrap(),
            capability: manifest.capability.kind.clone(),
            benchmark_suite_revision: "fixture-contention-r1".to_owned(),
            runtime: manifest.runtime.runtime.clone(),
            runtime_revision: "r1".to_owned(),
            backend: "fixture".to_owned(),
            sample_count: 32,
            placements,
        },
        signatures: vec![CatalogSignatureV1 {
            key_id: "fixture-key".to_owned(),
            algorithm: "fixture".to_owned(),
            signature: "valid".to_owned(),
        }],
    }
}

fn provenance(source: TelemetrySource) -> ObservationProvenance {
    ObservationProvenance {
        captured_unix_millis: 200_000,
        captured_monotonic_millis: 1_000,
        source,
    }
}

fn available<T>(value: T, source: TelemetrySource) -> Observation<T> {
    Observation::Available {
        value,
        provenance: provenance(source),
    }
}

fn telemetry(device: &Sha256Digest, pid: u32) -> ResourceTelemetrySnapshotV1 {
    ResourceTelemetrySnapshotV1 {
        schema: RESOURCE_TELEMETRY_SCHEMA_V1.to_owned(),
        captured_unix_millis: 200_000,
        captured_monotonic_millis: 1_000,
        selected_game_pid: Some(pid),
        physical_ram_bytes: available(32_000, TelemetrySource::Win32GlobalMemoryStatusEx),
        available_ram_bytes: available(20_000, TelemetrySource::Win32GlobalMemoryStatusEx),
        adapter: available(
            GraphicsAdapterIdentity {
                description: "Fixture GPU".to_owned(),
                luid: AdapterLuid {
                    low_part: 1,
                    high_part: 2,
                },
                vendor_id: 1,
                device_id: 2,
                subsystem_id: 3,
                revision: 4,
            },
            TelemetrySource::DxgiAdapterDescription,
        ),
        device_fingerprint_sha256: available(
            device.as_str().to_owned(),
            TelemetrySource::DxgiAdapterDescription,
        ),
        dedicated_vram_bytes: available(12_000, TelemetrySource::DxgiAdapterDescription),
        os_local_vram_budget_bytes: available(12_000, TelemetrySource::DxgiProcessVideoMemoryInfo),
        current_process_local_vram_bytes: available(0, TelemetrySource::DxgiProcessVideoMemoryInfo),
        total_device_pressure_vram_bytes: available(3_000, TelemetrySource::NvmlDeviceMemoryInfo),
        selected_game_working_set_bytes: available(4_000, TelemetrySource::Win32ProcessMemoryInfo),
        selected_game_vram_bytes: available(1_000, TelemetrySource::NvmlRunningProcesses),
    }
}

fn policy() -> ResourceGovernorPolicyV1 {
    ResourceGovernorPolicyV1 {
        schema: RESOURCE_GOVERNOR_POLICY_SCHEMA_V1.to_owned(),
        vram_soft_ceiling_basis_points: 9_000,
        ram_soft_ceiling_basis_points: 9_000,
        minimum_vram_safety_bytes: 500,
        proportional_vram_safety_basis_points: 1_000,
        minimum_ram_safety_bytes: 500,
        maximum_snapshot_age_millis: 100,
        keep_warm_millis: 10,
        unload_ttl_millis: 100,
    }
}

fn manager(
    omit: Option<ModelPackKindV1>,
) -> (
    SelectedLoadoutManagerV1<FixtureVerifier, FixtureEnvelopeSource>,
    SelectedLoadoutSelectionV1,
    ResourceTelemetrySnapshotV1,
) {
    let device = digest("selected-device");
    let manifests = vec![
        manifest("aggregate.llm", ModelPackKindV1::LanguageModel),
        manifest("aggregate.stt", ModelPackKindV1::SpeechRecognition),
        manifest("aggregate.tts", ModelPackKindV1::SpeechSynthesis),
        manifest("aggregate.embed", ModelPackKindV1::Embedding),
    ];
    let catalog = trusted_catalog(&manifests);
    let mut source = FixtureEnvelopeSource::default();
    for manifest in &manifests {
        if omit.as_ref() == Some(&manifest.capability.kind) {
            continue;
        }
        let placements = if manifest.capability.kind == ModelPackKindV1::LanguageModel {
            BTreeMap::from([
                (
                    ResidencyModeV1::GpuResident,
                    placement(1_000, 2_000, 500, 100),
                ),
                (
                    ResidencyModeV1::CpuResidentGpuCold,
                    placement(1_500, 0, 0, 500),
                ),
            ])
        } else {
            BTreeMap::from([(ResidencyModeV1::CpuResident, placement(500, 0, 0, 50))])
        };
        source.envelopes.insert(
            manifest.identity(),
            signed_measurement(manifest, &device, placements),
        );
    }
    let roles = manifests
        .iter()
        .map(|manifest| SelectedPackV1 {
            role: manifest.capability.kind.clone(),
            identity: manifest.identity(),
            preferred_residency: if manifest.capability.kind == ModelPackKindV1::LanguageModel {
                ResidencyModeV1::GpuResident
            } else {
                ResidencyModeV1::CpuResident
            },
        })
        .collect();
    (
        SelectedLoadoutManagerV1::new(
            catalog,
            FixtureVerifier,
            source,
            MeasurementTrustPolicyV1 {
                signature_threshold: 1,
                minimum_samples: 20,
                maximum_lifetime_seconds: 20_000,
                maximum_clock_skew_seconds: 10,
            },
            policy(),
            WorkQueuePolicyV1 { maximum_pending: 4 },
        )
        .unwrap(),
        SelectedLoadoutSelectionV1 {
            selection_id: "selected-loadout-fixture".to_owned(),
            roles,
            expected_idle_millis: 200,
        },
        telemetry(&device, 4242),
    )
}

fn context<'a>(snapshot: &'a ResourceTelemetrySnapshotV1) -> NativeAdmissionContextV1<'a> {
    NativeAdmissionContextV1 {
        exact_target_pid: Some(4242),
        now_unix_seconds: 200,
        now_monotonic_millis: 1_050,
        configured_game_reserve_vram_bytes: 3_000,
        game_additional_reserve_ram_bytes: 1_000,
        resource_pressure: ResourcePressureLevelV1::Normal,
        telemetry: Some(snapshot),
    }
}

fn setup_snapshot(snapshot: &ResourceTelemetrySnapshotV1) -> ResourceTelemetrySnapshotV1 {
    let mut setup = snapshot.clone();
    setup.selected_game_pid = None;
    setup.selected_game_working_set_bytes = Observation::Unavailable {
        reason: UnavailableReason::GameProcessNotSelected,
        provenance: snapshot
            .selected_game_working_set_bytes
            .provenance()
            .clone(),
    };
    setup.selected_game_vram_bytes = Observation::Unavailable {
        reason: UnavailableReason::GameProcessNotSelected,
        provenance: snapshot.selected_game_vram_bytes.provenance().clone(),
    };
    setup
}

fn setup_context<'a>(snapshot: &'a ResourceTelemetrySnapshotV1) -> NativeAdmissionContextV1<'a> {
    NativeAdmissionContextV1 {
        exact_target_pid: None,
        now_unix_seconds: 200,
        now_monotonic_millis: 1_050,
        configured_game_reserve_vram_bytes: 3_000,
        game_additional_reserve_ram_bytes: 1_000,
        resource_pressure: ResourcePressureLevelV1::Normal,
        telemetry: Some(snapshot),
    }
}

fn catalog_snapshot(selected: &SelectedPackV1) -> TrustedReleasePackSnapshotV1 {
    TrustedReleasePackSnapshotV1 {
        identity: selected.identity.clone(),
        manifest_raw_sha256: digest("fixture-raw-manifest"),
        display_name: "Fixture snapshot".into(),
        description: "Fixture snapshot".into(),
        recommendation_reason: None,
        capability: ModelPackCapabilityV1 {
            kind: selected.role.clone(),
            scope: ModelPackScopeV1::Generic,
        },
        runtime: "fixture-runtime".into(),
        runtime_revision: Some("r1".into()),
        abi: "npc-fixture-v1".into(),
        backends: BTreeSet::from(["fixture-cpu".into()]),
        exact_artifact_download_bytes: 1,
        installed_bytes: 1,
        planning_storage_bytes: 1,
        planning_peak_install_bytes: 2,
        admission_state: ModelPackAdmissionStateV2::BlockedPendingMeasurement,
        admission_reason: "Fixture remains subject to live admission.".into(),
        allowed_residencies: BTreeSet::from([selected.preferred_residency.clone()]),
        license: ModelPackLicenseBindingV1 {
            spdx_expression: Some("MIT".into()),
            license_name: "MIT".into(),
            license_url: "https://example.test/license".into(),
            redistributable: true,
            acceptance_required: false,
            component_ids: BTreeSet::new(),
        },
        lifecycle: ModelPackLifecycleV2 {
            explicit_download_required: true,
            automatic_download_allowed: false,
            install_strategy: InstallStrategyV2::VerifyThenAtomicActivate,
            repair_strategy: RepairStrategyV2::VerifyQuarantineReinstall,
            remove_requires_unreferenced: true,
            activation_gates: BTreeSet::from([
                ActivationGateV2::ExplicitUserApproval,
                ActivationGateV2::CatalogTrust,
                ActivationGateV2::ArtifactIntegrity,
                ActivationGateV2::LicenseAcceptance,
                ActivationGateV2::RuntimeCompatibility,
                ActivationGateV2::SelfTestAttestation,
                ActivationGateV2::CurrentDeviceMeasurement,
                ActivationGateV2::WholeLoadoutAdmission,
            ]),
        },
        non_qualifying_review_evidence_count: 0,
        qualified_envelope_count: 0,
        measurement_status: TrustedPackMeasurementStatusV1::Unavailable,
        measurement_detail: "not inspected".into(),
        qualified_measurement: None,
    }
}

#[test]
fn trusted_pack_snapshot_populates_metrics_only_after_native_envelope_verification() {
    let (mut manager, selection, _) = manager(None);
    let mut snapshot = catalog_snapshot(&selection.roles[0]);
    manager.refresh_trusted_pack_measurement(&mut snapshot, &digest("wrong-device"), 200);
    assert_eq!(
        snapshot.measurement_status,
        TrustedPackMeasurementStatusV1::Unavailable
    );
    assert!(snapshot.qualified_measurement.is_none());

    manager.refresh_trusted_pack_measurement(&mut snapshot, &digest("selected-device"), 200);
    assert_eq!(
        snapshot.measurement_status,
        TrustedPackMeasurementStatusV1::Qualified
    );
    let measurement = snapshot.qualified_measurement.unwrap();
    assert_eq!(measurement.sample_count, 32);
    assert_eq!(
        measurement.placements[&ResidencyModeV1::GpuResident].p99_reload_millis,
        100
    );
}

#[test]
fn aggregate_admits_exact_pid_four_role_co_residency_and_uses_measured_reload_policy() {
    let (mut manager, selection, snapshot) = manager(None);
    manager
        .submit_work(
            WorkItemV1 {
                work_id: "admission-background-embed".to_owned(),
                kind: WorkKindV1::Embedding,
                submitted_monotonic_millis: 1_000,
                deadline_monotonic_millis: 2_000,
                visual: None,
            },
            1_000,
        )
        .unwrap();
    let mut native = context(&snapshot);
    native.resource_pressure = ResourcePressureLevelV1::Elevated;
    let decision = manager.admit(&selection, native);
    assert!(decision.admitted(), "{}", decision.detail);
    assert_eq!(decision.exact_target_pid, Some(4242));
    let receipt = decision.admission_receipt.as_ref().unwrap();
    assert_eq!(receipt.models().len(), 4);
    assert_eq!(receipt.protected_desktop_and_game_vram_bytes(), 5_000);
    assert_eq!(receipt.selected_peak_vram_bytes(), 2_500);
    assert_eq!(receipt.vram_safety_bytes(), 1_200);
    assert_eq!(receipt.projected_total_vram_bytes(), 8_700);

    let llm = decision
        .residency_decisions
        .iter()
        .find(|item| item.role == ModelPackKindV1::LanguageModel)
        .unwrap();
    assert_eq!(llm.disposition, ResidencyDispositionV1::CpuResidentGpuCold);
    assert_eq!(llm.compared_p99_reload_millis, 500);
    assert!(decision
        .residency_decisions
        .iter()
        .filter(|item| item.role != ModelPackKindV1::LanguageModel)
        .all(|item| item.disposition == ResidencyDispositionV1::Unload));
    assert_eq!(
        decision.pressure_cancellations,
        vec![WorkCancellationV1 {
            work_id: "admission-background-embed".to_owned(),
            reason: CancellationReasonV1::QueuePressure,
        }]
    );
    assert_eq!(manager.planner_snapshot().trusted_measurement_streams, 4);

    let serialized = serde_json::to_value(&decision).unwrap();
    assert_eq!(serialized["status"], "admitted");
    assert!(serialized.get("admission_receipt").is_some());
}

#[test]
fn setup_admission_reserves_game_budget_without_minting_target_authority() {
    let (mut setup_manager, selection, runtime_snapshot) = manager(None);
    let setup_snapshot = setup_snapshot(&runtime_snapshot);
    setup_manager
        .submit_work(
            work("setup-background-embed", WorkKindV1::Embedding, None),
            10,
        )
        .unwrap();
    let mut setup_native = setup_context(&setup_snapshot);
    setup_native.resource_pressure = ResourcePressureLevelV1::Elevated;
    let decision = setup_manager.admit_for_setup(&selection, setup_native);
    assert!(decision.admitted(), "{}", decision.detail);
    assert_eq!(decision.exact_target_pid, None);
    assert!(decision.detail.contains("not runtime target authority"));
    let receipt = decision.admission_receipt.expect("setup receipt");
    assert_eq!(receipt.models().len(), 4);
    assert_eq!(receipt.protected_desktop_and_game_vram_bytes(), 6_000);
    assert!(decision.pressure_cancellations.is_empty());
    assert_eq!(
        setup_manager.planner_snapshot().trusted_measurement_streams,
        0
    );
    assert_eq!(setup_manager.planner_snapshot().pending_work, 1);

    let (mut normal, selection, _) = manager(None);
    let blocked = normal.admit(&selection, setup_context(&setup_snapshot));
    assert_eq!(
        blocked.reason_code,
        Some(SelectedLoadoutBlockCodeV1::MissingTargetPid)
    );
    assert_eq!(normal.planner_snapshot().trusted_measurement_streams, 0);

    let (mut contaminated, selection, _) = manager(None);
    let mut invalid = setup_context(&setup_snapshot);
    invalid.exact_target_pid = Some(4242);
    let blocked = contaminated.admit_for_setup(&selection, invalid);
    assert_eq!(
        blocked.reason_code,
        Some(SelectedLoadoutBlockCodeV1::InvalidSelection)
    );
    assert!(blocked.admission_receipt.is_none());
    assert_eq!(
        contaminated.planner_snapshot().trusted_measurement_streams,
        0
    );
}

#[test]
fn aggregate_blocks_missing_envelope_pid_mismatch_and_whole_loadout_contention() {
    let (mut missing, selection, snapshot) = manager(Some(ModelPackKindV1::Embedding));
    let blocked = missing.admit(&selection, context(&snapshot));
    assert_eq!(blocked.status, SelectedLoadoutStatusV1::Blocked);
    assert_eq!(
        blocked.reason_code,
        Some(SelectedLoadoutBlockCodeV1::MissingMeasuredEnvelope)
    );
    assert!(blocked.admission_receipt.is_none());
    // Failed aggregate resolution must not partially advance trusted state.
    assert_eq!(missing.planner_snapshot().trusted_measurement_streams, 0);

    let (mut wrong_pid, selection, snapshot) = manager(None);
    let mut native = context(&snapshot);
    native.exact_target_pid = Some(7);
    let blocked = wrong_pid.admit(&selection, native);
    assert_eq!(
        blocked.reason_code,
        Some(SelectedLoadoutBlockCodeV1::TargetPidMismatch)
    );

    let (mut pressured, selection, snapshot) = manager(None);
    let mut native = context(&snapshot);
    native.configured_game_reserve_vram_bytes = 8_000;
    let blocked = pressured.admit(&selection, native);
    assert_eq!(
        blocked.reason_code,
        Some(SelectedLoadoutBlockCodeV1::VramContention)
    );
}

#[test]
fn public_selection_dto_rejects_native_authority_fields() {
    let json = r#"{
        "selection_id":"selection-strict-fixture",
        "roles":[],
        "expected_idle_millis":100,
        "exact_target_pid":4242
    }"#;
    assert!(serde_json::from_str::<SelectedLoadoutSelectionV1>(json).is_err());
}

fn work(id: &str, kind: WorkKindV1, visual: Option<(u64, u64)>) -> WorkItemV1 {
    WorkItemV1 {
        work_id: id.to_owned(),
        kind,
        submitted_monotonic_millis: 10,
        deadline_monotonic_millis: 1_000,
        visual: visual.map(|(generation_id, frame_id)| VisualWorkAddressV1 {
            generation_id,
            frame_id,
        }),
    }
}

#[test]
fn bounded_priority_queue_sheds_pressure_and_drops_stale_lipsync_deterministically() {
    let mut scheduler =
        WorkSchedulerV1::with_policy(WorkQueuePolicyV1 { maximum_pending: 2 }).unwrap();
    scheduler
        .submit(work("queue-tts-one", WorkKindV1::SpeechSynthesis, None), 10)
        .unwrap();
    scheduler
        .submit(work("queue-tts-two", WorkKindV1::SpeechSynthesis, None), 10)
        .unwrap();
    let result = scheduler
        .submit(
            work("queue-stt-new", WorkKindV1::SpeechRecognition, None),
            10,
        )
        .unwrap();
    assert!(result.accepted);
    assert_eq!(
        result.cancellations,
        vec![WorkCancellationV1 {
            work_id: "queue-tts-one".to_owned(),
            reason: CancellationReasonV1::QueueCapacity,
        }]
    );
    assert_eq!(scheduler.pop_next(11).unwrap().work_id, "queue-stt-new");

    let mut pressure =
        WorkSchedulerV1::with_policy(WorkQueuePolicyV1 { maximum_pending: 8 }).unwrap();
    pressure
        .submit(work("pressure-tts", WorkKindV1::SpeechSynthesis, None), 10)
        .unwrap();
    pressure
        .submit(work("pressure-embed", WorkKindV1::Embedding, None), 10)
        .unwrap();
    assert_eq!(
        pressure.apply_pressure(ResourcePressureLevelV1::Elevated, 11),
        vec![WorkCancellationV1 {
            work_id: "pressure-embed".to_owned(),
            reason: CancellationReasonV1::QueuePressure,
        }]
    );

    pressure
        .submit(
            work("lipsync-current", WorkKindV1::LipSync, Some((2, 7))),
            12,
        )
        .unwrap();
    assert_eq!(
        pressure.advance_visual_clock(2, 8, 13).unwrap(),
        vec![WorkCancellationV1 {
            work_id: "lipsync-current".to_owned(),
            reason: CancellationReasonV1::StaleFrame,
        }]
    );

    pressure
        .submit(work("visual-only", WorkKindV1::LipSync, Some((2, 9))), 14)
        .unwrap();
    pressure
        .submit(work("speech-safe", WorkKindV1::SpeechSynthesis, None), 14)
        .unwrap();
    assert_eq!(
        pressure
            .pop_next_kind(&WorkKindV1::LipSync, 15)
            .unwrap()
            .work_id,
        "visual-only"
    );
    assert_eq!(pressure.pending_by_kind()[&WorkKindV1::SpeechSynthesis], 2);
    assert_eq!(
        pressure.cancel_all_kind(
            &WorkKindV1::SpeechSynthesis,
            CancellationReasonV1::AdmissionRevoked,
        ),
        vec![
            WorkCancellationV1 {
                work_id: "pressure-tts".to_owned(),
                reason: CancellationReasonV1::AdmissionRevoked,
            },
            WorkCancellationV1 {
                work_id: "speech-safe".to_owned(),
                reason: CancellationReasonV1::AdmissionRevoked,
            },
        ]
    );
}
