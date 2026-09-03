use crate::{
    ActorTrackSnapshotV1, ActorTrackerV1, FrameActorsV1, IdentityDecisionV1, IdentityError,
    IdentityResolverV1, TrackEventV1, TrackId, TrackPhaseV1, TrackerError, TrackerUpdateV1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorIdentityV1 {
    pub track: ActorTrackSnapshotV1,
    pub identity: IdentityDecisionV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameIdentityUpdateV1 {
    pub frame_index: u64,
    pub actors: Vec<ActorIdentityV1>,
    pub track_events: Vec<TrackEventV1>,
    pub selected_track_id: Option<TrackId>,
}

/// Integration façade combining temporal association with conservative
/// recognition. Capture and inference stay outside this crate.
#[derive(Clone, Debug)]
pub struct ActorIdentityEngineV1 {
    tracker: ActorTrackerV1,
    resolver: IdentityResolverV1,
}

impl ActorIdentityEngineV1 {
    pub fn new(tracker: ActorTrackerV1, resolver: IdentityResolverV1) -> Self {
        Self { tracker, resolver }
    }

    pub fn tracker(&self) -> &ActorTrackerV1 {
        &self.tracker
    }

    pub fn resolver(&self) -> &IdentityResolverV1 {
        &self.resolver
    }

    pub fn select_actor(&mut self, track_id: TrackId) -> Result<(), EngineError> {
        self.tracker.select_actor(track_id)?;
        Ok(())
    }

    pub fn clear_selection(&mut self) {
        self.tracker.clear_selection();
    }

    pub fn assign_subject(
        &mut self,
        track_id: TrackId,
        subject_id: impl Into<String>,
    ) -> Result<(), EngineError> {
        if !self
            .tracker
            .snapshot()
            .iter()
            .any(|track| track.track_id == track_id)
        {
            return Err(TrackerError::UnknownTrack(track_id).into());
        }
        self.resolver.assign_explicit(track_id, subject_id)?;
        Ok(())
    }

    pub fn assign_selected_subject(
        &mut self,
        subject_id: impl Into<String>,
    ) -> Result<(), EngineError> {
        let track_id = self
            .tracker
            .selected_track_id()
            .ok_or(EngineError::NoSelectedActor)?;
        self.resolver.assign_explicit(track_id, subject_id)?;
        Ok(())
    }

    pub fn clear_subject_assignment(&mut self, track_id: TrackId) {
        self.resolver.clear_explicit(track_id);
    }

    pub fn process_frame(
        &mut self,
        frame: FrameActorsV1,
    ) -> Result<FrameIdentityUpdateV1, EngineError> {
        let TrackerUpdateV1 {
            frame_index,
            tracks,
            observations,
            events,
            selected_track_id,
        } = self.tracker.update(frame)?;

        for event in &events {
            if let TrackEventV1::Retired { track_id, .. } = event {
                self.resolver.retire_track(*track_id);
            }
        }

        let observations: BTreeMap<_, _> = observations
            .into_iter()
            .map(|observation| (observation.track_id, observation.embedding))
            .collect();
        let mut actors = Vec::with_capacity(tracks.len());
        for track in tracks {
            let identity = if track.phase == TrackPhaseV1::Visible {
                self.resolver.observe(
                    track.track_id,
                    observations
                        .get(&track.track_id)
                        .and_then(|embedding| embedding.as_ref()),
                    &track.encounter_id,
                )?
            } else {
                self.resolver.decision_for_phase(
                    track.track_id,
                    track.track_epoch,
                    track.phase,
                    &track.encounter_id,
                )
            };
            actors.push(ActorIdentityV1 { track, identity });
        }

        Ok(FrameIdentityUpdateV1 {
            frame_index,
            actors,
            track_events: events,
            selected_track_id,
        })
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum EngineError {
    #[error("no actor is explicitly selected")]
    NoSelectedActor,
    #[error(transparent)]
    Tracker(#[from] TrackerError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
}
