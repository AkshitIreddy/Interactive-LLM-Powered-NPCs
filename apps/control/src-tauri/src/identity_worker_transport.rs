//! Hidden, authenticated stdio transport for the optional CPU identity worker.
//!
//! The transport never exposes the pixel mapping capability or an embedding to
//! the WebView. On Windows the child is launched without a console, assigned to
//! the app's kill-on-close Job Object, and authenticated before it is admitted
//! as the process named in a broker identity-frame request.

#![allow(dead_code)]

use crate::identity_runtime::{
    IdentityActivationMode, IdentityRuntimeError, IdentityWorkerTransport, PrivateEvaluationLoad,
    UntrustedWorkerObservationsV1,
};
use crate::media_broker::IdentityWorkerIdentity;
use crate::sidecar_supervisor::RuntimeSupervisor;
use async_trait::async_trait;
use npc_identity_engine::{
    PortableReferenceImportV1, QualifiedReferenceProvenanceV1, ReferenceSourceClassV1,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const PROTOCOL_VERSION: &str = "1.0";
const PACK_ID: &str = "opencv-yunet-sface-private-eval";
const PACK_REVISION: &str = "zoo-47534e27-opencv-5.0.0.93";
const MANIFEST_FILE_NAME: &str = "opencv-yunet-sface-private-evaluation.json";
const MANIFEST_SHA256: &str = "a4af4874af77c4dc517fe41990371e96e68a4469ee6817102876b7b074302e67";
const MAX_FRAME_BYTES: usize = 1_048_576;
const MAX_IMAGE_EDGE: u32 = 8_192;
const MAX_SHARED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const INFERENCE_TIMEOUT: Duration = Duration::from_millis(450);
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REPLAYED_GENERATIONS: u64 = 4_096;

#[derive(Clone, Debug)]
pub(crate) struct IdentityWorkerLaunchConfig {
    /// Exact, Model-Manager-owned Python executable. PATH lookup is forbidden.
    pub python_executable: PathBuf,
    pub python_size_bytes: u64,
    pub python_sha256: String,
    /// Exact checked-in worker entrypoint (or its installed read-only copy).
    pub worker_script: PathBuf,
    pub worker_script_size_bytes: u64,
    pub worker_script_sha256: String,
    /// Exact v2 manifest reviewed by Model Manager.
    pub manifest_path: PathBuf,
    pub manifest_size_bytes: u64,
    pub manifest_sha256: String,
    /// Parent supervisor owning the app-wide kill-on-close Job Object.
    pub parent: RuntimeSupervisor,
}

/// Broker-issued, local-session, read-only pixels for one reference extraction.
///
/// No path is present. Debug intentionally omits the mapping capability and
/// lease nonce. The native bridge owns release/expiry even when extraction
/// fails, times out, or is cancelled.
pub(crate) struct ReferencePixelLeaseV1 {
    lease_id: zeroize::Zeroizing<String>,
    shared_memory_name: zeroize::Zeroizing<String>,
    lease_nonce: zeroize::Zeroizing<String>,
    byte_length: u64,
    width: u32,
    height: u32,
    stride_bytes: u32,
    content_sha256: String,
}

impl std::fmt::Debug for ReferencePixelLeaseV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReferencePixelLeaseV1")
            .field("byte_length", &self.byte_length)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride_bytes", &self.stride_bytes)
            .finish_non_exhaustive()
    }
}

impl ReferencePixelLeaseV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        lease_id: String,
        shared_memory_name: String,
        lease_nonce: String,
        byte_length: u64,
        width: u32,
        height: u32,
        stride_bytes: u32,
        content_sha256: String,
    ) -> Result<Self, IdentityRuntimeError> {
        let lease = Self {
            lease_id: zeroize::Zeroizing::new(lease_id),
            shared_memory_name: zeroize::Zeroizing::new(shared_memory_name),
            lease_nonce: zeroize::Zeroizing::new(lease_nonce),
            byte_length,
            width,
            height,
            stride_bytes,
            content_sha256,
        };
        validate_reference_pixel_lease(&lease)?;
        Ok(lease)
    }

    pub(crate) fn lease_id(&self) -> &str {
        self.lease_id.as_str()
    }

    pub(crate) fn lease_nonce(&self) -> &str {
        self.lease_nonce.as_str()
    }
}

#[derive(Debug)]
pub(crate) struct NativeReferenceExtractionV1 {
    pub provenance: QualifiedReferenceProvenanceV1,
    pub subject_display_name: String,
    pub pixel_lease: ReferencePixelLeaseV1,
}

/// Additive reference path kept separate from live WGC observation transport.
/// The control bridge can require this trait only for picker/import methods,
/// leaving the frozen live-frame bridge generic unchanged.
#[async_trait]
pub(crate) trait IdentityReferenceImportTransport: IdentityWorkerTransport {
    async fn extract_reference(
        &self,
        request: &NativeReferenceExtractionV1,
        cancellation_generation: u64,
    ) -> Result<PortableReferenceImportV1, IdentityRuntimeError>;
}

#[derive(Debug)]
struct ManagedWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    instance_id: String,
    next_sequence: u64,
    generation: u64,
}

#[derive(Debug, Default)]
struct TransportState {
    managed: Option<ManagedWorker>,
    last_load: Option<PrivateEvaluationLoad>,
    generation: u64,
}

pub(crate) struct FramedIdentityWorkerTransport {
    config: IdentityWorkerLaunchConfig,
    state: Mutex<TransportState>,
    identity: RwLock<Option<IdentityWorkerIdentity>>,
    authenticated: AtomicBool,
    parent_death_bound: AtomicBool,
}

impl std::fmt::Debug for FramedIdentityWorkerTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FramedIdentityWorkerTransport")
            .field(
                "identity",
                &self.identity.read().ok().and_then(|value| value.clone()),
            )
            .field("authenticated", &self.authenticated.load(Ordering::Acquire))
            .field(
                "parent_death_bound",
                &self.parent_death_bound.load(Ordering::Acquire),
            )
            .finish_non_exhaustive()
    }
}

impl FramedIdentityWorkerTransport {
    pub(crate) async fn launch(
        config: IdentityWorkerLaunchConfig,
    ) -> Result<Arc<Self>, IdentityRuntimeError> {
        validate_launch_config(&config)?;
        let transport = Arc::new(Self {
            config,
            state: Mutex::new(TransportState::default()),
            identity: RwLock::new(None),
            authenticated: AtomicBool::new(false),
            parent_death_bound: AtomicBool::new(false),
        });
        {
            let mut state = transport.state.lock().await;
            let managed = transport.spawn_authenticated(0).await?;
            state.managed = Some(managed);
        }
        Ok(transport)
    }

    async fn spawn_authenticated(
        &self,
        generation: u64,
    ) -> Result<ManagedWorker, IdentityRuntimeError> {
        self.authenticated.store(false, Ordering::Release);
        self.parent_death_bound.store(false, Ordering::Release);
        self.clear_identity();

        let launch_nonce = random_hex(32)?;
        let instance_id = format!("identity-{}", random_hex(16)?);
        // Re-hash every executable input immediately before each initial or
        // replacement child launch. Model Manager admission is immutable, but
        // a file can still change after the earlier authority check.
        let python = canonical_bound_file(
            &self.config.python_executable,
            self.config.python_size_bytes,
            &self.config.python_sha256,
        )?;
        let script = canonical_bound_file(
            &self.config.worker_script,
            self.config.worker_script_size_bytes,
            &self.config.worker_script_sha256,
        )?;
        let manifest = canonical_bound_file(
            &self.config.manifest_path,
            self.config.manifest_size_bytes,
            &self.config.manifest_sha256,
        )?;
        let executable_name = python
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && value.len() <= 260)
            .ok_or_else(invalid_launch)?
            .to_owned();

        let mut command = Command::new(&python);
        command
            .arg("-I")
            .arg("-B")
            .arg(&script)
            .arg("--launch-nonce")
            .arg(&launch_nonce)
            .arg("--worker-instance-id")
            .arg(&instance_id)
            .arg("--manifest")
            .arg(&manifest)
            .current_dir(script.parent().ok_or_else(invalid_launch)?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .env_clear()
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONNOUSERSITE", "1")
            .env("PYTHONUTF8", "1");
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "LOCALAPPDATA"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(windows)]
        {
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let mut child = command
            .spawn()
            .map_err(|_| transport_failure("worker launch"))?;
        #[cfg(windows)]
        if let Err(error) = self.assign_parent_job(&child) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
        #[cfg(not(windows))]
        {
            let _ = &child;
            return Err(transport_failure(
                "Windows identity transport is unavailable",
            ));
        }

        #[cfg(windows)]
        let creation_time = process_creation_time(&child)?;
        #[cfg(windows)]
        let process_id = child
            .id()
            .filter(|value| *value != 0)
            .ok_or_else(|| transport_failure("worker process identity"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| transport_failure("worker control input"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| transport_failure("worker control output"))?;
        let mut managed = ManagedWorker {
            child,
            stdin,
            stdout,
            instance_id,
            next_sequence: 1,
            generation: 0,
        };
        self.parent_death_bound.store(true, Ordering::Release);

        let handshake = json!({
            "launch_nonce": launch_nonce,
            "supervisor": "npc-runtime",
        });
        let result = request_response(
            &mut managed,
            "handshake",
            handshake,
            0,
            HANDSHAKE_TIMEOUT,
            Some("completed"),
        )
        .await;
        let descriptor = match result {
            Ok(Some(payload)) => payload,
            Ok(None) => {
                terminate_worker(&mut managed).await;
                self.parent_death_bound.store(false, Ordering::Release);
                return Err(transport_failure("worker handshake descriptor is missing"));
            }
            Err(error) => {
                terminate_worker(&mut managed).await;
                self.parent_death_bound.store(false, Ordering::Release);
                return Err(error);
            }
        };
        if validate_handshake_descriptor(descriptor, &managed.instance_id).is_err() {
            terminate_worker(&mut managed).await;
            self.parent_death_bound.store(false, Ordering::Release);
            return Err(transport_failure("worker handshake descriptor mismatch"));
        }

        #[cfg(windows)]
        self.set_identity(IdentityWorkerIdentity {
            process_id,
            process_creation_time: creation_time,
            executable_name,
        })?;
        self.authenticated.store(true, Ordering::Release);

        for next in 1..=generation {
            if let Err(error) = request_response(
                &mut managed,
                "cancel",
                json!({"cancellation_generation": next}),
                next,
                CONTROL_TIMEOUT,
                None,
            )
            .await
            {
                self.authenticated.store(false, Ordering::Release);
                self.parent_death_bound.store(false, Ordering::Release);
                self.clear_identity();
                terminate_worker(&mut managed).await;
                return Err(error);
            }
            managed.generation = next;
        }
        Ok(managed)
    }

    #[cfg(windows)]
    fn assign_parent_job(&self, child: &Child) -> Result<(), IdentityRuntimeError> {
        let handle = child
            .raw_handle()
            .ok_or_else(|| transport_failure("worker process handle"))?;
        self.config
            .parent
            .assign_raw_process_to_parent_job(handle as usize)
            .map_err(|_| transport_failure("parent-death job assignment"))
    }

    fn set_identity(&self, identity: IdentityWorkerIdentity) -> Result<(), IdentityRuntimeError> {
        let mut destination = self
            .identity
            .write()
            .map_err(|_| transport_failure("worker identity state"))?;
        *destination = Some(identity);
        Ok(())
    }

    fn clear_identity(&self) {
        if let Ok(mut identity) = self.identity.write() {
            *identity = None;
        }
    }

    async fn restart_locked(
        &self,
        state: &mut TransportState,
        generation: u64,
    ) -> Result<(), IdentityRuntimeError> {
        if generation != state.generation.saturating_add(1) || generation > MAX_REPLAYED_GENERATIONS
        {
            return Err(transport_failure("invalid cancellation generation"));
        }
        self.authenticated.store(false, Ordering::Release);
        self.parent_death_bound.store(false, Ordering::Release);
        self.clear_identity();
        if let Some(mut managed) = state.managed.take() {
            terminate_worker(&mut managed).await;
        }
        let mut replacement = self.spawn_authenticated(generation).await?;
        if let Some(load) = state.last_load.as_ref() {
            if let Err(error) = send_load(&mut replacement, load).await {
                self.authenticated.store(false, Ordering::Release);
                self.parent_death_bound.store(false, Ordering::Release);
                self.clear_identity();
                terminate_worker(&mut replacement).await;
                return Err(error);
            }
        }
        state.generation = generation;
        state.managed = Some(replacement);
        Ok(())
    }
}

#[async_trait]
impl IdentityWorkerTransport for FramedIdentityWorkerTransport {
    fn identity(&self) -> IdentityWorkerIdentity {
        self.identity
            .read()
            .ok()
            .and_then(|identity| identity.clone())
            .unwrap_or(IdentityWorkerIdentity {
                process_id: 0,
                process_creation_time: 0,
                executable_name: String::new(),
            })
    }

    fn authenticated(&self) -> bool {
        self.authenticated.load(Ordering::Acquire)
    }

    fn parent_death_bound(&self) -> bool {
        self.parent_death_bound.load(Ordering::Acquire)
    }

    async fn load_private_evaluation(
        &self,
        request: &PrivateEvaluationLoad,
    ) -> Result<(), IdentityRuntimeError> {
        let canonical_request_manifest = canonical_file(&request.manifest_path)?;
        let canonical_config_manifest = canonical_file(&self.config.manifest_path)?;
        if canonical_request_manifest != canonical_config_manifest
            || request.manifest_sha256 != MANIFEST_SHA256
        {
            return Err(IdentityRuntimeError::PrivateEvaluationGate);
        }
        let mut state = self.state.lock().await;
        let managed = state
            .managed
            .as_mut()
            .ok_or_else(|| transport_failure("identity worker is unavailable"))?;
        send_load(managed, request).await?;
        state.last_load = Some(request.clone());
        Ok(())
    }

    async fn infer_wgc_frame(
        &self,
        payload: Value,
        cancellation_generation: u64,
    ) -> Result<UntrustedWorkerObservationsV1, IdentityRuntimeError> {
        let mut state = self.state.lock().await;
        if cancellation_generation != state.generation {
            return Err(transport_failure("stale identity inference generation"));
        }
        let managed = state
            .managed
            .as_mut()
            .ok_or_else(|| transport_failure("identity worker is unavailable"))?;
        let observation = request_response(
            managed,
            "infer",
            payload,
            cancellation_generation,
            INFERENCE_TIMEOUT,
            Some("identity_observations"),
        )
        .await?
        .ok_or_else(|| transport_failure("identity observation event is missing"))?;
        serde_json::from_value(observation)
            .map_err(|_| transport_failure("identity observation contract"))
    }

    async fn cancel_and_restart(
        &self,
        cancellation_generation: u64,
    ) -> Result<(), IdentityRuntimeError> {
        let mut state = self.state.lock().await;
        self.restart_locked(&mut state, cancellation_generation)
            .await
    }

    async fn shutdown(&self) {
        self.authenticated.store(false, Ordering::Release);
        self.parent_death_bound.store(false, Ordering::Release);
        self.clear_identity();
        let mut state = self.state.lock().await;
        if let Some(mut managed) = state.managed.take() {
            let generation = managed.generation;
            let _ = request_response(
                &mut managed,
                "shutdown",
                json!({}),
                generation,
                CONTROL_TIMEOUT,
                None,
            )
            .await;
            terminate_worker(&mut managed).await;
        }
        state.last_load = None;
    }
}

#[async_trait]
impl IdentityReferenceImportTransport for FramedIdentityWorkerTransport {
    async fn extract_reference(
        &self,
        request: &NativeReferenceExtractionV1,
        cancellation_generation: u64,
    ) -> Result<PortableReferenceImportV1, IdentityRuntimeError> {
        validate_reference_request(request)?;
        let mut state = self.state.lock().await;
        if cancellation_generation != state.generation {
            return Err(transport_failure("stale reference extraction generation"));
        }
        let managed = state
            .managed
            .as_mut()
            .ok_or_else(|| transport_failure("identity worker is unavailable"))?;
        let source_class = match request.provenance.source_class {
            ReferenceSourceClassV1::UserPrivate => "user_private",
            ReferenceSourceClassV1::OriginalSynthetic => "original_synthetic",
        };
        let payload = json!({
            "contract_version": "npc.identity-reference-import-request/v1",
            "mode": "reference_import",
            "game_profile_id": request.provenance.game_profile_id,
            "subject_id": request.provenance.subject_id,
            "reference_id": request.provenance.reference_id,
            "subject_display_name": request.subject_display_name,
            "source_class": source_class,
            "source_content_sha256": request.provenance.source_content_sha256,
            "owner_user_id": request.provenance.owner_user_id,
            "original_work_license": request.provenance.original_work_license,
            "explicit_user_consent": request.provenance.explicit_user_consent,
            "local_only": request.provenance.local_only,
            "imported_at_ms": request.provenance.imported_at_ms,
            "pixel_lease": {
                "lease_id": request.pixel_lease.lease_id.as_str(),
                "shared_memory_name": request.pixel_lease.shared_memory_name.as_str(),
                "lease_nonce": request.pixel_lease.lease_nonce.as_str(),
                "byte_length": request.pixel_lease.byte_length,
                "width": request.pixel_lease.width,
                "height": request.pixel_lease.height,
                "stride_bytes": request.pixel_lease.stride_bytes,
                "pixel_format": "b8g8r8a8_unorm",
                "content_sha256": request.pixel_lease.content_sha256,
            },
        });
        let result = request_response(
            managed,
            "infer",
            payload,
            cancellation_generation,
            INFERENCE_TIMEOUT,
            Some("identity_observations"),
        )
        .await?
        .ok_or_else(|| transport_failure("reference extraction event is missing"))?;
        let wrapper: ReferenceImportResult = serde_json::from_value(result)
            .map_err(|_| transport_failure("reference extraction result contract"))?;
        if wrapper.contract_version != "npc.portable-reference-import-result/v1"
            || wrapper.portable_reference_import.provenance != request.provenance
            || wrapper.portable_reference_import.subject_display_name
                != request.subject_display_name
        {
            return Err(transport_failure("reference extraction binding mismatch"));
        }
        Ok(wrapper.portable_reference_import)
    }
}

async fn send_load(
    managed: &mut ManagedWorker,
    request: &PrivateEvaluationLoad,
) -> Result<(), IdentityRuntimeError> {
    let activation_mode = match request.activation_mode {
        IdentityActivationMode::PrivateEvaluation => "private_evaluation",
        IdentityActivationMode::QualifiedCatalog => "qualified_catalog",
    };
    let artifact_root = request
        .artifact_root
        .to_str()
        .ok_or(IdentityRuntimeError::PrivateEvaluationGate)?;
    request_response(
        managed,
        "load",
        json!({
            "lease_id": request.lease_id,
            "pack_id": PACK_ID,
            "revision": PACK_REVISION,
            "manifest_sha256": request.manifest_sha256,
            "artifact_root": artifact_root,
            "backend": "cpu",
            "cpu_threads": request.cpu_threads,
            "explicit_user_confirmation": request.explicit_user_confirmation,
            "activation_mode": activation_mode,
            "verified_catalog_admission_sha256": request.verified_catalog_admission_sha256,
        }),
        managed.generation,
        LOAD_TIMEOUT,
        None,
    )
    .await
    .map(|_| ())
}

#[derive(Debug, Serialize)]
struct RequestEnvelope<'a> {
    protocol_version: &'static str,
    worker_instance_id: &'a str,
    request_id: &'a str,
    sequence: u64,
    generation: u64,
    deadline_unix_ms: u64,
    operation: &'a str,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerEvent {
    protocol_version: String,
    worker_instance_id: String,
    request_id: String,
    sequence: u64,
    generation: u64,
    event_index: u32,
    event: String,
    terminal: bool,
    payload: Value,
    #[serde(default)]
    error: Option<WorkerError>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerError {
    code: String,
    message: String,
    retryable: bool,
    details: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceImportResult {
    contract_version: String,
    portable_reference_import: PortableReferenceImportV1,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandshakeDescriptor {
    worker_id: String,
    kind: String,
    engine: String,
    pack_id: String,
    pack_revision: String,
    operations: Vec<String>,
    compute_backends: Vec<String>,
    network_access: bool,
    queue_depth: u8,
    cancellation: String,
    output_authority: String,
    character_selection: bool,
    demographic_inference: bool,
    activation_modes: Vec<String>,
    current_manifest_admission: String,
    model: HandshakeModel,
    detector: HandshakeDetector,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandshakeModel {
    provider: String,
    model_id: String,
    revision: String,
    dimensions: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandshakeDetector {
    detector_id: String,
    revision: String,
    preprocessing: String,
}

fn validate_handshake_descriptor(
    payload: Value,
    expected_worker_id: &str,
) -> Result<(), IdentityRuntimeError> {
    let descriptor: HandshakeDescriptor = serde_json::from_value(payload)
        .map_err(|_| transport_failure("worker handshake descriptor contract"))?;
    let expected_operations = [
        "cancel",
        "capabilities",
        "handshake",
        "health",
        "infer",
        "load",
        "shutdown",
        "unload",
    ];
    let expected_modes = ["private_evaluation", "qualified_catalog"];
    if descriptor.worker_id != expected_worker_id
        || descriptor.kind != "vision"
        || descriptor.engine != "opencv-yunet-sface-private-evaluation"
        || descriptor.pack_id != PACK_ID
        || descriptor.pack_revision != PACK_REVISION
        || descriptor.operations
            != expected_operations
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        || descriptor.compute_backends != ["cpu"]
        || descriptor.network_access
        || descriptor.queue_depth != 1
        || descriptor.cancellation != "generation_barrier_or_process_exit"
        || descriptor.output_authority != "untrusted_observations_native_revalidation_required"
        || descriptor.character_selection
        || descriptor.demographic_inference
        || descriptor.activation_modes
            != expected_modes
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        || descriptor.current_manifest_admission != "blocked_pending_measurement"
        || descriptor.model.provider != "opencv-zoo"
        || descriptor.model.model_id != "sface-2021dec-mobilefacenet"
        || descriptor.model.revision
            != "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"
        || descriptor.model.dimensions != 128
        || descriptor.detector.detector_id != "opencv-zoo-yunet-2026may"
        || descriptor.detector.revision
            != "sha256:ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0"
        || descriptor.detector.preprocessing
            != "opencv-face-recognizer-sf-aligncrop-bgr-112x112-l2-f32-v1"
    {
        return Err(transport_failure("worker handshake descriptor mismatch"));
    }
    Ok(())
}

async fn request_response(
    managed: &mut ManagedWorker,
    operation: &str,
    payload: Value,
    generation: u64,
    timeout: Duration,
    capture_event: Option<&str>,
) -> Result<Option<Value>, IdentityRuntimeError> {
    let sequence = managed.next_sequence;
    managed.next_sequence = managed
        .next_sequence
        .checked_add(1)
        .ok_or_else(|| transport_failure("worker request sequence exhausted"))?;
    let request_id = format!("req-{sequence}");
    let deadline_unix_ms = unix_millis()?
        .checked_add(timeout.as_millis() as u64)
        .ok_or_else(|| transport_failure("worker deadline overflow"))?;
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        worker_instance_id: &managed.instance_id,
        request_id: &request_id,
        sequence,
        generation,
        deadline_unix_ms,
        operation,
        payload,
    };

    let exchange = async {
        write_frame(&mut managed.stdin, &request).await?;
        let mut expected_index = 0_u32;
        let mut captured = None;
        loop {
            let event: WorkerEvent = read_frame(&mut managed.stdout).await?;
            validate_event(
                &event,
                &managed.instance_id,
                &request_id,
                sequence,
                generation,
                expected_index,
            )?;
            expected_index = expected_index
                .checked_add(1)
                .ok_or_else(|| transport_failure("worker event index exhausted"))?;
            if event.error.is_some() || event.event == "error" {
                let failure = event
                    .error
                    .as_ref()
                    .map(|value| value.code.as_str())
                    .unwrap_or("unspecified");
                return Err(transport_failure(match failure {
                    "deadline_exceeded" => "worker deadline exceeded",
                    "worker_busy" => "identity worker is busy",
                    "cancellation_timeout" => "identity cancellation timeout",
                    "model_not_loaded" => "identity model is not loaded",
                    "lease_digest_mismatch" => "identity frame digest mismatch",
                    _ => "identity worker rejected the request",
                }));
            }
            if capture_event == Some(event.event.as_str()) {
                if captured.is_some() {
                    return Err(transport_failure("duplicate identity observation event"));
                }
                captured = Some(event.payload.clone());
            }
            if event.terminal {
                if event.event != "completed" {
                    return Err(transport_failure("invalid terminal worker event"));
                }
                return Ok(captured);
            }
            if expected_index > 3 {
                return Err(transport_failure("worker emitted too many events"));
            }
        }
    };
    tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| transport_failure("worker request timeout"))?
}

fn validate_event(
    event: &WorkerEvent,
    instance_id: &str,
    request_id: &str,
    sequence: u64,
    generation: u64,
    expected_index: u32,
) -> Result<(), IdentityRuntimeError> {
    if event.protocol_version != PROTOCOL_VERSION
        || event.worker_instance_id != instance_id
        || event.request_id != request_id
        || event.sequence != sequence
        || event.generation != generation
        || event.event_index != expected_index
        || !event.payload.is_object()
        || event.error.as_ref().is_some_and(|error| {
            error.code.is_empty()
                || error.code.len() > 128
                || error.message.len() > 512
                || !error.details.is_object()
                || (error.retryable && event.event != "error")
        })
    {
        return Err(transport_failure("worker event binding mismatch"));
    }
    Ok(())
}

async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), IdentityRuntimeError> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| transport_failure("worker request serialization"))?;
    if encoded.is_empty() || encoded.len() > MAX_FRAME_BYTES {
        return Err(transport_failure("worker request frame bounds"));
    }
    writer
        .write_all(&(encoded.len() as u32).to_be_bytes())
        .await
        .map_err(|_| transport_failure("worker request write"))?;
    writer
        .write_all(&encoded)
        .await
        .map_err(|_| transport_failure("worker request write"))?;
    writer
        .flush()
        .await
        .map_err(|_| transport_failure("worker request flush"))
}

async fn read_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> Result<T, IdentityRuntimeError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .await
        .map_err(|_| transport_failure("worker response prefix"))?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(transport_failure("worker response frame bounds"));
    }
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| transport_failure("worker response body"))?;
    serde_json::from_slice(&body).map_err(|_| transport_failure("worker response contract"))
}

async fn terminate_worker(worker: &mut ManagedWorker) {
    if matches!(worker.child.try_wait(), Ok(None)) {
        let _ = worker.child.kill().await;
    }
    let _ = tokio::time::timeout(EXIT_TIMEOUT, worker.child.wait()).await;
}

fn validate_launch_config(config: &IdentityWorkerLaunchConfig) -> Result<(), IdentityRuntimeError> {
    if !cfg!(windows) {
        return Err(transport_failure(
            "Windows identity transport is unavailable",
        ));
    }
    let python = canonical_bound_file(
        &config.python_executable,
        config.python_size_bytes,
        &config.python_sha256,
    )?;
    let script = canonical_bound_file(
        &config.worker_script,
        config.worker_script_size_bytes,
        &config.worker_script_sha256,
    )?;
    let manifest = canonical_bound_file(
        &config.manifest_path,
        config.manifest_size_bytes,
        &config.manifest_sha256,
    )?;
    if !python.is_absolute()
        || script.file_name().and_then(|value| value.to_str()) != Some("worker.py")
        || manifest.file_name().and_then(|value| value.to_str()) != Some(MANIFEST_FILE_NAME)
        || Sha256::digest(
            std::fs::read(&manifest).map_err(|_| transport_failure("identity manifest read"))?,
        )
        .as_slice()
            != hex::decode(MANIFEST_SHA256)
                .map_err(|_| transport_failure("identity manifest digest"))?
                .as_slice()
    {
        return Err(invalid_launch());
    }
    Ok(())
}

fn validate_reference_request(
    request: &NativeReferenceExtractionV1,
) -> Result<(), IdentityRuntimeError> {
    validate_reference_pixel_lease(&request.pixel_lease)?;
    let provenance = &request.provenance;
    if !valid_identifier(&provenance.game_profile_id)
        || !valid_identifier(&provenance.subject_id)
        || !valid_identifier(&provenance.reference_id)
        || provenance.source_content_sha256 != request.pixel_lease.content_sha256
        || request.subject_display_name.trim().is_empty()
        || request.subject_display_name.len() > 256
        || !provenance.local_only
    {
        return Err(transport_failure("invalid reference extraction provenance"));
    }
    match provenance.source_class {
        ReferenceSourceClassV1::UserPrivate => {
            if !provenance.explicit_user_consent
                || provenance
                    .owner_user_id
                    .as_deref()
                    .map_or(true, |owner| !valid_identifier(owner))
                || provenance.original_work_license.is_some()
            {
                return Err(transport_failure("invalid user-private reference consent"));
            }
        }
        ReferenceSourceClassV1::OriginalSynthetic => {
            if provenance.owner_user_id.is_some()
                || provenance
                    .original_work_license
                    .as_deref()
                    .map_or(true, |license| {
                        license.trim().is_empty() || license.len() > 512
                    })
            {
                return Err(transport_failure("invalid original reference license"));
            }
        }
    }
    Ok(())
}

fn validate_reference_pixel_lease(
    lease: &ReferencePixelLeaseV1,
) -> Result<(), IdentityRuntimeError> {
    let expected_stride = lease
        .width
        .checked_mul(4)
        .ok_or_else(|| transport_failure("reference pixel stride overflow"))?;
    let expected_bytes = u64::from(lease.stride_bytes)
        .checked_mul(u64::from(lease.height))
        .ok_or_else(|| transport_failure("reference pixel length overflow"))?;
    let mut binding = Sha256::new();
    binding.update(lease.lease_id.as_bytes());
    binding.update([0]);
    binding.update(lease.lease_nonce.as_bytes());
    let expected_mapping = format!(r"Local\npc.identity.{}", hex::encode(binding.finalize()));
    if !valid_worker_opaque(&lease.lease_id)
        || !valid_worker_opaque(&lease.lease_nonce)
        || lease.shared_memory_name.as_str() != expected_mapping
        || lease.width == 0
        || lease.height == 0
        || lease.width > MAX_IMAGE_EDGE
        || lease.height > MAX_IMAGE_EDGE
        || lease.stride_bytes != expected_stride
        || lease.byte_length == 0
        || lease.byte_length > MAX_SHARED_IMAGE_BYTES
        || lease.byte_length != expected_bytes
        || !valid_sha256(&lease.content_sha256)
    {
        return Err(transport_failure("invalid broker reference pixel lease"));
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_worker_opaque(value: &str) -> bool {
    (2..=256).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn canonical_file(path: &Path) -> Result<PathBuf, IdentityRuntimeError> {
    if !path.is_absolute() || path.is_symlink() || !path.is_file() {
        return Err(invalid_launch());
    }
    path.canonicalize().map_err(|_| invalid_launch())
}

fn canonical_bound_file(
    path: &Path,
    expected_size_bytes: u64,
    expected_sha256: &str,
) -> Result<PathBuf, IdentityRuntimeError> {
    if expected_size_bytes == 0 || !valid_sha256(expected_sha256) {
        return Err(invalid_launch());
    }
    let canonical = canonical_file(path)?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid_launch())?;
    if metadata.len() != expected_size_bytes {
        return Err(invalid_launch());
    }
    let bytes = std::fs::read(path).map_err(|_| invalid_launch())?;
    if bytes.len() as u64 != expected_size_bytes
        || hex::encode(Sha256::digest(&bytes)) != expected_sha256
    {
        return Err(invalid_launch());
    }
    Ok(canonical)
}

fn random_hex(bytes: usize) -> Result<String, IdentityRuntimeError> {
    let mut value = vec![0_u8; bytes];
    getrandom::fill(&mut value).map_err(|_| transport_failure("secure launch randomness"))?;
    Ok(hex::encode(value))
}

fn unix_millis() -> Result<u64, IdentityRuntimeError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| transport_failure("system clock"))?
        .as_millis();
    u64::try_from(millis).map_err(|_| transport_failure("system clock range"))
}

#[cfg(windows)]
fn process_creation_time(child: &Child) -> Result<u64, IdentityRuntimeError> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let handle = child
        .raw_handle()
        .ok_or_else(|| transport_failure("worker process handle"))?;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the child owns a live process handle and every FILETIME points to
    // initialized writable storage for the duration of this synchronous call.
    if unsafe {
        GetProcessTimes(
            handle.cast(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(transport_failure("worker process creation time"));
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn invalid_launch() -> IdentityRuntimeError {
    transport_failure("invalid identity worker launch configuration")
}

fn transport_failure(message: &str) -> IdentityRuntimeError {
    IdentityRuntimeError::Transport(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    fn reference_lease() -> ReferencePixelLeaseV1 {
        let lease_id = "reference-lease-01".to_owned();
        let lease_nonce = "reference-nonce-01".to_owned();
        let mut binding = Sha256::new();
        binding.update(lease_id.as_bytes());
        binding.update([0]);
        binding.update(lease_nonce.as_bytes());
        ReferencePixelLeaseV1::new(
            lease_id,
            format!(r"Local\npc.identity.{}", hex::encode(binding.finalize())),
            lease_nonce,
            64 * 64 * 4,
            64,
            64,
            64 * 4,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        )
        .expect("bounded reference lease")
    }

    #[test]
    fn checked_in_manifest_digest_matches_transport_pin() {
        let bytes = include_bytes!(
            "../../../../packaging/model-packs/opencv-yunet-sface-private-evaluation.json"
        );
        assert_eq!(hex::encode(Sha256::digest(bytes)), MANIFEST_SHA256);
    }

    #[test]
    fn admitted_worker_file_is_rehashed_and_rejects_same_size_tamper() {
        let directory = tempfile::tempdir().expect("worker binding directory");
        let path = directory.path().join("python.exe");
        std::fs::write(&path, b"trusted-python").expect("bound worker file");
        let expected = hex::encode(Sha256::digest(b"trusted-python"));
        assert!(canonical_bound_file(&path, 14, &expected).is_ok());

        std::fs::write(&path, b"tamper-python!").expect("same-size tamper");
        assert!(canonical_bound_file(&path, 14, &expected).is_err());
        assert!(canonical_bound_file(&path, 13, &expected).is_err());
        assert!(canonical_bound_file(&path, 14, &"A".repeat(64)).is_err());
    }

    #[tokio::test]
    async fn bounded_big_endian_frame_round_trips() {
        let (mut producer, mut consumer) = tokio::io::duplex(4_096);
        let write = tokio::spawn(async move {
            write_frame(&mut producer, &json!({"hello": "identity"}))
                .await
                .expect("write bounded frame");
        });
        let value: Value = read_frame(&mut consumer).await.expect("read bounded frame");
        write.await.expect("writer task");
        assert_eq!(value, json!({"hello": "identity"}));
    }

    #[tokio::test]
    async fn oversized_response_is_rejected_before_allocation() {
        let (mut producer, mut consumer) = tokio::io::duplex(16);
        producer
            .write_all(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes())
            .await
            .expect("prefix");
        let result: Result<Value, _> = read_frame(&mut consumer).await;
        assert!(result.is_err());
    }

    #[test]
    fn worker_event_parser_denies_unknown_fields() {
        let event = json!({
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": "identity-test",
            "request_id": "req-1",
            "sequence": 1,
            "generation": 0,
            "event_index": 0,
            "event": "completed",
            "terminal": true,
            "payload": {},
            "unexpected": true,
        });
        assert!(serde_json::from_value::<WorkerEvent>(event).is_err());
    }

    #[test]
    fn worker_event_must_match_exact_process_request_and_generation() {
        let event: WorkerEvent = serde_json::from_value(json!({
            "protocol_version": PROTOCOL_VERSION,
            "worker_instance_id": "identity-test",
            "request_id": "req-1",
            "sequence": 1,
            "generation": 0,
            "event_index": 0,
            "event": "completed",
            "terminal": true,
            "payload": {},
        }))
        .expect("strict event");
        assert!(validate_event(&event, "identity-test", "req-1", 1, 0, 0).is_ok());
        assert!(validate_event(&event, "other-worker", "req-1", 1, 0, 0).is_err());
        assert!(validate_event(&event, "identity-test", "req-1", 1, 1, 0).is_err());
    }

    #[test]
    fn handshake_descriptor_is_an_exact_non_authoritative_cpu_contract() {
        let descriptor = json!({
            "worker_id": "identity-test",
            "kind": "vision",
            "engine": "opencv-yunet-sface-private-evaluation",
            "pack_id": PACK_ID,
            "pack_revision": PACK_REVISION,
            "operations": ["cancel", "capabilities", "handshake", "health", "infer", "load", "shutdown", "unload"],
            "compute_backends": ["cpu"],
            "network_access": false,
            "queue_depth": 1,
            "cancellation": "generation_barrier_or_process_exit",
            "output_authority": "untrusted_observations_native_revalidation_required",
            "character_selection": false,
            "demographic_inference": false,
            "activation_modes": ["private_evaluation", "qualified_catalog"],
            "current_manifest_admission": "blocked_pending_measurement",
            "model": {
                "provider": "opencv-zoo",
                "model_id": "sface-2021dec-mobilefacenet",
                "revision": "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
                "dimensions": 128,
            },
            "detector": {
                "detector_id": "opencv-zoo-yunet-2026may",
                "revision": "sha256:ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0",
                "preprocessing": "opencv-face-recognizer-sf-aligncrop-bgr-112x112-l2-f32-v1",
            },
        });
        assert!(validate_handshake_descriptor(descriptor.clone(), "identity-test").is_ok());
        assert!(validate_handshake_descriptor(descriptor.clone(), "identity-other").is_err());
        let mut authoritative = descriptor;
        authoritative["character_selection"] = Value::Bool(true);
        assert!(validate_handshake_descriptor(authoritative, "identity-test").is_err());
    }

    #[test]
    fn reference_extraction_accepts_only_private_consent_or_original_license() {
        let mut private = NativeReferenceExtractionV1 {
            provenance: QualifiedReferenceProvenanceV1 {
                game_profile_id: "game-01".into(),
                subject_id: "npc-01".into(),
                reference_id: "reference-01".into(),
                source_class: ReferenceSourceClassV1::UserPrivate,
                source_content_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                owner_user_id: Some("local-user".into()),
                original_work_license: None,
                explicit_user_consent: true,
                local_only: true,
                imported_at_ms: 1_700_000_000_000,
            },
            subject_display_name: "Test NPC".into(),
            pixel_lease: reference_lease(),
        };
        assert!(validate_reference_request(&private).is_ok());
        private.provenance.explicit_user_consent = false;
        assert!(validate_reference_request(&private).is_err());

        private.provenance.source_class = ReferenceSourceClassV1::OriginalSynthetic;
        private.provenance.owner_user_id = None;
        private.provenance.original_work_license = Some("CC0-1.0".into());
        assert!(validate_reference_request(&private).is_ok());
        private.provenance.original_work_license = None;
        assert!(validate_reference_request(&private).is_err());
    }

    #[test]
    fn reference_pixel_capabilities_are_redacted_and_mapping_bound() {
        let lease = reference_lease();
        let rendered = format!("{lease:?}");
        assert!(!rendered.contains(lease.lease_id()));
        assert!(!rendered.contains(lease.lease_nonce()));
        assert!(!rendered.contains("Local\\npc.identity"));
        assert!(!rendered.contains(&lease.content_sha256));
        assert_eq!(lease.lease_id(), "reference-lease-01");

        let wrong = ReferencePixelLeaseV1::new(
            "reference-lease-01".into(),
            r"Local\npc.identity.ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                .into(),
            "reference-nonce-01".into(),
            64 * 64 * 4,
            64,
            64,
            64 * 4,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        );
        assert!(wrong.is_err());
    }
}
