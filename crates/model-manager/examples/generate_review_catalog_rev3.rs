use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use model_manager::{
    build_release_catalog_bundle_v1, canonical_measured_resource_envelope_bytes,
    load_release_catalog_bundle_v1, parse_and_normalize_model_pack_manifest,
    verify_measured_resource_envelope, verify_release_catalog_bundle_v1, CatalogSignatureV1,
    MeasuredResourceEnvelopePayloadV1, MeasurementTrustPolicyV1, MeasurementTrustStateV1,
    ModelPackKindV1, ModelPackNormalizationOriginV2, NonQualifyingReviewEvidenceIndexV1,
    PackRevision, QualifiedEnvelopeBindingV1, ReleaseCatalogSignerV1, ReleaseManifestSourceV1,
    ResidencyModeV1, Sha256Digest, SignedMeasuredResourceEnvelopeV1, ED25519_CATALOG_ALGORITHM,
    MODEL_PACK_MANIFEST_SCHEMA_V2, NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1,
    NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_SCHEMA_V1, RELEASE_CATALOG_FILE_V1,
    RELEASE_CATALOG_ROOT_FILE_V1,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const BGE_PACK_ID: &str = "bge-small-en-v1.5-onnx-fp32";
const BGE_REVISION: &str = "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a.1";
const OPENSEEFACE_PACK_ID: &str = "openseeface-mnv3-lm1-mouth-signal";
const OPENSEEFACE_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
const FINAL_VISUAL_RAW: &str = "2d10e01c1b1ab5177d594375f94dbc62ccd926b4fa82f75b35cda33c27e38dae";
const FINAL_VISUAL_CANONICAL: &str =
    "f430962ca08a6f93684b8bad82a6ae3ed966815e7c672dd1560095a7964a65bf";
// The raw and canonical v2 document did not change. The core digest changed
// when archive strip-prefix and closed-world required-path bindings became
// part of ModelPackManifestV1, so the review rotation explicitly binds both
// the prior derivation digest and the current normalized contract.
const PREVIOUS_VISUAL_CORE: &str =
    "faafcf7295c9002ca9637f1b91a652b36bcaf36b12b97ab17a33a84e4cbfcc00";
const CURRENT_VISUAL_CORE: &str =
    "0127b61113b1f838bf70bdc4127bd7d5f7f9a75fde95429c89c5e726751f0552";
const VISUAL_REPORT_SHA256: &str =
    "e8ee03e11f8ca55c5e588e393c4668c188c6896722ac4e7bfb382dd9aa2435c8";
const VISUAL_CANDIDATE_SHA256: &str =
    "b74876101b2ffddcedc201a24f5b62f6f0bfd62798aa138db0a64a42714f4b9c";
const VISUAL_DERIVATION_SHA256: &str =
    "34a2c799110642d4493bc64e5efa355051caeb10866ff76f5dfd3c729fe431ae";
const DEVICE_FINGERPRINT: &str = "480a0ebfe9b7f9ce9edc0b5e98d97491c7c3a2c8e5d3e52bcb4d13f9e3a230ce";

#[allow(clippy::print_stdout)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 8 {
        return Err("usage: generate_review_catalog_rev3 <manifest-dir> <output-dir> <version> <generated-unix-seconds> <expires-unix-seconds> <visual-report> <visual-candidate-envelope> <visual-derivation>".into());
    }
    let manifest_directory = PathBuf::from(&args[0]);
    let output_directory = PathBuf::from(&args[1]);
    let version = args[2].parse::<u64>()?;
    let generated = args[3].parse::<u64>()?;
    let expires = args[4].parse::<u64>()?;
    let report_path = PathBuf::from(&args[5]);
    let candidate_path = PathBuf::from(&args[6]);
    let derivation_path = PathBuf::from(&args[7]);
    let (old_root, old_bundle) = load_release_catalog_bundle_v1(&manifest_directory)?;
    let old_verified = verify_release_catalog_bundle_v1(
        &old_root,
        &old_bundle,
        &model_manager::CatalogTrustState::default(),
        generated,
    )?;
    if version <= old_bundle.signed.version {
        return Err("review catalog revision must increase monotonically".into());
    }

    let bge_identity = identity(BGE_PACK_ID, BGE_REVISION)?;
    let old_bge_binding = old_verified
        .source_inventory
        .manifests
        .iter()
        .find(|source| source.identity == bge_identity)
        .and_then(|source| source.qualified_envelopes.first())
        .ok_or("rev2 BGE qualified-envelope binding is absent")?;
    let old_bge_path = envelope_path(
        &manifest_directory,
        &bge_identity,
        &old_bge_binding.device_fingerprint_sha256,
        &old_bge_binding.placement,
    );
    let old_bge_bytes = fs::read(&old_bge_path)?;
    if Sha256Digest::of_bytes(&old_bge_bytes) != old_bge_binding.envelope_sha256 {
        return Err("rev2 BGE envelope digest changed".into());
    }
    let old_bge: SignedMeasuredResourceEnvelopeV1 = serde_json::from_slice(&old_bge_bytes)?;
    let policy = MeasurementTrustPolicyV1 {
        signature_threshold: old_verified.signature_threshold,
        ..MeasurementTrustPolicyV1::default()
    };
    let (qualified_bge, _) = verify_measured_resource_envelope(
        &old_bge,
        &old_verified.verifier,
        &policy,
        &MeasurementTrustStateV1::default(),
        generated,
    )?;
    if qualified_bge.payload().identity != bge_identity
        || qualified_bge.payload().device_fingerprint_sha256.as_str() != DEVICE_FINGERPRINT
        || qualified_bge.payload().sample_count < 20
    {
        return Err("rev2 BGE qualified payload contract changed".into());
    }

    let visual_manifest_path = manifest_directory.join("openseeface-mnv3-lm1-mouth-signal.json");
    let visual_manifest_bytes = fs::read(&visual_manifest_path)?;
    let visual_manifest = parse_and_normalize_model_pack_manifest(&visual_manifest_bytes)?;
    if Sha256Digest::of_bytes(&visual_manifest_bytes).as_str() != FINAL_VISUAL_RAW
        || visual_manifest.canonical_document_sha256.as_str() != FINAL_VISUAL_CANONICAL
        || visual_manifest.core_manifest.digest()?.as_str() != CURRENT_VISUAL_CORE
        || visual_manifest.core_manifest.identity()
            != identity(OPENSEEFACE_PACK_ID, OPENSEEFACE_REVISION)?
        || visual_manifest.core_manifest.capability.kind != ModelPackKindV1::Vision
        || visual_manifest.document.runtime.runtime != "onnxruntime"
        || visual_manifest
            .document
            .runtime
            .immutable_revision
            .as_deref()
            != Some("1.22.1")
        || visual_manifest.document.admission.allowed_residencies
            != std::collections::BTreeSet::from([ResidencyModeV1::CpuResident])
    {
        return Err(format!(
            "final OpenSeeFace manifest contract changed: raw={}, canonical={}, core={}",
            Sha256Digest::of_bytes(&visual_manifest_bytes),
            visual_manifest.canonical_document_sha256,
            visual_manifest.core_manifest.digest()?
        )
        .into());
    }

    let report_bytes = read_exact_hash(&report_path, VISUAL_REPORT_SHA256)?;
    let candidate_bytes = read_exact_hash(&candidate_path, VISUAL_CANDIDATE_SHA256)?;
    let derivation_bytes = read_exact_hash(&derivation_path, VISUAL_DERIVATION_SHA256)?;
    let report: Value = serde_json::from_slice(&report_bytes)?;
    let candidate: SignedMeasuredResourceEnvelopeV1 = serde_json::from_slice(&candidate_bytes)?;
    let derivation: Value = serde_json::from_slice(&derivation_bytes)?;
    verify_visual_derivation(&derivation, &report, &candidate)?;

    let signers = fresh_ephemeral_signers(version)?;
    let bge = sign_payload(qualified_bge.payload().clone(), &signers)?;
    let mut visual_payload = candidate.signed.clone();
    visual_payload.report_id = format!("{}-final-manifest-review-rebind", visual_payload.report_id);
    visual_payload.sequence = visual_payload
        .sequence
        .checked_add(1)
        .ok_or("visual envelope sequence overflow")?;
    visual_payload.device_fingerprint_sha256 = Sha256Digest::parse(DEVICE_FINGERPRINT)?;
    visual_payload.manifest_sha256 = visual_manifest.core_manifest.digest()?;
    if visual_payload.sample_count != 20
        || visual_payload.runtime != "onnxruntime"
        || visual_payload.runtime_revision != "1.22.1"
        || visual_payload.backend != "cpu-execution-provider-one-thread"
        || visual_payload.placements
            != BTreeMap::from([(
                ResidencyModeV1::CpuResident,
                model_manager::PlacementMeasurementV1 {
                    resident_ram_bytes: 35_086_336,
                    p99_total_ram_bytes: 125_935_616,
                    resident_vram_bytes: 0,
                    p99_workspace_vram_bytes: 0,
                    p99_load_millis: 103,
                    p99_reload_millis: 148,
                    p99_operation_millis: 55,
                },
            )])
    {
        return Err("derived OpenSeeFace placement differs from immutable evidence".into());
    }
    let visual = sign_payload(visual_payload, &signers)?;
    let bge_bytes = pretty_json(&bge)?;
    let visual_bytes = pretty_json(&visual)?;
    let envelopes = BTreeMap::from([
        (
            bge_identity.clone(),
            vec![qualified_binding(
                &bge,
                &bge_bytes,
                ResidencyModeV1::CpuResident,
                old_bge_binding.source_evidence_sha256.clone(),
            )],
        ),
        (
            visual.signed.identity.clone(),
            vec![qualified_binding(
                &visual,
                &visual_bytes,
                ResidencyModeV1::CpuResident,
                Some(Sha256Digest::parse(VISUAL_DERIVATION_SHA256)?),
            )],
        ),
    ]);
    let sources = read_sources(&manifest_directory, envelopes)?;
    let (root, bundle) = build_release_catalog_bundle_v1(
        sources,
        version,
        generated,
        expires,
        &signers,
        signers.len(),
    )?;
    // The derivation verified above is the explicit review migration for this
    // metadata-only immutable-identity correction. A clean install starts from
    // the freshly rotated local-review trust root. The version check above
    // remains monotonic, and the resulting state rejects an older rollback.
    let verified = verify_release_catalog_bundle_v1(
        &root,
        &bundle,
        &model_manager::CatalogTrustState::default(),
        generated,
    )?;
    if verified.catalog.payload().version != version
        || root.production_trust
        || root.promotion_supported
        || root.publication_supported
        || !root.rotation_required_before_release
    {
        return Err("rev3 bootstrap trust boundary is invalid".into());
    }

    write_atomic(
        &envelope_path(
            &output_directory,
            &bge.signed.identity,
            &bge.signed.device_fingerprint_sha256,
            &ResidencyModeV1::CpuResident,
        ),
        &bge_bytes,
    )?;
    write_atomic(
        &envelope_path(
            &output_directory,
            &visual.signed.identity,
            &visual.signed.device_fingerprint_sha256,
            &ResidencyModeV1::CpuResident,
        ),
        &visual_bytes,
    )?;
    write_atomic(
        &output_directory.join(RELEASE_CATALOG_ROOT_FILE_V1),
        &pretty_json(&root)?,
    )?;
    write_atomic(
        &output_directory.join(RELEASE_CATALOG_FILE_V1),
        &pretty_json(&bundle)?,
    )?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "version": version,
            "root_sha256": Sha256Digest::of_bytes(&pretty_json(&root)?),
            "catalog_sha256": Sha256Digest::of_bytes(&pretty_json(&bundle)?),
            "bge_envelope_sha256": Sha256Digest::of_bytes(&bge_bytes),
            "openseeface_envelope_sha256": Sha256Digest::of_bytes(&visual_bytes),
            "production_trust": false,
            "signer_private_material_persisted": false
        }))?
    );
    Ok(())
}

fn identity(pack_id: &str, revision: &str) -> Result<PackRevision, Box<dyn std::error::Error>> {
    Ok(PackRevision {
        pack_id: model_manager::PackId::parse(pack_id)?,
        revision: model_manager::Revision::parse(revision)?,
    })
}

fn read_exact_hash(path: &Path, expected: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if Sha256Digest::of_bytes(&bytes).as_str() != expected {
        return Err(format!("immutable evidence digest changed: {}", path.display()).into());
    }
    Ok(bytes)
}

fn verify_visual_derivation(
    document: &Value,
    report: &Value,
    candidate: &SignedMeasuredResourceEnvelopeV1,
) -> Result<(), Box<dyn std::error::Error>> {
    let derivation = document
        .get("derivation")
        .ok_or("visual derivation payload is absent")?;
    let proof = document
        .get("verification")
        .ok_or("visual derivation verification is absent")?;
    let signature = document
        .get("signatures")
        .and_then(Value::as_array)
        .and_then(|signatures| signatures.first())
        .ok_or("visual derivation signature is absent")?;
    let public_key = base64::engine::general_purpose::STANDARD.decode(
        proof
            .get("public_key_base64")
            .and_then(Value::as_str)
            .ok_or("visual derivation public key is absent")?,
    )?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| "visual derivation public key length is invalid")?;
    let signature_bytes = base64::engine::general_purpose::STANDARD.decode(
        signature
            .get("signature")
            .and_then(Value::as_str)
            .ok_or("visual derivation signature bytes are absent")?,
    )?;
    let signature = Signature::from_slice(&signature_bytes)?;
    VerifyingKey::from_bytes(&public_key)?.verify(&serde_json::to_vec(derivation)?, &signature)?;
    if derivation.pointer("/status").and_then(Value::as_str)
        != Some("candidate_non_authoritative_requires_resource_governor_review")
        || derivation
            .pointer("/final_manifest/raw_sha256")
            .and_then(Value::as_str)
            != Some(FINAL_VISUAL_RAW)
        || derivation
            .pointer("/final_manifest/canonical_sha256")
            .and_then(Value::as_str)
            != Some(FINAL_VISUAL_CANONICAL)
        || derivation
            .pointer("/final_manifest/normalized_core_sha256")
            .and_then(Value::as_str)
            != Some(PREVIOUS_VISUAL_CORE)
        || derivation
            .pointer("/immutable_inputs/qualification_report_sha256")
            .and_then(Value::as_str)
            != Some(VISUAL_REPORT_SHA256)
        || derivation
            .pointer("/immutable_inputs/candidate_envelope_sha256")
            .and_then(Value::as_str)
            != Some(VISUAL_CANDIDATE_SHA256)
        || derivation
            .pointer("/authoritative_device_binding/device_fingerprint_sha256")
            .and_then(Value::as_str)
            != Some(DEVICE_FINGERPRINT)
        || derivation
            .pointer("/authoritative_device_binding/raw_serial_or_pii_recorded")
            .and_then(Value::as_bool)
            != Some(false)
        || derivation
            .pointer("/admission_truth/activation_authority")
            .and_then(Value::as_bool)
            != Some(false)
        || derivation
            .pointer(
                "/admission_truth/resource_governor_must_verify_and_threshold_sign_new_envelope",
            )
            .and_then(Value::as_bool)
            != Some(true)
        || derivation
            .pointer("/copied_without_recomputation/sample_count")
            .and_then(Value::as_u64)
            != Some(candidate.signed.sample_count.into())
        || report
            .pointer("/pack/normalized_manifest_sha256")
            .and_then(Value::as_str)
            != Some("3f442ae21a4456cba1d83680b2926d89345b9485c5908bc928201dd535dc9c56")
    {
        return Err("visual derivation binding or trust truth changed".into());
    }
    let copied: model_manager::PlacementMeasurementV1 = serde_json::from_value(
        derivation
            .pointer("/copied_without_recomputation/placements/cpu_resident")
            .cloned()
            .ok_or("derived visual CPU placement is absent")?,
    )?;
    if candidate
        .signed
        .placements
        .get(&ResidencyModeV1::CpuResident)
        != Some(&copied)
    {
        return Err("visual derivation placement differs from the signed candidate".into());
    }
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
        seed.fill(0);
    }
    Ok(result)
}

fn sign_payload(
    payload: MeasuredResourceEnvelopePayloadV1,
    signers: &[ReleaseCatalogSignerV1],
) -> Result<SignedMeasuredResourceEnvelopeV1, Box<dyn std::error::Error>> {
    let bytes = canonical_measured_resource_envelope_bytes(&payload)?;
    Ok(SignedMeasuredResourceEnvelopeV1 {
        signed: payload,
        signatures: signers
            .iter()
            .map(|signer| CatalogSignatureV1 {
                key_id: signer.key_id.clone(),
                algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
                signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(signer.signing_key.sign(&bytes).to_bytes()),
            })
            .collect(),
    })
}

fn qualified_binding(
    envelope: &SignedMeasuredResourceEnvelopeV1,
    bytes: &[u8],
    placement: ResidencyModeV1,
    source_evidence_sha256: Option<Sha256Digest>,
) -> QualifiedEnvelopeBindingV1 {
    QualifiedEnvelopeBindingV1 {
        report_id: envelope.signed.report_id.clone(),
        device_fingerprint_sha256: envelope.signed.device_fingerprint_sha256.clone(),
        placement,
        envelope_sha256: Sha256Digest::of_bytes(bytes),
        source_evidence_sha256,
    }
}

fn read_sources(
    directory: &Path,
    mut envelopes: BTreeMap<PackRevision, Vec<QualifiedEnvelopeBindingV1>>,
) -> Result<Vec<ReleaseManifestSourceV1>, Box<dyn std::error::Error>> {
    let index: NonQualifyingReviewEvidenceIndexV1 = serde_json::from_slice(&fs::read(
        directory.join(NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1),
    )?)?;
    if index.schema != NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_SCHEMA_V1 {
        return Err("review evidence index schema is invalid".into());
    }
    let mut review = index
        .entries
        .into_iter()
        .map(|entry| (entry.identity, entry.evidence))
        .collect::<BTreeMap<_, _>>();
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut sources = Vec::new();
    for path in paths {
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || matches!(
                file_name,
                "model-pack-manifest.schema.json"
                    | "model-pack-manifest.example.json"
                    | RELEASE_CATALOG_ROOT_FILE_V1
                    | RELEASE_CATALOG_FILE_V1
                    | NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1
            )
        {
            continue;
        }
        let bytes = fs::read(&path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        if value.get("schema").and_then(Value::as_str) != Some(MODEL_PACK_MANIFEST_SCHEMA_V2) {
            return Err(
                format!("non-v2 document in manifest inventory: {}", path.display()).into(),
            );
        }
        let normalized = parse_and_normalize_model_pack_manifest(&bytes)?;
        if normalized.origin != ModelPackNormalizationOriginV2::CanonicalV2 {
            return Err("release source is not canonical v2".into());
        }
        let identity = normalized.core_manifest.identity();
        sources.push(ReleaseManifestSourceV1 {
            file_name: file_name.to_owned(),
            raw_document_sha256: Sha256Digest::of_bytes(&bytes),
            normalized,
            review_evidence: review.remove(&identity).into_iter().collect(),
            qualified_envelopes: envelopes.remove(&identity).unwrap_or_default(),
        });
    }
    if !review.is_empty() || !envelopes.is_empty() {
        return Err("review evidence or envelope names an absent manifest".into());
    }
    Ok(sources)
}

fn envelope_path(
    root: &Path,
    identity: &PackRevision,
    fingerprint: &Sha256Digest,
    placement: &ResidencyModeV1,
) -> PathBuf {
    let placement = match placement {
        ResidencyModeV1::CpuResident => "cpu_resident",
        ResidencyModeV1::GpuResident => "gpu_resident",
        ResidencyModeV1::CpuResidentGpuCold => "cpu_resident_gpu_cold",
    };
    root.join("qual")
        .join(identity.pack_id.as_str())
        .join(identity.revision.as_str())
        .join(fingerprint.as_str())
        .join(format!("{placement}.json"))
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().ok_or("generated file has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".review-catalog-{}-{}.tmp",
        std::process::id(),
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("output")
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}
