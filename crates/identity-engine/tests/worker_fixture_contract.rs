#![allow(clippy::unwrap_used)]

use npc_identity_engine::{
    ActorDetectionV1, ActorTrackerV1, BoundingBoxV1, EmbeddingMetadataV1, EmbeddingModelV1,
    IdentityConfigV1, IdentityDecisionV1, PinnedIdentityQualificationV1, PortableReferenceImportV1,
    PortableTensorTransportV1, QualifiedIdentityGalleryV1, QualifiedReferenceProvenanceV1,
    ReferenceSourceClassV1, TrackId, TrackerConfigV1, TrustedCaptureTargetV1,
    TrustedWgcEvidenceAdapterV1, TrustedWgcIdentityFrameV1, QUALIFIED_IDENTITY_SCHEMA_VERSION,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("../../../fixtures/identity/worker-observations.v1.json");

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerObservationEnvelopeV1 {
    contract_version: String,
    authority: String,
    native_revalidation_required: bool,
    schema_version: u32,
    target: TrustedCaptureTargetV1,
    frame_sequence: u64,
    device_generation: u64,
    geometry_epoch: u64,
    source_frame_qpc: u64,
    qpc_frequency: u64,
    captured_at_ms: u64,
    content_sha256: String,
    advancing_frame_verified: bool,
    overlay_capture_excluded: bool,
    protected_online_detected: bool,
    anti_cheat_detected: bool,
    observations: Vec<ActorDetectionV1>,
}

fn digest(value: impl AsRef<[u8]>) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture() -> WorkerObservationEnvelopeV1 {
    serde_json::from_str(FIXTURE).unwrap()
}

fn model() -> EmbeddingModelV1 {
    EmbeddingModelV1 {
        provider: "opencv-zoo".into(),
        model_id: "sface-2021dec-mobilefacenet".into(),
        revision: "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79".into(),
        dimensions: 128,
    }
}

fn qualification() -> PinnedIdentityQualificationV1 {
    PinnedIdentityQualificationV1 {
        qualification_id: "opencv-yunet-sface-private-eval-2026-08-30".into(),
        model: model(),
        detector_id: "opencv-zoo-yunet-2026may".into(),
        detector_revision:
            "sha256:ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0".into(),
        preprocessing: "opencv-face-recognizer-sf-aligncrop-bgr-112x112-l2-f32-v1".into(),
        calibration_fixture_sha256: digest(FIXTURE),
        calibrated_config: IdentityConfigV1 {
            consensus_window_frames: 3,
            minimum_consensus_frames: 3,
            minimum_similarity: 0.72,
            ambiguity_margin: 0.08,
            hysteresis_retention_similarity: 0.66,
            hysteresis_switch_margin: 0.12,
        },
    }
}

fn tensor(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn import(
    subject_id: &str,
    name: &str,
    reference_id: &str,
    axis: usize,
) -> PortableReferenceImportV1 {
    let source_digest = digest(format!("original-reference:{reference_id}"));
    let mut values = vec![0.0_f32; 128];
    values[axis] = 1.0;
    let bytes = tensor(&values);
    PortableReferenceImportV1 {
        schema_version: QUALIFIED_IDENTITY_SCHEMA_VERSION,
        provenance: QualifiedReferenceProvenanceV1 {
            game_profile_id: "eclipse-harbor".into(),
            subject_id: subject_id.into(),
            reference_id: reference_id.into(),
            source_class: ReferenceSourceClassV1::OriginalSynthetic,
            source_content_sha256: source_digest.clone(),
            owner_user_id: None,
            original_work_license: Some("CC0-1.0 original synthetic fixture".into()),
            explicit_user_consent: false,
            local_only: true,
            imported_at_ms: 1,
        },
        subject_display_name: name.into(),
        model: model(),
        metadata: EmbeddingMetadataV1 {
            source_frame_index: 0,
            crop_bounds: BoundingBoxV1::new(0.0, 0.0, 64.0, 64.0).unwrap(),
            detector_id: qualification().detector_id,
            detector_revision: qualification().detector_revision,
            preprocessing: qualification().preprocessing,
            source_digest_sha256: Some(source_digest),
        },
        transport: PortableTensorTransportV1::F32Le,
        tensor_sha256: digest(&bytes),
        tensor_f32le: bytes,
    }
}

fn gallery() -> QualifiedIdentityGalleryV1 {
    let mut gallery = QualifiedIdentityGalleryV1::new("eclipse-harbor", qualification()).unwrap();
    gallery
        .import_reference(import("mara-venn", "Mara Venn", "mara-original-01", 0))
        .unwrap();
    gallery
        .import_reference(import("orin-kade", "Orin Kade", "orin-original-01", 1))
        .unwrap();
    gallery
}

fn target() -> TrustedCaptureTargetV1 {
    fixture().target
}

fn frame(sequence: u64, mut observations: Vec<ActorDetectionV1>) -> TrustedWgcIdentityFrameV1 {
    let content_sha256 = digest(format!("worker-frame:{sequence}"));
    for observation in &mut observations {
        if let Some(embedding) = &mut observation.embedding {
            embedding.metadata.source_frame_index = sequence;
            embedding.metadata.source_digest_sha256 = Some(content_sha256.clone());
        }
    }
    TrustedWgcIdentityFrameV1 {
        schema_version: QUALIFIED_IDENTITY_SCHEMA_VERSION,
        target: target(),
        frame_sequence: sequence,
        device_generation: 1,
        geometry_epoch: 1,
        source_frame_qpc: 10_000 + sequence,
        qpc_frequency: 10_000_000,
        captured_at_ms: sequence * 16,
        content_sha256,
        advancing_frame_verified: true,
        overlay_capture_excluded: true,
        protected_online_detected: false,
        anti_cheat_detected: false,
        observations,
    }
}

fn tracker() -> ActorTrackerV1 {
    let config = TrackerConfigV1 {
        encounter_namespace: "worker-fixture-session".into(),
        maximum_missed_frames: 2,
        selected_actor_maximum_missed_frames: 5,
        ..TrackerConfigV1::default()
    };
    ActorTrackerV1::new(config).unwrap()
}

#[test]
fn worker_json_is_non_authoritative_and_exactly_deserializes_rust_observations() {
    let value = fixture();
    assert_eq!(value.contract_version, "npc.identity-observations/v1");
    assert_eq!(value.authority, "untrusted_worker_observations");
    assert!(value.native_revalidation_required);
    assert_eq!(value.schema_version, 1);
    assert_eq!(value.frame_sequence, 1);
    assert_eq!(value.device_generation, 1);
    assert_eq!(value.geometry_epoch, 1);
    assert_eq!(value.source_frame_qpc, 10_001);
    assert_eq!(value.qpc_frequency, 10_000_000);
    assert_eq!(value.captured_at_ms, 16);
    assert_eq!(value.content_sha256.len(), 64);
    assert!(value.advancing_frame_verified);
    assert!(value.overlay_capture_excluded);
    assert!(!value.protected_online_detected);
    assert!(!value.anti_cheat_detected);
    assert_eq!(value.observations.len(), 2);
    for observation in value.observations {
        let embedding = observation.embedding.unwrap();
        assert_eq!(embedding.values.len(), 128);
        embedding.validate().unwrap();
    }
}

#[test]
fn worker_observations_produce_sticky_tracks_across_reordered_multi_face_frames() {
    let source = fixture().observations;
    let qualified = gallery();
    let mut engine = qualified.into_engine(tracker()).unwrap();
    let mut adapter = TrustedWgcEvidenceAdapterV1::new(target(), qualification()).unwrap();
    let mut last = None;
    for sequence in 1..=3 {
        let mut observations = source.clone();
        for observation in &mut observations {
            observation.bounds.x += sequence as f32 * 2.0;
        }
        if sequence % 2 == 0 {
            observations.reverse();
        }
        last = Some(
            adapter
                .process(&mut engine, frame(sequence, observations))
                .unwrap(),
        );
    }
    let update = last.unwrap();
    let left = update
        .actors
        .iter()
        .min_by(|a, b| a.track.bounds.x.total_cmp(&b.track.bounds.x))
        .unwrap();
    let right = update
        .actors
        .iter()
        .max_by(|a, b| a.track.bounds.x.total_cmp(&b.track.bounds.x))
        .unwrap();
    assert_eq!(left.track.track_id, TrackId(1));
    assert_eq!(right.track.track_id, TrackId(2));
    assert!(
        matches!(left.identity, IdentityDecisionV1::Matched { ref subject_id, .. } if subject_id == "mara-venn")
    );
    assert!(
        matches!(right.identity, IdentityDecisionV1::Matched { ref subject_id, .. } if subject_id == "orin-kade")
    );
}

#[test]
fn calibrated_ambiguity_never_switches_then_manual_correction_persists_offscreen() {
    let mut source = fixture().observations;
    source.truncate(1);
    let embedding = source[0].embedding.as_mut().unwrap();
    embedding.values.fill(0.0);
    let norm = (1.0_f32 + 0.9_f32.powi(2)).sqrt();
    embedding.values[0] = 1.0 / norm;
    embedding.values[1] = 0.9 / norm;

    let qualified = gallery();
    let mut engine = qualified.into_engine(tracker()).unwrap();
    let mut adapter = TrustedWgcEvidenceAdapterV1::new(target(), qualification()).unwrap();
    let mut last = None;
    for sequence in 1..=3 {
        last = Some(
            adapter
                .process(&mut engine, frame(sequence, source.clone()))
                .unwrap(),
        );
    }
    assert!(matches!(
        last.unwrap().actors[0].identity,
        IdentityDecisionV1::Ambiguous { .. }
    ));

    engine.select_actor(TrackId(1)).unwrap();
    engine.assign_selected_subject("mara-venn").unwrap();
    let corrected = adapter.process(&mut engine, frame(4, source)).unwrap();
    assert!(
        matches!(corrected.actors[0].identity, IdentityDecisionV1::Explicit { ref subject_id, .. } if subject_id == "mara-venn")
    );

    let offscreen = adapter.process(&mut engine, frame(5, vec![])).unwrap();
    assert!(
        matches!(offscreen.actors[0].identity, IdentityDecisionV1::Offscreen { last_confirmed_subject_id: Some(ref subject_id), .. } if subject_id == "mara-venn")
    );
}

#[test]
fn native_adapter_rejects_worker_observation_revision_or_frame_digest_drift() {
    let source = fixture().observations;
    let mut adapter = TrustedWgcEvidenceAdapterV1::new(target(), qualification()).unwrap();

    let mut wrong_detector = frame(1, source.clone());
    wrong_detector.observations[0]
        .embedding
        .as_mut()
        .unwrap()
        .metadata
        .detector_revision = "sha256:changed".into();
    assert!(adapter.adapt(wrong_detector).is_err());

    let mut wrong_digest = frame(1, source);
    wrong_digest.observations[0]
        .embedding
        .as_mut()
        .unwrap()
        .metadata
        .source_digest_sha256 = Some("00".repeat(32));
    assert!(adapter.adapt(wrong_digest).is_err());
}
