#![allow(clippy::unwrap_used)]

use model_manager::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn pack_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/model-packs")
        .join(name)
}

fn example_bytes() -> Vec<u8> {
    fs::read(pack_path("model-pack-manifest.example.json")).unwrap()
}

#[test]
fn canonical_v2_example_is_strict_deterministic_and_core_normalizable() {
    let first = parse_and_normalize_model_pack_manifest(&example_bytes()).unwrap();
    let second = parse_and_normalize_model_pack_manifest(&example_bytes()).unwrap();
    assert_eq!(first.origin, ModelPackNormalizationOriginV2::CanonicalV2);
    assert_eq!(first.document.schema, MODEL_PACK_MANIFEST_SCHEMA_V2);
    assert_eq!(first.core_manifest.schema, MODEL_PACK_MANIFEST_SCHEMA_V1);
    assert_eq!(
        first.core_manifest.identity(),
        second.core_manifest.identity()
    );
    assert_eq!(
        first.canonical_document_sha256,
        second.canonical_document_sha256
    );
    assert!(
        first
            .document
            .resources
            .measurement
            .signed_evidence_required
    );
    assert!(
        first
            .document
            .resources
            .measurement
            .manifest_values_not_for_admission
    );
    assert_eq!(
        first.document.admission.state,
        ModelPackAdmissionStateV2::CandidateUnqualified
    );
}

#[test]
fn strict_parser_rejects_unknowns_role_mismatch_and_missing_archive_format() {
    let mut value: serde_json::Value = serde_json::from_slice(&example_bytes()).unwrap();
    value["unexpected"] = json!(true);
    assert!(matches!(
        parse_and_normalize_model_pack_manifest(&serde_json::to_vec(&value).unwrap()),
        Err(ModelPackManifestV2Error::Json(_))
    ));

    let mut mismatch: serde_json::Value = serde_json::from_slice(&example_bytes()).unwrap();
    mismatch["capability"]["kind"] = json!("vision");
    assert!(matches!(
        parse_and_normalize_model_pack_manifest(&serde_json::to_vec(&mismatch).unwrap()),
        Err(ModelPackManifestV2Error::RoleExtensionMismatch)
    ));

    let mut archive: serde_json::Value = serde_json::from_slice(&example_bytes()).unwrap();
    archive["artifacts"][0]["kind"] = json!("archive");
    assert!(matches!(
        parse_and_normalize_model_pack_manifest(&serde_json::to_vec(&archive).unwrap()),
        Err(ModelPackManifestV2Error::ArchiveFormatRequired(_))
    ));

    let mut omitted_nullable: serde_json::Value = serde_json::from_slice(&example_bytes()).unwrap();
    omitted_nullable["extensions"]["lip_sync"]
        .as_object_mut()
        .unwrap()
        .remove("causal");
    assert!(matches!(
        parse_and_normalize_model_pack_manifest(
            &serde_json::to_vec(&omitted_nullable).unwrap()
        ),
        Err(ModelPackManifestV2Error::MissingRequiredField(path)) if path == "extensions.lip_sync.causal"
    ));
}

#[test]
fn tar_bz2_is_explicit_and_normalizes_without_relabeling() {
    let mut value: serde_json::Value = serde_json::from_slice(&example_bytes()).unwrap();
    value["artifacts"][0]["kind"] = json!("archive");
    value["artifacts"][0]["archive_format"] = json!("tar_bz2");
    value["artifacts"][0]["strip_prefix"] = json!("fixture-root");
    value["artifacts"][0]["required_paths"] = json!(["model.example"]);
    let normalized =
        parse_and_normalize_model_pack_manifest(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        normalized.core_manifest.artifacts[0].archive_format,
        Some(ArchiveFormatV1::TarBz2)
    );
}

#[test]
fn generic_v1_adapter_is_deterministic_blocked_and_never_promotes_estimates() {
    let canonical = parse_and_normalize_model_pack_manifest(&example_bytes()).unwrap();
    let mut value = serde_json::to_value(&canonical.core_manifest).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("$schema".to_owned(), json!("legacy-v1-schema.json"));
    let bytes = serde_json::to_vec(&value).unwrap();
    let first = parse_and_normalize_model_pack_manifest(&bytes).unwrap();
    let second = parse_and_normalize_model_pack_manifest(&bytes).unwrap();
    assert_eq!(
        first.origin,
        ModelPackNormalizationOriginV2::DeterministicGenericV1Adapter
    );
    assert_eq!(
        first.document.admission.state,
        ModelPackAdmissionStateV2::BlockedPendingMeasurement
    );
    assert!(
        first
            .document
            .resources
            .measurement
            .manifest_values_not_for_admission
    );
    assert_eq!(
        first.canonical_document_sha256,
        second.canonical_document_sha256
    );
}

#[test]
fn role_specific_legacy_schemas_name_the_required_adapter() {
    let stt = br#"{"schemaVersion":"npc.local-model-pack/v2"}"#;
    assert!(matches!(
        parse_and_normalize_model_pack_manifest(stt),
        Err(ModelPackManifestV2Error::LegacyRoleSchema {
            adapter: "moonshine_local_model_v2_to_npc_model_pack_v2",
            ..
        })
    ));
    let tts = br#"{"schema_version":"npc.local-tts-pack/v1"}"#;
    assert!(matches!(
        parse_and_normalize_model_pack_manifest(tts),
        Err(ModelPackManifestV2Error::LegacyRoleSchema {
            adapter: "local_tts_v1_to_npc_model_pack_v2",
            ..
        })
    ));
}

#[test]
fn migrated_qwen_manifest_normalizes_with_null_planning_measurements() {
    let bytes = fs::read(pack_path("qwen3-4b-instruct-2507-q4-k-m.json")).unwrap();
    let normalized = parse_and_normalize_model_pack_manifest(&bytes).unwrap();
    assert_eq!(
        normalized.core_manifest.capability.kind,
        ModelPackKindV1::LanguageModel
    );
    assert_eq!(
        normalized.document.resources.planning_resident_ram_bytes,
        None
    );
    assert_eq!(
        normalized.document.resources.planning_resident_vram_bytes,
        None
    );
    assert_eq!(normalized.document.resources.planning_load_millis, None);
    assert_eq!(
        normalized.document.admission.state,
        ModelPackAdmissionStateV2::BlockedPendingMeasurement
    );
}

#[test]
fn every_distributed_real_manifest_is_canonical_v2_and_fail_closed() {
    let directory = pack_path("");
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .filter(|path| {
            !matches!(
                path.file_name().and_then(|value| value.to_str()),
                Some(
                    "model-pack-manifest.schema.json"
                        | "model-pack-manifest.example.json"
                        | "model-catalog-root-v1.json"
                        | "model-catalog-v1.json"
                        | "review-evidence-index-v1.json"
                )
            )
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let bytes = fs::read(&path).unwrap();
        let normalized = parse_and_normalize_model_pack_manifest(&bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            normalized.origin,
            ModelPackNormalizationOriginV2::CanonicalV2,
            "{}",
            path.display()
        );
        assert_ne!(
            normalized.document.admission.state,
            ModelPackAdmissionStateV2::EligibleAfterExternalAdmission,
            "{} must not claim admission before a separate current-device envelope is verified",
            path.display()
        );
    }
}
