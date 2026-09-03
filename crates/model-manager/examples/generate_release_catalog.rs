use ed25519_dalek::SigningKey;
use model_manager::{
    build_release_catalog_bundle_v1, parse_and_normalize_model_pack_manifest,
    verify_measured_resource_envelope, Ed25519CatalogVerifier, MeasurementTrustPolicyV1,
    MeasurementTrustStateV1, ModelPackNormalizationOriginV2, NonQualifyingReviewEvidenceIndexV1,
    QualifiedEnvelopeBindingV1, ReleaseCatalogSignerV1, ReleaseManifestSourceV1, ResidencyModeV1,
    Sha256Digest, SignedMeasuredResourceEnvelopeV1, MODEL_PACK_MANIFEST_SCHEMA_V2,
    NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1, NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_SCHEMA_V1,
    RELEASE_CATALOG_FILE_V1, RELEASE_CATALOG_ROOT_FILE_V1,
};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err("usage: generate_release_catalog <manifest-dir> <output-dir> <version> <generated-unix-seconds> <expires-unix-seconds>".into());
    }
    let manifest_directory = PathBuf::from(&args[0]);
    let output_directory = PathBuf::from(&args[1]);
    let version = args[2].parse::<u64>()?;
    let generated = args[3].parse::<u64>()?;
    let expires = args[4].parse::<u64>()?;
    let signers = read_signers()?;
    let sources = read_sources(&manifest_directory, &signers, generated)?;
    let (root, bundle) = build_release_catalog_bundle_v1(
        sources,
        version,
        generated,
        expires,
        &signers,
        signers.len(),
    )?;
    fs::create_dir_all(&output_directory)?;
    write_pretty_json(&output_directory.join(RELEASE_CATALOG_ROOT_FILE_V1), &root)?;
    write_pretty_json(&output_directory.join(RELEASE_CATALOG_FILE_V1), &bundle)?;
    Ok(())
}

fn read_signers() -> Result<Vec<ReleaseCatalogSignerV1>, Box<dyn std::error::Error>> {
    let raw = env::var("NPC_MODEL_CATALOG_SIGNERS")
        .map_err(|_| "NPC_MODEL_CATALOG_SIGNERS must contain key-id=64-hex-seed pairs")?;
    let mut signers = Vec::new();
    for item in raw.split(',') {
        let (key_id, seed) = item
            .split_once('=')
            .ok_or("catalog signer must be key-id=64-hex-seed")?;
        let seed = hex::decode(seed)?;
        let seed: [u8; 32] = seed
            .try_into()
            .map_err(|_| "catalog signer seed must be exactly 32 bytes")?;
        signers.push(ReleaseCatalogSignerV1 {
            key_id: key_id.to_owned(),
            signing_key: SigningKey::from_bytes(&seed),
        });
    }
    if signers.len() < 2 {
        return Err("at least two distinct catalog signer keys are required".into());
    }
    Ok(signers)
}

fn read_sources(
    directory: &Path,
    signers: &[ReleaseCatalogSignerV1],
    now_unix_seconds: u64,
) -> Result<Vec<ReleaseManifestSourceV1>, Box<dyn std::error::Error>> {
    let evidence_path = directory.join(NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1);
    let mut review_evidence = if evidence_path.exists() {
        let index: NonQualifyingReviewEvidenceIndexV1 =
            serde_json::from_slice(&fs::read(&evidence_path)?)?;
        if index.schema != NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_SCHEMA_V1 {
            return Err("non-qualifying review evidence index schema is invalid".into());
        }
        index
            .entries
            .into_iter()
            .map(|entry| (entry.identity, entry.evidence))
            .collect::<std::collections::BTreeMap<_, _>>()
    } else {
        std::collections::BTreeMap::new()
    };
    let mut qualified_envelopes = read_qualified_envelopes(
        &directory.join("qual"),
        signers,
        now_unix_seconds,
        &review_evidence,
    )?;
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut sources = Vec::new();
    for path in paths {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("manifest file name is not UTF-8")?;
        if matches!(
            file_name,
            "model-pack-manifest.schema.json"
                | "model-pack-manifest.example.json"
                | RELEASE_CATALOG_ROOT_FILE_V1
                | RELEASE_CATALOG_FILE_V1
                | NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1
        ) {
            continue;
        }
        let bytes = fs::read(&path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        if value.get("schema").and_then(Value::as_str) != Some(MODEL_PACK_MANIFEST_SCHEMA_V2) {
            return Err(format!(
                "{} is not a canonical npc.model-pack/v2 document",
                path.display()
            )
            .into());
        }
        let normalized = parse_and_normalize_model_pack_manifest(&bytes)?;
        if normalized.origin != ModelPackNormalizationOriginV2::CanonicalV2 {
            return Err(format!("{} did not parse as canonical v2", path.display()).into());
        }
        let identity = normalized.core_manifest.identity();
        let evidence = review_evidence.remove(&identity).into_iter().collect();
        let envelopes = qualified_envelopes.remove(&identity).unwrap_or_default();
        sources.push(ReleaseManifestSourceV1 {
            file_name: file_name.to_owned(),
            raw_document_sha256: Sha256Digest::of_bytes(&bytes),
            normalized,
            review_evidence: evidence,
            // A measurement is listed only after a separate importer verifies its
            // threshold signatures and exact device/manifest bindings.
            qualified_envelopes: envelopes,
        });
    }
    if sources.is_empty() {
        return Err("no canonical release manifests were found".into());
    }
    if !review_evidence.is_empty() {
        return Err("review evidence index names a pack absent from canonical manifests".into());
    }
    if !qualified_envelopes.is_empty() {
        return Err("qualified envelope names a pack absent from canonical manifests".into());
    }
    Ok(sources)
}

fn read_qualified_envelopes(
    directory: &Path,
    signers: &[ReleaseCatalogSignerV1],
    now_unix_seconds: u64,
    review_evidence: &std::collections::BTreeMap<
        model_manager::PackRevision,
        model_manager::NonQualifyingReviewEvidenceBindingV1,
    >,
) -> Result<
    std::collections::BTreeMap<model_manager::PackRevision, Vec<QualifiedEnvelopeBindingV1>>,
    Box<dyn std::error::Error>,
> {
    if !directory.exists() {
        return Ok(std::collections::BTreeMap::new());
    }
    let verifier = Ed25519CatalogVerifier::new(signers.iter().map(|signer| {
        (
            signer.key_id.clone(),
            signer.signing_key.verifying_key().to_bytes(),
        )
    }))?;
    let policy = MeasurementTrustPolicyV1 {
        signature_threshold: signers.len(),
        ..MeasurementTrustPolicyV1::default()
    };
    let mut state = MeasurementTrustStateV1::default();
    let mut result = std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            || entry.file_name() == "import-context.json"
        {
            continue;
        }
        let placement = match entry.path().file_stem().and_then(|value| value.to_str()) {
            Some("cpu_resident") => ResidencyModeV1::CpuResident,
            Some("gpu_resident") => ResidencyModeV1::GpuResident,
            Some("cpu_resident_gpu_cold") => ResidencyModeV1::CpuResidentGpuCold,
            _ => {
                return Err(format!(
                    "unexpected qualified-envelope file {}",
                    entry.path().display()
                )
                .into())
            }
        };
        let bytes = fs::read(entry.path())?;
        let signed: SignedMeasuredResourceEnvelopeV1 = serde_json::from_slice(&bytes)?;
        let (qualified, next) = verify_measured_resource_envelope(
            &signed,
            &verifier,
            &policy,
            &state,
            now_unix_seconds,
        )?;
        if !qualified.payload().placements.contains_key(&placement) {
            return Err("qualified envelope file placement is absent from payload".into());
        }
        state = next;
        let payload = qualified.payload();
        result
            .entry(payload.identity.clone())
            .or_insert_with(Vec::new)
            .push(QualifiedEnvelopeBindingV1 {
                report_id: payload.report_id.clone(),
                device_fingerprint_sha256: payload.device_fingerprint_sha256.clone(),
                placement,
                envelope_sha256: Sha256Digest::of_bytes(&bytes),
                source_evidence_sha256: review_evidence
                    .get(&payload.identity)
                    .map(|evidence| evidence.evidence_sha256.clone()),
            });
    }
    for bindings in result.values_mut() {
        bindings.sort_by(|left, right| left.placement.cmp(&right.placement));
    }
    Ok(result)
}

fn write_pretty_json(
    path: &Path,
    value: &impl serde::Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}
