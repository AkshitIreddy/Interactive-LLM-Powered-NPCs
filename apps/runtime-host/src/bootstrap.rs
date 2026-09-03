use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use interactive_npcs_credential_vault::CredentialVault;
#[cfg(not(windows))]
use interactive_npcs_credential_vault::MemoryCredentialVault;
#[cfg(windows)]
use interactive_npcs_credential_vault::WindowsCredentialVault;
use model_manager::{
    CatalogSignatureVerifier as ModelCatalogSignatureVerifier, Ed25519CatalogVerifier,
};
use npc_memory::MemoryStore;
use npc_provider_catalog::{
    CatalogDocument, CatalogError, SignatureAlgorithm, SignatureVerifier, TrustPolicy,
};
use serde::Serialize;
use thiserror::Error;

use crate::{
    profiles::ProfileCorpus, supervisor::ChildSupervisor, tts_bridge::TtsVoiceDiscoveryService,
};

const MAX_CATALOG_BYTES: u64 = 2 * 1024 * 1024;

/// Catalog trust state established while bootstrapping the runtime.
///
/// Development builds deliberately retain an unsigned local-review path. A
/// release build can only reach `ReleaseSignatureVerified` after validating a
/// detached signature against the immutable key configuration below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogTrustState {
    DevelopmentUnsignedAllowed,
    ReleaseSignatureVerified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeBuildMode {
    Development,
    Release,
}

impl RuntimeBuildMode {
    const fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Development
        } else {
            Self::Release
        }
    }
}

#[derive(Clone, Copy)]
struct ReleaseTrustedCatalogKey {
    key_id: &'static str,
    public_key: [u8; 32],
}

// This trust root is compiled into the runtime and cannot be supplied by a
// catalog, environment variable, command-line option, or downloaded metadata.
// It intentionally remains empty for the local-review build: a reviewed
// production public key must be provisioned here before any release build can
// accept a catalog. Do not substitute a fixture or generated key.
const RELEASE_TRUSTED_CATALOG_KEYS: &[ReleaseTrustedCatalogKey] = &[];

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub repo_root: PathBuf,
    pub app_data: PathBuf,
}

impl HostConfig {
    #[must_use]
    pub fn profiles_root(&self) -> PathBuf {
        self.repo_root.join("profiles").join("games")
    }

    #[must_use]
    pub fn catalog_path(&self) -> PathBuf {
        self.repo_root
            .join("catalog")
            .join("v1")
            .join("catalog.json")
    }

    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.app_data.join("runtime").join("memory.sqlite3")
    }
}

#[derive(Clone)]
pub struct HostState {
    pub config: HostConfig,
    pub profiles: ProfileCorpus,
    pub catalog: CatalogDocument,
    pub catalog_trust: CatalogTrustState,
    pub memory: MemoryStore,
    pub vault: Arc<dyn CredentialVault>,
    pub tts_voice_discovery: Arc<TtsVoiceDiscoveryService>,
    pub children: ChildSupervisor,
    pub(crate) consumed_selected_stt_receipts: Arc<Mutex<VecDeque<String>>>,
}

impl HostState {
    pub async fn initialize(config: HostConfig) -> Result<Self, BootstrapError> {
        let vault = platform_vault()?;
        Self::initialize_with_resolved_vault(config, vault).await
    }

    /// Hermetic integration-test seam. It is absent from normal runtime builds
    /// so production callers cannot replace the platform-owned credential
    /// boundary.
    #[cfg(feature = "test-fixture-vault")]
    #[doc(hidden)]
    pub async fn initialize_with_test_vault(
        config: HostConfig,
        vault: Arc<dyn CredentialVault>,
    ) -> Result<Self, BootstrapError> {
        Self::initialize_with_resolved_vault(config, vault).await
    }

    async fn initialize_with_resolved_vault(
        config: HostConfig,
        vault: Arc<dyn CredentialVault>,
    ) -> Result<Self, BootstrapError> {
        ensure_private_app_data(&config.app_data)?;
        let profiles = ProfileCorpus::load(config.profiles_root())?;
        let (catalog, catalog_trust) = load_catalog_for_current_build(&config.catalog_path())?;
        let memory = MemoryStore::open(config.database_path())
            .await
            .map_err(|_| BootstrapError::Memory)?;
        let tts_voice_discovery = Arc::new(TtsVoiceDiscoveryService::hosted(Arc::clone(&vault)));
        Ok(Self {
            config,
            profiles,
            catalog,
            catalog_trust,
            memory,
            vault,
            tts_voice_discovery,
            children: ChildSupervisor::new(),
            consumed_selected_stt_receipts: Arc::new(Mutex::new(VecDeque::new())),
        })
    }
}

fn load_catalog_for_current_build(
    path: &std::path::Path,
) -> Result<(CatalogDocument, CatalogTrustState), BootstrapError> {
    match RuntimeBuildMode::current() {
        RuntimeBuildMode::Development => load_catalog_with_policy(
            path,
            RuntimeBuildMode::Development,
            &[],
            &RejectAllSignatures,
        ),
        RuntimeBuildMode::Release => {
            if RELEASE_TRUSTED_CATALOG_KEYS.is_empty() {
                return Err(BootstrapError::CatalogTrustRootUnavailable);
            }

            let verifier = Ed25519CatalogVerifier::new(
                RELEASE_TRUSTED_CATALOG_KEYS
                    .iter()
                    .map(|key| (key.key_id.to_owned(), key.public_key)),
            )
            .map_err(|_| BootstrapError::CatalogTrustRootUnavailable)?;
            let trusted_key_ids = RELEASE_TRUSTED_CATALOG_KEYS
                .iter()
                .map(|key| key.key_id)
                .collect::<Vec<_>>();
            load_catalog_with_policy(
                path,
                RuntimeBuildMode::Release,
                &trusted_key_ids,
                &ProviderCatalogVerifier { inner: &verifier },
            )
        }
    }
}

fn load_catalog_with_policy(
    path: &std::path::Path,
    mode: RuntimeBuildMode,
    trusted_key_ids: &[&str],
    verifier: &dyn SignatureVerifier,
) -> Result<(CatalogDocument, CatalogTrustState), BootstrapError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| BootstrapError::Catalog)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_CATALOG_BYTES
    {
        return Err(BootstrapError::Catalog);
    }
    let bytes = fs::read(path).map_err(|_| BootstrapError::Catalog)?;
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return Err(BootstrapError::Catalog);
    }

    let (policy, trust_state) = match mode {
        RuntimeBuildMode::Development => (
            TrustPolicy::DevelopmentAllowUnsigned,
            CatalogTrustState::DevelopmentUnsignedAllowed,
        ),
        RuntimeBuildMode::Release => (
            TrustPolicy::RequireSigned { trusted_key_ids },
            CatalogTrustState::ReleaseSignatureVerified,
        ),
    };
    let document =
        CatalogDocument::load_with_trust(&bytes, policy, verifier).map_err(map_catalog_error)?;
    Ok((document, trust_state))
}

fn map_catalog_error(error: CatalogError) -> BootstrapError {
    match error {
        CatalogError::SignatureRequired | CatalogError::SignatureInvalid => {
            BootstrapError::CatalogTrustVerification
        }
        CatalogError::Io(_) | CatalogError::Json(_) | CatalogError::Validation(_) => {
            BootstrapError::Catalog
        }
    }
}

struct RejectAllSignatures;

impl SignatureVerifier for RejectAllSignatures {
    fn verify(
        &self,
        _algorithm: SignatureAlgorithm,
        _key_id: &str,
        _message: &[u8],
        _signature_base64: &str,
    ) -> bool {
        false
    }
}

struct ProviderCatalogVerifier<'a> {
    inner: &'a Ed25519CatalogVerifier,
}

impl SignatureVerifier for ProviderCatalogVerifier<'_> {
    fn verify(
        &self,
        algorithm: SignatureAlgorithm,
        key_id: &str,
        message: &[u8],
        signature_base64: &str,
    ) -> bool {
        let algorithm = match algorithm {
            SignatureAlgorithm::Ed25519 => model_manager::ED25519_CATALOG_ALGORITHM,
        };
        self.inner
            .verify(key_id, algorithm, message, signature_base64)
    }
}

#[cfg(windows)]
fn ensure_private_app_data(path: &std::path::Path) -> Result<(), BootstrapError> {
    private_app_data_windows::ensure(path)
}

#[cfg(not(windows))]
fn ensure_private_app_data(path: &std::path::Path) -> Result<(), BootstrapError> {
    ensure_plain_directory(path)?;
    ensure_plain_directory(&path.join("runtime"))
}

#[cfg(not(windows))]
fn ensure_plain_directory(path: &std::path::Path) -> Result<(), BootstrapError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(BootstrapError::AppData);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| BootstrapError::AppData)?;
            let metadata = fs::symlink_metadata(path).map_err(|_| BootstrapError::AppData)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(BootstrapError::AppData);
            }
        }
        Err(_) => return Err(BootstrapError::AppData),
    }
    Ok(())
}

#[cfg(windows)]
mod private_app_data_windows {
    use std::{
        ffi::c_void,
        fs,
        mem::size_of,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
        path::Path,
        ptr::{null, null_mut},
    };

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, LocalFree, ERROR_INSUFFICIENT_BUFFER, HANDLE,
            INVALID_HANDLE_VALUE,
        },
        Security::{
            AclSizeInformation, AddAccessAllowedAceEx,
            Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
            CreateWellKnownSid, EqualSid, GetAce, GetAclInformation, GetLengthSid,
            GetSecurityDescriptorControl, GetTokenInformation, InitializeAcl, IsValidSid,
            TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE,
            ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION, CONTAINER_INHERIT_ACE,
            DACL_SECURITY_INFORMATION, INHERITED_ACE, OBJECT_INHERIT_ACE,
            PROTECTED_DACL_SECURITY_INFORMATION, PSID, SECURITY_MAX_SID_SIZE, SE_DACL_PROTECTED,
            TOKEN_QUERY, TOKEN_USER,
        },
        Storage::FileSystem::{
            CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ALL_ACCESS,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    use super::BootstrapError;

    const REQUIRED_ACE_FLAGS: u32 = OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;
    const ACCESS_ALLOWED_ACE_TYPE_VALUE: u8 = 0;

    pub(super) fn ensure(app_data: &Path) -> Result<(), BootstrapError> {
        ensure_directory_without_reparse(app_data)?;
        // Keep a no-delete-share handle open while resolving `runtime`. A concurrent process
        // cannot replace the checked app-data path with a junction between validation and use.
        let app_data_guard = DirectoryHandle::open_guard(app_data)?;
        app_data_guard.reject_reparse_point()?;
        let runtime = app_data.join("runtime");
        ensure_directory_without_reparse(&runtime)?;

        let directory = DirectoryHandle::open(&runtime)?;
        directory.reject_reparse_point()?;
        let allowed = AllowedSids::for_current_user()?;
        let acl = OwnedAcl::new(&allowed.as_slice())?;
        directory.apply_dacl(&acl)?;
        directory.verify_dacl(&allowed)
    }

    fn ensure_directory_without_reparse(path: &Path) -> Result<(), BootstrapError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => reject_unsafe_metadata(&metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(path).map_err(|_| BootstrapError::AppData)?;
                let metadata = fs::symlink_metadata(path).map_err(|_| BootstrapError::AppData)?;
                reject_unsafe_metadata(&metadata)
            }
            Err(_) => Err(BootstrapError::AppData),
        }
    }

    fn reject_unsafe_metadata(metadata: &fs::Metadata) -> Result<(), BootstrapError> {
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(BootstrapError::AppData);
        }
        Ok(())
    }

    struct DirectoryHandle(HANDLE);

    impl DirectoryHandle {
        fn open(path: &Path) -> Result<Self, BootstrapError> {
            Self::open_with_access(path, READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES)
        }

        fn open_guard(path: &Path) -> Result<Self, BootstrapError> {
            Self::open_with_access(path, FILE_READ_ATTRIBUTES)
        }

        fn open_with_access(path: &Path, desired_access: u32) -> Result<Self, BootstrapError> {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            // OPEN_REPARSE_POINT ensures a junction/symlink is opened as the reparse object
            // itself. It can therefore be rejected without traversing to its target.
            // SAFETY: `wide` is NUL-terminated and lives for the call; all optional pointers
            // are null, and the returned owned handle is closed by `Drop`.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    desired_access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(BootstrapError::AppData);
            }
            Ok(Self(handle))
        }

        fn reject_reparse_point(&self) -> Result<(), BootstrapError> {
            let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
            // SAFETY: `self.0` is a live directory handle and `info` is a correctly sized,
            // writable `FILE_ATTRIBUTE_TAG_INFO` for the duration of the call.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    self.0,
                    FileAttributeTagInfo,
                    (&raw mut info).cast(),
                    size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                )
            };
            if ok == 0 || info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(BootstrapError::AppData);
            }
            Ok(())
        }

        fn apply_dacl(&self, acl: &OwnedAcl) -> Result<(), BootstrapError> {
            self.apply_dacl_with_security_info(
                acl,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            )
        }

        fn apply_dacl_with_security_info(
            &self,
            acl: &OwnedAcl,
            security_information: u32,
        ) -> Result<(), BootstrapError> {
            // SAFETY: the handle is live, `acl` owns a valid initialized ACL for the call, and
            // null owner/group/SACL pointers mean those security descriptor fields are unchanged.
            let status = unsafe {
                SetSecurityInfo(
                    self.0,
                    SE_FILE_OBJECT,
                    security_information,
                    null_mut(),
                    null_mut(),
                    acl.as_ptr(),
                    null(),
                )
            };
            if status != 0 {
                return Err(BootstrapError::AppData);
            }
            Ok(())
        }

        fn verify_dacl(&self, allowed: &AllowedSids) -> Result<(), BootstrapError> {
            let descriptor = SecurityDescriptor::read(self.0)?;
            let snapshot = descriptor.dacl_snapshot()?;
            if !snapshot.protected || snapshot.entries.len() != allowed.as_slice().len() {
                return Err(BootstrapError::AppData);
            }

            let mut found = vec![false; allowed.as_slice().len()];
            for entry in &snapshot.entries {
                if entry.inherited
                    || entry.mask != FILE_ALL_ACCESS
                    || entry.flags != REQUIRED_ACE_FLAGS
                {
                    return Err(BootstrapError::AppData);
                }
                let Some(index) = allowed
                    .as_slice()
                    .iter()
                    .position(|candidate| candidate.equals_raw(entry.sid))
                else {
                    return Err(BootstrapError::AppData);
                };
                if found[index] {
                    return Err(BootstrapError::AppData);
                }
                found[index] = true;
            }
            if found.into_iter().all(|present| present) {
                Ok(())
            } else {
                Err(BootstrapError::AppData)
            }
        }
    }

    impl Drop for DirectoryHandle {
        fn drop(&mut self) {
            // SAFETY: `DirectoryHandle` exclusively owns this valid handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct OwnedSid {
        words: Vec<u32>,
        byte_len: u32,
    }

    impl OwnedSid {
        fn current_user() -> Result<Self, BootstrapError> {
            let mut token = null_mut();
            // SAFETY: the pseudo-process handle is always valid and `token` is writable.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
                return Err(BootstrapError::AppData);
            }
            let token = OwnedHandle(token);

            let mut needed = 0;
            // SAFETY: a null buffer with length zero is the documented size-query pattern.
            let first =
                unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &raw mut needed) };
            // SAFETY: immediately reads the calling thread's last-error value for the failed
            // size query above.
            if first != 0 || needed == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
                return Err(BootstrapError::AppData);
            }

            let word_count = (needed as usize).div_ceil(size_of::<usize>());
            let mut buffer = vec![0usize; word_count];
            // SAFETY: the aligned buffer has at least `needed` writable bytes and the token
            // remains live for the call.
            if unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &raw mut needed,
                )
            } == 0
            {
                return Err(BootstrapError::AppData);
            }
            // SAFETY: a successful `TokenUser` query initialized the buffer with TOKEN_USER.
            let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
            Self::copy_from_raw(user.User.Sid)
        }

        fn well_known(kind: i32) -> Result<Self, BootstrapError> {
            let mut words = vec![0u32; (SECURITY_MAX_SID_SIZE as usize).div_ceil(4)];
            let mut byte_len = SECURITY_MAX_SID_SIZE;
            // SAFETY: the aligned output buffer is `SECURITY_MAX_SID_SIZE` bytes and its length
            // pointer is valid; no domain SID is required for these well-known SIDs.
            if unsafe {
                CreateWellKnownSid(
                    kind,
                    null_mut(),
                    words.as_mut_ptr().cast(),
                    &raw mut byte_len,
                )
            } == 0
            {
                return Err(BootstrapError::AppData);
            }
            words.truncate((byte_len as usize).div_ceil(4));
            Ok(Self { words, byte_len })
        }

        fn copy_from_raw(sid: PSID) -> Result<Self, BootstrapError> {
            // SAFETY: the caller obtained `sid` from a successful Windows token query.
            if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
                return Err(BootstrapError::AppData);
            }
            // SAFETY: `IsValidSid` succeeded, so Windows may read the SID header and length.
            let byte_len = unsafe { GetLengthSid(sid) };
            if byte_len == 0 || byte_len > SECURITY_MAX_SID_SIZE {
                return Err(BootstrapError::AppData);
            }
            let mut words = vec![0u32; (byte_len as usize).div_ceil(4)];
            // SAFETY: the destination has at least `byte_len` bytes and the validated source SID
            // reports exactly that many readable bytes. The regions do not overlap.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    sid.cast::<u8>(),
                    words.as_mut_ptr().cast::<u8>(),
                    byte_len as usize,
                );
            }
            Ok(Self { words, byte_len })
        }

        fn as_ptr(&self) -> PSID {
            self.words.as_ptr().cast_mut().cast()
        }

        fn equals_raw(&self, other: PSID) -> bool {
            if other.is_null() {
                return false;
            }
            // SAFETY: callers obtain `other` from a parsed live ACL. Reject malformed SIDs before
            // comparing them; `self` owns a SID created or validated by Windows.
            unsafe { IsValidSid(other) != 0 && EqualSid(self.as_ptr(), other) != 0 }
        }
    }

    struct AllowedSids {
        current_user: OwnedSid,
        local_system: OwnedSid,
        administrators: OwnedSid,
    }

    impl AllowedSids {
        fn for_current_user() -> Result<Self, BootstrapError> {
            Ok(Self {
                current_user: OwnedSid::current_user()?,
                local_system: OwnedSid::well_known(WinLocalSystemSid)?,
                administrators: OwnedSid::well_known(WinBuiltinAdministratorsSid)?,
            })
        }

        fn as_slice(&self) -> [&OwnedSid; 3] {
            [&self.current_user, &self.local_system, &self.administrators]
        }
    }

    struct OwnedAcl {
        words: Vec<u32>,
    }

    impl OwnedAcl {
        fn new(sids: &[&OwnedSid]) -> Result<Self, BootstrapError> {
            let header_bytes = size_of::<ACL>();
            let ace_fixed_bytes = size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>();
            let byte_len = sids.iter().try_fold(header_bytes, |total, sid| {
                total.checked_add(ace_fixed_bytes + sid.byte_len as usize)
            });
            let Some(byte_len) = byte_len.filter(|length| *length <= u16::MAX as usize) else {
                return Err(BootstrapError::AppData);
            };
            let mut words = vec![0u32; byte_len.div_ceil(4)];
            let acl = words.as_mut_ptr().cast::<ACL>();
            // SAFETY: `words` is aligned and provides `byte_len` writable bytes.
            if unsafe { InitializeAcl(acl, byte_len as u32, ACL_REVISION) } == 0 {
                return Err(BootstrapError::AppData);
            }
            for sid in sids {
                // SAFETY: `acl` remains initialized with sufficient precomputed capacity and
                // every `sid` owns a valid SID for the duration of the call.
                if unsafe {
                    AddAccessAllowedAceEx(
                        acl,
                        ACL_REVISION,
                        REQUIRED_ACE_FLAGS,
                        FILE_ALL_ACCESS,
                        sid.as_ptr(),
                    )
                } == 0
                {
                    return Err(BootstrapError::AppData);
                }
            }
            Ok(Self { words })
        }

        fn as_ptr(&self) -> *const ACL {
            self.words.as_ptr().cast()
        }
    }

    struct SecurityDescriptor(*mut c_void);

    impl SecurityDescriptor {
        fn read(handle: HANDLE) -> Result<Self, BootstrapError> {
            let mut descriptor = null_mut();
            // SAFETY: `handle` is live, all omitted component outputs are null, and Windows
            // allocates the returned descriptor for release with `LocalFree`.
            let status = unsafe {
                GetSecurityInfo(
                    handle,
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &raw mut descriptor,
                )
            };
            if status != 0 || descriptor.is_null() {
                return Err(BootstrapError::AppData);
            }
            Ok(Self(descriptor))
        }

        fn dacl_snapshot(&self) -> Result<DaclSnapshot, BootstrapError> {
            self.read_dacl_from_descriptor()
        }

        fn read_dacl_from_descriptor(&self) -> Result<DaclSnapshot, BootstrapError> {
            use windows_sys::Win32::Security::GetSecurityDescriptorDacl;

            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = null_mut();
            // SAFETY: `self.0` is a live security descriptor returned by `GetSecurityInfo`, and
            // all three output locations are writable for the call.
            if unsafe {
                GetSecurityDescriptorDacl(
                    self.0,
                    &raw mut present,
                    &raw mut dacl,
                    &raw mut defaulted,
                )
            } == 0
                || present == 0
                || dacl.is_null()
            {
                return Err(BootstrapError::AppData);
            }

            let mut control = 0;
            let mut revision = 0;
            // SAFETY: the descriptor is live and both scalar output pointers are writable.
            if unsafe { GetSecurityDescriptorControl(self.0, &raw mut control, &raw mut revision) }
                == 0
            {
                return Err(BootstrapError::AppData);
            }

            let mut size_info = ACL_SIZE_INFORMATION::default();
            // SAFETY: `dacl` was returned from the live descriptor and `size_info` is a correctly
            // sized writable output structure.
            if unsafe {
                GetAclInformation(
                    dacl,
                    (&raw mut size_info).cast(),
                    size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
            } == 0
            {
                return Err(BootstrapError::AppData);
            }

            let mut entries = Vec::with_capacity(size_info.AceCount as usize);
            for index in 0..size_info.AceCount {
                let mut raw_ace = null_mut();
                // SAFETY: `index` is bounded by the ACL's reported ACE count and the output
                // pointer is writable.
                if unsafe { GetAce(dacl, index, &raw mut raw_ace) } == 0 || raw_ace.is_null() {
                    return Err(BootstrapError::AppData);
                }
                // SAFETY: successful `GetAce` returns a pointer to an ACE beginning with a header.
                let header = unsafe { &*raw_ace.cast::<ACE_HEADER>() };
                if header.AceType != ACCESS_ALLOWED_ACE_TYPE_VALUE {
                    return Err(BootstrapError::AppData);
                }
                // SAFETY: the ACE type was verified as `ACCESS_ALLOWED_ACE_TYPE`.
                let ace = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
                entries.push(DaclEntry {
                    sid: (&raw const ace.SidStart).cast_mut().cast(),
                    mask: ace.Mask,
                    flags: u32::from(header.AceFlags),
                    inherited: u32::from(header.AceFlags) & INHERITED_ACE != 0,
                });
            }

            Ok(DaclSnapshot {
                protected: control & SE_DACL_PROTECTED != 0,
                entries,
            })
        }
    }

    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            // SAFETY: the descriptor was allocated by `GetSecurityInfo` and is owned here.
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    struct DaclEntry {
        sid: PSID,
        mask: u32,
        flags: u32,
        inherited: bool,
    }

    struct DaclSnapshot {
        protected: bool,
        entries: Vec<DaclEntry>,
    }

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: `OwnedHandle` exclusively owns this valid token handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{fs, process::Command};

        use windows_sys::Win32::Security::{WinWorldSid, UNPROTECTED_DACL_SECURITY_INFORMATION};

        use super::*;

        #[test]
        fn replaces_broad_inherited_acl_with_exact_protected_dacl() {
            let temp = tempfile::tempdir().expect("temporary directory");
            let app_data = temp.path().join("app-data");
            fs::create_dir(&app_data).expect("app-data directory");

            let allowed = AllowedSids::for_current_user().expect("allowed SIDs");
            let everyone = OwnedSid::well_known(WinWorldSid).expect("Everyone SID");
            let broad_acl = OwnedAcl::new(&[
                &allowed.current_user,
                &allowed.local_system,
                &allowed.administrators,
                &everyone,
            ])
            .expect("broad test ACL");
            let app_data_handle = DirectoryHandle::open(&app_data).expect("open app-data");
            app_data_handle
                .apply_dacl_with_security_info(
                    &broad_acl,
                    DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                )
                .expect("set broad parent ACL");

            let runtime = app_data.join("runtime");
            fs::create_dir(&runtime).expect("runtime directory");
            let runtime_handle = DirectoryHandle::open(&runtime).expect("open runtime");
            let before_descriptor =
                SecurityDescriptor::read(runtime_handle.0).expect("read inherited ACL");
            let before = before_descriptor
                .dacl_snapshot()
                .expect("inspect inherited ACL");
            assert!(
                before
                    .entries
                    .iter()
                    .any(|entry| entry.inherited && everyone.equals_raw(entry.sid)),
                "test setup must inherit an Everyone grant"
            );

            ensure(&app_data).expect("harden runtime directory");

            let hardened_handle = DirectoryHandle::open(&runtime).expect("reopen runtime");
            hardened_handle
                .verify_dacl(&allowed)
                .expect("exact private DACL");
            let after_descriptor =
                SecurityDescriptor::read(hardened_handle.0).expect("read hardened ACL");
            let after = after_descriptor
                .dacl_snapshot()
                .expect("inspect hardened ACL");
            assert!(after.protected, "DACL inheritance must be disabled");
            assert!(after.entries.iter().all(|entry| !entry.inherited));
            assert!(
                after
                    .entries
                    .iter()
                    .all(|entry| !everyone.equals_raw(entry.sid)),
                "Everyone must not retain access"
            );
        }

        #[test]
        fn rejects_runtime_directory_junction_without_following_it() {
            let temp = tempfile::tempdir().expect("temporary directory");
            let app_data = temp.path().join("app-data");
            let target = temp.path().join("junction-target");
            fs::create_dir(&app_data).expect("app-data directory");
            fs::create_dir(&target).expect("junction target");
            let runtime = app_data.join("runtime");

            let status = Command::new("cmd")
                .args(["/d", "/c", "mklink", "/j"])
                .arg(&runtime)
                .arg(&target)
                .status()
                .expect("run mklink");
            assert!(status.success(), "create test directory junction");

            assert!(matches!(ensure(&app_data), Err(BootstrapError::AppData)));
            assert!(target.is_dir(), "junction target must remain untouched");
        }
    }
}

#[cfg(windows)]
fn platform_vault() -> Result<Arc<dyn CredentialVault>, BootstrapError> {
    WindowsCredentialVault::new("interactive-npcs/v2")
        .map(|vault| Arc::new(vault) as Arc<dyn CredentialVault>)
        .map_err(|_| BootstrapError::CredentialVault)
}

#[cfg(not(windows))]
fn platform_vault() -> Result<Arc<dyn CredentialVault>, BootstrapError> {
    // Portable CI never persists or resolves live provider credentials.
    Ok(Arc::new(MemoryCredentialVault::default()))
}

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("application data directory is unavailable or unsafe")]
    AppData,
    #[error("game profile corpus could not be loaded: {0}")]
    Profiles(#[from] crate::profiles::ProfileCorpusError),
    #[error("provider catalog is missing, oversized, or invalid")]
    Catalog,
    #[error("release catalog trust root has not been provisioned")]
    CatalogTrustRootUnavailable,
    #[error("provider catalog is not signed by a trusted release key")]
    CatalogTrustVerification,
    #[error("SQLite memory initialization failed")]
    Memory,
    #[error("credential vault initialization failed")]
    CredentialVault,
}

#[cfg(test)]
mod catalog_trust_tests {
    use std::path::PathBuf;

    use npc_provider_catalog::DetachedSignature;

    use super::*;

    fn catalog_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog/v1/catalog.json")
    }

    #[test]
    fn development_mode_accepts_unsigned_catalog_and_records_that_exception() {
        let (document, trust) = load_catalog_with_policy(
            &catalog_path(),
            RuntimeBuildMode::Development,
            &[],
            &RejectAllSignatures,
        )
        .expect("development catalog should remain usable for local review");

        assert!(!document.content.providers.is_empty());
        assert_eq!(trust, CatalogTrustState::DevelopmentUnsignedAllowed);
    }

    #[test]
    fn release_mode_rejects_the_unsigned_development_catalog() {
        let result = load_catalog_with_policy(
            &catalog_path(),
            RuntimeBuildMode::Release,
            &["fixture-release-key"],
            &RejectAllSignatures,
        );

        assert!(matches!(
            result,
            Err(BootstrapError::CatalogTrustVerification)
        ));
    }

    #[test]
    fn release_mode_accepts_only_a_signature_confirmed_by_the_injected_verifier() {
        let temp = tempfile::tempdir().expect("temporary catalog root");
        let path = temp.path().join("catalog.json");
        let mut document = CatalogDocument::load(catalog_path()).expect("fixture catalog");
        document.signatures.push(DetachedSignature {
            key_id: "fixture-release-key".to_owned(),
            algorithm: SignatureAlgorithm::Ed25519,
            signature_base64: "fixture-signature".to_owned(),
        });
        fs::write(
            &path,
            serde_json::to_vec(&document).expect("serialize fixture catalog"),
        )
        .expect("write fixture catalog");

        let (loaded, trust) = load_catalog_with_policy(
            &path,
            RuntimeBuildMode::Release,
            &["fixture-release-key"],
            &FixtureVerifier,
        )
        .expect("trusted fixture signature");

        assert_eq!(loaded.catalog_revision, document.catalog_revision);
        assert_eq!(trust, CatalogTrustState::ReleaseSignatureVerified);
    }

    struct FixtureVerifier;

    impl SignatureVerifier for FixtureVerifier {
        fn verify(
            &self,
            algorithm: SignatureAlgorithm,
            key_id: &str,
            _message: &[u8],
            signature_base64: &str,
        ) -> bool {
            algorithm == SignatureAlgorithm::Ed25519
                && key_id == "fixture-release-key"
                && signature_base64 == "fixture-signature"
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use std::{fs, os::unix::fs::symlink};

    use super::*;

    #[test]
    fn creates_runtime_directory_deterministically() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let app_data = temp.path().join("app-data");

        ensure_private_app_data(&app_data).expect("create private app-data");
        assert!(app_data.join("runtime").is_dir());
        ensure_private_app_data(&app_data).expect("existing directory remains valid");
    }

    #[test]
    fn rejects_runtime_symlink() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let app_data = temp.path().join("app-data");
        let target = temp.path().join("target");
        fs::create_dir(&app_data).expect("app-data directory");
        fs::create_dir(&target).expect("target directory");
        symlink(&target, app_data.join("runtime")).expect("runtime symlink");

        assert!(matches!(
            ensure_private_app_data(&app_data),
            Err(BootstrapError::AppData)
        ));
    }
}
