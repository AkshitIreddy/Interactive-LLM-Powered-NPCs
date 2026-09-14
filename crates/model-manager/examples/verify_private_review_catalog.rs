use model_manager::{
    load_release_catalog_bundle_v1, verify_release_catalog_bundle_v1,
    verify_release_envelope_files_v1, verify_release_manifest_files_v1, CatalogTrustState,
    ReleaseCatalogTrustScopeV1, Sha256Digest,
};
use serde::Serialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

const PACK_ID: &str = "openseeface-yunet640-lm1-mouth-signal";
const PACK_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
const MANIFEST_FILE: &str = "openseeface-yunet640-lm1-mouth-signal.json";
const RECEIPT_SCHEMA: &str = "interactive-npcs-private-catalog-verification/v1";
const IMPORT_CONTEXT_SCHEMA: &str = "npc.review-envelope-import-context/v1";

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeFile {
    path: String,
    sha256: String,
}

#[allow(clippy::print_stdout)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: verify_private_review_catalog <runtime-root> <receipt> <import-context> [now-unix-seconds]"
                .into(),
        );
    }
    let runtime_root = PathBuf::from(&args[0]);
    let receipt_path = PathBuf::from(&args[1]);
    let import_context_path = PathBuf::from(&args[2]);
    let now = if args.len() == 4 {
        args[3].parse::<u64>()?
    } else {
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
    };

    let result = verify(&runtime_root, &receipt_path, &import_context_path, now)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn verify(
    runtime_root: &Path,
    receipt_path: &Path,
    import_context_path: &Path,
    now: u64,
) -> Result<Value, Box<dyn std::error::Error>> {
    let (root, bundle) = load_release_catalog_bundle_v1(runtime_root)?;
    if root.trust_scope != ReleaseCatalogTrustScopeV1::AutomatedLocalReviewBootstrap
        || root.production_trust
        || !root.rotation_required_before_release
        || root.promotion_supported
        || root.publication_supported
        || root.signature_threshold != 2
        || root.keys.len() != 2
    {
        return Err("private catalog violates the automated local-review trust boundary".into());
    }
    let verified =
        verify_release_catalog_bundle_v1(&root, &bundle, &CatalogTrustState::default(), now)?;
    verify_release_manifest_files_v1(runtime_root, &verified)?;
    verify_release_envelope_files_v1(runtime_root, &verified, now)?;

    let snapshots = verified.trusted_pack_snapshots()?;
    let snapshot = snapshots
        .first()
        .filter(|_| snapshots.len() == 1)
        .ok_or("private catalog must expose exactly one trusted pack snapshot")?;
    if snapshot.identity.pack_id.as_str() != PACK_ID
        || snapshot.identity.revision.as_str() != PACK_REVISION
        || snapshot.qualified_envelope_count != 1
    {
        return Err("private catalog pack identity or qualified-envelope count changed".into());
    }
    let source = verified
        .source_inventory
        .manifests
        .first()
        .filter(|_| verified.source_inventory.manifests.len() == 1)
        .ok_or("private catalog source inventory must contain exactly one manifest")?;
    if source.file_name != MANIFEST_FILE || source.qualified_envelopes.len() != 1 {
        return Err(
            "private catalog source inventory is not the reviewed YuNet closed world".into(),
        );
    }
    let envelope = &source.qualified_envelopes[0];
    let envelope_relative = format!("qual/{}.json", envelope.envelope_sha256.as_str());
    let expected_runtime = [
        "model-catalog-root-v1.json".to_owned(),
        "model-catalog-v1.json".to_owned(),
        MANIFEST_FILE.to_owned(),
        envelope_relative,
    ];
    let actual_runtime = regular_closed_world_files(runtime_root)?;
    if actual_runtime != expected_runtime.into_iter().collect::<Vec<_>>() {
        return Err("private catalog runtime root is not the exact four-file closed world".into());
    }

    let runtime_files = actual_runtime
        .iter()
        .map(|relative| {
            let bytes = read_bounded_regular(&runtime_root.join(relative))?;
            Ok(RuntimeFile {
                path: relative.clone(),
                sha256: Sha256Digest::of_bytes(&bytes).to_string(),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;

    let context_bytes = read_bounded_regular(import_context_path)?;
    let context: Value = serde_json::from_slice(&context_bytes)?;
    let native_report_sha256 = required_string(&context, "/native_qualification_report_sha256")?;
    if required_string(&context, "/schema")? != IMPORT_CONTEXT_SCHEMA
        || required_string(&context, "/identity/pack_id")? != PACK_ID
        || required_string(&context, "/identity/revision")? != PACK_REVISION
        || required_string(&context, "/device_fingerprint_sha256")?
            != envelope.device_fingerprint_sha256.as_str()
        || context
            .pointer("/native_inference_qualification_validated")
            .and_then(Value::as_bool)
            != Some(true)
        || context
            .pointer("/provider_load_self_test_attested")
            .and_then(Value::as_bool)
            != Some(false)
        || context
            .pointer("/production_trust")
            .and_then(Value::as_bool)
            != Some(false)
        || context
            .pointer("/rotation_required_before_release")
            .and_then(Value::as_bool)
            != Some(true)
        || context
            .pointer("/promotion_supported")
            .and_then(Value::as_bool)
            != Some(false)
        || context
            .pointer("/publication_supported")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err("private catalog import context violates its review boundary".into());
    }

    let receipt_bytes = read_bounded_regular(receipt_path)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    let recorded_runtime = receipt
        .pointer("/runtimeFiles")
        .and_then(Value::as_array)
        .ok_or("private catalog receipt runtimeFiles are absent")?;
    let recorded_runtime = recorded_runtime
        .iter()
        .map(|entry| {
            Ok(RuntimeFile {
                path: required_string(entry, "/path")?.to_owned(),
                sha256: required_string(entry, "/sha256")?.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    if recorded_runtime != runtime_files
        || required_string(&receipt, "/schema")? != RECEIPT_SCHEMA
        || receipt
            .pointer("/verification/catalogBundlePublicApi")
            .and_then(Value::as_bool)
            != Some(true)
        || receipt
            .pointer("/verification/manifestFilesPublicApi")
            .and_then(Value::as_bool)
            != Some(true)
        || receipt
            .pointer("/verification/qualifiedEnvelopeFilesPublicApi")
            .and_then(Value::as_bool)
            != Some(true)
        || receipt
            .pointer("/verification/trustedPackSnapshotPublicApi")
            .and_then(Value::as_bool)
            != Some(true)
        || receipt.pointer("/catalog/version").and_then(Value::as_u64)
            != Some(bundle.signed.version)
        || required_string(&receipt, "/catalog/trustScope")? != "automated_local_review_bootstrap"
        || receipt
            .pointer("/catalog/signatureThreshold")
            .and_then(Value::as_u64)
            != Some(2)
        || receipt.pointer("/catalog/keyCount").and_then(Value::as_u64) != Some(2)
        || receipt
            .pointer("/catalog/productionTrust")
            .and_then(Value::as_bool)
            != Some(false)
        || receipt
            .pointer("/catalog/rotationRequiredBeforeRelease")
            .and_then(Value::as_bool)
            != Some(true)
        || receipt
            .pointer("/catalog/promotionSupported")
            .and_then(Value::as_bool)
            != Some(false)
        || receipt
            .pointer("/catalog/publicationSupported")
            .and_then(Value::as_bool)
            != Some(false)
        || required_string(&receipt, "/pack/id")? != PACK_ID
        || required_string(&receipt, "/pack/revision")? != PACK_REVISION
        || receipt
            .pointer("/pack/qualifiedEnvelopeCount")
            .and_then(Value::as_u64)
            != Some(1)
        || receipt
            .pointer("/pack/nativeInferenceQualified")
            .and_then(Value::as_bool)
            != Some(true)
        || receipt
            .pointer("/pack/providerLoadSelfTestAttested")
            .and_then(Value::as_bool)
            != Some(false)
        || receipt.pointer("/pack/installed").and_then(Value::as_bool) != Some(false)
        || receipt.pointer("/pack/activated").and_then(Value::as_bool) != Some(false)
        || required_string(&receipt, "/auditEvidence/sha256")?
            != Sha256Digest::of_bytes(&context_bytes).as_str()
        || required_string(&receipt, "/auditEvidence/nativeReportSha256")? != native_report_sha256
        || receipt
            .pointer("/signerPrivateMaterialPersisted")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err(
            "private catalog receipt disagrees with the cryptographically verified files".into(),
        );
    }

    Ok(serde_json::json!({
        "schema": "interactive-npcs-private-catalog-live-verification/v1",
        "status": "passed",
        "verifiedAtUnixSeconds": now,
        "catalogVersion": bundle.signed.version,
        "catalogExpiresUnixSeconds": bundle.signed.expires_unix_seconds,
        "qualifiedEnvelopeExpiresUnixSeconds": read_envelope_expiry(runtime_root, envelope)?,
        "packId": PACK_ID,
        "packRevision": PACK_REVISION,
        "runtimeFiles": runtime_files,
        "receiptSha256": Sha256Digest::of_bytes(&receipt_bytes),
        "importContextSha256": Sha256Digest::of_bytes(&context_bytes),
        "productionTrust": false,
        "rotationRequiredBeforeRelease": true,
        "promotionSupported": false,
        "publicationSupported": false,
        "signerPrivateMaterialPersisted": false
    }))
}

fn read_envelope_expiry(
    runtime_root: &Path,
    binding: &model_manager::QualifiedEnvelopeBindingV1,
) -> Result<u64, Box<dyn std::error::Error>> {
    let path = runtime_root
        .join("qual")
        .join(format!("{}.json", binding.envelope_sha256.as_str()));
    let value: Value = serde_json::from_slice(&read_bounded_regular(&path)?)?;
    value
        .pointer("/signed/expires_unix_seconds")
        .and_then(Value::as_u64)
        .ok_or_else(|| "qualified envelope expiry is absent".into())
}

fn regular_closed_world_files(root: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        if entry.path() == root {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(format!(
                "private catalog contains a link or special file: {}",
                entry.path().display()
            )
            .into());
        }
        if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            // Source review directories keep their receipt and immutable
            // import context below evidence/. Portable packages keep those
            // two files in review-evidence and pass them explicitly.
            if !relative.starts_with("evidence/") {
                files.push(relative);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn read_bounded_regular(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 8 * 1024 * 1024
    {
        return Err(format!(
            "review metadata is not a bounded regular file: {}",
            path.display()
        )
        .into());
    }
    Ok(fs::read(path)?)
}

fn required_string<'a>(
    value: &'a Value,
    pointer: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("required string is absent: {pointer}").into())
}
