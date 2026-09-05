use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use interactive_npcs_game_discovery::{
    Confidence, DetectionSource, EditionEvidence, InstallationCandidate, InstallationEvidence,
    StoreKind,
};
use npc_game_profile::{CapabilityTier, GAME_PROFILE_V2_VERSION};
use npc_runtime_core::{MemoryContext, TurnEvent, TurnLifecycle};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    simulation::{SimulationSafetyContext, SimulationSafetyEvidenceState},
    HostState, SimulationRequest, REQUIRED_AUTHORED_GAME_PROFILE_COUNT, REQUIRED_PROFILE_COUNT,
    REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT, SYNTHETIC_REVIEW_PROFILE_ID,
};

const REPLAY_SCHEMA_VERSION: &str = "1.0.0";
const MAX_LEDGER_BYTES: u64 = 512 * 1024;
const MAX_REPLAY_BYTES: u64 = 512 * 1024;
const RISK_GATED_PROFILE_IDS: [&str; 3] = [
    "elden-ring-offline",
    "gta-v-story",
    "red-dead-redemption-2-story",
];

#[derive(Clone, Debug)]
pub struct ProfileReplayCorpus {
    replays: Vec<ProfileReplayV1>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReplayReport {
    pub schema_version: &'static str,
    pub valid: bool,
    pub authored_profile_count: usize,
    pub synthetic_review_profile_count: usize,
    pub profile_count: usize,
    pub replay_count: usize,
    pub integrity_entries_verified: usize,
    pub detection_assertions_passed: usize,
    pub explicit_offscreen_selection_assertions_passed: usize,
    pub conversation_routes_passed: usize,
    pub subtitle_routes_passed: usize,
    pub memory_routes_passed: usize,
    pub audio_routes_passed: usize,
    pub risk_refusals_passed: usize,
    pub live_certified_claims: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrityLedgerV1 {
    schema_version: String,
    hash_algorithm: String,
    authored_profile_count: usize,
    synthetic_review_profile_count: usize,
    entries: Vec<IntegrityEntryV1>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrityEntryV1 {
    profile_id: String,
    profile_path: String,
    profile_sha256: String,
    replay_path: String,
    replay_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileReplayV1 {
    schema_version: String,
    profile_id: String,
    assertions: ReplayAssertionsV1,
    simulation: ReplaySimulationV1,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayAssertionsV1 {
    profile_validation: ProfileValidationAssertionV1,
    detection: DetectionAssertionV1,
    selection: SelectionAssertionV1,
    routes: RouteAssertionsV1,
    refusal: RefusalAssertionsV1,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileValidationAssertionV1 {
    expected_valid: bool,
    expected_schema_version: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectionAssertionV1 {
    store: String,
    store_id: Option<String>,
    display_name: String,
    install_directory: String,
    executable: String,
    expected_match: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionAssertionV1 {
    character_id: String,
    offscreen: bool,
    expected_strategy: String,
    expected_evidence: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteAssertionsV1 {
    conversation: bool,
    subtitles: bool,
    memory: bool,
    audio: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusalAssertionsV1 {
    protected_online: String,
    anti_cheat: String,
    risk_gated_profile: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaySimulationV1 {
    session_id: String,
    turn_id: String,
    transcript: String,
    locale: String,
}

impl ProfileReplayCorpus {
    pub fn load(repo_root: &Path, state: &HostState) -> Result<Self, ProfileReplayError> {
        let ledger_path = repo_root
            .join("fixtures")
            .join("profile-replays")
            .join("v1")
            .join("integrity-ledger.json");
        let ledger: IntegrityLedgerV1 = read_json(&ledger_path, MAX_LEDGER_BYTES)?;
        if ledger.schema_version != REPLAY_SCHEMA_VERSION
            || ledger.hash_algorithm != "sha256"
            || ledger.authored_profile_count != REQUIRED_AUTHORED_GAME_PROFILE_COUNT
            || ledger.synthetic_review_profile_count != REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT
            || ledger.entries.len() != REQUIRED_PROFILE_COUNT
        {
            return Err(ProfileReplayError::InvalidLedger);
        }

        let mut ids = BTreeSet::new();
        let mut replays = Vec::with_capacity(ledger.entries.len());
        for entry in ledger.entries {
            if !ids.insert(entry.profile_id.clone())
                || !valid_sha256(&entry.profile_sha256)
                || !valid_sha256(&entry.replay_sha256)
            {
                return Err(ProfileReplayError::InvalidLedger);
            }
            let profile_path = resolve_safe_relative(repo_root, &entry.profile_path)?;
            let replay_path = resolve_safe_relative(repo_root, &entry.replay_path)?;
            if hash_file(&profile_path, MAX_REPLAY_BYTES)? != entry.profile_sha256
                || hash_file(&replay_path, MAX_REPLAY_BYTES)? != entry.replay_sha256
            {
                return Err(ProfileReplayError::IntegrityMismatch(entry.profile_id));
            }
            let loaded = state
                .profiles
                .profiles()
                .iter()
                .find(|loaded| loaded.profile.id == entry.profile_id)
                .ok_or_else(|| ProfileReplayError::MissingProfile(entry.profile_id.clone()))?;
            if loaded.source.canonicalize().ok().as_ref() != Some(&profile_path) {
                return Err(ProfileReplayError::InvalidLedger);
            }
            let replay: ProfileReplayV1 = read_json(&replay_path, MAX_REPLAY_BYTES)?;
            validate_static_replay(&replay, &loaded.profile)?;
            replays.push(replay);
        }
        if state.profiles.profiles().len() != REQUIRED_PROFILE_COUNT
            || ids.len() != REQUIRED_PROFILE_COUNT
        {
            return Err(ProfileReplayError::WrongCount);
        }
        replays.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
        Ok(Self { replays })
    }

    pub async fn validate_runtime(
        &self,
        state: &HostState,
    ) -> Result<ProfileReplayReport, ProfileReplayError> {
        let synthetic_review_profile_count = state
            .profiles
            .profiles()
            .iter()
            .filter(|loaded| loaded.profile.id == SYNTHETIC_REVIEW_PROFILE_ID)
            .count();
        let authored_profile_count = state
            .profiles
            .profiles()
            .len()
            .saturating_sub(synthetic_review_profile_count);
        let mut report = ProfileReplayReport {
            schema_version: REPLAY_SCHEMA_VERSION,
            valid: true,
            authored_profile_count,
            synthetic_review_profile_count,
            profile_count: state.profiles.profiles().len(),
            replay_count: self.replays.len(),
            integrity_entries_verified: self.replays.len(),
            detection_assertions_passed: 0,
            explicit_offscreen_selection_assertions_passed: 0,
            conversation_routes_passed: 0,
            subtitle_routes_passed: 0,
            memory_routes_passed: 0,
            audio_routes_passed: 0,
            risk_refusals_passed: 0,
            live_certified_claims: 0,
        };

        for replay in &self.replays {
            let candidate = replay.assertions.detection.candidate()?;
            let matches = state.profiles.match_installations(&[candidate]);
            if !replay.assertions.detection.expected_match
                || matches.len() != 1
                || matches[0].profile_id != replay.profile_id
            {
                return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
            }
            report.detection_assertions_passed += 1;

            let first = state
                .simulate_turn(replay.request(1, SimulationSafetyContext::verified_safe()))
                .await
                .map_err(|_| ProfileReplayError::Assertion(replay.profile_id.clone()))?;
            if first.outcome.lifecycle != TurnLifecycle::Completed {
                return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
            }
            let explicit_identity = first.events.iter().any(|event| {
                matches!(event, TurnEvent::CharacterResolved { character, .. }
                    if character.explicit_selection
                        && character.character_id.as_deref() == Some(replay.assertions.selection.character_id.as_str())
                        && character.evidence.iter().any(|value| value == &replay.assertions.selection.expected_evidence))
            });
            if !explicit_identity || !replay.assertions.selection.offscreen {
                return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
            }
            report.explicit_offscreen_selection_assertions_passed += 1;

            if replay.assertions.routes.conversation {
                if first.outcome.full_response.trim().is_empty() {
                    return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
                }
                report.conversation_routes_passed += 1;
            }
            if replay.assertions.routes.subtitles {
                if !first
                    .events
                    .iter()
                    .any(|event| matches!(event, TurnEvent::SentenceReady { text, .. } if !text.trim().is_empty()))
                {
                    return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
                }
                report.subtitle_routes_passed += 1;
            }
            if replay.assertions.routes.audio {
                if !first
                    .outcome
                    .delivered
                    .iter()
                    .any(|item| item.audible_frames > 0)
                {
                    return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
                }
                report.audio_routes_passed += 1;
            }
            if replay.assertions.routes.memory {
                let second = state
                    .simulate_turn(replay.request(2, SimulationSafetyContext::verified_safe()))
                    .await
                    .map_err(|_| ProfileReplayError::Assertion(replay.profile_id.clone()))?;
                let recalled = second.events.iter().any(|event| {
                    matches!(event, TurnEvent::MemoryReady { context, .. }
                        if context != &MemoryContext::default())
                });
                if !recalled {
                    return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
                }
                report.memory_routes_passed += 1;
            }
            if replay.assertions.refusal.risk_gated_profile {
                for context in [
                    SimulationSafetyContext {
                        evidence_state: SimulationSafetyEvidenceState::Blocked,
                        profile_policy: Default::default(),
                        visuals_allowed: false,
                        protected_online_detected: true,
                        anti_cheat_detected: false,
                    },
                    SimulationSafetyContext {
                        evidence_state: SimulationSafetyEvidenceState::Blocked,
                        profile_policy: Default::default(),
                        visuals_allowed: false,
                        protected_online_detected: false,
                        anti_cheat_detected: true,
                    },
                ] {
                    if state
                        .simulate_turn(replay.request(3, context))
                        .await
                        .is_ok()
                    {
                        return Err(ProfileReplayError::Assertion(replay.profile_id.clone()));
                    }
                    report.risk_refusals_passed += 1;
                }
            }
        }
        Ok(report)
    }
}

impl ProfileReplayV1 {
    fn request(
        &self,
        ordinal: usize,
        safety_context: SimulationSafetyContext,
    ) -> SimulationRequest {
        SimulationRequest {
            session_id: self.simulation.session_id.clone(),
            turn_id: format!("{}-{ordinal}", self.simulation.turn_id),
            game_id: self.profile_id.clone(),
            character_id: Some(self.assertions.selection.character_id.clone()),
            effective_game_profile: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context,
            application_namespace: None,
            transcript: self.simulation.transcript.clone(),
            locale: self.simulation.locale.clone(),
            execution_mode: None,
            dev_live_tts: None,
            route_snapshot: None,
            input: Default::default(),
            delivery: Default::default(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            subtitle_presentation_context: None,
        }
    }
}

impl DetectionAssertionV1 {
    fn candidate(&self) -> Result<InstallationCandidate, ProfileReplayError> {
        let store = match self.store.as_str() {
            "steam" => StoreKind::Steam,
            "epic" => StoreKind::Epic,
            "gog" => StoreKind::Gog,
            "standalone" => StoreKind::Standalone,
            _ => return Err(ProfileReplayError::InvalidReplay),
        };
        Ok(InstallationCandidate {
            store,
            display_name: self.display_name.clone(),
            install_dir: PathBuf::from(&self.install_directory),
            executable: Some(PathBuf::from(&self.install_directory).join(&self.executable)),
            edition: EditionEvidence {
                edition_id: None,
                store_id: self.store_id.clone(),
                build_id: Some("deterministic-replay".into()),
            },
            evidence: vec![InstallationEvidence {
                source: DetectionSource::StoreManifest,
                confidence: Confidence::High,
                detail: "versioned deterministic profile replay".into(),
            }],
            warnings: BTreeSet::new(),
        })
    }
}

fn validate_static_replay(
    replay: &ProfileReplayV1,
    profile: &npc_game_profile::GameProfileV2,
) -> Result<(), ProfileReplayError> {
    if replay.schema_version != REPLAY_SCHEMA_VERSION
        || replay.profile_id != profile.id
        || !replay.assertions.profile_validation.expected_valid
        || replay.assertions.profile_validation.expected_schema_version != GAME_PROFILE_V2_VERSION
        || !profile.validate().is_valid()
        || !replay.assertions.selection.offscreen
        || replay.assertions.selection.expected_strategy != "explicit_selection"
        || replay.assertions.selection.expected_evidence != "explicit_selection"
        || replay.assertions.refusal.protected_online != "blocked"
        || replay.assertions.refusal.anti_cheat != "blocked"
        || replay.assertions.refusal.risk_gated_profile
            != RISK_GATED_PROFILE_IDS.contains(&profile.id.as_str())
    {
        return Err(ProfileReplayError::InvalidReplay);
    }
    let character = profile
        .characters
        .iter()
        .find(|character| character.id == replay.assertions.selection.character_id)
        .ok_or(ProfileReplayError::InvalidReplay)?;
    if character.identity.fallback != npc_game_profile::IdentityFallback::ExplicitSelection {
        return Err(ProfileReplayError::InvalidReplay);
    }
    let claims: BTreeMap<_, _> = profile.capabilities.iter().collect();
    for (name, route) in [
        ("conversation", replay.assertions.routes.conversation),
        ("identity", true),
        ("subtitles", replay.assertions.routes.subtitles),
        ("memory", replay.assertions.routes.memory),
    ] {
        let claim = claims.get(name).ok_or(ProfileReplayError::InvalidReplay)?;
        if claim.tier == CapabilityTier::ReplayVerified
            && (!route
                || !claim.evidence.iter().any(|item| {
                    item.kind == npc_game_profile::CapabilityEvidenceKind::DeterministicReplay
                        && item.reference == format!("profile-replay-v1:{}:{name}", profile.id)
                }))
        {
            return Err(ProfileReplayError::InvalidReplay);
        }
    }
    if profile
        .capabilities
        .iter()
        .any(|(_, claim)| claim.tier == CapabilityTier::LiveCertified)
    {
        return Err(ProfileReplayError::InvalidReplay);
    }
    let declared_process = profile.detection.processes.iter().any(|process| {
        process.required && process.executable == replay.assertions.detection.executable
    });
    let declared_store = profile.detection.stores.iter().any(|store| {
        format!("{:?}", store.store).eq_ignore_ascii_case(&replay.assertions.detection.store)
            && store.app_id == replay.assertions.detection.store_id
    });
    if !declared_process || !declared_store {
        return Err(ProfileReplayError::InvalidReplay);
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    maximum: u64,
) -> Result<T, ProfileReplayError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProfileReplayError::Io)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > maximum {
        return Err(ProfileReplayError::UnsafeFile);
    }
    let bytes = fs::read(path).map_err(|_| ProfileReplayError::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| ProfileReplayError::InvalidReplay)
}

fn resolve_safe_relative(root: &Path, relative: &str) -> Result<PathBuf, ProfileReplayError> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ProfileReplayError::UnsafeFile);
    }
    let canonical_root = root.canonicalize().map_err(|_| ProfileReplayError::Io)?;
    let resolved = root
        .join(relative)
        .canonicalize()
        .map_err(|_| ProfileReplayError::Io)?;
    if !resolved.starts_with(canonical_root) {
        return Err(ProfileReplayError::UnsafeFile);
    }
    Ok(resolved)
}

fn hash_file(path: &Path, maximum: u64) -> Result<String, ProfileReplayError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProfileReplayError::Io)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > maximum {
        return Err(ProfileReplayError::UnsafeFile);
    }
    let bytes = fs::read(path).map_err(|_| ProfileReplayError::Io)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Error)]
pub enum ProfileReplayError {
    #[error("profile replay ledger is invalid")]
    InvalidLedger,
    #[error("profile replay corpus must cover exactly 20 authored profiles")]
    WrongCount,
    #[error("profile replay file is invalid")]
    InvalidReplay,
    #[error("profile replay file is unsafe")]
    UnsafeFile,
    #[error("profile replay integrity mismatch for {0}")]
    IntegrityMismatch(String),
    #[error("profile replay references missing profile {0}")]
    MissingProfile(String),
    #[error("profile replay assertion failed for {0}")]
    Assertion(String),
    #[error("profile replay corpus could not be read")]
    Io,
}
