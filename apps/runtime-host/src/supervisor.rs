//! Attested worker-pack process supervision.
//!
//! No API in this module launches an arbitrary path: callers must first bind a
//! relative worker-pack declaration to a trusted root/version/runtime ABI.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};

use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_CHILDREN: usize = 16;
const MAX_ARGUMENTS: usize = 128;
const MAX_ARGUMENT_BYTES: usize = 4 * 1024;
const MAX_TOTAL_ARGUMENT_BYTES: usize = 32 * 1024;
const MAX_IDENTITY_BYTES: usize = 128;
const READ_BUFFER_BYTES: usize = 64 * 1024;

/// The minimum Windows runtime environment. Credentials and configuration must
/// instead use authenticated IPC, a private handle, or an in-worker vault lookup.
const SAFE_INHERITED_ENV: [&str; 4] = ["SystemRoot", "WINDIR", "TEMP", "TMP"];

#[derive(Clone, Eq, PartialEq)]
pub struct WorkerTrustPolicy {
    root: PathBuf,
    version: String,
    runtime_abi: String,
}

impl fmt::Debug for WorkerTrustPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkerTrustPolicy")
            .field("root", &"<redacted>")
            .field("version", &"<redacted>")
            .field("runtime_abi", &"<redacted>")
            .finish()
    }
}

impl WorkerTrustPolicy {
    pub fn new(
        root: PathBuf,
        version: impl Into<String>,
        runtime_abi: impl Into<String>,
    ) -> Result<Self, SupervisionError> {
        let version = version.into();
        let runtime_abi = runtime_abi.into();
        if !root.is_absolute() || !valid_identity(&version) || !valid_identity(&runtime_abi) {
            return Err(SupervisionError::InvalidAttestation);
        }
        Ok(Self {
            root,
            version,
            runtime_abi,
        })
    }
}

/// Signed argument bounds. The exact argument vector is signed separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerArgumentLimits {
    count: usize,
    per_argument_bytes: usize,
    total_bytes: usize,
}

impl WorkerArgumentLimits {
    pub fn new(
        count: usize,
        per_argument_bytes: usize,
        total_bytes: usize,
    ) -> Result<Self, SupervisionError> {
        if count == 0
            || count > MAX_ARGUMENTS
            || per_argument_bytes == 0
            || per_argument_bytes > MAX_ARGUMENT_BYTES
            || total_bytes < per_argument_bytes
            || total_bytes > MAX_TOTAL_ARGUMENT_BYTES
        {
            return Err(SupervisionError::InvalidAttestation);
        }
        Ok(Self {
            count,
            per_argument_bytes,
            total_bytes,
        })
    }
}

impl Default for WorkerArgumentLimits {
    fn default() -> Self {
        Self {
            count: MAX_ARGUMENTS,
            per_argument_bytes: MAX_ARGUMENT_BYTES,
            total_bytes: MAX_TOTAL_ARGUMENT_BYTES,
        }
    }
}

/// Untrusted evidence from a signed worker-pack manifest. All filesystem paths
/// are relative to the trusted root. Debug output is deliberately redacted.
#[derive(Clone)]
pub struct WorkerPackAttestation {
    version: String,
    runtime_abi: String,
    executable: PathBuf,
    working_directory: PathBuf,
    sha256: String,
    size: u64,
    args: Vec<OsString>,
    limits: WorkerArgumentLimits,
}

impl WorkerPackAttestation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: impl Into<String>,
        runtime_abi: impl Into<String>,
        executable: PathBuf,
        working_directory: PathBuf,
        sha256: impl Into<String>,
        size: u64,
        args: Vec<OsString>,
        limits: WorkerArgumentLimits,
    ) -> Self {
        Self {
            version: version.into(),
            runtime_abi: runtime_abi.into(),
            executable,
            working_directory,
            sha256: sha256.into(),
            size,
            args,
            limits,
        }
    }

    pub fn verify(
        self,
        policy: &WorkerTrustPolicy,
    ) -> Result<VerifiedWorkerPackAttestation, SupervisionError> {
        if !valid_identity(&self.version)
            || !valid_identity(&self.runtime_abi)
            || self.size == 0
            || !valid_sha256(&self.sha256)
        {
            return Err(SupervisionError::InvalidAttestation);
        }
        if self.version != policy.version {
            return Err(SupervisionError::PackVersionMismatch);
        }
        if self.runtime_abi != policy.runtime_abi {
            return Err(SupervisionError::RuntimeAbiMismatch);
        }
        validate_relative(&self.executable)?;
        validate_relative(&self.working_directory)?;
        validate_args(&self.args, self.limits)?;
        no_links_or_reparse(&policy.root)?;
        let root = std::fs::canonicalize(&policy.root)
            .map_err(|_| SupervisionError::InvalidAttestation)?;
        if !root.is_dir() {
            return Err(SupervisionError::InvalidAttestation);
        }
        let executable = checked_canonical_child(&root, &root.join(&self.executable))?;
        let working_directory =
            checked_canonical_child(&root, &root.join(&self.working_directory))?;
        if !executable.is_file() || !working_directory.is_dir() {
            return Err(SupervisionError::InvalidAttestation);
        }
        verify_executable(&executable, self.size, &self.sha256)?;
        Ok(VerifiedWorkerPackAttestation {
            root,
            version: self.version,
            runtime_abi: self.runtime_abi,
            executable,
            working_directory,
            sha256: self.sha256.to_ascii_lowercase(),
            size: self.size,
            args: self.args,
            limits: self.limits,
        })
    }
}

impl fmt::Debug for WorkerPackAttestation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerPackAttestation")
            .field("identity", &"<redacted>")
            .field("paths", &"<redacted>")
            .field("digest", &"<redacted>")
            .field("size", &self.size)
            .field("args", &format_args!("<redacted:{}>", self.args.len()))
            .field("limits", &self.limits)
            .finish()
    }
}

#[derive(Clone)]
pub struct VerifiedWorkerPackAttestation {
    root: PathBuf,
    version: String,
    runtime_abi: String,
    executable: PathBuf,
    working_directory: PathBuf,
    sha256: String,
    size: u64,
    args: Vec<OsString>,
    limits: WorkerArgumentLimits,
}

impl fmt::Debug for VerifiedWorkerPackAttestation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedWorkerPackAttestation")
            .field("identity", &"<redacted>")
            .field("paths", &"<redacted>")
            .field("digest", &"<redacted>")
            .field("size", &self.size)
            .field("args", &format_args!("<redacted:{}>", self.args.len()))
            .field("limits", &self.limits)
            .finish()
    }
}

/// Immutable launch capability. Its only constructor consumes verified evidence.
#[derive(Clone)]
pub struct ChildSpec {
    attestation: VerifiedWorkerPackAttestation,
}

impl ChildSpec {
    #[must_use]
    pub fn from_verified_attestation(attestation: VerifiedWorkerPackAttestation) -> Self {
        Self { attestation }
    }

    pub fn validate(&self) -> Result<(), SupervisionError> {
        let a = &self.attestation;
        if !valid_identity(&a.version) || !valid_identity(&a.runtime_abi) {
            return Err(SupervisionError::InvalidAttestation);
        }
        validate_args(&a.args, a.limits)?;
        no_links_or_reparse(&a.root)?;
        let root = checked_same_path(&a.root)?;
        let executable = checked_same_path(&a.executable)?;
        let working_directory = checked_same_path(&a.working_directory)?;
        ensure_child(&root, &executable)?;
        ensure_child(&root, &working_directory)?;
        if !executable.is_file() || !working_directory.is_dir() {
            return Err(SupervisionError::InvalidAttestation);
        }
        verify_executable(&executable, a.size, &a.sha256)
    }
}

impl fmt::Debug for ChildSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChildSpec")
            .field("attestation", &self.attestation)
            .finish()
    }
}

#[derive(Clone)]
pub struct ChildSupervisor {
    inner: Arc<Mutex<Vec<SupervisedChild>>>,
    #[cfg(windows)]
    job: Arc<WindowsJob>,
}

impl ChildSupervisor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
            #[cfg(windows)]
            job: Arc::new(WindowsJob::new().expect("Windows Job Object initialization failed")),
        }
    }

    pub fn spawn(&self, spec: &ChildSpec) -> Result<u32, SupervisionError> {
        let mut children = self.inner.lock().map_err(|_| SupervisionError::Poisoned)?;
        children.retain_mut(SupervisedChild::is_running);
        if children.len() >= MAX_CHILDREN {
            return Err(SupervisionError::Capacity);
        }

        // Intentionally adjacent to process creation: activation-time verification
        // alone does not protect a pack modified after activation.
        spec.validate()?;
        let a = &spec.attestation;
        let mut command = Command::new(&a.executable);
        command
            .args(&a.args)
            .current_dir(&a.working_directory)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for name in SAFE_INHERITED_ENV {
            if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().map_err(|_| SupervisionError::Spawn)?;
        #[cfg(windows)]
        if let Err(error) = self.job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let id = child.id();
        children.push(SupervisedChild { child });
        Ok(id)
    }

    pub fn terminate_all(&self) {
        if let Ok(mut children) = self.inner.lock() {
            for child in children.iter_mut() {
                child.terminate();
            }
            children.clear();
        }
    }

    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.lock().map_or(0, |children| children.len())
    }
}

impl Default for ChildSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for ChildSupervisor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.terminate_all();
        }
    }
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn validate_relative(path: &Path) -> Result<(), SupervisionError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(SupervisionError::PathEscape)
    } else {
        Ok(())
    }
}

fn ensure_child(root: &Path, path: &Path) -> Result<(), SupervisionError> {
    if path == root || !path.starts_with(root) {
        Err(SupervisionError::PathEscape)
    } else {
        Ok(())
    }
}

fn checked_canonical_child(root: &Path, path: &Path) -> Result<PathBuf, SupervisionError> {
    no_links_or_reparse(path)?;
    let canonical =
        std::fs::canonicalize(path).map_err(|_| SupervisionError::InvalidAttestation)?;
    ensure_child(root, &canonical)?;
    Ok(canonical)
}

fn checked_same_path(path: &Path) -> Result<PathBuf, SupervisionError> {
    no_links_or_reparse(path)?;
    let canonical =
        std::fs::canonicalize(path).map_err(|_| SupervisionError::InvalidAttestation)?;
    if canonical != path {
        return Err(SupervisionError::PackPathChanged);
    }
    Ok(canonical)
}

fn no_links_or_reparse(path: &Path) -> Result<(), SupervisionError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| SupervisionError::InvalidAttestation)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(SupervisionError::LinkOrReparsePoint);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &std::fs::Metadata) -> bool {
    false
}

fn validate_args(args: &[OsString], limits: WorkerArgumentLimits) -> Result<(), SupervisionError> {
    if args.len() > limits.count {
        return Err(SupervisionError::ArgumentLimitExceeded);
    }
    let mut total = 0_usize;
    for arg in args {
        let size = os_size(arg);
        if size > limits.per_argument_bytes || has_nul(arg) {
            return Err(SupervisionError::ArgumentLimitExceeded);
        }
        total = total
            .checked_add(size)
            .ok_or(SupervisionError::ArgumentLimitExceeded)?;
        if total > limits.total_bytes {
            return Err(SupervisionError::ArgumentLimitExceeded);
        }
        if credential_shaped(arg) {
            return Err(SupervisionError::CredentialInArguments);
        }
    }
    Ok(())
}

fn credential_shaped(argument: &OsStr) -> bool {
    let value = argument.to_string_lossy().to_ascii_lowercase();
    let markers = [
        "--api-key",
        "--apikey",
        "api_key=",
        "apikey=",
        "x-api-key",
        "--access-token",
        "access_token=",
        "--auth-token",
        "auth_token=",
        "authorization=",
        "bearer ",
        "--secret",
        "secret=",
        "--password",
        "password=",
    ];
    markers.iter().any(|m| value.contains(m))
        || ["sk-", "sk_live_", "sk_test_", "ghp_", "github_pat_", "xai-"]
            .iter()
            .any(|p| value.starts_with(p) && value.len() >= p.len() + 16)
}

#[cfg(unix)]
fn os_size(value: &OsStr) -> usize {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().len()
}
#[cfg(windows)]
fn os_size(value: &OsStr) -> usize {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().count().saturating_mul(2)
}
#[cfg(not(any(unix, windows)))]
fn os_size(value: &OsStr) -> usize {
    value.to_string_lossy().len()
}
#[cfg(unix)]
fn has_nul(value: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().contains(&0)
}
#[cfg(windows)]
fn has_nul(value: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().any(|c| c == 0)
}
#[cfg(not(any(unix, windows)))]
fn has_nul(value: &OsStr) -> bool {
    value.to_string_lossy().contains('\0')
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn verify_executable(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), SupervisionError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| SupervisionError::InvalidAttestation)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse(&metadata) {
        return Err(SupervisionError::LinkOrReparsePoint);
    }
    if metadata.len() != expected_size {
        return Err(SupervisionError::ExecutableTampered);
    }
    let mut file = File::open(path).map_err(|_| SupervisionError::InvalidAttestation)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; READ_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| SupervisionError::InvalidAttestation)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected_sha256) {
        Ok(())
    } else {
        Err(SupervisionError::ExecutableTampered)
    }
}

struct SupervisedChild {
    child: Child,
}
impl SupervisedChild {
    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for SupervisedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WindowsJob {
    fn new() -> Result<Self, SupervisionError> {
        use std::mem::size_of;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: null attributes and name request a private unnamed Job Object.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(SupervisionError::JobObject);
        }
        // SAFETY: the Windows POD permits all-zero initialization.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: handle is live and buffer/size match the requested information class.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            // SAFETY: this function owns the live handle on this error path.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err(SupervisionError::JobObject);
        }
        Ok(Self(handle))
    }
    fn assign(&self, child: &Child) -> Result<(), SupervisionError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        // SAFETY: both kernel handles are live for the duration of this call.
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle().cast()) } == 0 {
            Err(SupervisionError::JobObject)
        } else {
            Ok(())
        }
    }
}
#[cfg(windows)]
// SAFETY: kernel Job Object handles may be used across threads; Arc owns lifetime.
unsafe impl Send for WindowsJob {}
#[cfg(windows)]
// SAFETY: Job Object operations do not mutate Rust memory through this handle.
unsafe impl Sync for WindowsJob {}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        // SAFETY: this instance exclusively owns the live handle.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SupervisionError {
    #[error("worker pack attestation is invalid")]
    InvalidAttestation,
    #[error("worker pack path is outside its trusted root")]
    PathEscape,
    #[error("worker pack path contains a link or reparse point")]
    LinkOrReparsePoint,
    #[error("worker pack version does not match the trust policy")]
    PackVersionMismatch,
    #[error("worker runtime ABI does not match the trust policy")]
    RuntimeAbiMismatch,
    #[error("worker pack path changed after attestation")]
    PackPathChanged,
    #[error("worker executable failed its size or SHA-256 attestation")]
    ExecutableTampered,
    #[error("credentials are forbidden in child process arguments")]
    CredentialInArguments,
    #[error("worker argument limits were exceeded")]
    ArgumentLimitExceeded,
    #[error("child supervisor capacity was reached")]
    Capacity,
    #[error("child supervisor lock was poisoned")]
    Poisoned,
    #[error("child process failed to start")]
    Spawn,
    #[error("Windows Job Object setup failed")]
    JobObject,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    const VERSION: &str = "worker-pack-1.2.3";
    const ABI: &str = "npc-worker-v1";

    struct Pack {
        _temp: TempDir,
        root: PathBuf,
        executable: PathBuf,
    }
    impl Pack {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("pack");
            fs::create_dir_all(root.join("work")).unwrap();
            let source = std::env::current_exe().unwrap();
            let executable = root.join(source.file_name().unwrap());
            fs::copy(source, &executable).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut p = fs::metadata(&executable).unwrap().permissions();
                p.set_mode(0o700);
                fs::set_permissions(&executable, p).unwrap();
            }
            Self {
                _temp: temp,
                root,
                executable,
            }
        }
        fn policy(&self) -> WorkerTrustPolicy {
            WorkerTrustPolicy::new(self.root.clone(), VERSION, ABI).unwrap()
        }
        fn attestation(&self, args: Vec<OsString>) -> WorkerPackAttestation {
            WorkerPackAttestation::new(
                VERSION,
                ABI,
                PathBuf::from(self.executable.file_name().unwrap()),
                PathBuf::from("work"),
                hash(&self.executable),
                fs::metadata(&self.executable).unwrap().len(),
                args,
                WorkerArgumentLimits::default(),
            )
        }
        fn spec(&self, args: Vec<OsString>) -> ChildSpec {
            ChildSpec::from_verified_attestation(
                self.attestation(args).verify(&self.policy()).unwrap(),
            )
        }
    }
    fn hash(path: &Path) -> String {
        let mut f = File::open(path).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        format!("{:x}", Sha256::digest(b))
    }

    #[test]
    fn valid_temp_pack_helper() {
        let p = Pack::new();
        assert_eq!(p.spec(vec!["--help".into()]).validate(), Ok(()));
    }

    #[test]
    fn tamper_is_rejected_at_spawn() {
        let p = Pack::new();
        let spec = p.spec(vec!["--help".into()]);
        fs::OpenOptions::new()
            .append(true)
            .open(&p.executable)
            .unwrap()
            .write_all(b"x")
            .unwrap();
        assert_eq!(
            ChildSupervisor::new().spawn(&spec),
            Err(SupervisionError::ExecutableTampered)
        );
    }

    #[test]
    fn root_escape_and_alternate_executable_are_rejected() {
        let p = Pack::new();
        let mut traversal = p.attestation(Vec::new());
        traversal.executable = PathBuf::from("../outside");
        assert!(matches!(
            traversal.verify(&p.policy()),
            Err(SupervisionError::PathEscape)
        ));
        let mut absolute = p.attestation(Vec::new());
        absolute.executable = std::env::current_exe().unwrap();
        assert!(matches!(
            absolute.verify(&p.policy()),
            Err(SupervisionError::PathEscape)
        ));
    }

    #[test]
    fn abi_and_version_mismatch_are_rejected() {
        let p = Pack::new();
        let mut a = p.attestation(Vec::new());
        a.runtime_abi = "npc-worker-v2".into();
        assert!(matches!(
            a.verify(&p.policy()),
            Err(SupervisionError::RuntimeAbiMismatch)
        ));
        let mut a = p.attestation(Vec::new());
        a.version = "worker-pack-9".into();
        assert!(matches!(
            a.verify(&p.policy()),
            Err(SupervisionError::PackVersionMismatch)
        ));
    }

    #[test]
    fn credential_argv_and_debug_are_redacted() {
        let p = Pack::new();
        let canary = "sk-live-CREDENTIAL-CANARY-DO-NOT-LOG";
        let a = p.attestation(vec![format!("--api-key={canary}").into()]);
        let debug = format!("{a:?}");
        let result = a.verify(&p.policy());
        assert!(matches!(
            &result,
            Err(SupervisionError::CredentialInArguments)
        ));
        assert!(!debug.contains(canary));
        assert!(!format!("{result:?}").contains(canary));
        assert!(!result.unwrap_err().to_string().contains(canary));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn linked_or_reparse_working_directory_is_rejected() {
        let p = Pack::new();
        let target = p.root.join("target");
        let link = p.root.join("linked");
        fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &link).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(target, &link) {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("could not create reparse-point fixture: {error}");
        }
        let mut a = p.attestation(Vec::new());
        a.working_directory = PathBuf::from("linked");
        assert!(matches!(
            a.verify(&p.policy()),
            Err(SupervisionError::LinkOrReparsePoint)
        ));
    }

    #[test]
    #[ignore]
    fn credential_probe_child() {
        let temp = std::env::var_os("TEMP")
            .or_else(|| std::env::var_os("TMP"))
            .unwrap();
        let leaked = [
            "OPENAI_API_KEY",
            "COHERE_API_KEY",
            "ASSEMBLYAI_API_KEY",
            "ELEVENLABS_API_KEY",
            "NPC_SUPERVISOR_DRIVER",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
        fs::write(
            PathBuf::from(temp).join("credential-canary-result"),
            if leaked { "leaked" } else { "clean" },
        )
        .unwrap();
    }

    #[test]
    #[ignore]
    fn credential_canary_driver() {
        if std::env::var_os("NPC_SUPERVISOR_DRIVER").is_none() {
            return;
        }
        let temp = PathBuf::from(std::env::var_os("TEMP").unwrap());
        let source = std::env::current_exe().unwrap();
        let root = temp.join("attested-pack");
        fs::create_dir_all(root.join("work")).unwrap();
        let executable = root.join(source.file_name().unwrap());
        fs::copy(source, &executable).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&executable).unwrap().permissions();
            p.set_mode(0o700);
            fs::set_permissions(&executable, p).unwrap();
        }
        let policy = WorkerTrustPolicy::new(root, VERSION, ABI).unwrap();
        let a = WorkerPackAttestation::new(
            VERSION,
            ABI,
            PathBuf::from(executable.file_name().unwrap()),
            PathBuf::from("work"),
            hash(&executable),
            fs::metadata(&executable).unwrap().len(),
            vec![
                "--ignored".into(),
                "--exact".into(),
                "supervisor::tests::credential_probe_child".into(),
            ],
            WorkerArgumentLimits::default(),
        );
        let supervisor = ChildSupervisor::new();
        supervisor
            .spawn(&ChildSpec::from_verified_attestation(
                a.verify(&policy).unwrap(),
            ))
            .unwrap();
        let marker = temp.join("credential-canary-result");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fs::read_to_string(marker).unwrap(), "clean");
        supervisor.terminate_all();
    }

    #[test]
    fn process_level_credential_canary_is_absent() {
        let d = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "supervisor::tests::credential_canary_driver",
            ])
            .env("NPC_SUPERVISOR_DRIVER", "1")
            .env("TEMP", d.path())
            .env("TMP", d.path())
            .env("OPENAI_API_KEY", "sk-live-process-canary")
            .env("COHERE_API_KEY", "cohere-canary")
            .env("ASSEMBLYAI_API_KEY", "assembly-canary")
            .env("ELEVENLABS_API_KEY", "eleven-canary")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "driver failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(d.path().join("credential-canary-result")).unwrap(),
            "clean"
        );
    }
}
