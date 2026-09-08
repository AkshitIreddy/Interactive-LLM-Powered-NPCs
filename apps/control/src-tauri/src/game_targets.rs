use crate::catalog::ResourceCatalog;
use crate::media_broker::{
    CapturePixelScope, CapturePixelSource, MediaBrokerError, MediaBrokerSupervisor,
    NativeCaptureEvidence,
};
use async_trait::async_trait;
use npc_game_discovery::{
    BoundGameTarget, ProcessWindowRule, RunningGameWindow, TargetPolicy, TargetRevalidationError,
    TargetSelectionError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectGameTargetRequest {
    pub game_profile_id: String,
    pub native_window_hint: Option<u64>,
    /// Recorded for the operator-facing explanation only. A checkbox is not
    /// trusted anti-cheat or network evidence and cannot authorize visuals.
    #[serde(default)]
    pub explicit_user_confirmed_offline_single_player: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameTargetCandidate {
    pub process_id: u32,
    pub native_window: u64,
    pub executable_name: String,
    pub executable_path_sha256: String,
    pub title: String,
    pub foreground: bool,
    pub client_width: u32,
    pub client_height: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameTargetSelection {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub target: GameTargetCandidate,
    pub process_instance_bound: bool,
    pub user_confirmed_offline_single_player: bool,
    pub capture_authorized: bool,
    pub safety_state: String,
    pub safety_detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameCaptureVerification {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub target: GameTargetCandidate,
    pub capture: NativeCaptureEvidence,
    pub exact_pid_hwnd_executable_match: bool,
    pub frame_sequence_advanced: bool,
    pub content_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_fixture_motion_mode: Option<String>,
    pub safety_state: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GameTargetError {
    #[error("game target request is invalid: {0}")]
    Invalid(String),
    #[error("running-window observation is unavailable: {0}")]
    Observation(String),
    #[error("game target selection failed: {0}")]
    Selection(String),
    #[error("selected game target is stale or mismatched: {0}")]
    Revalidation(String),
    #[error("capture is blocked: verified offline/protection evidence is unavailable")]
    SafetyUnverified,
    #[error("native media broker operation failed: {0}")]
    Broker(String),
    #[error("game target state is temporarily unavailable")]
    State,
}

#[async_trait]
pub(crate) trait GameCaptureBroker: Sync {
    async fn bind_exact_target(
        &self,
        native_window: u64,
        process_id: u32,
        executable_basename: &str,
    ) -> Result<(), MediaBrokerError>;
    async fn capture_evidence(&self) -> Result<NativeCaptureEvidence, MediaBrokerError>;
    async fn clear_target(&self) -> Result<(), MediaBrokerError>;
}

#[async_trait]
impl GameCaptureBroker for MediaBrokerSupervisor {
    async fn bind_exact_target(
        &self,
        native_window: u64,
        process_id: u32,
        executable_basename: &str,
    ) -> Result<(), MediaBrokerError> {
        self.bind_game_capture_target(native_window, process_id, executable_basename)
            .await
    }

    async fn capture_evidence(&self) -> Result<NativeCaptureEvidence, MediaBrokerError> {
        self.native_capture_evidence().await
    }

    async fn clear_target(&self) -> Result<(), MediaBrokerError> {
        self.clear_bound_target().await
    }
}

#[derive(Debug)]
struct SelectedTarget {
    game_profile_id: String,
    binding: BoundGameTarget,
    rules: Vec<ProcessWindowRule>,
    excluded_titles: Vec<String>,
    user_confirmed_offline: bool,
    broker_bound: bool,
    previous_capture_sequence: u64,
    verified_capture: Option<NativeCaptureEvidence>,
}

#[derive(Debug, Default)]
pub struct GameTargetManager {
    selected: Mutex<Option<SelectedTarget>>,
}

impl GameTargetManager {
    pub fn selected_process_id(&self) -> Option<u32> {
        self.selected
            .lock()
            .ok()
            .and_then(|selected| selected.as_ref().map(|target| target.binding.pid))
    }

    pub fn discover(
        &self,
        resources: &ResourceCatalog,
        game_profile_id: &str,
    ) -> Result<Vec<GameTargetCandidate>, GameTargetError> {
        let (rules, excluded) = profile_policy(resources, game_profile_id)?;
        let policy = TargetPolicy::new(rules, excluded)
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        let observations = observe_running_windows()?;
        Ok(observations
            .iter()
            .filter_map(|observation| {
                policy
                    .select(std::slice::from_ref(observation))
                    .ok()
                    .map(|_| candidate(observation))
            })
            .collect())
    }

    pub fn select(
        &self,
        resources: &ResourceCatalog,
        request: SelectGameTargetRequest,
    ) -> Result<GameTargetSelection, GameTargetError> {
        let (rules, excluded_titles) = profile_policy(resources, &request.game_profile_id)?;
        let policy = TargetPolicy::new(rules.clone(), excluded_titles.clone())
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        let observations = observe_running_windows()?;
        let eligible = if let Some(hwnd) = request.native_window_hint {
            observations
                .iter()
                .filter(|observation| observation.hwnd == hwnd)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            observations
        };
        let binding = policy.select(&eligible).map_err(selection_error)?;
        let observation = policy
            .revalidate(&binding, &eligible)
            .map_err(revalidation_error)?;
        let target = candidate(observation);
        let supported_profile_user_confirmed = request.game_profile_id == "cyberpunk-2077"
            && request.explicit_user_confirmed_offline_single_player;
        *self.selected.lock().map_err(|_| GameTargetError::State)? = Some(SelectedTarget {
            game_profile_id: request.game_profile_id.clone(),
            binding,
            rules,
            excluded_titles,
            user_confirmed_offline: request.explicit_user_confirmed_offline_single_player,
            broker_bound: false,
            previous_capture_sequence: 0,
            verified_capture: None,
        });
        Ok(GameTargetSelection {
            schema_version: 1,
            game_profile_id: request.game_profile_id,
            target,
            process_instance_bound: true,
            user_confirmed_offline_single_player: request
                .explicit_user_confirmed_offline_single_player,
            capture_authorized: false,
            safety_state: if supported_profile_user_confirmed {
                "supported_profile_user_confirmed".into()
            } else {
                "unverified".into()
            },
            safety_detail: if supported_profile_user_confirmed {
                "The selected executable matches the supported Cyberpunk 2077 profile and the user chose a single-player session. This is user-provided provenance, not machine-proven offline or anti-cheat evidence; broker binding is still required.".into()
            } else {
                "The target is PID/HWND/process-instance bound, but a user confirmation alone does not authorize visual capture.".into()
            },
        })
    }

    pub fn snapshot(&self) -> Result<Option<GameTargetSelection>, GameTargetError> {
        let selected = self.selected.lock().map_err(|_| GameTargetError::State)?;
        Ok(selected.as_ref().map(|selected| {
            selection_snapshot(selected, candidate_from_binding(&selected.binding))
        }))
    }

    /// Binds the exact selected Cyberpunk window to the production broker.
    ///
    /// The bundled profile and explicit single-player user choice permit this
    /// bounded operation. They are recorded separately from capture proof and
    /// never stand in for anti-cheat, network-state, or advancing-frame
    /// evidence.
    pub async fn bind_selected_capture(
        &self,
        resources: &ResourceCatalog,
        broker: &MediaBrokerSupervisor,
    ) -> Result<GameTargetSelection, GameTargetError> {
        self.bind_selected_capture_with_observer(resources, broker, observe_running_windows)
            .await
    }

    async fn bind_selected_capture_with_observer<B, F>(
        &self,
        resources: &ResourceCatalog,
        broker: &B,
        mut observe: F,
    ) -> Result<GameTargetSelection, GameTargetError>
    where
        B: GameCaptureBroker,
        F: FnMut() -> Result<Vec<RunningGameWindow>, GameTargetError>,
    {
        let before = observe()?;
        let (binding, _) = self.revalidate_supported_capture(resources, &before)?;
        broker
            .bind_exact_target(binding.hwnd, binding.pid, &binding.executable_leaf)
            .await
            .map_err(|error| GameTargetError::Broker(error.to_string()))?;

        let after = match observe()
            .and_then(|observations| self.revalidate_supported_capture(resources, &observations))
        {
            Ok((_, current)) => current,
            Err(error) => {
                let _ = broker.clear_target().await;
                return Err(error);
            }
        };
        let update = (|| {
            let mut selected = self.selected.lock().map_err(|_| GameTargetError::State)?;
            let target = selected
                .as_mut()
                .ok_or_else(|| GameTargetError::Invalid("no game target is selected".into()))?;
            if target.binding != binding {
                return Err(GameTargetError::Revalidation(
                    "selected target changed while the broker was binding".into(),
                ));
            }
            target.broker_bound = true;
            target.previous_capture_sequence = 0;
            target.verified_capture = None;
            Ok(selection_snapshot(target, candidate(&after)))
        })();
        if update.is_err() {
            let _ = broker.clear_target().await;
        }
        update
    }

    fn revalidate_supported_capture(
        &self,
        resources: &ResourceCatalog,
        observations: &[RunningGameWindow],
    ) -> Result<(BoundGameTarget, RunningGameWindow), GameTargetError> {
        let (game_profile_id, binding, user_confirmed) = {
            let selected = self.selected.lock().map_err(|_| GameTargetError::State)?;
            let selected = selected
                .as_ref()
                .ok_or_else(|| GameTargetError::Invalid("no game target is selected".into()))?;
            (
                selected.game_profile_id.clone(),
                selected.binding.clone(),
                selected.user_confirmed_offline,
            )
        };
        if game_profile_id != "cyberpunk-2077" || !user_confirmed {
            return Err(GameTargetError::SafetyUnverified);
        }
        let (rules, excluded_titles) = profile_policy(resources, &game_profile_id)?;
        let policy = TargetPolicy::new(rules, excluded_titles)
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        let current = policy
            .revalidate(&binding, observations)
            .map_err(revalidation_error)?;
        Ok((binding, current.clone()))
    }

    /// Revalidate PID, process creation identity, HWND, full executable path,
    /// and current profile window policy against a fresh native observation.
    /// Resource admission and benchmarks use this instead of trusting a stale
    /// persisted PID from the original selection.
    pub fn revalidated_snapshot(
        &self,
        resources: &ResourceCatalog,
    ) -> Result<Option<GameTargetSelection>, GameTargetError> {
        let (game_profile_id, binding, user_confirmed_offline, broker_bound, capture_verified) = {
            let selected = self.selected.lock().map_err(|_| GameTargetError::State)?;
            let Some(selected) = selected.as_ref() else {
                return Ok(None);
            };
            (
                selected.game_profile_id.clone(),
                selected.binding.clone(),
                selected.user_confirmed_offline,
                selected.broker_bound,
                selected.broker_bound && selected.verified_capture.is_some(),
            )
        };
        let (rules, excluded_titles) = profile_policy(resources, &game_profile_id)?;
        let policy = TargetPolicy::new(rules, excluded_titles)
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        let observations = observe_running_windows()?;
        let current = policy
            .revalidate(&binding, &observations)
            .map_err(revalidation_error)?;
        let (safety_state, safety_detail) = capture_status(
            &game_profile_id,
            user_confirmed_offline,
            broker_bound,
            capture_verified,
        );
        Ok(Some(GameTargetSelection {
            schema_version: 1,
            game_profile_id,
            target: candidate(current),
            process_instance_bound: true,
            user_confirmed_offline_single_player: user_confirmed_offline,
            capture_authorized: capture_verified,
            safety_state: safety_state.into(),
            safety_detail: safety_detail.into(),
        }))
    }

    pub async fn clear(&self, broker: &MediaBrokerSupervisor) -> Result<(), GameTargetError> {
        let broker_bound = self
            .selected
            .lock()
            .map_err(|_| GameTargetError::State)?
            .take()
            .is_some_and(|target| target.broker_bound);
        if broker_bound && broker.health().connected {
            broker
                .clear_bound_target()
                .await
                .map_err(|error| GameTargetError::Broker(error.to_string()))?;
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub fn mark_debug_synthetic_broker_bound(
        &self,
        process_id: u32,
        native_window: u64,
        executable_name: &str,
    ) -> Result<(), GameTargetError> {
        let mut selected = self.selected.lock().map_err(|_| GameTargetError::State)?;
        let target = selected
            .as_mut()
            .ok_or_else(|| GameTargetError::Invalid("no game target is selected".into()))?;
        if target.game_profile_id != "eclipse-harbor"
            || target.binding.pid != process_id
            || target.binding.hwnd != native_window
            || !target
                .binding
                .executable_leaf
                .eq_ignore_ascii_case(executable_name)
        {
            return Err(GameTargetError::Revalidation(
                "synthetic broker target does not match the immutable selected target".into(),
            ));
        }
        target.broker_bound = true;
        Ok(())
    }

    pub async fn verify_capture(
        &self,
        broker: &MediaBrokerSupervisor,
    ) -> Result<GameCaptureVerification, GameTargetError> {
        #[cfg(debug_assertions)]
        {
            let synthetic_selected = self
                .selected
                .lock()
                .map_err(|_| GameTargetError::State)?
                .as_ref()
                .is_some_and(|target| {
                    target.game_profile_id == "eclipse-harbor" && target.broker_bound
                });
            if synthetic_selected {
                let snapshot = broker
                    .debug_synthetic_replay_capture_diagnostics()
                    .await
                    .map_err(|error| GameTargetError::Broker(error.to_string()))?;
                let evidence = snapshot.capture_evidence.ok_or_else(|| {
                    GameTargetError::Broker("synthetic capture evidence is unavailable".into())
                })?;
                let observations = observe_running_windows()?;
                let mut verification =
                    self.verify_evidence_from_observations(&observations, evidence)?;
                verification.review_fixture_motion_mode = Some(snapshot.fixture_motion_mode);
                return Ok(verification);
            }
        }
        self.verify_supported_capture_with_observer(broker, observe_running_windows)
            .await
    }

    async fn verify_supported_capture_with_observer<B, F>(
        &self,
        broker: &B,
        mut observe: F,
    ) -> Result<GameCaptureVerification, GameTargetError>
    where
        B: GameCaptureBroker,
        F: FnMut() -> Result<Vec<RunningGameWindow>, GameTargetError>,
    {
        let supported_and_bound = self
            .selected
            .lock()
            .map_err(|_| GameTargetError::State)?
            .as_ref()
            .is_some_and(|target| {
                target.game_profile_id == "cyberpunk-2077"
                    && target.user_confirmed_offline
                    && target.broker_bound
            });
        if !supported_and_bound {
            return Err(GameTargetError::SafetyUnverified);
        }
        let evidence = broker
            .capture_evidence()
            .await
            .map_err(|error| GameTargetError::Broker(error.to_string()))?;
        let observations = observe()?;
        self.verify_evidence_from_observations(&observations, evidence)
    }

    #[cfg(test)]
    fn select_from_observations(
        &self,
        game_profile_id: &str,
        rules: Vec<ProcessWindowRule>,
        excluded_titles: Vec<String>,
        observations: &[RunningGameWindow],
    ) -> Result<BoundGameTarget, GameTargetError> {
        let policy = TargetPolicy::new(rules.clone(), excluded_titles.clone())
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        let binding = policy.select(observations).map_err(selection_error)?;
        *self.selected.lock().map_err(|_| GameTargetError::State)? = Some(SelectedTarget {
            game_profile_id: game_profile_id.into(),
            binding: binding.clone(),
            rules,
            excluded_titles,
            user_confirmed_offline: true,
            broker_bound: false,
            previous_capture_sequence: 0,
            verified_capture: None,
        });
        Ok(binding)
    }

    fn verify_evidence_from_observations(
        &self,
        observations: &[RunningGameWindow],
        evidence: NativeCaptureEvidence,
    ) -> Result<GameCaptureVerification, GameTargetError> {
        let mut selected = self.selected.lock().map_err(|_| GameTargetError::State)?;
        let selected = selected
            .as_mut()
            .ok_or_else(|| GameTargetError::Invalid("no game target is selected".into()))?;
        let policy = TargetPolicy::new(selected.rules.clone(), selected.excluded_titles.clone())
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        let current = policy
            .revalidate(&selected.binding, observations)
            .map_err(revalidation_error)?;
        let content_changed = evidence.content_hash_changes > 0
            || evidence.initial_content_hash != evidence.latest_content_hash;
        if evidence.selected_process_id != selected.binding.pid
            || evidence.selected_window_handle != selected.binding.hwnd
            || !evidence
                .selected_executable_name
                .eq_ignore_ascii_case(&selected.binding.executable_leaf)
            || evidence.latest_frame_sequence <= selected.previous_capture_sequence
            || evidence.pixel_source != CapturePixelSource::WindowsGraphicsCaptureTexture
            || evidence.pixel_scope != CapturePixelScope::ExactSelectedWindow
            || !evidence.external_display_overlay_pixels_excluded
            || !evidence.desktop_luminance_excluded_from_pixel_evidence
            || !content_changed
        {
            return Err(GameTargetError::Revalidation(
                "broker evidence does not match the immutable target or did not advance".into(),
            ));
        }
        selected.previous_capture_sequence = evidence.latest_frame_sequence;
        selected.verified_capture = Some(evidence.clone());
        Ok(GameCaptureVerification {
            schema_version: 1,
            game_profile_id: selected.game_profile_id.clone(),
            target: candidate(current),
            content_changed,
            frame_sequence_advanced: true,
            exact_pid_hwnd_executable_match: true,
            capture: evidence,
            review_fixture_motion_mode: None,
            safety_state: if selected.game_profile_id == "cyberpunk-2077" {
                "verified_exact_window_wgc".into()
            } else {
                "verified_synthetic_fixture".into()
            },
        })
    }
}

fn selection_snapshot(
    selected: &SelectedTarget,
    target: GameTargetCandidate,
) -> GameTargetSelection {
    let capture_verified = selected.broker_bound && selected.verified_capture.is_some();
    let (safety_state, safety_detail) = capture_status(
        &selected.game_profile_id,
        selected.user_confirmed_offline,
        selected.broker_bound,
        capture_verified,
    );
    GameTargetSelection {
        schema_version: 1,
        game_profile_id: selected.game_profile_id.clone(),
        target,
        process_instance_bound: true,
        user_confirmed_offline_single_player: selected.user_confirmed_offline,
        capture_authorized: capture_verified,
        safety_state: safety_state.into(),
        safety_detail: safety_detail.into(),
    }
}

fn capture_status(
    game_profile_id: &str,
    user_confirmed_offline: bool,
    broker_bound: bool,
    capture_verified: bool,
) -> (&'static str, &'static str) {
    let supported_cyberpunk = game_profile_id == "cyberpunk-2077" && user_confirmed_offline;
    if capture_verified && supported_cyberpunk {
        (
            "verified_exact_window_wgc",
            "The supported Cyberpunk 2077 target has fresh PID/HWND/process-instance/executable revalidation and an advancing exact-window WGC receipt. The single-player state remains the user's explicit choice, not a machine network-state claim.",
        )
    } else if capture_verified {
        (
            "verified_synthetic_fixture",
            "The task-owned synthetic target has advancing exact-window broker evidence bound to this PID/HWND/process instance.",
        )
    } else if broker_bound && supported_cyberpunk {
        (
            "capture_pending",
            "The supported Cyberpunk 2077 profile and explicit single-player choice allowed exact broker binding. Capture remains pending until an advancing exact-window WGC receipt is verified.",
        )
    } else if supported_cyberpunk {
        (
            "supported_profile_user_confirmed",
            "The selected executable matches the supported Cyberpunk 2077 profile and the user chose a single-player session. This is user-provided provenance, not machine-proven offline or anti-cheat evidence; broker binding is still required.",
        )
    } else {
        (
            "unverified",
            "Visual capture remains fail-closed until a supported target is bound and exact-window evidence is verified.",
        )
    }
}

fn profile_policy(
    resources: &ResourceCatalog,
    game_profile_id: &str,
) -> Result<(Vec<ProcessWindowRule>, Vec<String>), GameTargetError> {
    #[cfg(debug_assertions)]
    if game_profile_id == "eclipse-harbor" {
        let profile = resources
            .load_debug_synthetic_review_profile()
            .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
        if !profile.safety.single_player_only || !profile.safety.declarative_only {
            return Err(GameTargetError::Invalid(
                "synthetic profile is not declarative single-player-only".into(),
            ));
        }
        return Ok((
            vec![ProcessWindowRule {
                executable: "interactive-npcs-synthetic-target.exe".into(),
                required: true,
                window_title_regex: None,
            }],
            profile.detection.capture.excluded_window_title_regexes,
        ));
    }
    #[cfg(debug_assertions)]
    let profile = resources
        .load_game_profile(game_profile_id)
        .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
    #[cfg(not(debug_assertions))]
    let profile = resources
        .load_game_profile(game_profile_id)
        .map_err(|error| GameTargetError::Invalid(error.to_string()))?;
    if !profile.safety.single_player_only || !profile.safety.declarative_only {
        return Err(GameTargetError::Invalid(
            "profile is not a declarative single-player-only profile".into(),
        ));
    }
    Ok((
        profile
            .detection
            .processes
            .into_iter()
            .map(|process| ProcessWindowRule {
                executable: process.executable,
                required: process.required,
                window_title_regex: process.window_title_regex,
            })
            .collect(),
        profile.detection.capture.excluded_window_title_regexes,
    ))
}

fn candidate(observation: &RunningGameWindow) -> GameTargetCandidate {
    GameTargetCandidate {
        process_id: observation.pid,
        native_window: observation.hwnd,
        executable_name: observation
            .executable_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown.exe")
            .to_owned(),
        executable_path_sha256: path_digest(&observation.executable_path),
        title: observation.title.clone(),
        foreground: observation.foreground,
        client_width: observation.client_width,
        client_height: observation.client_height,
    }
}

fn candidate_from_binding(binding: &BoundGameTarget) -> GameTargetCandidate {
    GameTargetCandidate {
        process_id: binding.pid,
        native_window: binding.hwnd,
        executable_name: binding.executable_leaf.clone(),
        executable_path_sha256: path_digest(&binding.executable_path),
        title: "Selected game window".into(),
        foreground: false,
        client_width: 0,
        client_height: 0,
    }
}

fn path_digest(path: &std::path::Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(path.to_string_lossy().to_ascii_lowercase().as_bytes())
    )
}

fn selection_error(error: TargetSelectionError) -> GameTargetError {
    GameTargetError::Selection(error.to_string())
}

fn revalidation_error(error: TargetRevalidationError) -> GameTargetError {
    GameTargetError::Revalidation(error.to_string())
}

#[cfg(not(windows))]
fn observe_running_windows() -> Result<Vec<RunningGameWindow>, GameTargetError> {
    Err(GameTargetError::Observation(
        "native window discovery is supported only by the Windows desktop build".into(),
    ))
}

#[cfg(windows)]
fn observe_running_windows() -> Result<Vec<RunningGameWindow>, GameTargetError> {
    windows_observer::observe()
}

#[cfg(windows)]
mod windows_observer {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetClientRect, GetForegroundWindow, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GA_ROOT,
    };

    pub(super) fn observe() -> Result<Vec<RunningGameWindow>, GameTargetError> {
        let mut windows = Vec::<RunningGameWindow>::new();
        // SAFETY: the callback receives the address of `windows` for the
        // duration of this synchronous call and never retains it.
        let ok = unsafe { EnumWindows(Some(enumerate_window), (&mut windows as *mut _) as LPARAM) };
        if ok == 0 {
            return Err(GameTargetError::Observation("EnumWindows failed".into()));
        }
        Ok(windows)
    }

    unsafe extern "system" fn enumerate_window(hwnd: HWND, context: LPARAM) -> i32 {
        let windows = &mut *(context as *mut Vec<RunningGameWindow>);
        if let Some(observation) = observe_one(hwnd) {
            windows.push(observation);
        }
        1
    }

    unsafe fn observe_one(hwnd: HWND) -> Option<RunningGameWindow> {
        let mut pid = 0_u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let result = (|| {
            let mut capacity = 32_768_u32;
            let mut path = vec![0_u16; capacity as usize];
            if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut capacity) == 0 {
                return None;
            }
            path.truncate(capacity as usize);
            let executable_path = PathBuf::from(OsString::from_wide(&path));
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            if GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) == 0 {
                return None;
            }
            let process_started_at_ticks =
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
            let title_len = GetWindowTextLengthW(hwnd).max(0) as usize;
            let mut title = vec![0_u16; title_len.saturating_add(1)];
            let copied =
                GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32).max(0) as usize;
            title.truncate(copied);
            let title = OsString::from_wide(&title).to_string_lossy().into_owned();
            let mut client = RECT::default();
            if GetClientRect(hwnd, &mut client) == 0 {
                return None;
            }
            let width = client.right.saturating_sub(client.left) as u32;
            let height = client.bottom.saturating_sub(client.top) as u32;
            let mut cloaked = 0_u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            Some(RunningGameWindow {
                pid,
                process_started_at_ticks,
                executable_path,
                hwnd: hwnd as usize as u64,
                title,
                visible: IsWindowVisible(hwnd) != 0,
                top_level: GetAncestor(hwnd, GA_ROOT) == hwnd,
                cloaked: cloaked != 0,
                minimized: IsIconic(hwnd) != 0,
                client_width: width,
                client_height: height,
                foreground: GetForegroundWindow() == hwnd,
                observed_at_monotonic_ms: monotonic_ms(),
                frame_sequence: None,
                frame_observed_at_monotonic_ms: None,
            })
        })();
        CloseHandle(process);
        result
    }

    fn monotonic_ms() -> u64 {
        static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        ORIGIN
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
            + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_broker::{BrokerStatus, MediaBrokerError};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct MockCaptureBroker {
        block_target: bool,
        evidence: Mutex<Option<NativeCaptureEvidence>>,
        bound: Mutex<Vec<(u64, u32, String)>>,
        clear_count: AtomicUsize,
    }

    impl MockCaptureBroker {
        fn accepting(evidence: NativeCaptureEvidence) -> Self {
            Self {
                block_target: false,
                evidence: Mutex::new(Some(evidence)),
                bound: Mutex::new(Vec::new()),
                clear_count: AtomicUsize::new(0),
            }
        }

        fn protected_target() -> Self {
            Self {
                block_target: true,
                evidence: Mutex::new(None),
                bound: Mutex::new(Vec::new()),
                clear_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl GameCaptureBroker for MockCaptureBroker {
        async fn bind_exact_target(
            &self,
            native_window: u64,
            process_id: u32,
            executable_basename: &str,
        ) -> Result<(), MediaBrokerError> {
            self.bound.lock().expect("bound calls").push((
                native_window,
                process_id,
                executable_basename.to_owned(),
            ));
            if self.block_target {
                Err(MediaBrokerError::Remote(BrokerStatus::TargetBlocked))
            } else {
                Ok(())
            }
        }

        async fn capture_evidence(&self) -> Result<NativeCaptureEvidence, MediaBrokerError> {
            self.evidence
                .lock()
                .expect("evidence")
                .clone()
                .ok_or(MediaBrokerError::Malformed)
        }

        async fn clear_target(&self) -> Result<(), MediaBrokerError> {
            self.clear_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn cyberpunk_rules() -> Vec<ProcessWindowRule> {
        vec![ProcessWindowRule {
            executable: "Cyberpunk2077.exe".into(),
            required: true,
            window_title_regex: Some("(?i)^Cyberpunk 2077$".into()),
        }]
    }

    fn observations(
        before: RunningGameWindow,
        after: RunningGameWindow,
    ) -> impl FnMut() -> Result<Vec<RunningGameWindow>, GameTargetError> {
        let mut values = VecDeque::from([vec![before], vec![after]]);
        move || {
            values.pop_front().ok_or_else(|| {
                GameTargetError::Observation("test observation sequence exhausted".into())
            })
        }
    }

    fn observation(pid: u32, hwnd: u64, executable: &str) -> RunningGameWindow {
        let executable_path = if cfg!(windows) {
            PathBuf::from(r"C:\Games\Fixture").join(executable)
        } else {
            PathBuf::from("/games/fixture").join(executable)
        };
        RunningGameWindow {
            pid,
            process_started_at_ticks: 91,
            executable_path,
            hwnd,
            title: "Fixture Game".into(),
            visible: true,
            top_level: true,
            cloaked: false,
            minimized: false,
            client_width: 1280,
            client_height: 720,
            foreground: true,
            observed_at_monotonic_ms: 20,
            frame_sequence: None,
            frame_observed_at_monotonic_ms: None,
        }
    }

    fn evidence(pid: u32, hwnd: u64, executable: &str, sequence: u64) -> NativeCaptureEvidence {
        NativeCaptureEvidence {
            schema_version: 3,
            selected_process_id: pid,
            selected_window_handle: hwnd,
            device_generation: 1,
            geometry_epoch: 1,
            latest_frame_sequence: sequence,
            latest_frame_qpc: 99,
            initial_content_hash: 1,
            latest_content_hash: 2,
            content_hash_changes: 1,
            geometry_changes: 0,
            nonadvancing_frames: 0,
            content_width: 1280,
            content_height: 720,
            overlay_capture_excluded: true,
            overlay_visuals_allowed: true,
            pixel_source: CapturePixelSource::WindowsGraphicsCaptureTexture,
            pixel_scope: CapturePixelScope::ExactSelectedWindow,
            external_display_overlay_pixels_excluded: true,
            desktop_luminance_excluded_from_pixel_evidence: true,
            external_display_overlays_may_change_perceived_brightness: true,
            selected_executable_name: executable.into(),
        }
    }

    #[test]
    fn hidden_synthetic_review_profile_has_a_debug_capture_policy() {
        let resources = ResourceCatalog::new(None);
        assert!(!resources
            .game_summaries()
            .iter()
            .any(|game| game.id == "eclipse-harbor"));
        let (rules, _) = profile_policy(&resources, "eclipse-harbor").expect("synthetic policy");
        assert!(rules.iter().any(|rule| {
            rule.required && rule.executable == "interactive-npcs-synthetic-target.exe"
        }));
    }

    #[test]
    fn wrong_pid_hwnd_or_executable_evidence_fails_closed() {
        for evidence in [
            evidence(8, 22, "fixture.exe", 1),
            evidence(7, 23, "fixture.exe", 1),
            evidence(7, 22, "other.exe", 1),
        ] {
            let manager = GameTargetManager::default();
            let observation = observation(7, 22, "fixture.exe");
            manager
                .select_from_observations(
                    "fixture",
                    vec![ProcessWindowRule {
                        executable: "fixture.exe".into(),
                        required: true,
                        window_title_regex: None,
                    }],
                    vec![],
                    std::slice::from_ref(&observation),
                )
                .expect("select");
            assert!(manager
                .verify_evidence_from_observations(&[observation], evidence)
                .is_err());
        }
    }

    #[test]
    fn full_display_pixels_cannot_satisfy_exact_selected_window_proof() {
        let manager = GameTargetManager::default();
        let observation = observation(7, 22, "fixture.exe");
        manager
            .select_from_observations(
                "fixture",
                vec![ProcessWindowRule {
                    executable: "fixture.exe".into(),
                    required: true,
                    window_title_regex: None,
                }],
                vec![],
                std::slice::from_ref(&observation),
            )
            .expect("select");
        let mut display_evidence = evidence(7, 22, "fixture.exe", 1);
        display_evidence.pixel_source = CapturePixelSource::DesktopDuplicationTexture;
        display_evidence.pixel_scope = CapturePixelScope::FullDisplayOutput;
        display_evidence.external_display_overlay_pixels_excluded = false;
        display_evidence.desktop_luminance_excluded_from_pixel_evidence = false;
        assert!(manager
            .verify_evidence_from_observations(&[observation], display_evidence)
            .is_err());
    }

    #[test]
    fn unchanged_synthetic_pixels_cannot_satisfy_advancing_capture_proof() {
        let manager = GameTargetManager::default();
        let observation = observation(7, 22, "fixture.exe");
        manager
            .select_from_observations(
                "fixture",
                vec![ProcessWindowRule {
                    executable: "fixture.exe".into(),
                    required: true,
                    window_title_regex: None,
                }],
                vec![],
                std::slice::from_ref(&observation),
            )
            .expect("select");
        let mut unchanged = evidence(7, 22, "fixture.exe", 3);
        unchanged.latest_content_hash = unchanged.initial_content_hash;
        unchanged.content_hash_changes = 0;
        assert!(manager
            .verify_evidence_from_observations(&[observation], unchanged)
            .is_err());
    }

    #[test]
    fn verified_broker_bound_capture_is_reflected_in_selection_snapshot() {
        let manager = GameTargetManager::default();
        let observation = observation(7, 22, "fixture.exe");
        manager
            .select_from_observations(
                "fixture",
                vec![ProcessWindowRule {
                    executable: "fixture.exe".into(),
                    required: true,
                    window_title_regex: None,
                }],
                vec![],
                std::slice::from_ref(&observation),
            )
            .expect("select");
        manager
            .selected
            .lock()
            .expect("target state")
            .as_mut()
            .expect("selected target")
            .broker_bound = true;
        manager
            .verify_evidence_from_observations(
                std::slice::from_ref(&observation),
                evidence(7, 22, "fixture.exe", 2),
            )
            .expect("capture verification");

        let snapshot = manager.snapshot().expect("snapshot").expect("selection");
        assert!(snapshot.capture_authorized);
        assert_eq!(snapshot.safety_state, "verified_synthetic_fixture");
    }

    #[test]
    fn stale_process_instance_and_nonadvancing_frames_fail_closed() {
        let manager = GameTargetManager::default();
        let observation = observation(7, 22, "fixture.exe");
        manager
            .select_from_observations(
                "fixture",
                vec![ProcessWindowRule {
                    executable: "fixture.exe".into(),
                    required: true,
                    window_title_regex: None,
                }],
                vec![],
                std::slice::from_ref(&observation),
            )
            .expect("select");
        manager
            .verify_evidence_from_observations(
                std::slice::from_ref(&observation),
                evidence(7, 22, "fixture.exe", 2),
            )
            .expect("first evidence");
        assert!(manager
            .verify_evidence_from_observations(
                std::slice::from_ref(&observation),
                evidence(7, 22, "fixture.exe", 2),
            )
            .is_err());

        let mut reused = observation;
        reused.process_started_at_ticks += 1;
        assert!(manager
            .verify_evidence_from_observations(&[reused], evidence(7, 22, "fixture.exe", 3))
            .is_err());
    }

    #[tokio::test]
    async fn supported_cyberpunk_selection_binds_exact_target_but_awaits_capture_proof() {
        let resources = ResourceCatalog::new(None);
        let manager = GameTargetManager::default();
        let mut current = observation(2077, 0x2077, "Cyberpunk2077.exe");
        current.title = "Cyberpunk 2077".into();
        manager
            .select_from_observations(
                "cyberpunk-2077",
                cyberpunk_rules(),
                vec![],
                std::slice::from_ref(&current),
            )
            .expect("select Cyberpunk");
        let broker = MockCaptureBroker::accepting(evidence(
            current.pid,
            current.hwnd,
            "Cyberpunk2077.exe",
            2,
        ));

        let selection = manager
            .bind_selected_capture_with_observer(
                &resources,
                &broker,
                observations(current.clone(), current),
            )
            .await
            .expect("bind supported target");

        assert_eq!(
            broker.bound.lock().expect("bound calls").as_slice(),
            &[(0x2077, 2077, "Cyberpunk2077.exe".into())]
        );
        assert!(!selection.capture_authorized);
        assert_eq!(selection.safety_state, "capture_pending");
        assert!(selection
            .safety_detail
            .contains("supported Cyberpunk 2077 profile"));
    }

    #[tokio::test]
    async fn changed_process_path_after_broker_binding_revokes_the_target() {
        let resources = ResourceCatalog::new(None);
        let manager = GameTargetManager::default();
        let mut before = observation(2077, 0x2077, "Cyberpunk2077.exe");
        before.title = "Cyberpunk 2077".into();
        manager
            .select_from_observations(
                "cyberpunk-2077",
                cyberpunk_rules(),
                vec![],
                std::slice::from_ref(&before),
            )
            .expect("select Cyberpunk");
        let mut changed = before.clone();
        changed.executable_path = if cfg!(windows) {
            PathBuf::from(r"C:\Modded\Cyberpunk2077.exe")
        } else {
            PathBuf::from("/modded/Cyberpunk2077.exe")
        };
        let broker =
            MockCaptureBroker::accepting(evidence(before.pid, before.hwnd, "Cyberpunk2077.exe", 2));

        let result = manager
            .bind_selected_capture_with_observer(&resources, &broker, observations(before, changed))
            .await;

        assert!(matches!(result, Err(GameTargetError::Revalidation(_))));
        assert_eq!(broker.clear_count.load(Ordering::SeqCst), 1);
        assert!(
            !manager
                .snapshot()
                .expect("snapshot")
                .expect("selected")
                .capture_authorized
        );
    }

    #[tokio::test]
    async fn broker_protected_target_refusal_does_not_mark_capture_bound() {
        let resources = ResourceCatalog::new(None);
        let manager = GameTargetManager::default();
        let mut current = observation(2077, 0x2077, "Cyberpunk2077.exe");
        current.title = "Cyberpunk 2077".into();
        manager
            .select_from_observations(
                "cyberpunk-2077",
                cyberpunk_rules(),
                vec![],
                std::slice::from_ref(&current),
            )
            .expect("select Cyberpunk");
        let broker = MockCaptureBroker::protected_target();

        let result = manager
            .bind_selected_capture_with_observer(
                &resources,
                &broker,
                observations(current.clone(), current),
            )
            .await;

        assert!(matches!(result, Err(GameTargetError::Broker(_))));
        let snapshot = manager.snapshot().expect("snapshot").expect("selected");
        assert!(!snapshot.capture_authorized);
        assert_eq!(snapshot.safety_state, "supported_profile_user_confirmed");
    }

    #[tokio::test]
    async fn checkbox_absence_cannot_bind_even_the_supported_profile() {
        let resources = ResourceCatalog::new(None);
        let manager = GameTargetManager::default();
        let mut current = observation(2077, 0x2077, "Cyberpunk2077.exe");
        current.title = "Cyberpunk 2077".into();
        manager
            .select_from_observations(
                "cyberpunk-2077",
                cyberpunk_rules(),
                vec![],
                std::slice::from_ref(&current),
            )
            .expect("select Cyberpunk");
        manager
            .selected
            .lock()
            .expect("selected target")
            .as_mut()
            .expect("selected target")
            .user_confirmed_offline = false;
        let broker = MockCaptureBroker::accepting(evidence(
            current.pid,
            current.hwnd,
            "Cyberpunk2077.exe",
            2,
        ));

        let result = manager
            .bind_selected_capture_with_observer(
                &resources,
                &broker,
                observations(current.clone(), current),
            )
            .await;

        assert!(matches!(result, Err(GameTargetError::SafetyUnverified)));
        assert!(broker.bound.lock().expect("bound calls").is_empty());
    }

    #[tokio::test]
    async fn desktop_duplication_cannot_verify_supported_cyberpunk_capture() {
        let resources = ResourceCatalog::new(None);
        let manager = GameTargetManager::default();
        let mut current = observation(2077, 0x2077, "Cyberpunk2077.exe");
        current.title = "Cyberpunk 2077".into();
        manager
            .select_from_observations(
                "cyberpunk-2077",
                cyberpunk_rules(),
                vec![],
                std::slice::from_ref(&current),
            )
            .expect("select Cyberpunk");
        let mut fallback = evidence(2077, 0x2077, "Cyberpunk2077.exe", 2);
        fallback.pixel_source = CapturePixelSource::DesktopDuplicationTexture;
        fallback.pixel_scope = CapturePixelScope::FullDisplayOutput;
        fallback.external_display_overlay_pixels_excluded = false;
        fallback.desktop_luminance_excluded_from_pixel_evidence = false;
        let broker = MockCaptureBroker::accepting(fallback);
        manager
            .bind_selected_capture_with_observer(
                &resources,
                &broker,
                observations(current.clone(), current.clone()),
            )
            .await
            .expect("bind supported target");

        let result = manager
            .verify_supported_capture_with_observer(&broker, move || Ok(vec![current.clone()]))
            .await;

        assert!(matches!(result, Err(GameTargetError::Revalidation(_))));
        assert!(
            !manager
                .snapshot()
                .expect("snapshot")
                .expect("selected")
                .capture_authorized
        );
    }

    #[tokio::test]
    async fn advancing_exact_window_wgc_receipt_authorizes_supported_capture() {
        let resources = ResourceCatalog::new(None);
        let manager = GameTargetManager::default();
        let mut current = observation(2077, 0x2077, "Cyberpunk2077.exe");
        current.title = "Cyberpunk 2077".into();
        manager
            .select_from_observations(
                "cyberpunk-2077",
                cyberpunk_rules(),
                vec![],
                std::slice::from_ref(&current),
            )
            .expect("select Cyberpunk");
        let broker = MockCaptureBroker::accepting(evidence(
            current.pid,
            current.hwnd,
            "Cyberpunk2077.exe",
            2,
        ));
        manager
            .bind_selected_capture_with_observer(
                &resources,
                &broker,
                observations(current.clone(), current.clone()),
            )
            .await
            .expect("bind supported target");

        let verification = manager
            .verify_supported_capture_with_observer(&broker, move || Ok(vec![current.clone()]))
            .await
            .expect("verify exact-window WGC");

        assert_eq!(verification.safety_state, "verified_exact_window_wgc");
        assert!(verification.exact_pid_hwnd_executable_match);
        assert!(verification.frame_sequence_advanced);
        let snapshot = manager.snapshot().expect("snapshot").expect("selected");
        assert!(snapshot.capture_authorized);
        assert_eq!(snapshot.safety_state, "verified_exact_window_wgc");
    }
}
