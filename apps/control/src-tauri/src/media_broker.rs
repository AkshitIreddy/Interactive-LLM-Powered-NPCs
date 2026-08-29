use crate::domain::{MediaBrokerDiagnostics, MediaBrokerHealthSnapshot, RuntimeConnectionState};
use crate::sidecar_supervisor::RuntimeSupervisor;
use prost::{Enumeration, Message};
#[cfg(debug_assertions)]
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
#[cfg(not(windows))]
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const BROKER_FILE_NAME: &str = if cfg!(windows) {
    "npc-media-broker.exe"
} else {
    "npc-media-broker"
};
const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES: usize = 3;
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_TARGET_BASENAME: &str = "interactive-npcs-synthetic-target.exe";
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_METADATA_MAX_BYTES: u64 = 32 * 1024;
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_METADATA_FILE_NAME: &str = "debug-synthetic-replay-target.json";

#[derive(Clone, PartialEq, Message)]
struct BrokerEnvelope {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(bytes = "vec", tag = "2")]
    launch_nonce: Vec<u8>,
    #[prost(string, tag = "3")]
    session_id: String,
    #[prost(uint64, tag = "4")]
    sequence: u64,
    #[prost(uint64, tag = "5")]
    deadline_qpc: u64,
    #[prost(uint64, tag = "6")]
    cancellation_generation: u64,
    #[prost(enumeration = "BrokerCommand", tag = "7")]
    command: i32,
    #[prost(bytes = "vec", tag = "8")]
    payload: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct BrokerResponse {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(uint64, tag = "2")]
    response_to_sequence: u64,
    #[prost(enumeration = "BrokerStatus", tag = "3")]
    status: i32,
    #[prost(uint64, tag = "4")]
    cancellation_generation: u64,
    #[prost(bytes = "vec", tag = "5")]
    payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
enum BrokerCommand {
    Unspecified = 0,
    Health = 1,
    #[cfg(debug_assertions)]
    SelectTarget = 2,
    #[cfg(debug_assertions)]
    ClearTarget = 3,
    Diagnostics = 9,
    Shutdown = 10,
}

impl BrokerCommand {
    fn advances_cancellation_generation_on_success(self) -> bool {
        #[cfg(debug_assertions)]
        if matches!(self, Self::SelectTarget | Self::ClearTarget) {
            return true;
        }
        false
    }
}

#[cfg(debug_assertions)]
#[derive(Clone, PartialEq, Message)]
struct SelectTargetPayload {
    #[prost(uint64, tag = "1")]
    native_window: u64,
    #[prost(uint32, tag = "2")]
    expected_process_id: u32,
    #[prost(string, repeated, tag = "3")]
    allowed_process_names: Vec<String>,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct DebugSyntheticTargetIdentity {
    native_window: u64,
    process_id: u32,
    executable_basename: String,
}

#[cfg(debug_assertions)]
#[derive(Debug, Deserialize)]
struct DebugSyntheticTargetMetadata {
    schema_version: u32,
    fixture_kind: String,
    state: String,
    pid: u32,
    process_id: u32,
    window_handle: i64,
    hwnd: i64,
    executable_basename: String,
    exe_basename: String,
    decoded_frames: u64,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugSyntheticReplayCaptureSnapshot {
    pub target_process_id: u32,
    pub target_window_handle: u64,
    pub target_executable_basename: String,
    pub diagnostics: MediaBrokerDiagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum BrokerStatus {
    Ok = 0,
    InvalidFrame = 1,
    UnsupportedVersion = 2,
    AuthenticationFailed = 3,
    SessionMismatch = 4,
    SequenceReplayed = 5,
    DeadlineExpired = 6,
    DeadlineTooFar = 7,
    CancellationMismatch = 8,
    PayloadInvalid = 9,
    TargetBlocked = 10,
    CapabilityUnavailable = 11,
    InternalError = 12,
}

struct BrokerConnection {
    stream: BrokerStream,
    nonce: [u8; 32],
    session_id: String,
    next_sequence: u64,
    cancellation_generation: u64,
}

type BrokerStream = std::fs::File;

#[derive(Clone)]
struct BrokerClient(Arc<Mutex<BrokerConnection>>);

impl std::fmt::Debug for BrokerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BrokerClient { authenticated: true }")
    }
}

impl BrokerClient {
    fn new(stream: BrokerStream, nonce: [u8; 32], session_id: String) -> Self {
        Self(Arc::new(Mutex::new(BrokerConnection {
            stream,
            nonce,
            session_id,
            next_sequence: 1,
            cancellation_generation: 0,
        })))
    }

    async fn health(&self) -> Result<BrokerHealth, MediaBrokerError> {
        let response = self.request(BrokerCommand::Health).await?;
        if response.payload.len() != 12 {
            return Err(MediaBrokerError::Malformed);
        }
        Ok(BrokerHealth {
            broker_state: read_u32(&response.payload, 8)?,
        })
    }

    async fn diagnostics(&self) -> Result<MediaBrokerDiagnostics, MediaBrokerError> {
        let response = self.request(BrokerCommand::Diagnostics).await?;
        decode_diagnostics(&response.payload)
    }

    #[cfg(debug_assertions)]
    async fn select_debug_synthetic_target(
        &self,
        identity: &DebugSyntheticTargetIdentity,
    ) -> Result<(), MediaBrokerError> {
        let payload = SelectTargetPayload {
            native_window: identity.native_window,
            expected_process_id: identity.process_id,
            allowed_process_names: vec![identity.executable_basename.clone()],
        }
        .encode_to_vec();
        self.request_with_payload(BrokerCommand::SelectTarget, payload)
            .await
            .map(|_| ())
    }

    #[cfg(debug_assertions)]
    async fn clear_debug_synthetic_target(&self) -> Result<(), MediaBrokerError> {
        self.request(BrokerCommand::ClearTarget).await.map(|_| ())
    }

    async fn shutdown(&self) -> Result<(), MediaBrokerError> {
        self.request(BrokerCommand::Shutdown).await.map(|_| ())
    }

    async fn request(&self, command: BrokerCommand) -> Result<BrokerResponse, MediaBrokerError> {
        self.request_with_payload(command, Vec::new()).await
    }

    async fn request_with_payload(
        &self,
        command: BrokerCommand,
        payload: Vec<u8>,
    ) -> Result<BrokerResponse, MediaBrokerError> {
        let connection = Arc::clone(&self.0);
        let task = tauri::async_runtime::spawn_blocking(move || {
            request_blocking(&connection, command, payload)
        });
        tokio::time::timeout(REQUEST_TIMEOUT, task)
            .await
            .map_err(|_| MediaBrokerError::Timeout)?
            .map_err(|_| MediaBrokerError::Connection)?
    }
}

fn request_blocking(
    connection: &Mutex<BrokerConnection>,
    command: BrokerCommand,
    payload: Vec<u8>,
) -> Result<BrokerResponse, MediaBrokerError> {
    if payload.len().saturating_add(4) > MAX_FRAME_BYTES {
        return Err(MediaBrokerError::Payload);
    }
    let mut connection = connection.lock().map_err(|_| MediaBrokerError::State)?;
    let sequence = connection.next_sequence;
    connection.next_sequence = connection.next_sequence.saturating_add(1);
    let (now, frequency) = qpc_now();
    let envelope = BrokerEnvelope {
        version: PROTOCOL_VERSION,
        launch_nonce: connection.nonce.to_vec(),
        session_id: connection.session_id.clone(),
        sequence,
        deadline_qpc: now.saturating_add(frequency.saturating_mul(5)),
        cancellation_generation: connection.cancellation_generation,
        command: command as i32,
        payload,
    };
    let body = envelope.encode_to_vec();
    write_frame(&mut connection.stream, &body)?;
    let frame = read_frame(&mut connection.stream)?;
    let response =
        BrokerResponse::decode(frame.as_slice()).map_err(|_| MediaBrokerError::Malformed)?;
    if response.version != PROTOCOL_VERSION || response.response_to_sequence != sequence {
        return Err(MediaBrokerError::Malformed);
    }
    let status = response.status();
    connection.cancellation_generation = reconcile_response_generation(
        command,
        connection.cancellation_generation,
        response.cancellation_generation,
        status,
    )?;
    if status != BrokerStatus::Ok {
        return Err(MediaBrokerError::Remote(status));
    }
    Ok(response)
}

fn reconcile_response_generation(
    command: BrokerCommand,
    current: u64,
    response: u64,
    status: BrokerStatus,
) -> Result<u64, MediaBrokerError> {
    let mutating = command.advances_cancellation_generation_on_success();
    if !mutating {
        return (response == current)
            .then_some(current)
            .ok_or(MediaBrokerError::Malformed);
    }

    let next = current.checked_add(1).ok_or(MediaBrokerError::Malformed)?;
    if response != current && response != next {
        return Err(MediaBrokerError::Malformed);
    }
    if status == BrokerStatus::Ok {
        // Newer brokers return the post-command generation. The original V1
        // SelectTarget/ClearTarget response was assembled before dispatch and
        // still carried `current`; both represent the same guaranteed one-step
        // transition after a successful target mutation.
        Ok(next)
    } else {
        // A rejected command may fail before mutation. Only adopt a transition
        // when the broker explicitly reports it.
        Ok(response)
    }
}

#[derive(Debug, Clone, Copy)]
struct BrokerHealth {
    broker_state: u32,
}

#[derive(Clone, Debug)]
pub struct MediaBrokerLaunchConfig {
    pub executable: PathBuf,
    pub development_fixture_allowed: bool,
    #[cfg(debug_assertions)]
    pub debug_synthetic_metadata_path: PathBuf,
}

impl MediaBrokerLaunchConfig {
    pub fn from_application(
        development_fixture_allowed: bool,
        app_config_directory: &Path,
    ) -> Result<Self, MediaBrokerError> {
        #[cfg(not(debug_assertions))]
        let _ = app_config_directory;
        let executable = std::env::current_exe()
            .map_err(|_| MediaBrokerError::InvalidBundle)?
            .parent()
            .ok_or(MediaBrokerError::InvalidBundle)?
            .join(BROKER_FILE_NAME);
        Ok(Self {
            executable,
            development_fixture_allowed,
            #[cfg(debug_assertions)]
            debug_synthetic_metadata_path: app_config_directory
                .join(DEBUG_SYNTHETIC_METADATA_FILE_NAME),
        })
    }
}

#[derive(Debug)]
struct ManagedBroker {
    child: BrokerChild,
    client: BrokerClient,
    pid: u32,
}

#[derive(Debug)]
struct BrokerSupervisorState {
    connection: RuntimeConnectionState,
    detail: String,
    process_id: Option<u32>,
    restart_count: u32,
    recent_failures: VecDeque<Instant>,
    broker_state: Option<u32>,
    diagnostics: Option<MediaBrokerDiagnostics>,
    #[cfg(debug_assertions)]
    debug_synthetic_target: Option<DebugSyntheticTargetIdentity>,
}

#[derive(Clone)]
pub struct MediaBrokerSupervisor {
    config: Arc<MediaBrokerLaunchConfig>,
    parent_job: RuntimeSupervisor,
    state: Arc<Mutex<BrokerSupervisorState>>,
    managed: Arc<tokio::sync::Mutex<Option<ManagedBroker>>>,
    startup_gate: Arc<tokio::sync::Mutex<()>>,
    shutdown_token: CancellationToken,
    #[cfg(debug_assertions)]
    debug_synthetic_capture_gate: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for MediaBrokerSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaBrokerSupervisor")
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl MediaBrokerSupervisor {
    pub fn new(config: MediaBrokerLaunchConfig, parent_job: RuntimeSupervisor) -> Self {
        let exists = config.executable.is_file();
        let (connection, detail) = if exists {
            (
                RuntimeConnectionState::Cold,
                "Bundled media broker is ready to start.",
            )
        } else if config.development_fixture_allowed {
            (
                RuntimeConnectionState::DevelopmentFixture,
                "Media broker is skipped in this unbundled source-development build.",
            )
        } else {
            (
                RuntimeConnectionState::Unavailable,
                "Required bundled media broker is missing.",
            )
        };
        Self {
            config: Arc::new(config),
            parent_job,
            state: Arc::new(Mutex::new(BrokerSupervisorState {
                connection,
                detail: detail.into(),
                process_id: None,
                restart_count: 0,
                recent_failures: VecDeque::new(),
                broker_state: None,
                diagnostics: None,
                #[cfg(debug_assertions)]
                debug_synthetic_target: None,
            })),
            managed: Arc::new(tokio::sync::Mutex::new(None)),
            startup_gate: Arc::new(tokio::sync::Mutex::new(())),
            shutdown_token: CancellationToken::new(),
            #[cfg(debug_assertions)]
            debug_synthetic_capture_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn health(&self) -> MediaBrokerHealthSnapshot {
        let Ok(state) = self.state.lock() else {
            return unavailable_health("Media broker supervisor state is unavailable.");
        };
        let diagnostics = state.diagnostics.as_ref();
        MediaBrokerHealthSnapshot {
            state: state.connection,
            connected: state.connection == RuntimeConnectionState::Ready,
            process_id: state.process_id,
            restart_count: state.restart_count,
            recent_failure_count: state.recent_failures.len() as u32,
            protocol_version: (state.connection == RuntimeConnectionState::Ready)
                .then_some(PROTOCOL_VERSION),
            fixture_only: state.connection == RuntimeConnectionState::DevelopmentFixture,
            broker_state: state.broker_state.map(broker_state_name).map(str::to_owned),
            capture_available: diagnostics.is_some_and(|value| value.capture_backend != "none"),
            overlay_available: diagnostics.is_some_and(|value| value.overlay_backend != "none"),
            capture_audio_available: diagnostics
                .is_some_and(|value| matches!(value.capture_audio.as_str(), "ready" | "capturing")),
            render_audio_available: diagnostics
                .is_some_and(|value| matches!(value.render_audio.as_str(), "ready" | "playing")),
            detail: state.detail.clone(),
        }
    }

    pub fn start_background(&self) {
        if self.health().state == RuntimeConnectionState::DevelopmentFixture {
            return;
        }
        let supervisor = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                if supervisor.shutdown_token.is_cancelled() {
                    break;
                }
                let _ = supervisor.ensure_ready().await;
                tokio::select! {
                    _ = supervisor.shutdown_token.cancelled() => break,
                    _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {}
                }
            }
        });
    }

    async fn ensure_ready(&self) -> Result<BrokerClient, MediaBrokerError> {
        let health = self.health();
        if health.state == RuntimeConnectionState::DevelopmentFixture {
            return Err(MediaBrokerError::DevelopmentFixture);
        }
        if health.state == RuntimeConnectionState::Quarantined {
            return Err(MediaBrokerError::Quarantined);
        }
        let _startup = self.startup_gate.lock().await;
        if let Some(client) = self.live_client().await? {
            return Ok(client);
        }
        self.apply_backoff().await?;
        self.set_connection(
            RuntimeConnectionState::Starting,
            "Starting authenticated media broker.",
        );
        match self.launch().await {
            Ok(managed) => {
                let client = managed.client.clone();
                let pid = managed.pid;
                *self.managed.lock().await = Some(managed);
                self.set_ready(pid);
                Ok(client)
            }
            Err(error) => {
                self.record_failure(error.to_string()).await;
                Err(error)
            }
        }
    }

    pub async fn refresh_health(&self) -> Result<MediaBrokerHealthSnapshot, MediaBrokerError> {
        let client = self.ensure_ready().await?;
        let health = client.health().await?;
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.broker_state = Some(health.broker_state);
            state.diagnostics = Some(diagnostics);
        }
        Ok(self.health())
    }

    pub async fn diagnostics(&self) -> Result<MediaBrokerDiagnostics, MediaBrokerError> {
        let client = self.ensure_ready().await?;
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.diagnostics = Some(diagnostics.clone());
        }
        Ok(diagnostics)
    }

    /// Selects the task-owned synthetic replay window in debug builds only.
    ///
    /// The native broker still performs its complete same-user/session,
    /// anti-cheat, HWND/PID, and executable-name policy inspection. This
    /// control-plane gate additionally prevents the debug command from being
    /// reused to capture an arbitrary game or application.
    #[cfg(debug_assertions)]
    pub async fn debug_select_synthetic_replay_capture_target(
        &self,
    ) -> Result<DebugSyntheticReplayCaptureSnapshot, MediaBrokerError> {
        let identity = read_debug_synthetic_target(&self.config.debug_synthetic_metadata_path)?;
        let _operation = self.debug_synthetic_capture_gate.lock().await;
        {
            let state = self.state.lock().map_err(|_| MediaBrokerError::State)?;
            if state
                .debug_synthetic_target
                .as_ref()
                .is_some_and(|selected| selected != &identity)
            {
                return Err(MediaBrokerError::DebugSyntheticTargetMismatch);
            }
        }

        let client = self.ensure_ready().await?;
        if let Err(error) = client.select_debug_synthetic_target(&identity).await {
            if let Ok(mut state) = self.state.lock() {
                state.debug_synthetic_target = None;
                state.diagnostics = None;
            }
            return Err(error);
        }
        if let Ok(mut state) = self.state.lock() {
            state.debug_synthetic_target = Some(identity.clone());
        }
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.diagnostics = Some(diagnostics.clone());
        }
        Ok(debug_synthetic_capture_snapshot(identity, diagnostics))
    }

    #[cfg(debug_assertions)]
    pub async fn debug_clear_synthetic_replay_capture_target(
        &self,
    ) -> Result<DebugSyntheticReplayCaptureSnapshot, MediaBrokerError> {
        let _operation = self.debug_synthetic_capture_gate.lock().await;
        let identity = self.selected_debug_synthetic_target()?;
        let client = self.ensure_ready().await?;
        client.clear_debug_synthetic_target().await?;
        if let Ok(mut state) = self.state.lock() {
            state.debug_synthetic_target = None;
            state.diagnostics = None;
        }
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.diagnostics = Some(diagnostics.clone());
        }
        Ok(debug_synthetic_capture_snapshot(identity, diagnostics))
    }

    #[cfg(debug_assertions)]
    pub async fn debug_synthetic_replay_capture_diagnostics(
        &self,
    ) -> Result<DebugSyntheticReplayCaptureSnapshot, MediaBrokerError> {
        let _operation = self.debug_synthetic_capture_gate.lock().await;
        let identity = self.selected_debug_synthetic_target()?;
        let diagnostics = self.diagnostics().await?;
        Ok(debug_synthetic_capture_snapshot(identity, diagnostics))
    }

    #[cfg(debug_assertions)]
    fn selected_debug_synthetic_target(
        &self,
    ) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
        let state = self.state.lock().map_err(|_| MediaBrokerError::State)?;
        state
            .debug_synthetic_target
            .clone()
            .ok_or(MediaBrokerError::DebugSyntheticTargetMismatch)
    }

    pub async fn shutdown(&self) {
        self.shutdown_token.cancel();
        self.set_connection(
            RuntimeConnectionState::ShuttingDown,
            "Media broker is shutting down.",
        );
        if let Some(mut managed) = self.managed.lock().await.take() {
            let _ = managed.client.shutdown().await;
            if tokio::time::timeout(SHUTDOWN_TIMEOUT, managed.child.wait())
                .await
                .is_err()
            {
                let _ = managed.child.kill().await;
                let _ = managed.child.wait().await;
            }
        }
        self.set_connection(RuntimeConnectionState::Stopped, "Media broker is stopped.");
    }

    async fn live_client(&self) -> Result<Option<BrokerClient>, MediaBrokerError> {
        let (client, exited) = {
            let mut managed = self.managed.lock().await;
            let Some(managed) = managed.as_mut() else {
                return Ok(None);
            };
            let exited = managed
                .child
                .try_wait()
                .map_err(|_| MediaBrokerError::Process)?;
            (managed.client.clone(), exited.is_some())
        };
        if exited {
            self.record_failure("Media broker exited unexpectedly.".into())
                .await;
            return Ok(None);
        }
        match client.health().await {
            Ok(health) => {
                if let Ok(mut state) = self.state.lock() {
                    state.broker_state = Some(health.broker_state);
                }
                Ok(Some(client))
            }
            Err(error) => {
                self.record_failure(error.to_string()).await;
                Ok(None)
            }
        }
    }

    async fn launch(&self) -> Result<ManagedBroker, MediaBrokerError> {
        let executable = validate_fixed_broker(&self.config.executable)?;
        let parent_pid = std::process::id();
        let mut nonce = [0_u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| MediaBrokerError::Random)?;
        let session_id = format!("media-{}", uuid::Uuid::new_v4().simple());
        let nonce_hex = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let args = [
            format!("--parent-pid={parent_pid}"),
            format!("--session={session_id}"),
            format!("--nonce={nonce_hex}"),
        ];
        let child = spawn_broker_process(&executable, &args, &self.parent_job)?;
        let pid = child.id();
        let pipe = format!(r"\\.\pipe\npc-media-broker-{session_id}");
        let stream = connect_broker_pipe(&pipe).await?;
        let client = BrokerClient::new(stream, nonce, session_id);
        client.health().await?;
        Ok(ManagedBroker { child, client, pid })
    }

    async fn record_failure(&self, detail: String) {
        if let Some(mut managed) = self.managed.lock().await.take() {
            let _ = managed.child.kill().await;
            let _ = managed.child.wait().await;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let now = Instant::now();
        state.recent_failures.push_back(now);
        while state
            .recent_failures
            .front()
            .is_some_and(|failure| now.duration_since(*failure) > FAILURE_WINDOW)
        {
            state.recent_failures.pop_front();
        }
        state.process_id = None;
        state.diagnostics = None;
        #[cfg(debug_assertions)]
        {
            state.debug_synthetic_target = None;
        }
        if state.recent_failures.len() >= MAX_FAILURES {
            state.connection = RuntimeConnectionState::Quarantined;
            state.detail = "Media broker quarantined after three failures in sixty seconds; runtime conversation remains available with audio/subtitle fallbacks.".into();
        } else {
            state.connection = RuntimeConnectionState::RestartBackoff;
            state.restart_count = state.restart_count.saturating_add(1);
            state.detail = detail;
        }
    }

    async fn apply_backoff(&self) -> Result<(), MediaBrokerError> {
        let failures = self
            .state
            .lock()
            .map_err(|_| MediaBrokerError::State)?
            .recent_failures
            .len();
        if failures >= MAX_FAILURES {
            return Err(MediaBrokerError::Quarantined);
        }
        let delay = match failures {
            0 => Duration::ZERO,
            1 => Duration::from_millis(250),
            _ => Duration::from_secs(1),
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        Ok(())
    }

    fn set_ready(&self, pid: u32) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.connection = RuntimeConnectionState::Ready;
        state.process_id = Some(pid);
        state.detail = "Authenticated media broker control channel is ready.".into();
    }

    fn set_connection(&self, connection: RuntimeConnectionState, detail: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.connection = connection;
        state.detail = detail.into();
        if connection != RuntimeConnectionState::Ready {
            state.process_id = None;
        }
    }
}

#[cfg(debug_assertions)]
fn validate_debug_synthetic_target(
    native_window: u64,
    process_id: u32,
    executable_basename: &str,
) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
    if native_window == 0 || usize::try_from(native_window).is_err() {
        return Err(MediaBrokerError::DebugSyntheticTargetInvalid);
    }
    if process_id == 0 || executable_basename != DEBUG_SYNTHETIC_TARGET_BASENAME {
        return Err(MediaBrokerError::DebugSyntheticTargetInvalid);
    }
    Ok(DebugSyntheticTargetIdentity {
        native_window,
        process_id,
        executable_basename: executable_basename.into(),
    })
}

#[cfg(debug_assertions)]
fn read_debug_synthetic_target(
    path: &Path,
) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
    if path.file_name().and_then(|value| value.to_str()) != Some(DEBUG_SYNTHETIC_METADATA_FILE_NAME)
    {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| MediaBrokerError::DebugSyntheticMetadataUnavailable)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > DEBUG_SYNTHETIC_METADATA_MAX_BYTES
    {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    let bytes = std::fs::read(path).map_err(|_| MediaBrokerError::DebugSyntheticMetadataInvalid)?;
    if bytes.len() as u64 > DEBUG_SYNTHETIC_METADATA_MAX_BYTES {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    decode_debug_synthetic_target_metadata(&bytes)
}

#[cfg(debug_assertions)]
fn decode_debug_synthetic_target_metadata(
    bytes: &[u8],
) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
    let metadata: DebugSyntheticTargetMetadata = serde_json::from_slice(bytes)
        .map_err(|_| MediaBrokerError::DebugSyntheticMetadataInvalid)?;
    if metadata.schema_version != 1
        || metadata.fixture_kind != "synthetic-original-video-replay"
        || metadata.state != "playing"
        || metadata.decoded_frames == 0
        || metadata.pid != metadata.process_id
        || metadata.hwnd != metadata.window_handle
        || metadata.window_handle <= 0
        || metadata.exe_basename != metadata.executable_basename
    {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    validate_debug_synthetic_target(
        metadata.window_handle as u64,
        metadata.process_id,
        &metadata.executable_basename,
    )
}

#[cfg(debug_assertions)]
fn debug_synthetic_capture_snapshot(
    identity: DebugSyntheticTargetIdentity,
    diagnostics: MediaBrokerDiagnostics,
) -> DebugSyntheticReplayCaptureSnapshot {
    DebugSyntheticReplayCaptureSnapshot {
        target_process_id: identity.process_id,
        target_window_handle: identity.native_window,
        target_executable_basename: identity.executable_basename,
        diagnostics,
    }
}

fn validate_fixed_broker(path: &Path) -> Result<PathBuf, MediaBrokerError> {
    if !path.is_absolute()
        || path.file_name().and_then(|value| value.to_str()) != Some(BROKER_FILE_NAME)
    {
        return Err(MediaBrokerError::InvalidBundle);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| MediaBrokerError::Missing)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(MediaBrokerError::InvalidBundle);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| MediaBrokerError::InvalidBundle)?;
    let parent = path
        .parent()
        .ok_or(MediaBrokerError::InvalidBundle)?
        .canonicalize()
        .map_err(|_| MediaBrokerError::InvalidBundle)?;
    if canonical.parent() != Some(parent.as_path()) {
        return Err(MediaBrokerError::InvalidBundle);
    }
    Ok(canonical)
}

#[cfg(windows)]
#[derive(Debug)]
struct BrokerChild {
    handle: windows_sys::Win32::Foundation::HANDLE,
    process_id: u32,
}

#[cfg(windows)]
// SAFETY: the process HANDLE is a kernel reference usable across threads; this
// wrapper owns it until Drop and does not expose borrowed process memory.
unsafe impl Send for BrokerChild {}

#[cfg(windows)]
impl BrokerChild {
    fn id(&self) -> u32 {
        self.process_id
    }

    fn try_wait(&mut self) -> Result<Option<u32>, MediaBrokerError> {
        use windows_sys::Win32::Foundation::STILL_ACTIVE;
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;
        let mut code = 0_u32;
        // SAFETY: handle is a live process handle and code is writable.
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(MediaBrokerError::Process);
        }
        if code == STILL_ACTIVE as u32 {
            Ok(None)
        } else {
            Ok(Some(code))
        }
    }

    async fn wait(&mut self) -> Result<u32, MediaBrokerError> {
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(code);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn kill(&mut self) -> Result<(), MediaBrokerError> {
        use windows_sys::Win32::System::Threading::TerminateProcess;
        // SAFETY: handle is a live process handle owned by this wrapper.
        if unsafe { TerminateProcess(self.handle, 1) } == 0 {
            return Err(MediaBrokerError::Process);
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for BrokerChild {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::TerminateProcess;
        if matches!(self.try_wait(), Ok(None)) {
            // SAFETY: handle is live; kill-on-drop prevents an orphan if async
            // teardown could not complete.
            unsafe { TerminateProcess(self.handle, 1) };
        }
        // SAFETY: this wrapper exclusively owns the process handle.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
fn spawn_broker_process(
    executable: &Path,
    args: &[String],
    parent_job: &RuntimeSupervisor,
) -> Result<BrokerChild, MediaBrokerError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, ResumeThread, TerminateProcess, CREATE_NO_WINDOW, CREATE_SUSPENDED,
        CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };

    let application: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
    let executable_text = executable.to_string_lossy().replace('"', "");
    let mut command_line = format!("\"{executable_text}\" {}", args.join(" "))
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let current_directory: Vec<u16> = executable
        .parent()
        .ok_or(MediaBrokerError::InvalidBundle)?
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    let environment = minimal_environment_block();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..STARTUPINFOW::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: all UTF-16 buffers are NUL-terminated and remain live. No handles
    // are inherited. The process starts suspended so it cannot inspect launch
    // context before joining the parent-owned kill-on-close Job Object.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            current_directory.as_ptr(),
            &startup,
            &mut process,
        )
    };
    if created == 0 || process.hProcess.is_null() || process.hThread.is_null() {
        return Err(MediaBrokerError::Process);
    }
    if let Err(error) = parent_job.assign_raw_process_to_parent_job(process.hProcess as usize) {
        // SAFETY: both handles are exclusively owned on this failure path.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
        return Err(MediaBrokerError::Job(error.to_string()));
    }
    // SAFETY: the primary thread is suspended exactly once and its live handle
    // remains valid until closed immediately below.
    let resumed = unsafe { ResumeThread(process.hThread) };
    // SAFETY: the primary thread handle is no longer required by the parent.
    unsafe { CloseHandle(process.hThread) };
    if resumed == u32::MAX {
        // SAFETY: process handle is live and exclusively owned.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hProcess);
        }
        return Err(MediaBrokerError::Process);
    }
    Ok(BrokerChild {
        handle: process.hProcess,
        process_id: process.dwProcessId,
    })
}

#[cfg(windows)]
fn minimal_environment_block() -> Vec<u16> {
    // Explicit allowlist: enough Windows/user profile context for COM, WinRT,
    // WASAPI, and D3D initialization, while excluding arbitrary variables that
    // may contain provider credentials, tokens, or developer secrets.
    let mut entries = [
        "ALLUSERSPROFILE",
        "APPDATA",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "CommonProgramW6432",
        "COMPUTERNAME",
        "ComSpec",
        "DriverData",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "NUMBER_OF_PROCESSORS",
        "OS",
        "Path",
        "PATHEXT",
        "PROCESSOR_ARCHITECTURE",
        "PROCESSOR_IDENTIFIER",
        "PROCESSOR_LEVEL",
        "PROCESSOR_REVISION",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "PUBLIC",
        "SystemDrive",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "WINDIR",
    ]
    .into_iter()
    .filter_map(|key| {
        std::env::var_os(key).map(|value| format!("{key}={}", value.to_string_lossy()))
    })
    .collect::<Vec<_>>();
    entries.sort_by_key(|value| value.to_ascii_uppercase());
    let mut block = Vec::new();
    for entry in entries {
        block.extend(entry.encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(not(windows))]
#[derive(Debug)]
struct BrokerChild(tokio::process::Child);

#[cfg(not(windows))]
impl BrokerChild {
    fn id(&self) -> u32 {
        self.0.id().unwrap_or(0)
    }
    fn try_wait(&mut self) -> Result<Option<u32>, MediaBrokerError> {
        self.0
            .try_wait()
            .map(|status| status.map(|value| value.code().unwrap_or(1) as u32))
            .map_err(|_| MediaBrokerError::Process)
    }
    async fn wait(&mut self) -> Result<u32, MediaBrokerError> {
        self.0
            .wait()
            .await
            .map(|status| status.code().unwrap_or(1) as u32)
            .map_err(|_| MediaBrokerError::Process)
    }
    async fn kill(&mut self) -> Result<(), MediaBrokerError> {
        self.0.kill().await.map_err(|_| MediaBrokerError::Process)
    }
}

#[cfg(not(windows))]
fn spawn_broker_process(
    executable: &Path,
    args: &[String],
    _parent_job: &RuntimeSupervisor,
) -> Result<BrokerChild, MediaBrokerError> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
        .spawn()
        .map(BrokerChild)
        .map_err(|_| MediaBrokerError::Process)
}

#[cfg(windows)]
async fn connect_broker_pipe(pipe: &str) -> Result<BrokerStream, MediaBrokerError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    let pipe: Vec<u16> = std::ffi::OsStr::new(pipe)
        .encode_wide()
        .chain([0])
        .collect();
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        // SAFETY: pipe is a fixed, NUL-terminated per-launch name. No handle is
        // inherited and OPEN_EXISTING cannot create a filesystem object.
        let handle = unsafe {
            CreateFileW(
                pipe.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            // SAFETY: the successful CreateFileW handle is transferred exactly
            // once to File, which closes it on Drop.
            return Ok(unsafe { std::fs::File::from_raw_handle(handle) });
        }
        // SAFETY: GetLastError has no preconditions and immediately follows the
        // failed CreateFileW call on this thread.
        let last_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if tokio::time::Instant::now() >= deadline {
            return Err(MediaBrokerError::ConnectionCode(last_error));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(not(windows))]
async fn connect_broker_pipe(_pipe: &str) -> Result<BrokerStream, MediaBrokerError> {
    Err(MediaBrokerError::DevelopmentFixture)
}

fn write_frame<W: Write>(writer: &mut W, body: &[u8]) -> Result<(), MediaBrokerError> {
    if body.is_empty() || body.len().saturating_add(4) > MAX_FRAME_BYTES {
        return Err(MediaBrokerError::Payload);
    }
    let length = u32::try_from(body.len()).map_err(|_| MediaBrokerError::Payload)?;
    writer
        .write_all(&length.to_le_bytes())
        .map_err(|_| MediaBrokerError::Connection)?;
    writer
        .write_all(body)
        .map_err(|_| MediaBrokerError::Connection)?;
    writer.flush().map_err(|_| MediaBrokerError::Connection)
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, MediaBrokerError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|_| MediaBrokerError::Connection)?;
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length.saturating_add(4) > MAX_FRAME_BYTES {
        return Err(MediaBrokerError::Payload);
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| MediaBrokerError::Connection)?;
    Ok(body)
}

fn decode_diagnostics(payload: &[u8]) -> Result<MediaBrokerDiagnostics, MediaBrokerError> {
    // v1 originally exposed ten fixed fields (80 bytes). Native residual
    // rejection counters were appended in a compatible broker update. Accept
    // both payload sizes and ignore unknown trailing diagnostics until the
    // desktop domain exposes them, rather than treating a healthy broker as an
    // authentication failure during its health refresh.
    if payload.len() != 80 && payload.len() != 104 {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(MediaBrokerDiagnostics {
        state: broker_state_name(read_u32(payload, 0)?).into(),
        capture_backend: enum_name(
            read_u32(payload, 4)?,
            &["none", "windowsGraphicsCapture", "desktopDuplication"],
        ),
        overlay_backend: enum_name(read_u32(payload, 8)?, &["none", "d3d11DirectComposition"]),
        capture_audio: enum_name(
            read_u32(payload, 12)?,
            &[
                "stopped",
                "initializing",
                "ready",
                "capturing",
                "playing",
                "recovering",
                "failed",
            ],
        ),
        render_audio: enum_name(
            read_u32(payload, 16)?,
            &[
                "stopped",
                "initializing",
                "ready",
                "capturing",
                "playing",
                "recovering",
                "failed",
            ],
        ),
        target_state: enum_name(
            read_u32(payload, 20)?,
            &[
                "none",
                "selected",
                "unavailable",
                "minimized",
                "occluded",
                "protectedContent",
                "closed",
            ],
        ),
        device_generation: read_u64(payload, 24)?,
        audio_device_generation: read_u64(payload, 32)?,
        cancellation_generation: read_u64(payload, 40)?,
        frames_received: read_u64(payload, 48)?,
        frames_presented: read_u64(payload, 56)?,
        frames_dropped: read_u64(payload, 64)?,
        overlays_suppressed: read_u64(payload, 72)?,
    })
}

fn broker_state_name(value: u32) -> &'static str {
    const VALUES: &[&str] = &[
        "stopped",
        "starting",
        "awaitingTarget",
        "capturingPrimary",
        "capturingFallback",
        "recoveringDevice",
        "blockedByPolicy",
        "degradedAudioOnly",
        "stopping",
        "failed",
    ];
    VALUES.get(value as usize).copied().unwrap_or("unknown")
}

fn enum_name(value: u32, values: &[&str]) -> String {
    values
        .get(value as usize)
        .copied()
        .unwrap_or("unknown")
        .into()
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, MediaBrokerError> {
    let value: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(MediaBrokerError::Malformed)?
        .try_into()
        .map_err(|_| MediaBrokerError::Malformed)?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, MediaBrokerError> {
    let value: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(MediaBrokerError::Malformed)?
        .try_into()
        .map_err(|_| MediaBrokerError::Malformed)?;
    Ok(u64::from_le_bytes(value))
}

fn qpc_now() -> (u64, u64) {
    #[cfg(windows)]
    {
        let mut value = 0_i64;
        let mut frequency = 0_i64;
        // SAFETY: both APIs write to live stack-owned outputs.
        unsafe {
            windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value);
            windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency);
        }
        (value.max(0) as u64, frequency.max(1) as u64)
    }
    #[cfg(not(windows))]
    {
        (0, 1)
    }
}

fn unavailable_health(detail: &str) -> MediaBrokerHealthSnapshot {
    MediaBrokerHealthSnapshot {
        state: RuntimeConnectionState::Unavailable,
        connected: false,
        process_id: None,
        restart_count: 0,
        recent_failure_count: 0,
        protocol_version: None,
        fixture_only: false,
        broker_state: None,
        capture_available: false,
        overlay_available: false,
        capture_audio_available: false,
        render_audio_available: false,
        detail: detail.into(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaBrokerError {
    #[error("bundled media broker is missing")]
    Missing,
    #[error("bundled media broker layout is invalid")]
    InvalidBundle,
    #[error("media broker launch nonce generation failed")]
    Random,
    #[error("media broker process could not be started or inspected")]
    Process,
    #[error("media broker could not join the parent Job Object: {0}")]
    Job(String),
    #[error("media broker control endpoint is unavailable")]
    Connection,
    #[error("media broker control endpoint is unavailable (Windows code {0})")]
    ConnectionCode(u32),
    #[error("media broker control request timed out")]
    Timeout,
    #[error("media broker control frame exceeded its bound")]
    Payload,
    #[error("media broker control response was malformed")]
    Malformed,
    #[error("media broker rejected the request with status {0:?}")]
    Remote(BrokerStatus),
    #[error("media broker supervisor is quarantined")]
    Quarantined,
    #[error("media broker is skipped in this unbundled development build")]
    DevelopmentFixture,
    #[error("media broker supervisor state is unavailable")]
    State,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture target is not the dedicated debug executable")]
    DebugSyntheticTargetInvalid,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture target does not match the selected debug process")]
    DebugSyntheticTargetMismatch,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture metadata is unavailable in the fixed app-data handoff")]
    DebugSyntheticMetadataUnavailable,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture metadata is malformed or not ready")]
    DebugSyntheticMetadataInvalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_codec_matches_health_envelope_contract() {
        let envelope = BrokerEnvelope {
            version: 1,
            launch_nonce: vec![7; 32],
            session_id: "media-fixture-session".into(),
            sequence: 1,
            deadline_qpc: 42,
            cancellation_generation: 0,
            command: BrokerCommand::Health as i32,
            payload: Vec::new(),
        };
        let decoded = BrokerEnvelope::decode(envelope.encode_to_vec().as_slice()).expect("decode");
        assert_eq!(decoded, envelope);
        assert_eq!(decoded.launch_nonce.len(), 32);
    }

    #[test]
    fn target_mutations_accept_only_one_monotonic_generation_step() {
        let wire = BrokerResponse {
            version: PROTOCOL_VERSION,
            response_to_sequence: 2,
            status: BrokerStatus::Ok as i32,
            cancellation_generation: 1,
            payload: Vec::new(),
        }
        .encode_to_vec();
        let decoded = BrokerResponse::decode(wire.as_slice()).expect("decode native response");
        assert_eq!(
            reconcile_response_generation(
                BrokerCommand::SelectTarget,
                0,
                decoded.cancellation_generation,
                decoded.status(),
            )
            .expect("post-command generation"),
            1
        );
        assert_eq!(
            reconcile_response_generation(BrokerCommand::SelectTarget, 0, 0, BrokerStatus::Ok)
                .expect("legacy pre-command response"),
            1
        );
        assert_eq!(
            reconcile_response_generation(BrokerCommand::ClearTarget, 7, 8, BrokerStatus::Ok)
                .expect("clear transition"),
            8
        );
        assert!(
            reconcile_response_generation(BrokerCommand::SelectTarget, 7, 9, BrokerStatus::Ok)
                .is_err()
        );
        assert!(
            reconcile_response_generation(BrokerCommand::SelectTarget, 7, 6, BrokerStatus::Ok)
                .is_err()
        );
        assert!(
            reconcile_response_generation(BrokerCommand::Health, 7, 8, BrokerStatus::Ok).is_err()
        );
    }

    #[test]
    fn synthetic_target_payload_matches_native_select_target_contract() {
        let payload = SelectTargetPayload {
            native_window: 0x1234,
            expected_process_id: 77,
            allowed_process_names: vec![DEBUG_SYNTHETIC_TARGET_BASENAME.into()],
        };
        let encoded = payload.encode_to_vec();
        let decoded = SelectTargetPayload::decode(encoded.as_slice()).expect("decode payload");
        assert_eq!(decoded, payload);
        assert_eq!(decoded.allowed_process_names.len(), 1);
    }

    #[test]
    fn synthetic_target_gate_accepts_only_the_task_owned_executable() {
        let accepted =
            validate_debug_synthetic_target(0x1234, 77, "interactive-npcs-synthetic-target.exe")
                .expect("dedicated fixture accepted");
        assert_eq!(accepted.native_window, 0x1234);
        assert_eq!(accepted.process_id, 77);
        assert_eq!(
            accepted.executable_basename,
            DEBUG_SYNTHETIC_TARGET_BASENAME
        );

        assert!(validate_debug_synthetic_target(0, 77, DEBUG_SYNTHETIC_TARGET_BASENAME).is_err());
        assert!(
            validate_debug_synthetic_target(0x1234, 0, DEBUG_SYNTHETIC_TARGET_BASENAME).is_err()
        );
        assert!(validate_debug_synthetic_target(0x1234, 77, "Cyberpunk2077.exe").is_err());
        assert!(validate_debug_synthetic_target(
            0x1234,
            77,
            "C:\\fixtures\\interactive-npcs-synthetic-target.exe"
        )
        .is_err());
    }

    #[test]
    fn synthetic_metadata_requires_ready_consistent_task_owned_identity() {
        let fixture = serde_json::json!({
            "schema_version": 1,
            "fixture_kind": "synthetic-original-video-replay",
            "state": "playing",
            "pid": 77,
            "process_id": 77,
            "window_handle": 4660,
            "hwnd": 4660,
            "executable_basename": DEBUG_SYNTHETIC_TARGET_BASENAME,
            "exe_basename": DEBUG_SYNTHETIC_TARGET_BASENAME,
            "decoded_frames": 3,
            "untrusted_extra_field": "ignored"
        });
        let accepted = decode_debug_synthetic_target_metadata(
            &serde_json::to_vec(&fixture).expect("serialize metadata"),
        )
        .expect("valid task metadata");
        assert_eq!(accepted.native_window, 4660);
        assert_eq!(accepted.process_id, 77);

        for (field, replacement) in [
            ("state", serde_json::json!("ready")),
            ("decoded_frames", serde_json::json!(0)),
            ("pid", serde_json::json!(78)),
            ("hwnd", serde_json::json!(4661)),
            ("window_handle", serde_json::json!(-1)),
            ("executable_basename", serde_json::json!("notepad.exe")),
            (
                "exe_basename",
                serde_json::json!("C:\\fixtures\\interactive-npcs-synthetic-target.exe"),
            ),
        ] {
            let mut invalid = fixture.clone();
            invalid[field] = replacement;
            assert!(decode_debug_synthetic_target_metadata(
                &serde_json::to_vec(&invalid).expect("serialize invalid metadata")
            )
            .is_err());
        }

        let directory = tempfile::tempdir().expect("metadata directory");
        let fixed_path = directory.path().join(DEBUG_SYNTHETIC_METADATA_FILE_NAME);
        std::fs::write(
            &fixed_path,
            serde_json::to_vec(&fixture).expect("serialize fixed metadata"),
        )
        .expect("write fixed metadata");
        assert!(read_debug_synthetic_target(&fixed_path).is_ok());
        assert!(
            read_debug_synthetic_target(&directory.path().join("capture-target.json")).is_err()
        );
    }

    #[test]
    fn diagnostics_decoder_rejects_unbounded_or_truncated_payloads() {
        assert!(decode_diagnostics(&[]).is_err());
        assert!(decode_diagnostics(&[0; 79]).is_err());
        assert!(decode_diagnostics(&[0; 81]).is_err());
        assert!(decode_diagnostics(&[0; 103]).is_err());
        assert!(decode_diagnostics(&[0; 80]).is_ok());
        assert!(decode_diagnostics(&[0; 104]).is_ok());
    }

    #[test]
    fn missing_source_dev_broker_is_explicit_fixture_skip() {
        let runtime = RuntimeSupervisor::try_new(crate::sidecar_supervisor::RuntimeLaunchConfig {
            executable: PathBuf::from("C:/missing/npc-runtime.exe"),
            resource_root: PathBuf::from("C:/missing/resources"),
            app_data: PathBuf::from("C:/missing/data"),
            development_fixture_allowed: true,
        })
        .expect("parent job");
        let supervisor = MediaBrokerSupervisor::new(
            MediaBrokerLaunchConfig {
                executable: PathBuf::from("C:/missing/npc-media-broker.exe"),
                development_fixture_allowed: true,
                debug_synthetic_metadata_path: PathBuf::from(
                    "C:/missing/debug-synthetic-replay-target.json",
                ),
            },
            runtime,
        );
        let health = supervisor.health();
        assert_eq!(health.state, RuntimeConnectionState::DevelopmentFixture);
        assert!(health.fixture_only);
        assert!(!health.connected);
    }
}
