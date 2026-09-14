use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use model_manager::{
    build_release_catalog_bundle_v1, canonical_measured_resource_envelope_bytes,
    load_release_catalog_bundle_v1, parse_and_normalize_model_pack_manifest,
    verify_release_catalog_bundle_v1, verify_release_envelope_files_v1,
    verify_release_manifest_files_v1, CatalogSignatureV1, CatalogTrustState,
    QualifiedEnvelopeBindingV1, ReleaseCatalogSignerV1, ReleaseCatalogTrustScopeV1,
    ReleaseManifestSourceV1, Sha256Digest, SignedMeasuredResourceEnvelopeV1,
    ED25519_CATALOG_ALGORITHM,
};
use serde::Serialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

const PACK_ID: &str = "openseeface-yunet640-lm1-mouth-signal";
const PACK_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
const MANIFEST_FILE: &str = "openseeface-yunet640-lm1-mouth-signal.json";
const MAX_CATALOG_LIFETIME_SECONDS: u64 = 14 * 24 * 60 * 60;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeFile {
    path: String,
    sha256: String,
}

#[allow(clippy::print_stdout)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "usage: refresh_private_review_catalog <source-dir> <output-dir> <version> <generated-unix-seconds> <expires-unix-seconds>"
                .into(),
        );
    }
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let version = args[2].parse::<u64>()?;
    let generated = args[3].parse::<u64>()?;
    let expires = args[4].parse::<u64>()?;
    let system_now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if generated.abs_diff(system_now) > 300 {
        return Err(
            "catalog generation time must be within five minutes of this machine clock".into(),
        );
    }
    if generated >= expires || expires - generated > MAX_CATALOG_LIFETIME_SECONDS {
        return Err(
            "refreshed catalog must use a positive validity window of at most 14 days".into(),
        );
    }
    if output.exists() {
        return Err(format!("output directory already exists: {}", output.display()).into());
    }

    let (old_root, old_bundle) = load_release_catalog_bundle_v1(&source)?;
    if old_root.trust_scope != ReleaseCatalogTrustScopeV1::AutomatedLocalReviewBootstrap
        || old_root.production_trust
        || !old_root.rotation_required_before_release
        || old_root.promotion_supported
        || old_root.publication_supported
        || old_root.signature_threshold != 2
        || old_root.keys.len() != 2
        || old_bundle.signed.version >= version
    {
        return Err("source is not an older two-of-two automated local-review catalog".into());
    }
    let historical_verification_time = old_bundle
        .signed
        .expires_unix_seconds
        .checked_sub(1)
        .ok_or("source catalog expiry is invalid")?;
    let old_verified = verify_release_catalog_bundle_v1(
        &old_root,
        &old_bundle,
        &CatalogTrustState::default(),
        historical_verification_time,
    )?;
    verify_release_manifest_files_v1(&source, &old_verified)?;
    // The catalog itself may have expired. The measured envelope must still be
    // current now; this rotates metadata and keys without inventing a new run.
    verify_release_envelope_files_v1(&source, &old_verified, generated)?;

    let old_binding = old_verified
        .source_inventory
        .manifests
        .first()
        .filter(|_| old_verified.source_inventory.manifests.len() == 1)
        .ok_or("source catalog must contain exactly one manifest")?;
    if old_binding.identity.pack_id.as_str() != PACK_ID
        || old_binding.identity.revision.as_str() != PACK_REVISION
        || old_binding.file_name != MANIFEST_FILE
        || old_binding.qualified_envelopes.len() != 1
    {
        return Err("source catalog is not the reviewed YuNet catalog".into());
    }
    let old_envelope_binding = &old_binding.qualified_envelopes[0];
    let old_envelope_path = source.join("qual").join(format!(
        "{}.json",
        old_envelope_binding.envelope_sha256.as_str()
    ));
    let old_envelope_bytes = read_bounded_regular(&old_envelope_path)?;
    let old_envelope: SignedMeasuredResourceEnvelopeV1 =
        serde_json::from_slice(&old_envelope_bytes)?;
    if old_envelope.signed.expires_unix_seconds < expires {
        return Err("catalog expiry cannot outlive the measured resource envelope".into());
    }

    let manifest_path = source.join(MANIFEST_FILE);
    let manifest_bytes = read_bounded_regular(&manifest_path)?;
    let normalized = parse_and_normalize_model_pack_manifest(&manifest_bytes)?;
    if normalized.core_manifest.identity() != old_binding.identity
        || Sha256Digest::of_bytes(&manifest_bytes) != old_binding.raw_document_sha256
        || normalized.canonical_document_sha256 != old_binding.canonical_document_sha256
        || normalized.core_manifest.digest()? != old_binding.normalized_manifest_sha256
    {
        return Err("source manifest changed from the signed inventory".into());
    }

    let import_context_path = source.join("evidence/yunet-import-context.json");
    let import_context_bytes = read_bounded_regular(&import_context_path)?;
    let import_context: Value = serde_json::from_slice(&import_context_bytes)?;
    validate_import_context(&import_context, &old_envelope)?;

    let signers = fresh_ephemeral_signers(version)?;
    let envelope_payload_bytes = canonical_measured_resource_envelope_bytes(&old_envelope.signed)?;
    let new_envelope = SignedMeasuredResourceEnvelopeV1 {
        signed: old_envelope.signed.clone(),
        signatures: signers
            .iter()
            .map(|signer| CatalogSignatureV1 {
                key_id: signer.key_id.clone(),
                algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
                signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(signer.signing_key.sign(&envelope_payload_bytes).to_bytes()),
            })
            .collect(),
    };
    let new_envelope_bytes = pretty_json(&new_envelope)?;
    let new_envelope_digest = Sha256Digest::of_bytes(&new_envelope_bytes);
    let source_entry = ReleaseManifestSourceV1 {
        file_name: MANIFEST_FILE.to_owned(),
        raw_document_sha256: Sha256Digest::of_bytes(&manifest_bytes),
        normalized,
        review_evidence: old_binding.review_evidence.clone(),
        qualified_envelopes: vec![QualifiedEnvelopeBindingV1 {
            report_id: new_envelope.signed.report_id.clone(),
            device_fingerprint_sha256: new_envelope.signed.device_fingerprint_sha256.clone(),
            placement: old_envelope_binding.placement.clone(),
            envelope_sha256: new_envelope_digest.clone(),
            source_evidence_sha256: old_envelope_binding.source_evidence_sha256.clone(),
        }],
    };
    let (new_root, new_bundle) = build_release_catalog_bundle_v1(
        vec![source_entry],
        version,
        generated,
        expires,
        &signers,
        2,
    )?;
    let root_bytes = pretty_json(&new_root)?;
    let catalog_bytes = pretty_json(&new_bundle)?;

    fs::create_dir_all(output.join("evidence"))?;
    fs::create_dir_all(output.join("qual"))?;
    write_new(&output.join("model-catalog-root-v1.json"), &root_bytes)?;
    write_new(&output.join("model-catalog-v1.json"), &catalog_bytes)?;
    write_new(&output.join(MANIFEST_FILE), &manifest_bytes)?;
    write_new(
        &output
            .join("qual")
            .join(format!("{}.json", new_envelope_digest.as_str())),
        &new_envelope_bytes,
    )?;
    write_new(
        &output.join("evidence/yunet-import-context.json"),
        &import_context_bytes,
    )?;

    let verified = verify_release_catalog_bundle_v1(
        &new_root,
        &new_bundle,
        &CatalogTrustState::default(),
        generated,
    )?;
    verify_release_manifest_files_v1(&output, &verified)?;
    verify_release_envelope_files_v1(&output, &verified, generated)?;

    let runtime_files = vec![
        runtime_file("model-catalog-root-v1.json", &root_bytes),
        runtime_file("model-catalog-v1.json", &catalog_bytes),
        runtime_file(MANIFEST_FILE, &manifest_bytes),
        runtime_file(
            &format!("qual/{}.json", new_envelope_digest.as_str()),
            &new_envelope_bytes,
        ),
    ];
    let receipt = serde_json::json!({
        "schema": "interactive-npcs-private-catalog-verification/v1",
        "verifiedAtUnixSeconds": generated,
        "verification": {
            "catalogBundlePublicApi": true,
            "manifestFilesPublicApi": true,
            "qualifiedEnvelopeFilesPublicApi": true,
            "trustedPackSnapshotPublicApi": true
        },
        "catalog": {
            "version": version,
            "trustScope": "automated_local_review_bootstrap",
            "signatureThreshold": 2,
            "keyCount": 2,
            "productionTrust": false,
            "rotationRequiredBeforeRelease": true,
            "promotionSupported": false,
            "publicationSupported": false
        },
        "pack": {
            "id": PACK_ID,
            "revision": PACK_REVISION,
            "qualifiedEnvelopeCount": 1,
            "nativeInferenceQualified": true,
            "providerLoadSelfTestAttested": false,
            "installed": false,
            "activated": false
        },
        "runtimeFiles": runtime_files,
        "auditEvidence": {
            "path": "evidence/yunet-import-context.json",
            "sha256": Sha256Digest::of_bytes(&import_context_bytes),
            "nativeReportSha256": required_string(&import_context, "/native_qualification_report_sha256")?
        },
        "signerPrivateMaterialPersisted": false
    });
    let receipt_bytes = pretty_json(&receipt)?;
    write_new(
        &output.join("evidence/catalog-verification-receipt.json"),
        &receipt_bytes,
    )?;

    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema": "interactive-npcs-private-catalog-refresh/v1",
            "status": "passed",
            "sourceCatalogVersion": old_bundle.signed.version,
            "catalogVersion": version,
            "catalogExpiresUnixSeconds": expires,
            "qualifiedEnvelopeExpiresUnixSeconds": new_envelope.signed.expires_unix_seconds,
            "outputDirectory": output,
            "runtimeFileCount": 4,
            "signerPrivateMaterialPersisted": false,
            "productionTrust": false,
            "promotionSupported": false,
            "publicationSupported": false
        }))?
    );
    Ok(())
}

fn fresh_ephemeral_signers(
    version: u64,
) -> Result<Vec<ReleaseCatalogSignerV1>, Box<dyn std::error::Error>> {
    let mut result = Vec::new();
    for ordinal in 1..=2 {
        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed)
            .map_err(|error| format!("operating-system randomness failed: {error:?}"))?;
        result.push(ReleaseCatalogSignerV1 {
            key_id: format!("automated-local-review-rev{version}-ephemeral-{ordinal}"),
            signing_key: SigningKey::from_bytes(&seed),
        });
        seed.zeroize();
    }
    Ok(result)
}

fn validate_import_context(
    context: &Value,
    envelope: &SignedMeasuredResourceEnvelopeV1,
) -> Result<(), Box<dyn std::error::Error>> {
    if required_string(context, "/schema")? != "npc.review-envelope-import-context/v1"
        || required_string(context, "/identity/pack_id")? != PACK_ID
        || required_string(context, "/identity/revision")? != PACK_REVISION
        || required_string(context, "/device_fingerprint_sha256")?
            != envelope.signed.device_fingerprint_sha256.as_str()
        || required_string(context, "/manifest_normalized_core_sha256")?
            != envelope.signed.manifest_sha256.as_str()
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
            .pointer("/promotion_supported")
            .and_then(Value::as_bool)
            != Some(false)
        || context
            .pointer("/publication_supported")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err("source import context violates the private-review boundary".into());
    }
    Ok(())
}

fn runtime_file(path: &str, bytes: &[u8]) -> RuntimeFile {
    RuntimeFile {
        path: path.to_owned(),
        sha256: Sha256Digest::of_bytes(bytes).to_string(),
    }
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

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err(format!("refusing to replace generated path: {}", path.display()).into());
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
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
