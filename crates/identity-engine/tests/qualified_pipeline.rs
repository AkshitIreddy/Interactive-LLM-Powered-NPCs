#![allow(clippy::unwrap_used)]

use npc_identity_engine::{
    ActorDetectionV1, ActorTrackerV1, BoundingBoxV1, EmbeddingMetadataV1, EmbeddingModelV1,
    IdentityConfigV1, IdentityDecisionV1, PinnedIdentityQualificationV1, PortableReferenceImportV1,
    PortableTensorTransportV1, QualifiedIdentityGalleryV1, QualifiedReferenceProvenanceV1,
    ReferenceSourceClassV1, TrackId, TrackerConfigV1, TrustedCaptureTargetV1,
    TrustedWgcEvidenceAdapterV1, TrustedWgcIdentityFrameV1, QUALIFIED_IDENTITY_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn model() -> EmbeddingModelV1 {
    EmbeddingModelV1 {
        provider: "local-qualified-fixture".into(),
        model_id: "appearance-reference".into(),
        revision: "sha256:qualified-revision-1".into(),
        dimensions: 3,
    }
}

fn config() -> IdentityConfigV1 {
    IdentityConfigV1 {
        consensus_window_frames: 3,
        minimum_consensus_frames: 3,
        minimum_similarity: 0.72,
        ambiguity_margin: 0.08,
        hysteresis_retention_similarity: 0.66,
        hysteresis_switch_margin: 0.12,
    }
}

fn qualification() -> PinnedIdentityQualificationV1 {
    PinnedIdentityQualificationV1 {
        qualification_id: "appearance-reference-review-1".into(),
        model: model(),
        detector_id: "fixture-detector".into(),
        detector_revision: "1.0.0".into(),
        preprocessing: "rgb-aligned-v1".into(),
        calibration_fixture_sha256: "ab".repeat(32),
        calibrated_config: config(),
    }
}

fn tensor(values: [f32; 3]) -> Vec<u8> {
    values
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>()
}

fn import(
    subject_id: &str,
    display_name: &str,
    reference_id: &str,
    values: [f32; 3],
    source_class: ReferenceSourceClassV1,
) -> PortableReferenceImportV1 {
    let source_digest = hash(format!("private-source:{reference_id}").as_bytes());
    let bytes = tensor(values);
    PortableReferenceImportV1 {
        schema_version: QUALIFIED_IDENTITY_SCHEMA_VERSION,
        provenance: QualifiedReferenceProvenanceV1 {
            game_profile_id: "eclipse-harbor".into(),
            subject_id: subject_id.into(),
            reference_id: reference_id.into(),
            source_class,
            source_content_sha256: source_digest.clone(),
            owner_user_id: (source_class == ReferenceSourceClassV1::UserPrivate)
                .then(|| "local-user".into()),
            original_work_license: (source_class == ReferenceSourceClassV1::OriginalSynthetic)
                .then(|| "MIT".into()),
            explicit_user_consent: source_class == ReferenceSourceClassV1::UserPrivate,
            local_only: true,
            imported_at_ms: 10,
        },
        subject_display_name: display_name.into(),
        model: model(),
        metadata: EmbeddingMetadataV1 {
            source_frame_index: 0,
            crop_bounds: BoundingBoxV1::new(0.0, 0.0, 64.0, 64.0).unwrap(),
            detector_id: "fixture-detector".into(),
            detector_revision: "1.0.0".into(),
            preprocessing: "rgb-aligned-v1".into(),
            source_digest_sha256: Some(source_digest),
        },
        transport: PortableTensorTransportV1::F32Le,
        tensor_sha256: hash(&bytes),
        tensor_f32le: bytes,
    }
}

fn qualified_gallery() -> QualifiedIdentityGalleryV1 {
    let mut gallery = QualifiedIdentityGalleryV1::new("eclipse-harbor", qualification()).unwrap();
    gallery
        .import_reference(import(
            "mara-venn",
            "Mara Venn",
            "mara-original-front",
            [1.0, 0.0, 0.0],
            ReferenceSourceClassV1::OriginalSynthetic,
        ))
        .unwrap();
    gallery
        .import_reference(import(
            "orin-kade",
            "Orin Kade",
            "orin-private-front",
            [0.0, 1.0, 0.0],
            ReferenceSourceClassV1::UserPrivate,
        ))
        .unwrap();
    gallery
}

fn target() -> TrustedCaptureTargetV1 {
    TrustedCaptureTargetV1 {
        capture_session_id: "native-capture-session-1".into(),
        process_id: 42,
        window_handle: 777,
        executable_name: "EclipseHarbor.exe".into(),
    }
}

fn frame(sequence: u64, values: Option<[f32; 3]>) -> TrustedWgcIdentityFrameV1 {
    let content = hash(format!("advancing-frame:{sequence}").as_bytes());
    let observation = values.map(|values| ActorDetectionV1 {
        detector_local_id: Some("face-0".into()),
        bounds: BoundingBoxV1::new(100.0, 80.0, 200.0, 240.0).unwrap(),
        confidence: 0.99,
        embedding: Some(
            npc_identity_engine::NormalizedEmbeddingV1::new(
                model(),
                EmbeddingMetadataV1 {
                    source_frame_index: sequence,
                    crop_bounds: BoundingBoxV1::new(100.0, 80.0, 200.0, 240.0).unwrap(),
                    detector_id: "fixture-detector".into(),
                    detector_revision: "1.0.0".into(),
                    preprocessing: "rgb-aligned-v1".into(),
                    source_digest_sha256: Some(content.clone()),
                },
                values.to_vec(),
            )
            .unwrap(),
        ),
    });
    TrustedWgcIdentityFrameV1 {
        schema_version: QUALIFIED_IDENTITY_SCHEMA_VERSION,
        target: target(),
        frame_sequence: sequence,
        device_generation: 1,
        geometry_epoch: 1,
        source_frame_qpc: 10_000 + sequence,
        qpc_frequency: 10_000_000,
        captured_at_ms: sequence * 16,
        content_sha256: content,
        advancing_frame_verified: true,
        overlay_capture_excluded: true,
        protected_online_detected: false,
        anti_cheat_detected: false,
        observations: observation.into_iter().collect(),
    }
}

#[test]
fn imports_only_explicit_private_or_original_f32le_references() {
    let gallery = qualified_gallery();
    gallery.validate().unwrap();
    assert_eq!(gallery.gallery.subjects.len(), 2);
    assert_eq!(gallery.reference_provenance.len(), 2);
    assert!(gallery
        .reference_provenance
        .values()
        .any(|reference| reference.source_class == ReferenceSourceClassV1::UserPrivate));

    let mut encoded = serde_json::to_value(import(
        "mara-venn",
        "Mara Venn",
        "pickle-must-not-enter",
        [1.0, 0.0, 0.0],
        ReferenceSourceClassV1::UserPrivate,
    ))
    .unwrap();
    encoded["transport"] = serde_json::json!("pickle");
    assert!(serde_json::from_value::<PortableReferenceImportV1>(encoded).is_err());

    let mut missing_consent = import(
        "mara-venn",
        "Mara Venn",
        "private-without-consent",
        [1.0, 0.0, 0.0],
        ReferenceSourceClassV1::UserPrivate,
    );
    missing_consent.provenance.explicit_user_consent = false;
    let mut gallery = qualified_gallery();
    assert!(gallery.import_reference(missing_consent).is_err());
}

#[test]
fn calibrated_wgc_replay_is_ambiguous_then_manual_correction_is_authoritative_and_offscreen() {
    let qualified = qualified_gallery();
    let tracker_config = TrackerConfigV1 {
        encounter_namespace: "eclipse-harbor-native-session".into(),
        maximum_missed_frames: 2,
        selected_actor_maximum_missed_frames: 5,
        ..TrackerConfigV1::default()
    };
    let tracker = ActorTrackerV1::new(tracker_config).unwrap();
    let mut engine = qualified.into_engine(tracker).unwrap();
    let mut adapter = TrustedWgcEvidenceAdapterV1::new(target(), qualification()).unwrap();

    let mut last = None;
    for sequence in 1..=3 {
        last = Some(
            adapter
                .process(&mut engine, frame(sequence, Some([1.0, 0.9, 0.0])))
                .unwrap(),
        );
    }
    let ambiguous = &last.unwrap().actors[0].identity;
    assert!(matches!(ambiguous, IdentityDecisionV1::Ambiguous { .. }));

    engine.select_actor(TrackId(1)).unwrap();
    engine.assign_selected_subject("mara-venn").unwrap();
    let corrected = adapter
        .process(&mut engine, frame(4, Some([1.0, 0.9, 0.0])))
        .unwrap();
    assert!(matches!(
        corrected.actors[0].identity,
        IdentityDecisionV1::Explicit { ref subject_id, .. } if subject_id == "mara-venn"
    ));

    let offscreen = adapter.process(&mut engine, frame(5, None)).unwrap();
    assert!(matches!(
        offscreen.actors[0].identity,
        IdentityDecisionV1::Offscreen {
            last_confirmed_subject_id: Some(ref subject_id),
            ..
        } if subject_id == "mara-venn"
    ));
}

#[test]
fn wgc_adapter_rejects_replay_stale_target_and_unqualified_generation() {
    let mut adapter = TrustedWgcEvidenceAdapterV1::new(target(), qualification()).unwrap();
    adapter.adapt(frame(1, Some([1.0, 0.0, 0.0]))).unwrap();
    assert!(adapter.adapt(frame(1, Some([1.0, 0.0, 0.0]))).is_err());

    let mut wrong_target = frame(2, Some([1.0, 0.0, 0.0]));
    wrong_target.target.window_handle = 778;
    assert!(adapter.adapt(wrong_target).is_err());

    let mut changed_generation = frame(2, Some([1.0, 0.0, 0.0]));
    changed_generation.device_generation = 2;
    assert!(adapter.adapt(changed_generation).is_err());
}
