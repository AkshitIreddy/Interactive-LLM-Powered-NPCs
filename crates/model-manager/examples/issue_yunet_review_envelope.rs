use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use model_manager::{
    canonical_measured_resource_envelope_bytes, parse_and_normalize_model_pack_manifest,
    CatalogSignatureV1, MeasuredResourceEnvelopePayloadV1, ModelPackArtifactRoleV2,
    ModelPackKindV1, ModelPackNormalizationOriginV2, PlacementMeasurementV1, ResidencyModeV1,
    Sha256Digest, SignedMeasuredResourceEnvelopeV1, ED25519_CATALOG_ALGORITHM,
    MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1,
};
use npc_system_telemetry::{collect, TelemetryRequest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, Zeroizing};

const PACK_ID: &str = "openseeface-yunet640-lm1-mouth-signal";
const PACK_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
const NATIVE_REPORT_SCHEMA: &str = "interactive-npcs-yunet-native-envelope/v1";
const RUNTIME_REVISION: &str = "1.22.1";
const BACKEND: &str = "cpu-execution-provider-one-thread";
const ABI: &str = "npc-yunet640-lm1-mouth-signal-v1";
const MINIMUM_ITERATIONS: usize = 20;
const ENVELOPE_LIFETIME_SECONDS: u64 = 30 * 24 * 60 * 60;

const DETECTOR_SIZE: u64 = 232_589;
const DETECTOR_SHA256: &str = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
const LANDMARK_SIZE: u64 = 4_842_329;
const LANDMARK_SHA256: &str = "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f";
const YUNET_LICENSE_SIZE: u64 = 1_085;
const YUNET_LICENSE_SHA256: &str =
    "c83b8120c50ccbd4c4f96edf53141bdd566ebb8f8e9227e415326aa1b1aba958";
const OPENSEEFACE_LICENSE_SIZE: u64 = 1_364;
const OPENSEEFACE_LICENSE_SHA256: &str =
    "28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146";
// These are the DLLs extracted from the canonical official ORT v1.22.1 release archive.
const RUNTIME_SIZE: u64 = 12_416_032;
const RUNTIME_SHA256: &str = "7788f3f38e9a339003f7d7e1bf47f928287cf409bb5273273149e1282fbf503f";
const RUNTIME_SHARED_SIZE: u64 = 22_048;
const RUNTIME_SHARED_SHA256: &str =
    "7a71dbee513692aeb0bd2346a50ae0edd28c8b34defc17254605a29e81c60569";
const RUNTIME_ARCHIVE_SIZE: u64 = 73_731_806;
const RUNTIME_ARCHIVE_SHA256: &str =
    "855276cd4be3cda14fe636c69eb038d75bf5bcd552bda1193a5d79c51f436dfe";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NativeReport {
    schema: String,
    suite_version: String,
    started_at_utc: String,
    completed_at_utc: String,
    scope: Scope,
    device: Device,
    pack: Pack,
    artifacts: Vec<ArtifactEvidence>,
    iteration_count: usize,
    session_count: usize,
    sessions: Vec<Session>,
    aggregates: Aggregates,
    counters: Counters,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Scope {
    provider: String,
    backend: String,
    runtime_revision: String,
    inference_threads: u16,
    gpu_used: bool,
    fixture_decode_excluded: bool,
    current_device_only: bool,
    fixture_idle: bool,
    game_pressure_measured: bool,
    capture_measured: bool,
    sixty_fps_measured: bool,
    os_disk_cache_state_controlled: bool,
    telemetry_fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Device {
    cpu: String,
    logical_processors: u32,
    page_size_bytes: u64,
    physical_memory_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    id: String,
    revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ArtifactEvidence {
    role: String,
    path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Session {
    iteration: usize,
    kind: String,
    load_ms: f64,
    inference_ms: f64,
    unload_ms: f64,
    detector_ms: f64,
    landmark_ms: f64,
    memory: Memory,
    detector_confidence: f64,
    landmark_confidence: f64,
    visibility_ratio: f64,
    packet_bound: bool,
    cancellation_advanced: bool,
    stale_generation_rejected: bool,
    unloaded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Memory {
    before_load_resident_bytes: u64,
    after_load_resident_bytes: u64,
    after_inference_resident_bytes: u64,
    after_unload_resident_bytes: u64,
    sampled_peak_resident_bytes: u64,
    sampled_peak_private_bytes: u64,
    absolute_process_peak_resident_bytes: u64,
    sampled_peak_resident_delta_from_baseline_bytes: i64,
    sampled_peak_private_delta_from_baseline_bytes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Aggregates {
    fresh_provider_load_ms: Statistics,
    reload_after_unload_ms: Statistics,
    fresh_inference_ms: Statistics,
    reload_inference_ms: Statistics,
    sampled_peak_resident_bytes: Statistics,
    sampled_peak_private_bytes: Statistics,
    absolute_process_peak_resident_bytes: Statistics,
    sampled_peak_resident_delta_from_baseline_bytes: Statistics,
    sampled_peak_private_delta_from_baseline_bytes: Statistics,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Statistics {
    count: usize,
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    maximum: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Counters {
    fresh_provider_loads: usize,
    reloads_after_unload: usize,
    packet_bound: usize,
    cancellation_passed: usize,
    stale_generation_rejected: usize,
    unload_passed: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: issue_yunet_review_envelope <canonical-manifest> <native-report> <output-root>"
                .into(),
        );
    }

    let manifest_bytes = fs::read(&args[0])?;
    let report_bytes = fs::read(&args[1])?;
    let manifest = parse_and_normalize_model_pack_manifest(&manifest_bytes)?;
    validate_manifest(&manifest)?;

    let report: NativeReport = serde_json::from_slice(&report_bytes)?;
    validate_native_report(&report)?;

    let telemetry = collect(TelemetryRequest::default());
    let live_physical_ram_bytes = telemetry
        .physical_ram_bytes
        .value()
        .copied()
        .ok_or("native system telemetry did not provide physical RAM")?;
    if live_physical_ram_bytes != report.device.physical_memory_bytes {
        return Err(
            "native qualification report does not match this machine's physical RAM".into(),
        );
    }
    let fingerprint = telemetry
        .device_fingerprint_sha256
        .value()
        .cloned()
        .ok_or("native system telemetry did not provide a device fingerprint")?;
    let fingerprint = Sha256Digest::parse(fingerprint)?;
    let now_unix_seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let measured_unix_seconds = parse_utc_timestamp(&report.completed_at_utc)?;
    if measured_unix_seconds > now_unix_seconds.saturating_add(5 * 60)
        || now_unix_seconds.saturating_sub(measured_unix_seconds) > 24 * 60 * 60
    {
        return Err("native qualification report is not a current measurement".into());
    }

    let observed_resident_delta = ceil_nonnegative(
        report
            .aggregates
            .sampled_peak_resident_delta_from_baseline_bytes
            .p99,
        "sampledPeakResidentDeltaFromBaselineBytes.p99",
    )?;
    let conservative_process_ram = ceil_nonnegative(
        report.aggregates.absolute_process_peak_resident_bytes.p99,
        "absoluteProcessPeakResidentBytes.p99",
    )?;
    let p99_load = ceil_nonnegative(
        report.aggregates.fresh_provider_load_ms.p99,
        "freshProviderLoadMs.p99",
    )?;
    let p99_reload = ceil_nonnegative(
        report.aggregates.reload_after_unload_ms.p99,
        "reloadAfterUnloadMs.p99",
    )?;
    let p99_operation = ceil_nonnegative(
        report
            .aggregates
            .fresh_inference_ms
            .p99
            .max(report.aggregates.reload_inference_ms.p99),
        "inference p99",
    )?;

    let report_digest = Sha256Digest::of_bytes(&report_bytes);
    let payload = MeasuredResourceEnvelopePayloadV1 {
        schema: MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1.to_owned(),
        report_id: format!("yunet-native-{}", &report_digest.as_str()[..32]),
        sequence: measured_unix_seconds,
        measured_unix_seconds,
        expires_unix_seconds: measured_unix_seconds + ENVELOPE_LIFETIME_SECONDS,
        device_fingerprint_sha256: fingerprint.clone(),
        identity: manifest.core_manifest.identity(),
        manifest_sha256: manifest.core_manifest.digest()?,
        capability: ModelPackKindV1::Vision,
        benchmark_suite_revision: report.suite_version.clone(),
        runtime: "onnxruntime".to_owned(),
        runtime_revision: RUNTIME_REVISION.to_owned(),
        backend: BACKEND.to_owned(),
        // Every placement statistic is backed by at least this many observations:
        // 20 fresh loads, 20 reloads, and 20 operations of each session kind.
        sample_count: u32::try_from(report.iteration_count)?,
        placements: BTreeMap::from([(
            ResidencyModeV1::CpuResident,
            PlacementMeasurementV1 {
                // The qualifier repeatedly unloads and reloads ORT in one process.
                // Allocator retention can make a per-session baseline delta look
                // smaller than the RAM a fresh process needs. Reserve the full
                // observed process peak for both admission fields.
                resident_ram_bytes: conservative_process_ram,
                p99_total_ram_bytes: conservative_process_ram,
                resident_vram_bytes: 0,
                p99_workspace_vram_bytes: 0,
                p99_load_millis: p99_load,
                p99_reload_millis: p99_reload,
                p99_operation_millis: p99_operation,
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

    let output_directory = PathBuf::from(&args[2])
        .join(PACK_ID)
        .join(PACK_REVISION)
        .join(fingerprint.as_str());
    fs::create_dir_all(&output_directory)?;
    write_pretty_json_atomic(&output_directory.join("cpu_resident.json"), &envelope)?;
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
        "native_qualification_report_sha256": report_digest,
        "native_report_started_at_utc": report.started_at_utc,
        "native_report_completed_at_utc": report.completed_at_utc,
        "runtime": envelope.signed.runtime,
        "runtime_revision": envelope.signed.runtime_revision,
        "backend": envelope.signed.backend,
        "sample_count": envelope.signed.sample_count,
        "native_session_count": report.session_count,
        "measurement_mapping": {
            "resident_ram_bytes": "p99 absoluteProcessPeakResidentBytes (conservative fresh-process reservation; sampled baseline delta retained separately)",
            "p99_total_ram_bytes": "p99 absoluteProcessPeakResidentBytes (conservatively includes qualifier host baseline)",
            "p99_load_millis": "p99 freshProviderLoadMs",
            "p99_reload_millis": "p99 reloadAfterUnloadMs",
            "p99_operation_millis": "max(p99 freshInferenceMs, p99 reloadInferenceMs)"
        },
        "observed_p99_resident_delta_bytes": observed_resident_delta,
        "native_inference_qualification_validated": true,
        "provider_load_self_test_attested": false,
        "natural_tracking_quality_claimed": false,
        "fixture_idle": report.scope.fixture_idle,
        "game_pressure_measured": report.scope.game_pressure_measured,
        "capture_measured": report.scope.capture_measured,
        "sixty_fps_measured": report.scope.sixty_fps_measured,
        "os_disk_cache_state_controlled": report.scope.os_disk_cache_state_controlled,
        "whole_loadout_fit_deferred_to_activation": true,
        "catalog_trust_scope": "automated_local_review_bootstrap",
        "production_trust": false,
        "rotation_required_before_release": true,
        "promotion_supported": false,
        "publication_supported": false
    });
    write_pretty_json_atomic(&output_directory.join("import-context.json"), &context)?;
    Ok(())
}

fn validate_manifest(
    manifest: &model_manager::NormalizedModelPackManifestV2,
) -> Result<(), Box<dyn std::error::Error>> {
    if manifest.origin != ModelPackNormalizationOriginV2::CanonicalV2
        || manifest.core_manifest.pack_id.as_str() != PACK_ID
        || manifest.core_manifest.revision.as_str() != PACK_REVISION
        || manifest.core_manifest.capability.kind != ModelPackKindV1::Vision
        || manifest.document.runtime.runtime != "onnxruntime"
        || manifest.document.runtime.immutable_revision.as_deref() != Some(RUNTIME_REVISION)
        || manifest.document.runtime.abi != ABI
        || manifest.document.runtime.backends.len() != 1
        || !manifest.document.runtime.backends.contains(BACKEND)
        || manifest.document.resources.measurement.minimum_samples < MINIMUM_ITERATIONS as u32
        || manifest.document.self_test.kind != "yunet-lm1-provider-load-only-v1"
    {
        return Err("YuNet canonical manifest identity/runtime contract mismatch".into());
    }

    if manifest.document.artifacts.len() != 5 {
        return Err("YuNet canonical manifest must contain exactly five pinned artifacts".into());
    }
    let required = [
        (
            "yunet-2023mar-onnx",
            ModelPackArtifactRoleV2::ModelWeights,
            DETECTOR_SIZE,
            DETECTOR_SHA256,
        ),
        (
            "lm-model1-opt",
            ModelPackArtifactRoleV2::ModelWeights,
            LANDMARK_SIZE,
            LANDMARK_SHA256,
        ),
        (
            "yunet-license",
            ModelPackArtifactRoleV2::License,
            YUNET_LICENSE_SIZE,
            YUNET_LICENSE_SHA256,
        ),
        (
            "openseeface-license",
            ModelPackArtifactRoleV2::License,
            OPENSEEFACE_LICENSE_SIZE,
            OPENSEEFACE_LICENSE_SHA256,
        ),
        (
            "onnxruntime-1.22.1-cpu-windows-x64",
            ModelPackArtifactRoleV2::Runtime,
            RUNTIME_ARCHIVE_SIZE,
            RUNTIME_ARCHIVE_SHA256,
        ),
    ];
    for (id, role, size, sha) in required {
        let artifact = manifest
            .document
            .artifacts
            .iter()
            .find(|artifact| artifact.id == id)
            .ok_or_else(|| format!("canonical manifest is missing artifact {id}"))?;
        if artifact.role != role || artifact.size_bytes != size || artifact.sha256.as_str() != sha {
            return Err(format!(
                "canonical manifest artifact {id} does not match the pinned tuple"
            )
            .into());
        }
    }
    Ok(())
}

fn validate_native_report(report: &NativeReport) -> Result<(), Box<dyn std::error::Error>> {
    if report.schema != NATIVE_REPORT_SCHEMA
        || report.suite_version != "native-yunet-envelope-2026-09-05.1"
        || report.pack.id != PACK_ID
        || report.pack.revision != PACK_REVISION
        || report.scope.provider != "native-windows-ort"
        || report.scope.backend != BACKEND
        || report.scope.runtime_revision != RUNTIME_REVISION
        || report.scope.inference_threads != 1
        || report.scope.gpu_used
        || !report.scope.fixture_decode_excluded
        || !report.scope.current_device_only
        || !report.scope.fixture_idle
        || report.scope.game_pressure_measured
        || report.scope.capture_measured
        || report.scope.sixty_fps_measured
        || report.scope.os_disk_cache_state_controlled
        || report.scope.telemetry_fingerprint != "issuer-collected"
    {
        return Err("native report identity/scope contract mismatch".into());
    }
    let started = parse_utc_timestamp(&report.started_at_utc)?;
    let completed = parse_utc_timestamp(&report.completed_at_utc)?;
    if completed < started {
        return Err("native report completed before it started".into());
    }
    if report.device.cpu.trim().is_empty()
        || report.device.logical_processors == 0
        || report.device.page_size_bytes == 0
        || report.device.physical_memory_bytes == 0
    {
        return Err("native report device observation is incomplete".into());
    }
    validate_artifacts(&report.artifacts)?;

    if report.iteration_count < MINIMUM_ITERATIONS
        || report.session_count
            != report
                .iteration_count
                .checked_mul(2)
                .ok_or("session count overflow")?
        || report.sessions.len() != report.session_count
    {
        return Err(
            "native report must contain paired fresh/reload sessions for at least 20 iterations"
                .into(),
        );
    }

    let mut observed = BTreeSet::new();
    let mut fresh_load = Vec::new();
    let mut reload_load = Vec::new();
    let mut fresh_inference = Vec::new();
    let mut reload_inference = Vec::new();
    let mut resident_peak = Vec::new();
    let mut private_peak = Vec::new();
    let mut absolute_peak = Vec::new();
    let mut resident_delta = Vec::new();
    let mut private_delta = Vec::new();

    for session in &report.sessions {
        if session.iteration >= report.iteration_count
            || !matches!(
                session.kind.as_str(),
                "freshProviderLoad" | "reloadAfterUnload"
            )
            || !observed.insert((session.iteration, session.kind.clone()))
        {
            return Err("native report has an invalid or duplicate iteration/session kind".into());
        }
        for (name, value) in [
            ("loadMs", session.load_ms),
            ("inferenceMs", session.inference_ms),
            ("unloadMs", session.unload_ms),
            ("detectorMs", session.detector_ms),
            ("landmarkMs", session.landmark_ms),
        ] {
            require_finite_nonnegative(value, name)?;
        }
        if session.load_ms <= 0.0
            || session.inference_ms <= 0.0
            || session.unload_ms <= 0.0
            || session.detector_ms <= 0.0
            || session.landmark_ms <= 0.0
        {
            return Err("native timing observations must be greater than zero".into());
        }
        for (name, value) in [
            ("detectorConfidence", session.detector_confidence),
            ("landmarkConfidence", session.landmark_confidence),
            ("visibilityRatio", session.visibility_ratio),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("{name} must be finite and between zero and one").into());
            }
        }
        if session.detector_confidence <= 0.0
            || session.landmark_confidence <= 0.0
            || session.visibility_ratio <= 0.0
            || !session.packet_bound
            || !session.cancellation_advanced
            || !session.stale_generation_rejected
            || !session.unloaded
        {
            return Err("native inference/cancellation/unload qualification did not pass".into());
        }
        let memory = &session.memory;
        if memory.before_load_resident_bytes == 0
            || memory.after_load_resident_bytes == 0
            || memory.after_inference_resident_bytes == 0
            || memory.after_unload_resident_bytes == 0
            || memory.sampled_peak_resident_bytes
                < memory
                    .before_load_resident_bytes
                    .max(memory.after_load_resident_bytes)
                    .max(memory.after_inference_resident_bytes)
                    .max(memory.after_unload_resident_bytes)
            || memory.sampled_peak_private_bytes == 0
            || memory.absolute_process_peak_resident_bytes < memory.sampled_peak_resident_bytes
            || memory.sampled_peak_resident_delta_from_baseline_bytes <= 0
            || memory.sampled_peak_private_delta_from_baseline_bytes <= 0
        {
            return Err(
                "native report contains impossible or unusable Win32 RAM observations".into(),
            );
        }

        match session.kind.as_str() {
            "freshProviderLoad" => {
                fresh_load.push(session.load_ms);
                fresh_inference.push(session.inference_ms);
            }
            "reloadAfterUnload" => {
                reload_load.push(session.load_ms);
                reload_inference.push(session.inference_ms);
            }
            _ => unreachable!(),
        }
        resident_peak.push(memory.sampled_peak_resident_bytes as f64);
        private_peak.push(memory.sampled_peak_private_bytes as f64);
        absolute_peak.push(memory.absolute_process_peak_resident_bytes as f64);
        resident_delta.push(memory.sampled_peak_resident_delta_from_baseline_bytes as f64);
        private_delta.push(memory.sampled_peak_private_delta_from_baseline_bytes as f64);
    }
    for iteration in 0..report.iteration_count {
        for kind in ["freshProviderLoad", "reloadAfterUnload"] {
            if !observed.contains(&(iteration, kind.to_owned())) {
                return Err(
                    format!("native report is missing iteration {iteration} {kind}").into(),
                );
            }
        }
    }

    verify_statistics(
        "freshProviderLoadMs",
        &fresh_load,
        &report.aggregates.fresh_provider_load_ms,
    )?;
    verify_statistics(
        "reloadAfterUnloadMs",
        &reload_load,
        &report.aggregates.reload_after_unload_ms,
    )?;
    verify_statistics(
        "freshInferenceMs",
        &fresh_inference,
        &report.aggregates.fresh_inference_ms,
    )?;
    verify_statistics(
        "reloadInferenceMs",
        &reload_inference,
        &report.aggregates.reload_inference_ms,
    )?;
    verify_statistics(
        "sampledPeakResidentBytes",
        &resident_peak,
        &report.aggregates.sampled_peak_resident_bytes,
    )?;
    verify_statistics(
        "sampledPeakPrivateBytes",
        &private_peak,
        &report.aggregates.sampled_peak_private_bytes,
    )?;
    verify_statistics(
        "absoluteProcessPeakResidentBytes",
        &absolute_peak,
        &report.aggregates.absolute_process_peak_resident_bytes,
    )?;
    verify_statistics(
        "sampledPeakResidentDeltaFromBaselineBytes",
        &resident_delta,
        &report
            .aggregates
            .sampled_peak_resident_delta_from_baseline_bytes,
    )?;
    verify_statistics(
        "sampledPeakPrivateDeltaFromBaselineBytes",
        &private_delta,
        &report
            .aggregates
            .sampled_peak_private_delta_from_baseline_bytes,
    )?;

    if report.counters.fresh_provider_loads != report.iteration_count
        || report.counters.reloads_after_unload != report.iteration_count
        || report.counters.packet_bound != report.session_count
        || report.counters.cancellation_passed != report.session_count
        || report.counters.stale_generation_rejected != report.session_count
        || report.counters.unload_passed != report.session_count
    {
        return Err("native report counters do not match the session evidence".into());
    }
    Ok(())
}

fn validate_artifacts(artifacts: &[ArtifactEvidence]) -> Result<(), Box<dyn std::error::Error>> {
    if artifacts.len() != 7 {
        return Err(
            "native report must bind exactly seven source/input/runtime/qualifier artifacts".into(),
        );
    }
    let expected = BTreeMap::from([
        ("detector", Some((DETECTOR_SIZE, DETECTOR_SHA256))),
        ("landmark", Some((LANDMARK_SIZE, LANDMARK_SHA256))),
        ("runtime", Some((RUNTIME_SIZE, RUNTIME_SHA256))),
        (
            "runtimeShared",
            Some((RUNTIME_SHARED_SIZE, RUNTIME_SHARED_SHA256)),
        ),
        (
            "runtimeArchive",
            Some((RUNTIME_ARCHIVE_SIZE, RUNTIME_ARCHIVE_SHA256)),
        ),
        ("qualifierExecutable", None),
        ("sourceFrame", None),
    ]);
    let mut roles = BTreeSet::new();
    for artifact in artifacts {
        if !roles.insert(artifact.role.as_str()) || !expected.contains_key(artifact.role.as_str()) {
            return Err(format!(
                "unexpected or duplicate report artifact role {}",
                artifact.role
            )
            .into());
        }
        if !is_lower_sha256(&artifact.sha256)
            || artifact.size_bytes == 0
            || !Path::new(&artifact.path).is_absolute()
        {
            return Err(format!(
                "artifact {} has invalid path/size/hash evidence",
                artifact.role
            )
            .into());
        }
        let extension = Path::new(&artifact.path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        if (artifact.role == "qualifierExecutable" && extension.as_deref() != Some("exe"))
            || (artifact.role == "sourceFrame" && extension.as_deref() != Some("ppm"))
            || (artifact.role == "runtimeArchive" && extension.as_deref() != Some("zip"))
        {
            return Err(format!("artifact {} has an unexpected file type", artifact.role).into());
        }
        if let Some((size, sha)) = expected[artifact.role.as_str()] {
            if artifact.size_bytes != size || artifact.sha256 != sha {
                return Err(format!(
                    "artifact {} does not match the canonical pinned tuple",
                    artifact.role
                )
                .into());
            }
        }
        let bytes = fs::read(&artifact.path)?;
        if bytes.len() as u64 != artifact.size_bytes
            || Sha256Digest::of_bytes(&bytes).as_str() != artifact.sha256
        {
            return Err(format!(
                "artifact {} no longer matches its native report evidence",
                artifact.role
            )
            .into());
        }
    }
    Ok(())
}

fn verify_statistics(
    name: &str,
    values: &[f64],
    reported: &Statistics,
) -> Result<(), Box<dyn std::error::Error>> {
    if values.is_empty() || reported.count != values.len() {
        return Err(format!("aggregate {name} has the wrong sample count").into());
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let expected = [
        values.iter().sum::<f64>() / values.len() as f64,
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        percentile(&sorted, 0.99),
        *sorted.last().ok_or("empty statistics")?,
    ];
    let actual = [
        reported.mean,
        reported.p50,
        reported.p95,
        reported.p99,
        reported.maximum,
    ];
    for (expected, actual) in expected.into_iter().zip(actual) {
        if !actual.is_finite() || (expected - actual).abs() > aggregate_tolerance(expected) {
            return Err(
                format!("aggregate {name} does not match its raw session observations").into(),
            );
        }
    }
    Ok(())
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    let position = (sorted.len() - 1) as f64 * quantile;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    sorted[lower] + (sorted[upper] - sorted[lower]) * (position - lower as f64)
}

fn aggregate_tolerance(expected: f64) -> f64 {
    // The native writer uses default stream precision, so large byte counters lose
    // a few low digits. This admits only the corresponding decimal serialization error.
    (expected.abs() * 5.0e-6).max(5.0e-5)
}

fn require_finite_nonnegative(value: f64, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{name} must be finite and nonnegative").into());
    }
    Ok(())
}

fn ceil_nonnegative(value: f64, name: &str) -> Result<u64, Box<dyn std::error::Error>> {
    require_finite_nonnegative(value, name)?;
    if value > u64::MAX as f64 {
        return Err(format!("{name} exceeds the resource envelope integer range").into());
    }
    Ok(value.ceil() as u64)
}

fn parse_utc_timestamp(value: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let bytes = value.as_bytes();
    let fixed = bytes.len() == 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z';
    let digits = bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
    });
    if !fixed || !digits {
        return Err("native report timestamps must use UTC YYYY-MM-DDTHH:MM:SS.mmmZ".into());
    }
    let number = |start: usize, end: usize| -> Result<u32, Box<dyn std::error::Error>> {
        Ok(value[start..end].parse()?)
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if year < 1970 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return Err("native report contains an invalid UTC timestamp".into());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day == 0 || day > month_days[(month - 1) as usize] {
        return Err("native report contains an invalid UTC calendar date".into());
    }
    let days_before_year = |year: u32| -> u64 {
        let previous = u64::from(year - 1);
        previous * 365 + previous / 4 - previous / 100 + previous / 400
    };
    let mut days = days_before_year(year) - days_before_year(1970);
    days += month_days[..(month - 1) as usize]
        .iter()
        .map(|days| u64::from(*days))
        .sum::<u64>();
    days += u64::from(day - 1);
    Ok(days * 86_400 + u64::from(hour) * 3_600 + u64::from(minute) * 60 + u64::from(second))
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn read_signers() -> Result<Vec<(String, SigningKey)>, Box<dyn std::error::Error>> {
    let raw = Zeroizing::new(env::var("NPC_MODEL_CATALOG_SIGNERS")?);
    // This is a single-threaded issuer utility. Remove the inherited environment
    // copy as soon as it has been placed in zeroizing storage.
    env::remove_var("NPC_MODEL_CATALOG_SIGNERS");
    let mut signers = Vec::new();
    let mut key_ids = BTreeSet::new();
    let mut public_keys = BTreeSet::new();
    for item in raw.split(',') {
        let (key_id, seed) = item.split_once('=').ok_or("invalid signer")?;
        if key_id.is_empty() || !key_ids.insert(key_id.to_owned()) {
            return Err("signer key IDs must be nonempty and distinct".into());
        }
        let decoded = Zeroizing::new(hex::decode(seed)?);
        if decoded.len() != 32 {
            return Err("signer seed must be 32 bytes".into());
        }
        let mut seed = [0_u8; 32];
        seed.copy_from_slice(decoded.as_slice());
        let signer = SigningKey::from_bytes(&seed);
        seed.zeroize();
        if !public_keys.insert(signer.verifying_key().to_bytes()) {
            return Err("signer key materials must be distinct".into());
        }
        signers.push((key_id.to_owned(), signer));
    }
    if signers.len() < 2 {
        return Err("at least two distinct signer keys are required".into());
    }
    Ok(signers)
}

fn write_pretty_json_atomic(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let parent = path.parent().ok_or("output file has no parent directory")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("output file name is not UTF-8")?;
    let temporary = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if let Err(error) = output.write_all(&bytes).and_then(|()| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    drop(output);
    // Create the final name without replacement. `rename` replaces an existing
    // destination on some supported filesystems, which is unsafe for signed
    // evidence output selected by the caller.
    if let Err(error) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    fs::remove_file(&temporary)?;
    Ok(())
}
