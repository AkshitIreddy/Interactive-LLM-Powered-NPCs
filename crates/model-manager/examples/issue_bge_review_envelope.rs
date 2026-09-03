use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use model_manager::{
    canonical_measured_resource_envelope_bytes, parse_and_normalize_model_pack_manifest,
    CatalogSignatureV1, MeasuredResourceEnvelopePayloadV1, ModelPackKindV1, PlacementMeasurementV1,
    ResidencyModeV1, Sha256Digest, SignedMeasuredResourceEnvelopeV1, ED25519_CATALOG_ALGORITHM,
    MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1,
};
use npc_system_telemetry::{collect, TelemetryRequest};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const BGE_PACK_ID: &str = "bge-small-en-v1.5-onnx-fp32";
const BGE_REVISION: &str = "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a.1";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: issue_bge_review_envelope <manifest> <qualification-report> <runtime-inventory> <output-root>".into());
    }
    let manifest_bytes = fs::read(&args[0])?;
    let report_bytes = fs::read(&args[1])?;
    let inventory_bytes = fs::read(&args[2])?;
    let manifest = parse_and_normalize_model_pack_manifest(&manifest_bytes)?;
    if manifest.core_manifest.pack_id.as_str() != BGE_PACK_ID
        || manifest.core_manifest.revision.as_str() != BGE_REVISION
        || manifest.core_manifest.capability.kind != ModelPackKindV1::Embedding
        || manifest.document.runtime.runtime != "onnxruntime"
        || manifest.document.runtime.immutable_revision.as_deref() != Some("1.29.0")
        || !manifest
            .document
            .runtime
            .backends
            .contains("onnxruntime-cpu")
    {
        return Err("BGE manifest identity/runtime contract mismatch".into());
    }
    let report: Value = serde_json::from_slice(&report_bytes)?;
    let inventory: Value = serde_json::from_slice(&inventory_bytes)?;
    validate_report(&report, &manifest_bytes, &inventory, &inventory_bytes)?;

    let telemetry = collect(TelemetryRequest::default());
    let fingerprint = telemetry
        .device_fingerprint_sha256
        .value()
        .cloned()
        .ok_or("native system telemetry did not provide a device fingerprint")?;
    let fingerprint = Sha256Digest::parse(fingerprint)?;
    let measured = required_u64(&report, &["measured_unix_seconds"])?;
    let sample_count = required_u64(&report, &["samples", "measured_operations"])? as u32;
    let placement = &report["placement"];
    let payload = MeasuredResourceEnvelopePayloadV1 {
        schema: MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1.to_owned(),
        report_id: required_str(&report, &["report_id"])?.to_owned(),
        sequence: 1,
        measured_unix_seconds: measured,
        expires_unix_seconds: measured + 30 * 24 * 60 * 60,
        device_fingerprint_sha256: fingerprint.clone(),
        identity: manifest.core_manifest.identity(),
        manifest_sha256: manifest.core_manifest.digest()?,
        capability: ModelPackKindV1::Embedding,
        benchmark_suite_revision: required_str(&report, &["suite_revision"])?.to_owned(),
        runtime: "onnxruntime".to_owned(),
        runtime_revision: "1.29.0".to_owned(),
        backend: "cpu".to_owned(),
        sample_count,
        placements: BTreeMap::from([(
            ResidencyModeV1::CpuResident,
            PlacementMeasurementV1 {
                resident_ram_bytes: required_u64(placement, &["resident_ram_bytes"])?,
                p99_total_ram_bytes: required_u64(placement, &["p99_total_ram_bytes"])?,
                resident_vram_bytes: required_u64(placement, &["resident_vram_bytes"])?,
                p99_workspace_vram_bytes: required_u64(placement, &["p99_workspace_vram_bytes"])?,
                p99_load_millis: required_u64(placement, &["p99_load_millis"])?,
                p99_reload_millis: required_u64(placement, &["p99_reload_millis"])?,
                p99_operation_millis: required_u64(placement, &["p99_operation_millis"])?,
            },
        )]),
    };
    let payload_bytes = canonical_measured_resource_envelope_bytes(&payload)?;
    let signers = read_signers()?;
    let envelope = SignedMeasuredResourceEnvelopeV1 {
        signed: payload,
        signatures: signers
            .iter()
            .map(|(key_id, signer)| CatalogSignatureV1 {
                key_id: key_id.clone(),
                algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
                signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(signer.sign(&payload_bytes).to_bytes()),
            })
            .collect(),
    };
    let output_directory = PathBuf::from(&args[3])
        .join(BGE_PACK_ID)
        .join(BGE_REVISION)
        .join(fingerprint.as_str());
    fs::create_dir_all(&output_directory)?;
    write_pretty_json(&output_directory.join("cpu_resident.json"), &envelope)?;
    let context = serde_json::json!({
        "schema": "npc.review-envelope-import-context/v1",
        "identity": envelope.signed.identity,
        "device_fingerprint_sha256": fingerprint,
        "captured_unix_millis": telemetry.captured_unix_millis,
        "captured_monotonic_millis": telemetry.captured_monotonic_millis,
        "raw_hardware_identifiers_stored": false,
        "manifest_raw_sha256": Sha256Digest::of_bytes(&manifest_bytes),
        "manifest_canonical_sha256": manifest.canonical_document_sha256,
        "manifest_normalized_core_sha256": envelope.signed.manifest_sha256,
        "qualification_report_sha256": Sha256Digest::of_bytes(&report_bytes),
        "runtime_inventory_sha256": Sha256Digest::of_bytes(&inventory_bytes),
        "runtime": envelope.signed.runtime,
        "runtime_revision": envelope.signed.runtime_revision,
        "backend": envelope.signed.backend,
        "sample_count": envelope.signed.sample_count,
        "whole_loadout_fit_deferred_to_activation": true
    });
    write_pretty_json(&output_directory.join("import-context.json"), &context)?;
    Ok(())
}

fn validate_report(
    report: &Value,
    manifest_bytes: &[u8],
    inventory: &Value,
    inventory_bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    if required_str(report, &["schema"])? != "npc.embedding-qualification-report/v1"
        || required_str(report, &["identity", "pack_id"])? != BGE_PACK_ID
        || required_str(report, &["identity", "revision"])? != BGE_REVISION
        || required_str(report, &["manifest_sha256"])?
            != Sha256Digest::of_bytes(manifest_bytes).as_str()
        || required_str(report, &["runtime", "onnxruntime"])? != "1.29.0"
        || required_str(report, &["runtime", "backend"])? != "cpu"
        || required_str(report, &["placement", "residency_mode"])? != "cpu_resident"
        || required_u64(report, &["samples", "measured_loads"])? < 20
        || required_u64(report, &["samples", "measured_reloads"])? < 20
        || required_u64(report, &["samples", "measured_operations"])? < 20
        || report["self_test"]["semantic_ordering_passed"].as_bool() != Some(true)
        || report["quality_test"]["top1_correct"].as_u64() != Some(6)
        || report["quality_test"]["top3_correct"].as_u64() != Some(6)
        || report["placement"]["resident_vram_bytes"].as_u64() != Some(0)
        || report["placement"]["p99_workspace_vram_bytes"].as_u64() != Some(0)
    {
        return Err("BGE qualification report contract mismatch".into());
    }
    let inventory_sha = Sha256Digest::of_bytes(inventory_bytes);
    let requirements: Value = serde_json::from_slice(&fs::read(
        "workers/local-embedding/runtime-requirements.windows-x86_64-cp312.v1.json",
    )?)?;
    if requirements["resolved_qualification_inventory"]["sha256"].as_str()
        != Some(inventory_sha.as_str())
        || !inventory["packages"].as_array().is_some_and(|packages| {
            packages.iter().any(|package| {
                package["name"].as_str() == Some("onnxruntime")
                    && package["version"].as_str() == Some("1.29.0")
            })
        })
    {
        return Err("BGE runtime inventory does not prove ONNX Runtime 1.29.0".into());
    }
    Ok(())
}

fn required_str<'a>(
    value: &'a Value,
    path: &[&str],
) -> Result<&'a str, Box<dyn std::error::Error>> {
    let mut current = value;
    for segment in path {
        current = &current[*segment];
    }
    current
        .as_str()
        .ok_or_else(|| format!("missing string {}", path.join(".")).into())
}

fn required_u64(value: &Value, path: &[&str]) -> Result<u64, Box<dyn std::error::Error>> {
    let mut current = value;
    for segment in path {
        current = &current[*segment];
    }
    current
        .as_u64()
        .ok_or_else(|| format!("missing integer {}", path.join(".")).into())
}

fn read_signers() -> Result<Vec<(String, SigningKey)>, Box<dyn std::error::Error>> {
    let raw = env::var("NPC_MODEL_CATALOG_SIGNERS")?;
    let mut signers = Vec::new();
    for item in raw.split(',') {
        let (key_id, seed) = item.split_once('=').ok_or("invalid signer")?;
        let seed: [u8; 32] = hex::decode(seed)?
            .try_into()
            .map_err(|_| "signer seed must be 32 bytes")?;
        signers.push((key_id.to_owned(), SigningKey::from_bytes(&seed)));
    }
    if signers.len() < 2 {
        return Err("at least two distinct signer keys are required".into());
    }
    Ok(signers)
}

fn write_pretty_json(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}
