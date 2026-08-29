use crate::domain::{RuntimeBackend, RuntimeConnectionState, RuntimeHealthSnapshot};
use crate::sidecar_protocol::{
    BoxedControlStream, ClientError, ControlClient, NativeDoctorReport, NativeProfile,
    NativeSimulationRequest, NativeSimulationResult,
};
use npc_protocol::LaunchNonce;
use serde::Deserialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

const SIDECAR_FILE_NAME: &str = if cfg!(windows) {
    "npc-runtime.exe"
} else {
    "npc-runtime"
};
const MAX_DESCRIPTOR_BYTES: usize = 16 * 1024;
const DESCRIPTOR_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES_IN_WINDOW: usize = 3;

#[derive(Clone, Debug)]
pub struct RuntimeLaunchConfig {
    pub executable: PathBuf,
    pub resource_root: PathBuf,
    pub app_data: PathBuf,
    pub development_fixture_allowed: bool,
}

impl RuntimeLaunchConfig {
    pub fn from_application_paths(
        resource_root: PathBuf,
        app_data: PathBuf,
        development_fixture_allowed: bool,
    ) -> Result<Self, SupervisorError> {
        let executable = std::env::current_exe()
            .map_err(|_| SupervisorError::InvalidBundle)?
            .parent()
            .ok_or(SupervisorError::InvalidBundle)?
            .join(SIDECAR_FILE_NAME);
        Ok(Self {
            executable,
            resource_root,
            app_data,
            development_fixture_allowed,
        })
    }
}

#[derive(Debug)]
struct ManagedRuntime {
    child: Child,
    client: ControlClient,
    pid: u32,
}

#[derive(Debug)]
struct SupervisorState {
    connection: RuntimeConnectionState,
    detail: String,
    pid: Option<u32>,
    restart_count: u32,
    recent_failures: VecDeque<Instant>,
    protocol_version: Option<String>,
}

impl SupervisorState {
    fn initial(config: &RuntimeLaunchConfig) -> Self {
        let sidecar_exists = config.executable.is_file();
        let (connection, detail) = if sidecar_exists {
            (
                RuntimeConnectionState::Cold,
                "Bundled runtime sidecar is ready to start.".to_owned(),
            )
        } else if config.development_fixture_allowed {
            (
                RuntimeConnectionState::DevelopmentFixture,
                "Bundled runtime is absent in an unbundled development build; the labeled UI fixture is available.".to_owned(),
            )
        } else {
            (
                RuntimeConnectionState::Unavailable,
                "The required bundled runtime sidecar is missing.".to_owned(),
            )
        };
        Self {
            connection,
            detail,
            pid: None,
            restart_count: 0,
            recent_failures: VecDeque::new(),
            protocol_version: None,
        }
    }
}

#[derive(Clone)]
pub struct RuntimeSupervisor {
    config: Arc<RuntimeLaunchConfig>,
    state: Arc<Mutex<SupervisorState>>,
    managed: Arc<tokio::sync::Mutex<Option<ManagedRuntime>>>,
    startup_gate: Arc<tokio::sync::Mutex<()>>,
    #[cfg(windows)]
    job: Arc<WindowsJob>,
}

impl std::fmt::Debug for RuntimeSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeSupervisor")
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl RuntimeSupervisor {
    pub fn try_new(config: RuntimeLaunchConfig) -> Result<Self, SupervisorError> {
        let state = SupervisorState::initial(&config);
        Ok(Self {
            config: Arc::new(config),
            state: Arc::new(Mutex::new(state)),
            managed: Arc::new(tokio::sync::Mutex::new(None)),
            startup_gate: Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(windows)]
            job: Arc::new(WindowsJob::new()?),
        })
    }

    pub fn health(&self) -> RuntimeHealthSnapshot {
        let Ok(state) = self.state.lock() else {
            return RuntimeHealthSnapshot {
                state: RuntimeConnectionState::Unavailable,
                connected: false,
                backend: RuntimeBackend::NativeRuntime,
                process_id: None,
                restart_count: 0,
                recent_failure_count: 0,
                protocol_version: None,
                fixture_only: false,
                detail: "Runtime supervisor state is unavailable.".into(),
            };
        };
        RuntimeHealthSnapshot {
            state: state.connection,
            connected: state.connection == RuntimeConnectionState::Ready,
            backend: if state.connection == RuntimeConnectionState::DevelopmentFixture {
                RuntimeBackend::DeterministicFixture
            } else {
                RuntimeBackend::NativeRuntime
            },
            process_id: state.pid,
            restart_count: state.restart_count,
            recent_failure_count: state.recent_failures.len() as u32,
            protocol_version: state.protocol_version.clone(),
            fixture_only: state.connection == RuntimeConnectionState::DevelopmentFixture,
            detail: state.detail.clone(),
        }
    }

    pub fn start_background(&self) {
        if self.health().state == RuntimeConnectionState::DevelopmentFixture {
            return;
        }
        let supervisor = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = supervisor.ensure_ready().await;
        });
    }

    #[cfg(windows)]
    pub(crate) fn assign_raw_process_to_parent_job(
        &self,
        process_handle: usize,
    ) -> Result<(), SupervisorError> {
        self.job
            .assign_raw(process_handle as *mut core::ffi::c_void)
    }

    pub async fn ensure_ready(&self) -> Result<ControlClient, SupervisorError> {
        if self.health().state == RuntimeConnectionState::DevelopmentFixture {
            return Err(SupervisorError::DevelopmentFixture);
        }
        if self.health().state == RuntimeConnectionState::Quarantined {
            return Err(SupervisorError::Quarantined);
        }
        let _startup = self.startup_gate.lock().await;

        if let Some(client) = self.live_client().await? {
            return Ok(client);
        }
        self.apply_backoff().await?;
        self.set_starting();
        match self.launch().await {
            Ok(runtime) => {
                let client = runtime.client.clone();
                let pid = runtime.pid;
                *self.managed.lock().await = Some(runtime);
                self.set_ready(pid);
                Ok(client)
            }
            Err(error) => {
                self.record_failure(error.to_string()).await;
                Err(error)
            }
        }
    }

    pub async fn ping(&self) -> Result<RuntimeHealthSnapshot, SupervisorError> {
        let client = self.ensure_ready().await?;
        if let Err(error) = client.ping().await {
            self.record_failure(error.to_string()).await;
            return Err(error.into());
        }
        Ok(self.health())
    }

    pub async fn doctor(&self) -> Result<NativeDoctorReport, SupervisorError> {
        let client = self.ensure_ready().await?;
        match client.doctor().await {
            Ok(report) => Ok(report),
            Err(error) => {
                self.observe_client_error(&error).await;
                Err(error.into())
            }
        }
    }

    pub async fn profiles(&self) -> Result<Vec<NativeProfile>, SupervisorError> {
        let client = self.ensure_ready().await?;
        match client.profiles().await {
            Ok(profiles) => Ok(profiles),
            Err(error) => {
                self.observe_client_error(&error).await;
                Err(error.into())
            }
        }
    }

    pub async fn simulate(
        &self,
        request: NativeSimulationRequest,
    ) -> Result<NativeSimulationResult, SupervisorError> {
        let client = self.ensure_ready().await?;
        match client.simulate(request).await {
            Ok(result) => Ok(result),
            Err(error) => {
                self.observe_client_error(&error).await;
                Err(error.into())
            }
        }
    }

    pub async fn cancel(&self) -> Result<u64, SupervisorError> {
        let client = self.ensure_ready().await?;
        match client.cancel().await {
            Ok(generation) => Ok(generation),
            Err(error) => {
                self.observe_client_error(&error).await;
                Err(error.into())
            }
        }
    }

    pub async fn shutdown(&self) {
        self.set_connection(
            RuntimeConnectionState::ShuttingDown,
            "Runtime shutdown is in progress.",
        );
        let mut runtime = self.managed.lock().await.take();
        if let Some(mut runtime) = runtime.take() {
            let _ = runtime.client.shutdown().await;
            match tokio::time::timeout(SHUTDOWN_TIMEOUT, runtime.child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    let _ = runtime.child.kill().await;
                    let _ = runtime.child.wait().await;
                }
            }
        }
        self.set_connection(
            RuntimeConnectionState::Stopped,
            "Runtime process is stopped.",
        );
    }

    async fn live_client(&self) -> Result<Option<ControlClient>, SupervisorError> {
        let (client, exited) = {
            let mut managed = self.managed.lock().await;
            let Some(runtime) = managed.as_mut() else {
                return Ok(None);
            };
            let exited = runtime
                .child
                .try_wait()
                .map_err(|_| SupervisorError::Process)?;
            (runtime.client.clone(), exited.is_some())
        };
        if exited {
            self.record_failure("Runtime process exited unexpectedly.".into())
                .await;
            return Ok(None);
        }
        if client.ping().await.is_ok() {
            Ok(Some(client))
        } else {
            self.record_failure("Runtime health handshake failed.".into())
                .await;
            Ok(None)
        }
    }

    async fn launch(&self) -> Result<ManagedRuntime, SupervisorError> {
        let executable = validate_fixed_sidecar(&self.config.executable)?;
        let resource_root = validate_directory(&self.config.resource_root)?;
        std::fs::create_dir_all(&self.config.app_data).map_err(|_| SupervisorError::AppData)?;
        let app_data = validate_directory(&self.config.app_data)?;
        let nonce = LaunchNonce::new();

        let mut command = Command::new(&executable);
        command
            .arg("--repo-root")
            .arg(&resource_root)
            .arg("--app-data")
            .arg(&app_data)
            .arg("serve")
            .arg("--nonce")
            .arg(nonce.to_string())
            .current_dir(executable.parent().ok_or(SupervisorError::InvalidBundle)?)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .env_clear();
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(windows)]
        {
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|_| SupervisorError::Process)?;
        #[cfg(windows)]
        if let Err(error) = self.job.assign(&child) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
        let pid = child.id().ok_or(SupervisorError::Process)?;
        let mut stdout = child.stdout.take().ok_or(SupervisorError::Descriptor)?;
        let descriptor =
            match tokio::time::timeout(DESCRIPTOR_TIMEOUT, read_descriptor(&mut stdout)).await {
                Ok(result) => result?,
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(SupervisorError::Descriptor);
                }
            };
        validate_descriptor(&descriptor, &nonce)?;
        let stream = connect_control_stream(&descriptor).await?;
        let client = ControlClient::connect(stream, nonce, env!("CARGO_PKG_VERSION")).await?;
        client.ping().await?;
        Ok(ManagedRuntime { child, client, pid })
    }

    async fn record_failure(&self, detail: String) {
        if let Some(mut runtime) = self.managed.lock().await.take() {
            let _ = runtime.child.kill().await;
            let _ = runtime.child.wait().await;
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
        state.pid = None;
        state.protocol_version = None;
        if state.recent_failures.len() >= MAX_FAILURES_IN_WINDOW {
            state.connection = RuntimeConnectionState::Quarantined;
            state.detail = "Runtime was quarantined after three failures in sixty seconds; restart the application after reviewing diagnostics.".into();
        } else {
            state.connection = RuntimeConnectionState::RestartBackoff;
            state.detail = detail;
            state.restart_count = state.restart_count.saturating_add(1);
        }
    }

    async fn observe_client_error(&self, error: &ClientError) {
        if error.should_restart_runtime() {
            self.record_failure(error.to_string()).await;
        }
    }

    async fn apply_backoff(&self) -> Result<(), SupervisorError> {
        let failures = self
            .state
            .lock()
            .map_err(|_| SupervisorError::State)?
            .recent_failures
            .len();
        if failures >= MAX_FAILURES_IN_WINDOW {
            return Err(SupervisorError::Quarantined);
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

    fn set_starting(&self) {
        self.set_connection(
            RuntimeConnectionState::Starting,
            "Starting the authenticated bundled runtime.",
        );
    }

    fn set_ready(&self, pid: u32) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.connection = RuntimeConnectionState::Ready;
        state.detail = "Authenticated runtime control channel is ready.".into();
        state.pid = Some(pid);
        state.protocol_version = Some("1.0.0".into());
    }

    fn set_connection(&self, connection: RuntimeConnectionState, detail: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.connection = connection;
        state.detail = detail.into();
        if connection != RuntimeConnectionState::Ready {
            state.pid = None;
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServeDescriptor {
    schema_version: String,
    transport: String,
    endpoint: Option<String>,
    launch_nonce: String,
    protocol_version: String,
    max_message_bytes: usize,
}

async fn read_descriptor(
    stdout: &mut tokio::process::ChildStdout,
) -> Result<ServeDescriptor, SupervisorError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        if bytes.len() >= MAX_DESCRIPTOR_BYTES {
            return Err(SupervisorError::Descriptor);
        }
        let remaining = chunk.len().min(MAX_DESCRIPTOR_BYTES - bytes.len());
        let read = stdout
            .read(&mut chunk[..remaining])
            .await
            .map_err(|_| SupervisorError::Descriptor)?;
        if read == 0 {
            return Err(SupervisorError::Descriptor);
        }
        bytes.extend_from_slice(&chunk[..read]);
        match serde_json::from_slice::<ServeDescriptor>(&bytes) {
            Ok(descriptor) => return Ok(descriptor),
            Err(error) if error.is_eof() => {}
            Err(_) => return Err(SupervisorError::Descriptor),
        }
    }
}

fn validate_descriptor(
    descriptor: &ServeDescriptor,
    expected_nonce: &LaunchNonce,
) -> Result<(), SupervisorError> {
    let nonce =
        LaunchNonce::from_str(&descriptor.launch_nonce).map_err(|_| SupervisorError::Descriptor)?;
    let expected_transport = if cfg!(windows) {
        "windows_named_pipe"
    } else {
        "portable_unix_socket"
    };
    if descriptor.schema_version != "1.0.0"
        || descriptor.protocol_version != "1.0.0"
        || descriptor.transport != expected_transport
        || &nonce != expected_nonce
        || descriptor.max_message_bytes != crate::sidecar_protocol::MAX_CONTROL_MESSAGE_BYTES
    {
        return Err(SupervisorError::Descriptor);
    }
    #[cfg(windows)]
    if descriptor.endpoint.is_some() {
        return Err(SupervisorError::Descriptor);
    }
    #[cfg(not(windows))]
    if descriptor.endpoint.is_none() {
        return Err(SupervisorError::Descriptor);
    }
    Ok(())
}

fn validate_fixed_sidecar(path: &Path) -> Result<PathBuf, SupervisorError> {
    if !path.is_absolute()
        || path.file_name().and_then(|value| value.to_str()) != Some(SIDECAR_FILE_NAME)
    {
        return Err(SupervisorError::InvalidBundle);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SupervisorError::MissingSidecar)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(SupervisorError::InvalidBundle);
    }
    let parent = path.parent().ok_or(SupervisorError::InvalidBundle)?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| SupervisorError::InvalidBundle)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| SupervisorError::InvalidBundle)?;
    if canonical.parent() != Some(canonical_parent.as_path()) {
        return Err(SupervisorError::InvalidBundle);
    }
    Ok(canonical)
}

fn validate_directory(path: &Path) -> Result<PathBuf, SupervisorError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SupervisorError::InvalidBundle)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SupervisorError::InvalidBundle);
    }
    path.canonicalize()
        .map_err(|_| SupervisorError::InvalidBundle)
}

#[cfg(windows)]
async fn connect_control_stream(
    _descriptor: &ServeDescriptor,
) -> Result<BoxedControlStream, SupervisorError> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let pipe_name = current_session_pipe_name()?;
    let deadline = tokio::time::Instant::now() + CONNECTION_TIMEOUT;
    loop {
        match ClientOptions::new().open(&pipe_name) {
            Ok(client) => return Ok(Box::new(client)),
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(_) => return Err(SupervisorError::Connection),
        }
    }
}

#[cfg(windows)]
fn current_session_pipe_name() -> Result<String, SupervisorError> {
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    // SAFETY: GetCurrentProcessId has no preconditions and `session` is a valid
    // writable u32 for ProcessIdToSessionId.
    let process = unsafe { GetCurrentProcessId() };
    let mut session = 0_u32;
    // SAFETY: `process` is the current process identifier and `session` points
    // to a live, writable `u32` for the duration of the Win32 call.
    if unsafe { ProcessIdToSessionId(process, &mut session) } == 0 {
        return Err(SupervisorError::Connection);
    }
    Ok(format!(r"\\.\pipe\interactive-npcs-v2-session-{session}"))
}

#[cfg(not(windows))]
async fn connect_control_stream(
    descriptor: &ServeDescriptor,
) -> Result<BoxedControlStream, SupervisorError> {
    let endpoint = descriptor
        .endpoint
        .as_deref()
        .ok_or(SupervisorError::Descriptor)?;
    let path = Path::new(endpoint);
    if !path.is_absolute() {
        return Err(SupervisorError::Descriptor);
    }
    let stream = tokio::time::timeout(CONNECTION_TIMEOUT, tokio::net::UnixStream::connect(path))
        .await
        .map_err(|_| SupervisorError::Connection)?
        .map_err(|_| SupervisorError::Connection)?;
    Ok(Box::new(stream))
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsJob {
    fn new() -> Result<Self, SupervisorError> {
        use std::mem::size_of;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: null security attributes and name create a private unnamed Job
        // Object owned by this shell process.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(SupervisorError::JobObject);
        }
        // SAFETY: this Windows POD type supports zero initialization before its
        // documented fields are populated.
        let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the Job handle and exact-size information buffer remain live
        // for this synchronous kernel call.
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&information as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            // SAFETY: this function exclusively owns the live handle on failure.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err(SupervisorError::JobObject);
        }
        Ok(Self(handle))
    }

    fn assign(&self, child: &Child) -> Result<(), SupervisorError> {
        let process = child.raw_handle().ok_or(SupervisorError::JobObject)?;
        self.assign_raw(process.cast())
    }

    fn assign_raw(
        &self,
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<(), SupervisorError> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        // SAFETY: both the Job Object and Tokio child process handles are live
        // and remain owned by their Rust wrappers for the duration of the call.
        let assigned = unsafe { AssignProcessToJobObject(self.0, process) };
        if assigned == 0 {
            Err(SupervisorError::JobObject)
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
// SAFETY: a Job Object HANDLE is a kernel reference usable across threads; the
// Arc retains ownership and the handle is closed exactly once in Drop.
unsafe impl Send for WindowsJob {}
#[cfg(windows)]
// SAFETY: Job Object assignment is kernel-serialized and does not expose Rust
// memory through the handle.
unsafe impl Sync for WindowsJob {}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        // SAFETY: this instance exclusively owns the valid Job Object handle.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("bundled runtime sidecar is missing")]
    MissingSidecar,
    #[error("bundled runtime layout is invalid")]
    InvalidBundle,
    #[error("runtime application data directory is unavailable")]
    AppData,
    #[error("runtime process could not be started or inspected")]
    Process,
    #[error("runtime serve descriptor was invalid")]
    Descriptor,
    #[error("runtime control endpoint could not be connected")]
    Connection,
    #[error("runtime supervisor is quarantined")]
    Quarantined,
    #[error("unbundled development build uses the labeled deterministic fixture")]
    DevelopmentFixture,
    #[error("runtime supervisor state is unavailable")]
    State,
    #[error("runtime parent-death Job Object could not be configured")]
    JobObject,
    #[error(transparent)]
    Client(#[from] ClientError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_without_sidecar_never_enables_fixture_fallback() {
        let config = RuntimeLaunchConfig {
            executable: PathBuf::from("C:/missing/npc-runtime.exe"),
            resource_root: PathBuf::from("C:/missing/resources"),
            app_data: PathBuf::from("C:/missing/data"),
            development_fixture_allowed: false,
        };
        let health = RuntimeSupervisor::try_new(config)
            .expect("job object")
            .health();
        assert_eq!(health.state, RuntimeConnectionState::Unavailable);
        assert!(!health.fixture_only);
    }

    #[test]
    fn development_fallback_is_labeled() {
        let config = RuntimeLaunchConfig {
            executable: PathBuf::from("C:/missing/npc-runtime.exe"),
            resource_root: PathBuf::from("C:/missing/resources"),
            app_data: PathBuf::from("C:/missing/data"),
            development_fixture_allowed: true,
        };
        let health = RuntimeSupervisor::try_new(config)
            .expect("job object")
            .health();
        assert_eq!(health.state, RuntimeConnectionState::DevelopmentFixture);
        assert!(health.fixture_only);
    }

    #[cfg(windows)]
    #[test]
    fn job_object_child_fixture() {
        if std::env::var_os("NPC_JOB_OBJECT_CHILD_FIXTURE").is_some() {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn closing_job_object_terminates_assigned_child() {
        let job = WindowsJob::new().expect("create parent-death job");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("sidecar_supervisor::tests::job_object_child_fixture")
            .arg("--nocapture")
            .env("NPC_JOB_OBJECT_CHILD_FIXTURE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fixture child");
        job.assign(&child).expect("assign fixture child");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(child.try_wait().expect("inspect child").is_none());

        drop(job);
        let _status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("job close must terminate child")
            .expect("wait for child");
    }
}
