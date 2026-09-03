// Fixture construction is intentionally infallible and should fail the test at
// the exact setup line if a public validation contract changes.
#![allow(clippy::field_reassign_with_default, clippy::unwrap_used)]

use npc_identity_engine::{
    ActorDetectionV1, ActorIdentityEngineV1, ActorTrackerV1, BoundingBoxV1, EmbeddingMetadataV1,
    EmbeddingModelV1, FrameActorsV1, IdentityConfigV1, IdentityDecisionV1, IdentityGalleryV1,
    IdentityReferenceV1, IdentityResolverV1, NormalizedEmbeddingV1, SubjectIdentityV1, TrackEpoch,
    TrackEventV1, TrackId, TrackPhaseV1, TrackerConfigV1,
};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn model() -> EmbeddingModelV1 {
    EmbeddingModelV1 {
        provider: "fixture".into(),
        model_id: "face-reference".into(),
        revision: "sha256:001122".into(),
        dimensions: 3,
    }
}

fn bounds(x: f32, y: f32) -> BoundingBoxV1 {
    BoundingBoxV1::new(x, y, 2.0, 2.0).unwrap()
}

fn embedding(values: [f32; 3], frame_index: u64, x: f32) -> NormalizedEmbeddingV1 {
    NormalizedEmbeddingV1::new(
        model(),
        EmbeddingMetadataV1 {
            source_frame_index: frame_index,
            crop_bounds: bounds(x, 0.0),
            detector_id: "fixture-detector".into(),
            detector_revision: "1.0.0".into(),
            preprocessing: "rgb-aligned-v1".into(),
            source_digest_sha256: Some("ab".repeat(32)),
        },
        values.to_vec(),
    )
    .unwrap()
}

fn detection(
    local_id: &str,
    x: f32,
    frame_index: u64,
    values: Option<[f32; 3]>,
) -> ActorDetectionV1 {
    ActorDetectionV1 {
        detector_local_id: Some(local_id.into()),
        bounds: bounds(x, 0.0),
        confidence: 0.99,
        embedding: values.map(|values| embedding(values, frame_index, x)),
    }
}

fn frame(frame_index: u64, detections: Vec<ActorDetectionV1>) -> FrameActorsV1 {
    FrameActorsV1 {
        frame_index,
        timestamp_ms: frame_index * 16,
        detections,
    }
}

fn gallery() -> IdentityGalleryV1 {
    let mut gallery = IdentityGalleryV1::new(model()).unwrap();
    gallery
        .insert_subject(SubjectIdentityV1 {
            subject_id: "arden".into(),
            display_name: "Arden".into(),
            references: vec![IdentityReferenceV1 {
                reference_id: "arden-front".into(),
                provenance: "User-approved game portrait crop".into(),
                embedding: embedding([1.0, 0.0, 0.0], 0, 0.0),
            }],
        })
        .unwrap();
    gallery
        .insert_subject(SubjectIdentityV1 {
            subject_id: "mira".into(),
            display_name: "Mira".into(),
            references: vec![IdentityReferenceV1 {
                reference_id: "mira-front".into(),
                provenance: "User-approved game portrait crop".into(),
                embedding: embedding([0.0, 1.0, 0.0], 0, 0.0),
            }],
        })
        .unwrap();
    gallery
}

fn engine_with(
    tracker_config: TrackerConfigV1,
    identity_config: IdentityConfigV1,
) -> ActorIdentityEngineV1 {
    ActorIdentityEngineV1::new(
        ActorTrackerV1::new(tracker_config).unwrap(),
        IdentityResolverV1::new(identity_config, gallery()).unwrap(),
    )
}

#[test]
fn embeddings_are_normalized_versioned_and_portable_json() {
    let value = embedding([3.0, 4.0, 0.0], 7, 12.0);
    let norm = value
        .values
        .iter()
        .map(|part| part * part)
        .sum::<f32>()
        .sqrt();
    assert!((norm - 1.0).abs() < 1.0e-6);
    assert_eq!(value.schema_version, 1);
    assert_eq!(value.model.revision, "sha256:001122");
    assert_eq!(value.metadata.source_frame_index, 7);

    let encoded = serde_json::to_string(&value).unwrap();
    let decoded: NormalizedEmbeddingV1 = serde_json::from_str(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, value);
    assert!(!encoded.contains("pickle"));
}

#[test]
fn gallery_rejects_unversioned_or_demographic_extension_fields() {
    let encoded = serde_json::to_value(gallery()).unwrap();
    let mut subject = encoded["subjects"]["arden"].clone();
    subject["age"] = serde_json::json!(42);
    let result = serde_json::from_value::<SubjectIdentityV1>(subject);
    assert!(
        result.is_err(),
        "unknown demographic fields must not enter the contract"
    );

    let mut invalid = embedding([1.0, 0.0, 0.0], 0, 0.0);
    invalid.schema_version = 99;
    assert!(invalid.validate().is_err());
}

#[test]
fn appearance_keeps_actor_ids_sticky_through_a_crossing_and_reordered_detections() {
    let mut config = TrackerConfigV1::default();
    config.encounter_namespace = "crossing-with-appearance".into();
    config.appearance_weight = 0.6;
    let mut tracker = ActorTrackerV1::new(config).unwrap();

    let first = tracker
        .update(frame(
            1,
            vec![
                detection("a", 0.0, 1, Some([1.0, 0.0, 0.0])),
                detection("b", 10.0, 1, Some([0.0, 1.0, 0.0])),
            ],
        ))
        .unwrap();
    assert_eq!(first.observations[0].track_id, TrackId(1));
    assert_eq!(first.observations[1].track_id, TrackId(2));

    tracker
        .update(frame(
            2,
            vec![
                detection("b", 6.0, 2, Some([0.0, 1.0, 0.0])),
                detection("a", 4.0, 2, Some([1.0, 0.0, 0.0])),
            ],
        ))
        .unwrap();
    let crossed = tracker
        .update(frame(
            3,
            vec![
                detection("b", 2.0, 3, Some([0.0, 1.0, 0.0])),
                detection("a", 8.0, 3, Some([1.0, 0.0, 0.0])),
            ],
        ))
        .unwrap();
    let track_one = crossed
        .tracks
        .iter()
        .find(|track| track.track_id == TrackId(1))
        .unwrap();
    let track_two = crossed
        .tracks
        .iter()
        .find(|track| track.track_id == TrackId(2))
        .unwrap();
    assert_eq!(track_one.bounds.x, 8.0);
    assert_eq!(track_two.bounds.x, 2.0);
}

#[test]
fn motion_prediction_handles_crossing_without_embeddings() {
    let mut config = TrackerConfigV1::default();
    config.encounter_namespace = "crossing-motion-only".into();
    config.velocity_smoothing = 1.0;
    config.maximum_normalized_center_distance = 4.0;
    let mut tracker = ActorTrackerV1::new(config).unwrap();
    tracker
        .update(frame(
            1,
            vec![detection("a", 0.0, 1, None), detection("b", 10.0, 1, None)],
        ))
        .unwrap();
    tracker
        .update(frame(
            2,
            vec![detection("b", 7.0, 2, None), detection("a", 3.0, 2, None)],
        ))
        .unwrap();
    let crossed = tracker
        .update(frame(
            3,
            vec![detection("a", 6.0, 3, None), detection("b", 4.0, 3, None)],
        ))
        .unwrap();
    assert_eq!(
        crossed
            .tracks
            .iter()
            .find(|track| track.track_id == TrackId(1))
            .unwrap()
            .bounds
            .x,
        6.0
    );
    assert_eq!(
        crossed
            .tracks
            .iter()
            .find(|track| track.track_id == TrackId(2))
            .unwrap()
            .bounds
            .x,
        4.0
    );
}

#[test]
fn loss_and_reacquisition_advance_epoch_while_selection_is_preserved() {
    let mut config = TrackerConfigV1::default();
    config.encounter_namespace = "selected-reacquisition".into();
    config.maximum_missed_frames = 1;
    config.selected_actor_maximum_missed_frames = 5;
    let mut tracker = ActorTrackerV1::new(config).unwrap();
    tracker
        .update(frame(1, vec![detection("actor", 2.0, 1, None)]))
        .unwrap();
    tracker.select_actor(TrackId(1)).unwrap();

    let lost = tracker.update(frame(2, vec![])).unwrap();
    assert_eq!(lost.selected_track_id, Some(TrackId(1)));
    assert_eq!(lost.tracks[0].phase, TrackPhaseV1::Offscreen);
    assert_eq!(lost.tracks[0].track_epoch, TrackEpoch(2));
    assert!(lost.events.iter().any(|event| matches!(
        event,
        TrackEventV1::Lost {
            track_id: TrackId(1),
            track_epoch: TrackEpoch(2)
        }
    )));

    let reacquired = tracker
        .update(frame(4, vec![detection("actor", 2.1, 4, None)]))
        .unwrap();
    assert_eq!(reacquired.selected_track_id, Some(TrackId(1)));
    assert_eq!(reacquired.tracks[0].track_epoch, TrackEpoch(3));
    assert!(reacquired.events.iter().any(|event| matches!(
        event,
        TrackEventV1::Reacquired {
            track_id: TrackId(1),
            track_epoch: TrackEpoch(3)
        }
    )));
}

#[test]
fn unselected_actor_retires_and_a_return_is_a_new_background_encounter() {
    let mut config = TrackerConfigV1::default();
    config.encounter_namespace = "retirement".into();
    config.maximum_missed_frames = 1;
    config.selected_actor_maximum_missed_frames = 1;
    let mut tracker = ActorTrackerV1::new(config).unwrap();
    let first = tracker
        .update(frame(1, vec![detection("actor", 2.0, 1, None)]))
        .unwrap();
    let first_encounter = first.tracks[0].encounter_id.clone();
    let retired = tracker.update(frame(3, vec![])).unwrap();
    assert!(retired.tracks.is_empty());
    assert!(retired.events.iter().any(|event| matches!(
        event,
        TrackEventV1::Retired {
            track_id: TrackId(1),
            ..
        }
    )));
    let returned = tracker
        .update(frame(4, vec![detection("actor", 2.0, 4, None)]))
        .unwrap();
    assert_eq!(returned.tracks[0].track_id, TrackId(2));
    assert_ne!(returned.tracks[0].encounter_id, first_encounter);
}

#[test]
fn multi_frame_consensus_requires_three_supporting_frames() {
    let mut engine = engine_with(TrackerConfigV1::default(), IdentityConfigV1::default());
    for frame_index in 1..=2 {
        let update = engine
            .process_frame(frame(
                frame_index,
                vec![detection("actor", 1.0, frame_index, Some([1.0, 0.02, 0.0]))],
            ))
            .unwrap();
        assert!(matches!(
            update.actors[0].identity,
            IdentityDecisionV1::Pending { .. }
        ));
    }
    let resolved = engine
        .process_frame(frame(
            3,
            vec![detection("actor", 1.0, 3, Some([1.0, 0.02, 0.0]))],
        ))
        .unwrap();
    match &resolved.actors[0].identity {
        IdentityDecisionV1::Matched {
            subject_id,
            supporting_frames,
            top1_top2_margin,
            ..
        } => {
            assert_eq!(subject_id, "arden");
            assert_eq!(*supporting_frames, 3);
            assert!(*top1_top2_margin > 0.9);
        }
        other => panic!("expected a match, got {other:?}"),
    }
}

#[test]
fn close_top_one_and_top_two_scores_are_explicitly_ambiguous() {
    let mut identity_config = IdentityConfigV1::default();
    identity_config.minimum_similarity = 0.65;
    identity_config.hysteresis_retention_similarity = 0.60;
    let mut engine = engine_with(TrackerConfigV1::default(), identity_config);
    let mut result = None;
    for frame_index in 1..=3 {
        result = Some(
            engine
                .process_frame(frame(
                    frame_index,
                    vec![detection("actor", 1.0, frame_index, Some([1.0, 1.0, 0.0]))],
                ))
                .unwrap(),
        );
    }
    match &result.unwrap().actors[0].identity {
        IdentityDecisionV1::Ambiguous {
            top_subject_id,
            runner_up_subject_id,
            top1_top2_margin,
            ..
        } => {
            assert_eq!(top_subject_id, "arden");
            assert_eq!(runner_up_subject_id.as_deref(), Some("mira"));
            assert!(top1_top2_margin.abs() < 1.0e-6);
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
}

#[test]
fn low_similarity_is_an_explicit_no_match_with_stable_encounter_id() {
    let mut engine = engine_with(TrackerConfigV1::default(), IdentityConfigV1::default());
    let mut encounter = None;
    let mut result = None;
    for frame_index in 1..=3 {
        let update = engine
            .process_frame(frame(
                frame_index,
                vec![detection("actor", 1.0, frame_index, Some([0.0, 0.0, 1.0]))],
            ))
            .unwrap();
        if let Some(expected) = &encounter {
            assert_eq!(update.actors[0].identity.encounter_id(), expected);
        } else {
            encounter = Some(update.actors[0].identity.encounter_id().to_owned());
        }
        result = Some(update);
    }
    assert!(matches!(
        result.unwrap().actors[0].identity,
        IdentityDecisionV1::NoMatch {
            best_similarity: Some(score),
            ..
        } if score.abs() < 1.0e-6
    ));
}

#[test]
fn hysteresis_prevents_a_small_challenger_advantage_from_flipping_identity() {
    let mut identity_config = IdentityConfigV1::default();
    identity_config.consensus_window_frames = 3;
    let mut engine = engine_with(TrackerConfigV1::default(), identity_config);
    for frame_index in 1..=3 {
        engine
            .process_frame(frame(
                frame_index,
                vec![detection("actor", 1.0, frame_index, Some([1.0, 0.0, 0.0]))],
            ))
            .unwrap();
    }
    let mut final_update = None;
    for frame_index in 4..=6 {
        final_update = Some(
            engine
                .process_frame(frame(
                    frame_index,
                    vec![detection(
                        "actor",
                        1.0,
                        frame_index,
                        Some([0.70, 0.714, 0.0]),
                    )],
                ))
                .unwrap(),
        );
    }
    assert!(matches!(
        final_update.unwrap().actors[0].identity,
        IdentityDecisionV1::Matched {
            ref subject_id,
            ref top_candidate_subject_id,
            top1_top2_margin,
            held_by_hysteresis: true,
            ..
        } if subject_id == "arden"
            && top_candidate_subject_id == "mira"
            && top1_top2_margin > 0.0
    ));
}

#[test]
fn explicit_selected_actor_survives_offscreen_and_reacquisition() {
    let mut tracker_config = TrackerConfigV1::default();
    tracker_config.maximum_missed_frames = 1;
    tracker_config.selected_actor_maximum_missed_frames = 5;
    let mut engine = engine_with(tracker_config, IdentityConfigV1::default());
    engine
        .process_frame(frame(
            1,
            vec![detection("actor", 1.0, 1, Some([0.0, 0.0, 1.0]))],
        ))
        .unwrap();
    engine.select_actor(TrackId(1)).unwrap();
    engine.assign_selected_subject("mira").unwrap();

    let offscreen = engine.process_frame(frame(2, vec![])).unwrap();
    assert_eq!(offscreen.selected_track_id, Some(TrackId(1)));
    assert!(matches!(
        offscreen.actors[0].identity,
        IdentityDecisionV1::Offscreen {
            ref last_confirmed_subject_id,
            track_epoch: TrackEpoch(2),
            ..
        } if last_confirmed_subject_id.as_deref() == Some("mira")
    ));

    let returned = engine
        .process_frame(frame(
            4,
            vec![detection("actor", 1.1, 4, Some([0.0, 0.0, 1.0]))],
        ))
        .unwrap();
    assert_eq!(returned.actors[0].track.track_id, TrackId(1));
    assert_eq!(returned.actors[0].track.track_epoch, TrackEpoch(3));
    assert!(matches!(
        returned.actors[0].identity,
        IdentityDecisionV1::Explicit { ref subject_id, .. } if subject_id == "mira"
    ));
}

#[test]
fn background_encounter_ids_are_deterministic_for_the_same_replay() {
    fn replay() -> Vec<String> {
        let mut config = TrackerConfigV1::default();
        config.encounter_namespace = "game-session-fixture".into();
        let mut tracker = ActorTrackerV1::new(config).unwrap();
        tracker
            .update(frame(
                100,
                vec![
                    detection("left", 1.0, 100, None),
                    detection("right", 9.0, 100, None),
                ],
            ))
            .unwrap()
            .tracks
            .into_iter()
            .map(|track| track.encounter_id)
            .collect()
    }
    let first = replay();
    let second = replay();
    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_ne!(first[0], first[1]);
    assert!(first.iter().all(|id| id.starts_with("enc_v1_")));
}

#[test]
fn frame_order_and_duplicate_detector_ids_are_rejected() {
    let mut tracker = ActorTrackerV1::new(TrackerConfigV1::default()).unwrap();
    tracker.update(frame(1, vec![])).unwrap();
    assert!(tracker.update(frame(1, vec![])).is_err());

    let mut tracker = ActorTrackerV1::new(TrackerConfigV1::default()).unwrap();
    assert!(tracker
        .update(frame(
            1,
            vec![
                detection("same", 1.0, 1, None),
                detection("same", 4.0, 1, None)
            ]
        ))
        .is_err());
}

#[test]
fn low_confidence_detections_are_ignored_without_poisoning_the_frame() {
    let mut tracker = ActorTrackerV1::new(TrackerConfigV1::default()).unwrap();
    let mut weak = detection("weak", 1.0, 1, None);
    weak.confidence = 0.2;
    let update = tracker.update(frame(1, vec![weak])).unwrap();
    assert!(update.tracks.is_empty());
    assert!(update.observations.is_empty());
}

proptest! {
    #[test]
    fn normalized_embedding_constructor_always_produces_unit_norm(
        x in -1000.0f32..1000.0,
        y in -1000.0f32..1000.0,
        z in -1000.0f32..1000.0,
    ) {
        prop_assume!(x.hypot(y).hypot(z) > 1.0e-4);
        let value = embedding([x, y, z], 1, 0.0);
        let norm = value.values.iter().map(|part| part * part).sum::<f32>().sqrt();
        prop_assert!((norm - 1.0).abs() < 1.0e-3);
    }

    #[test]
    fn intersection_over_union_is_symmetric_and_bounded(
        ax in -1000.0f32..1000.0,
        ay in -1000.0f32..1000.0,
        aw in 0.01f32..100.0,
        ah in 0.01f32..100.0,
        bx in -1000.0f32..1000.0,
        by in -1000.0f32..1000.0,
        bw in 0.01f32..100.0,
        bh in 0.01f32..100.0,
    ) {
        let a = BoundingBoxV1::new(ax, ay, aw, ah).unwrap();
        let b = BoundingBoxV1::new(bx, by, bw, bh).unwrap();
        let ab = a.intersection_over_union(b);
        let ba = b.intersection_over_union(a);
        prop_assert!((ab - ba).abs() < 1.0e-6);
        prop_assert!((0.0..=1.0).contains(&ab));
    }

    #[test]
    fn a_frame_never_assigns_one_track_to_multiple_detections(
        xs in proptest::collection::vec(-50.0f32..50.0, 1..12),
    ) {
        let mut config = TrackerConfigV1::default();
        config.encounter_namespace = "one-to-one-property".into();
        let mut tracker = ActorTrackerV1::new(config).unwrap();
        let detections = xs
            .iter()
            .enumerate()
            .map(|(index, x)| detection(&format!("d{index}"), *x, 1, None))
            .collect();
        tracker.update(frame(1, detections)).unwrap();
        let second_detections = xs
            .iter()
            .enumerate()
            .map(|(index, x)| detection(&format!("next{index}"), *x + 0.25, 2, None))
            .collect();
        let update = tracker.update(frame(2, second_detections)).unwrap();
        let unique: BTreeSet<_> = update
            .observations
            .iter()
            .map(|observation| observation.track_id)
            .collect();
        prop_assert_eq!(unique.len(), update.observations.len());
    }
}
