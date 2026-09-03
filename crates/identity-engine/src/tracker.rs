use crate::{BoundingBoxV1, EmbeddingError, NormalizedEmbeddingV1};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackEpoch(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackPhaseV1 {
    Visible,
    Offscreen,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorDetectionV1 {
    /// Optional ID scoped only to this detector frame. It is never treated as
    /// an actor identity.
    pub detector_local_id: Option<String>,
    pub bounds: BoundingBoxV1,
    pub confidence: f32,
    pub embedding: Option<NormalizedEmbeddingV1>,
}

impl ActorDetectionV1 {
    fn validate(&self) -> Result<(), TrackerError> {
        self.bounds.validate()?;
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(TrackerError::InvalidDetectionConfidence);
        }
        if self
            .detector_local_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err(TrackerError::InvalidDetectorLocalId);
        }
        if let Some(embedding) = &self.embedding {
            embedding.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameActorsV1 {
    pub frame_index: u64,
    pub timestamp_ms: u64,
    pub detections: Vec<ActorDetectionV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackerConfigV1 {
    pub encounter_namespace: String,
    pub minimum_detection_confidence: f32,
    pub minimum_intersection_over_union: f32,
    pub maximum_normalized_center_distance: f32,
    pub minimum_appearance_similarity: f32,
    pub appearance_weight: f32,
    pub selected_actor_match_bonus: f32,
    pub velocity_smoothing: f32,
    pub maximum_missed_frames: u64,
    pub selected_actor_maximum_missed_frames: u64,
}

impl Default for TrackerConfigV1 {
    fn default() -> Self {
        Self {
            encounter_namespace: "default-session".to_owned(),
            minimum_detection_confidence: 0.35,
            minimum_intersection_over_union: 0.05,
            maximum_normalized_center_distance: 2.5,
            minimum_appearance_similarity: 0.65,
            appearance_weight: 0.25,
            selected_actor_match_bonus: 0.08,
            velocity_smoothing: 0.55,
            maximum_missed_frames: 4,
            selected_actor_maximum_missed_frames: 30,
        }
    }
}

impl TrackerConfigV1 {
    pub fn validate(&self) -> Result<(), TrackerError> {
        if self.encounter_namespace.trim().is_empty() || self.encounter_namespace.len() > 256 {
            return Err(TrackerError::InvalidEncounterNamespace);
        }
        for value in [
            self.minimum_detection_confidence,
            self.minimum_intersection_over_union,
            self.appearance_weight,
            self.velocity_smoothing,
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(TrackerError::InvalidConfiguration);
            }
        }
        if !self.minimum_appearance_similarity.is_finite()
            || !(-1.0..=1.0).contains(&self.minimum_appearance_similarity)
            || !self.maximum_normalized_center_distance.is_finite()
            || self.maximum_normalized_center_distance <= 0.0
            || !self.selected_actor_match_bonus.is_finite()
            || self.selected_actor_match_bonus < 0.0
            || self.selected_actor_maximum_missed_frames < self.maximum_missed_frames
        {
            return Err(TrackerError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorTrackSnapshotV1 {
    pub track_id: TrackId,
    pub track_epoch: TrackEpoch,
    pub phase: TrackPhaseV1,
    pub bounds: BoundingBoxV1,
    pub velocity_x_per_frame: f32,
    pub velocity_y_per_frame: f32,
    pub first_seen_frame: u64,
    pub last_seen_frame: u64,
    pub missed_frames: u64,
    pub selected: bool,
    pub encounter_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackEventV1 {
    Created {
        track_id: TrackId,
        track_epoch: TrackEpoch,
    },
    Updated {
        track_id: TrackId,
        track_epoch: TrackEpoch,
    },
    Lost {
        track_id: TrackId,
        track_epoch: TrackEpoch,
    },
    Reacquired {
        track_id: TrackId,
        track_epoch: TrackEpoch,
    },
    Retired {
        track_id: TrackId,
        final_epoch: TrackEpoch,
        encounter_id: String,
    },
    SelectionCleared {
        retired_track_id: TrackId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackedDetectionV1 {
    pub detection_index: usize,
    pub track_id: TrackId,
    pub track_epoch: TrackEpoch,
    pub embedding: Option<NormalizedEmbeddingV1>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackerUpdateV1 {
    pub frame_index: u64,
    pub tracks: Vec<ActorTrackSnapshotV1>,
    pub observations: Vec<TrackedDetectionV1>,
    pub events: Vec<TrackEventV1>,
    pub selected_track_id: Option<TrackId>,
}

#[derive(Clone, Debug)]
struct TrackState {
    track_id: TrackId,
    track_epoch: TrackEpoch,
    phase: TrackPhaseV1,
    bounds: BoundingBoxV1,
    velocity_x_per_frame: f32,
    velocity_y_per_frame: f32,
    first_seen_frame: u64,
    last_seen_frame: u64,
    missed_frames: u64,
    encounter_id: String,
    last_embedding: Option<NormalizedEmbeddingV1>,
}

impl TrackState {
    fn snapshot(&self, selected: bool) -> ActorTrackSnapshotV1 {
        ActorTrackSnapshotV1 {
            track_id: self.track_id,
            track_epoch: self.track_epoch,
            phase: self.phase,
            bounds: self.bounds,
            velocity_x_per_frame: self.velocity_x_per_frame,
            velocity_y_per_frame: self.velocity_y_per_frame,
            first_seen_frame: self.first_seen_frame,
            last_seen_frame: self.last_seen_frame,
            missed_frames: self.missed_frames,
            selected,
            encounter_id: self.encounter_id.clone(),
        }
    }

    fn predicted_bounds(&self, frame_index: u64) -> BoundingBoxV1 {
        let elapsed = frame_index.saturating_sub(self.last_seen_frame) as f32;
        self.bounds.translated(
            self.velocity_x_per_frame * elapsed,
            self.velocity_y_per_frame * elapsed,
        )
    }
}

#[derive(Clone, Debug)]
pub struct ActorTrackerV1 {
    config: TrackerConfigV1,
    next_track_id: u64,
    tracks: BTreeMap<TrackId, TrackState>,
    selected_track_id: Option<TrackId>,
    last_frame_index: Option<u64>,
    last_timestamp_ms: Option<u64>,
}

impl ActorTrackerV1 {
    pub fn new(config: TrackerConfigV1) -> Result<Self, TrackerError> {
        config.validate()?;
        Ok(Self {
            config,
            next_track_id: 1,
            tracks: BTreeMap::new(),
            selected_track_id: None,
            last_frame_index: None,
            last_timestamp_ms: None,
        })
    }

    pub fn config(&self) -> &TrackerConfigV1 {
        &self.config
    }

    pub fn selected_track_id(&self) -> Option<TrackId> {
        self.selected_track_id
    }

    pub fn select_actor(&mut self, track_id: TrackId) -> Result<(), TrackerError> {
        if !self.tracks.contains_key(&track_id) {
            return Err(TrackerError::UnknownTrack(track_id));
        }
        self.selected_track_id = Some(track_id);
        Ok(())
    }

    pub fn clear_selection(&mut self) {
        self.selected_track_id = None;
    }

    pub fn snapshot(&self) -> Vec<ActorTrackSnapshotV1> {
        self.tracks
            .values()
            .map(|track| track.snapshot(self.selected_track_id == Some(track.track_id)))
            .collect()
    }

    pub fn update(&mut self, frame: FrameActorsV1) -> Result<TrackerUpdateV1, TrackerError> {
        self.validate_frame(&frame)?;
        let eligible_detections: BTreeSet<usize> = frame
            .detections
            .iter()
            .enumerate()
            .filter_map(|(index, detection)| {
                (detection.confidence >= self.config.minimum_detection_confidence).then_some(index)
            })
            .collect();

        let mut candidate_pairs = Vec::new();
        for track in self.tracks.values() {
            for (detection_index, detection) in frame.detections.iter().enumerate() {
                if !eligible_detections.contains(&detection_index) {
                    continue;
                }
                if let Some(score) = self.association_score(track, detection, frame.frame_index) {
                    candidate_pairs.push(CandidatePair {
                        track_id: track.track_id,
                        detection_index,
                        score,
                    });
                }
            }
        }
        candidate_pairs.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.track_id.cmp(&right.track_id))
                .then_with(|| left.detection_index.cmp(&right.detection_index))
        });

        let mut assigned_tracks = BTreeSet::new();
        let mut assigned_detections = BTreeSet::new();
        let mut assignments = Vec::new();
        for candidate in candidate_pairs {
            if !assigned_tracks.contains(&candidate.track_id)
                && !assigned_detections.contains(&candidate.detection_index)
            {
                assigned_tracks.insert(candidate.track_id);
                assigned_detections.insert(candidate.detection_index);
                assignments.push((candidate.track_id, candidate.detection_index));
            }
        }
        assignments.sort_by_key(|(_, detection_index)| *detection_index);

        let mut events = Vec::new();
        let mut observations = Vec::new();
        for (track_id, detection_index) in assignments {
            let detection = &frame.detections[detection_index];
            let track = self
                .tracks
                .get_mut(&track_id)
                .ok_or(TrackerError::InternalAssociationInvariant)?;
            let elapsed = frame
                .frame_index
                .saturating_sub(track.last_seen_frame)
                .max(1) as f32;
            let measured_vx = (detection.bounds.x - track.bounds.x) / elapsed;
            let measured_vy = (detection.bounds.y - track.bounds.y) / elapsed;
            let smoothing = self.config.velocity_smoothing;
            track.velocity_x_per_frame =
                smoothing * measured_vx + (1.0 - smoothing) * track.velocity_x_per_frame;
            track.velocity_y_per_frame =
                smoothing * measured_vy + (1.0 - smoothing) * track.velocity_y_per_frame;
            let reacquired = track.phase == TrackPhaseV1::Offscreen;
            if reacquired {
                track.track_epoch = next_epoch(track.track_epoch)?;
            }
            track.phase = TrackPhaseV1::Visible;
            track.bounds = detection.bounds;
            track.last_seen_frame = frame.frame_index;
            track.missed_frames = 0;
            track.last_embedding = detection.embedding.clone();
            events.push(if reacquired {
                TrackEventV1::Reacquired {
                    track_id,
                    track_epoch: track.track_epoch,
                }
            } else {
                TrackEventV1::Updated {
                    track_id,
                    track_epoch: track.track_epoch,
                }
            });
            observations.push(TrackedDetectionV1 {
                detection_index,
                track_id,
                track_epoch: track.track_epoch,
                embedding: detection.embedding.clone(),
            });
        }

        let previously_tracked: Vec<TrackId> = self.tracks.keys().copied().collect();
        let mut retire = Vec::new();
        for track_id in previously_tracked {
            if assigned_tracks.contains(&track_id) {
                continue;
            }
            let track = self
                .tracks
                .get_mut(&track_id)
                .ok_or(TrackerError::InternalAssociationInvariant)?;
            track.missed_frames = frame.frame_index.saturating_sub(track.last_seen_frame);
            if track.phase == TrackPhaseV1::Visible {
                track.phase = TrackPhaseV1::Offscreen;
                track.track_epoch = next_epoch(track.track_epoch)?;
                events.push(TrackEventV1::Lost {
                    track_id,
                    track_epoch: track.track_epoch,
                });
            }
            let grace = if self.selected_track_id == Some(track_id) {
                self.config.selected_actor_maximum_missed_frames
            } else {
                self.config.maximum_missed_frames
            };
            if track.missed_frames > grace {
                retire.push(track_id);
            }
        }

        for (detection_index, detection) in frame.detections.iter().enumerate() {
            if assigned_detections.contains(&detection_index)
                || !eligible_detections.contains(&detection_index)
            {
                continue;
            }
            let track_id = TrackId(self.next_track_id);
            self.next_track_id = self
                .next_track_id
                .checked_add(1)
                .ok_or(TrackerError::TrackIdExhausted)?;
            let track_epoch = TrackEpoch(1);
            let encounter_id = encounter_id(
                &self.config.encounter_namespace,
                track_id,
                frame.frame_index,
            );
            self.tracks.insert(
                track_id,
                TrackState {
                    track_id,
                    track_epoch,
                    phase: TrackPhaseV1::Visible,
                    bounds: detection.bounds,
                    velocity_x_per_frame: 0.0,
                    velocity_y_per_frame: 0.0,
                    first_seen_frame: frame.frame_index,
                    last_seen_frame: frame.frame_index,
                    missed_frames: 0,
                    encounter_id,
                    last_embedding: detection.embedding.clone(),
                },
            );
            events.push(TrackEventV1::Created {
                track_id,
                track_epoch,
            });
            observations.push(TrackedDetectionV1 {
                detection_index,
                track_id,
                track_epoch,
                embedding: detection.embedding.clone(),
            });
        }

        for track_id in retire {
            if let Some(track) = self.tracks.remove(&track_id) {
                events.push(TrackEventV1::Retired {
                    track_id,
                    final_epoch: track.track_epoch,
                    encounter_id: track.encounter_id,
                });
                if self.selected_track_id == Some(track_id) {
                    self.selected_track_id = None;
                    events.push(TrackEventV1::SelectionCleared {
                        retired_track_id: track_id,
                    });
                }
            }
        }

        observations.sort_by_key(|observation| observation.detection_index);
        self.last_frame_index = Some(frame.frame_index);
        self.last_timestamp_ms = Some(frame.timestamp_ms);
        Ok(TrackerUpdateV1 {
            frame_index: frame.frame_index,
            tracks: self.snapshot(),
            observations,
            events,
            selected_track_id: self.selected_track_id,
        })
    }

    fn validate_frame(&self, frame: &FrameActorsV1) -> Result<(), TrackerError> {
        if self
            .last_frame_index
            .is_some_and(|last| frame.frame_index <= last)
        {
            return Err(TrackerError::NonMonotonicFrameIndex);
        }
        if self
            .last_timestamp_ms
            .is_some_and(|last| frame.timestamp_ms < last)
        {
            return Err(TrackerError::NonMonotonicTimestamp);
        }
        let mut local_ids = BTreeSet::new();
        for detection in &frame.detections {
            detection.validate()?;
            if let Some(local_id) = &detection.detector_local_id {
                if !local_ids.insert(local_id) {
                    return Err(TrackerError::DuplicateDetectorLocalId(local_id.clone()));
                }
            }
        }
        Ok(())
    }

    fn association_score(
        &self,
        track: &TrackState,
        detection: &ActorDetectionV1,
        frame_index: u64,
    ) -> Option<f32> {
        let predicted = track.predicted_bounds(frame_index);
        let iou = predicted.intersection_over_union(detection.bounds);
        let distance = predicted.normalized_center_distance(detection.bounds);
        let appearance = match (&track.last_embedding, &detection.embedding) {
            (Some(previous), Some(current)) => previous.cosine_similarity(current).ok(),
            _ => None,
        };
        let geometric_gate = iou >= self.config.minimum_intersection_over_union
            || distance <= self.config.maximum_normalized_center_distance;
        let appearance_gate = appearance
            .is_some_and(|similarity| similarity >= self.config.minimum_appearance_similarity);
        if !geometric_gate && !appearance_gate {
            return None;
        }
        // When both observations carry embeddings from the exact same model
        // space, incompatible appearance is stronger evidence than proximity.
        if appearance
            .is_some_and(|similarity| similarity < self.config.minimum_appearance_similarity)
        {
            return None;
        }
        let proximity =
            (1.0 - distance / self.config.maximum_normalized_center_distance).clamp(0.0, 1.0);
        let appearance_score = appearance.map_or(0.5, |score| (score + 1.0) * 0.5);
        let geometry_weight = 1.0 - self.config.appearance_weight;
        let mut score = geometry_weight * (0.6 * iou + 0.4 * proximity)
            + self.config.appearance_weight * appearance_score;
        if self.selected_track_id == Some(track.track_id) {
            score += self.config.selected_actor_match_bonus;
        }
        Some(score)
    }
}

#[derive(Clone, Copy, Debug)]
struct CandidatePair {
    track_id: TrackId,
    detection_index: usize,
    score: f32,
}

fn next_epoch(epoch: TrackEpoch) -> Result<TrackEpoch, TrackerError> {
    epoch
        .0
        .checked_add(1)
        .map(TrackEpoch)
        .ok_or(TrackerError::TrackEpochExhausted)
}

fn encounter_id(namespace: &str, track_id: TrackId, first_seen_frame: u64) -> String {
    let mut digest = Sha256::new();
    digest.update(b"interactive-npcs/background-encounter/v1\0");
    digest.update(namespace.as_bytes());
    digest.update(b"\0");
    digest.update(track_id.0.to_le_bytes());
    digest.update(first_seen_frame.to_le_bytes());
    let bytes = digest.finalize();
    let mut suffix = String::with_capacity(24);
    for byte in &bytes[..12] {
        use std::fmt::Write as _;
        let _ = write!(suffix, "{byte:02x}");
    }
    format!("enc_v1_{suffix}")
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum TrackerError {
    #[error("tracker configuration is invalid")]
    InvalidConfiguration,
    #[error("encounter namespace must be non-empty and at most 256 bytes")]
    InvalidEncounterNamespace,
    #[error("detection confidence must be finite and inside [0, 1]")]
    InvalidDetectionConfidence,
    #[error("detector-local ID is empty or too long")]
    InvalidDetectorLocalId,
    #[error("duplicate detector-local ID {0:?} in one frame")]
    DuplicateDetectorLocalId(String),
    #[error("frame indexes must increase strictly")]
    NonMonotonicFrameIndex,
    #[error("frame timestamps may not move backwards")]
    NonMonotonicTimestamp,
    #[error("unknown actor track {0:?}")]
    UnknownTrack(TrackId),
    #[error("track ID space is exhausted")]
    TrackIdExhausted,
    #[error("track epoch space is exhausted")]
    TrackEpochExhausted,
    #[error("internal association invariant failed")]
    InternalAssociationInvariant,
    #[error(transparent)]
    InvalidGeometry(#[from] crate::GeometryError),
    #[error(transparent)]
    InvalidEmbedding(#[from] EmbeddingError),
}
