//! Native-only installed-candidate privacy probe receipts.
//!
//! Environment variables select a bounded runner-owned output location, but
//! they never assert success. A receipt is written only after a native product
//! policy operation proves the requested scenario completed without starting
//! a provider request.

use crate::local_resources::LocalResourceManager;
use crate::provider_loadouts::ProviderLoadoutManager;
use npc_provider_loadouts::{EgressClassV1, LoadoutContextV1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const RUN_ENV: &str = "NPC2_PRIVACY_PROOF_RUN_ID";
const SCENARIO_ENV: &str = "NPC2_PRIVACY_PROOF_SCENARIO";
const RECEIPT_ENV: &str = "NPC2_PRIVACY_PROOF_RECEIPT_PATH";
const OFFLINE_MODE: &str = "offline_mode";
const LOCAL_LIP_SYNC: &str = "local_lip_sync";

#[derive(Debug, Clone)]
struct ProbeRequest {
    run_id: String,
    scenario: String,
    receipt_path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagedPrivacyScenarioReceiptV1 {
    schema_version: &'static str,
    source: &'static str,
    run_id: String,
    scenario: String,
    process_id: u32,
    executable_sha256: String,
    observed_at_utc: String,
    scenario_completed: bool,
    provider_requests_started: u64,
}

pub(crate) fn start_probe(
    provider_loadouts: &ProviderLoadoutManager,
    local_resources: &LocalResourceManager,
) {
    let Some(request) = request_from_environment() else {
        return;
    };

    if !probe_authorized(&request, provider_loadouts, local_resources) {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let _ = write_receipt(request);
    });
}

fn probe_authorized(
    request: &ProbeRequest,
    provider_loadouts: &ProviderLoadoutManager,
    local_resources: &LocalResourceManager,
) -> bool {
    // The current product has no admitted complete lip-sync model/runtime
    // operation. In particular, the landmark signal worker is not lip-sync.
    // Therefore this scenario intentionally emits no receipt and the installed
    // proof runner remains fail-closed until that real operation exists.
    if request.scenario == LOCAL_LIP_SYNC {
        return false;
    }
    if request.scenario != OFFLINE_MODE || validate_receipt_target(request).is_err() {
        return false;
    }

    let Ok(review) = provider_loadouts.review(LoadoutContextV1::global(), true, local_resources)
    else {
        return false;
    };
    review.offline
        && !review.network_request_performed
        && !review.resolved.roles.is_empty()
        && review
            .resolved
            .roles
            .values()
            .all(|routes| routes.primary.disclosure.egress == EgressClassV1::None)
}

fn request_from_environment() -> Option<ProbeRequest> {
    let run_id = std::env::var(RUN_ENV).ok()?;
    let scenario = std::env::var(SCENARIO_ENV).ok()?;
    let receipt_path = PathBuf::from(std::env::var_os(RECEIPT_ENV)?);
    Some(ProbeRequest {
        run_id,
        scenario,
        receipt_path,
    })
}

fn validate_receipt_target(request: &ProbeRequest) -> Result<(), ()> {
    if !valid_id(&request.run_id, 128)
        || !matches!(request.scenario.as_str(), OFFLINE_MODE | LOCAL_LIP_SYNC)
        || !request.receipt_path.is_absolute()
        || request.receipt_path.exists()
        || request
            .receipt_path
            .file_name()
            .and_then(|value| value.to_str())
            != Some(format!("{}.json", request.run_id).as_str())
    {
        return Err(());
    }
    let parent = request.receipt_path.parent().ok_or(())?;
    let parent_name = parent
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(())?;
    if !parent_name.starts_with("npc-privacy-") || !valid_id(parent_name, 96) {
        return Err(());
    }
    let canonical_parent = fs::canonicalize(parent).map_err(|_| ())?;
    let canonical_temp = fs::canonicalize(std::env::temp_dir()).map_err(|_| ())?;
    if canonical_parent.parent() != Some(canonical_temp.as_path()) {
        return Err(());
    }
    let metadata = fs::symlink_metadata(&canonical_parent).map_err(|_| ())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(());
        }
    }
    Ok(())
}

fn write_receipt(request: ProbeRequest) -> Result<(), ()> {
    validate_receipt_target(&request)?;
    let executable = std::env::current_exe().map_err(|_| ())?;
    let executable_sha256 = sha256_file(&executable)?;
    let receipt = PackagedPrivacyScenarioReceiptV1 {
        schema_version: "1.0.0",
        source: "packaged_executable",
        run_id: request.run_id,
        scenario: request.scenario,
        process_id: std::process::id(),
        executable_sha256,
        observed_at_utc: OffsetDateTime::now_utc().format(&Rfc3339).map_err(|_| ())?,
        scenario_completed: true,
        provider_requests_started: 0,
    };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|_| ())?;
    if bytes.len() > 64 * 1024 {
        return Err(());
    }
    let parent = request.receipt_path.parent().ok_or(())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| ())?;
    temporary.write_all(&bytes).map_err(|_| ())?;
    temporary.as_file().sync_all().map_err(|_| ())?;
    temporary
        .persist_noclobber(&request.receipt_path)
        .map_err(|_| ())?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, ()> {
    let mut file = File::open(path).map_err(|_| ())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn valid_id(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(scenario: &str) -> (tempfile::TempDir, ProbeRequest) {
        let directory = tempfile::Builder::new()
            .prefix("npc-privacy-")
            .tempdir_in(std::env::temp_dir())
            .expect("runner directory");
        let run_id = format!("privacy-{}-0123456789abcdef", scenario.replace('_', "-"));
        let request = ProbeRequest {
            receipt_path: directory.path().join(format!("{run_id}.json")),
            run_id,
            scenario: scenario.into(),
        };
        (directory, request)
    }

    #[test]
    fn runner_target_is_fresh_bounded_and_directly_under_temp() {
        let (_directory, request) = request(OFFLINE_MODE);
        assert!(validate_receipt_target(&request).is_ok());

        let mut escaped = request.clone();
        escaped.receipt_path = std::env::temp_dir().join("escaped.json");
        assert!(validate_receipt_target(&escaped).is_err());
        let mut unknown = request;
        unknown.scenario = "fixture".into();
        assert!(validate_receipt_target(&unknown).is_err());
    }

    #[test]
    fn local_lip_sync_cannot_authorize_a_receipt_before_a_complete_runtime_exists() {
        let config = tempfile::tempdir().expect("private config");
        let providers = ProviderLoadoutManager::new(config.path().join("providers"));
        let resources = LocalResourceManager::new(config.path(), None).expect("local resources");
        let (_receipt_directory, request) = request(LOCAL_LIP_SYNC);

        assert!(!probe_authorized(&request, &providers, &resources));
        assert!(!request.receipt_path.exists());
    }

    #[test]
    fn api_first_hosted_routes_cannot_masquerade_as_offline_route_truth() {
        let config = tempfile::tempdir().expect("private config");
        let providers = ProviderLoadoutManager::new(config.path().join("providers"));
        let resources = LocalResourceManager::new(config.path(), None).expect("local resources");
        let (_receipt_directory, request) = request(OFFLINE_MODE);

        assert!(!probe_authorized(&request, &providers, &resources));
        assert!(!request.receipt_path.exists());
    }
}
