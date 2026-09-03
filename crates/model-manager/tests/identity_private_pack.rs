#![allow(clippy::unwrap_used)]

use model_manager::{
    parse_and_normalize_model_pack_manifest, Accelerator, LicensePermission,
    ModelPackAdmissionStateV2, ModelPackKindV1, ModelPackNormalizationOriginV2, ModelPackScopeV1,
    QualityTier, ResidencyModeV1,
};
use std::path::{Component, Path};

const MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packaging/model-packs/opencv-yunet-sface-private-evaluation.json"
));

const QUALIFICATION_PLAN_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/identity/identity-open-set-fixture-v1.json"
));

#[test]
fn strict_v2_identity_pack_has_permissive_model_provenance_but_is_not_measured() {
    let normalized = parse_and_normalize_model_pack_manifest(MANIFEST_JSON.as_bytes())
        .expect("strict identity v2 manifest must normalize");
    let manifest = &normalized.document;

    assert_eq!(
        normalized.origin,
        ModelPackNormalizationOriginV2::CanonicalV2
    );
    assert_eq!(manifest.pack_id.as_str(), "opencv-yunet-sface-private-eval");
    assert_eq!(manifest.capability.kind, ModelPackKindV1::Vision);
    assert_eq!(manifest.capability.scope, ModelPackScopeV1::Generic);
    assert_eq!(manifest.hardware.accelerators.len(), 1);
    assert!(manifest.hardware.accelerators.contains(&Accelerator::Cpu));
    assert_eq!(manifest.hardware.minimum_vram_bytes, Some(0));
    assert_eq!(manifest.hardware.recommended_vram_bytes, Some(0));

    assert!(manifest.license.redistributable);
    assert_eq!(manifest.license.commercial_use, LicensePermission::Allowed);
    assert_eq!(manifest.license.derivative_use, LicensePermission::Allowed);
    assert!(!manifest.license.acceptance_required);
    assert!(manifest
        .license
        .components
        .iter()
        .any(|component| component.id == "sface-model-apache2"
            && component.redistributable == Some(true)));
    assert!(manifest.artifacts.iter().any(|artifact| {
        artifact.id == "opencv-zoo-commercial-use-report"
            && artifact.sha256.as_str()
                == "bd1ccd892cfd306829fe219de9950b3d97847dbb7d5cfdf3453361701c089ec3"
    }));

    assert_eq!(manifest.resources.quality_tier, QualityTier::Experimental);
    assert_eq!(manifest.resources.planning_resident_ram_bytes, None);
    assert_eq!(manifest.resources.planning_resident_vram_bytes, None);
    assert_eq!(manifest.resources.planning_load_millis, None);
    assert_eq!(manifest.resources.planning_hardware, None);
    assert_eq!(manifest.resources.measurement.minimum_samples, 20);
    assert!(manifest.resources.measurement.signed_evidence_required);
    assert!(manifest.resources.measurement.p99_reload_required);
    assert!(
        manifest
            .resources
            .measurement
            .manifest_values_not_for_admission
    );
    assert_eq!(manifest.self_test.expected_output_sha256, None);

    assert_eq!(
        manifest.admission.state,
        ModelPackAdmissionStateV2::BlockedPendingMeasurement
    );
    assert_eq!(
        manifest.admission.allowed_residencies,
        [ResidencyModeV1::CpuResident].into_iter().collect()
    );
    assert!(manifest.admission.unknowns_fail_closed);
    assert!(!manifest.trust.unsigned_activation_allowed);
    assert!(manifest.trust.measured_resource_envelope_required);
}

#[test]
fn private_identity_pack_has_a_checked_in_fail_closed_qualification_plan() {
    let normalized = parse_and_normalize_model_pack_manifest(MANIFEST_JSON.as_bytes())
        .expect("strict identity v2 manifest must normalize");
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(&normalized.document.self_test.input_fixture);

    assert!(fixture_path.is_file(), "self-test fixture must exist");
    assert!(Path::new(&normalized.document.self_test.input_fixture)
        .components()
        .all(|component| !matches!(component, Component::ParentDir | Component::RootDir)));

    let plan: serde_json::Value = serde_json::from_str(QUALIFICATION_PLAN_JSON)
        .expect("qualification plan must be valid JSON");
    assert_eq!(plan["status"], "blocked_pending_consented_fixture_corpus");
    assert_eq!(plan["privacy"]["local_only"], true);
    assert_eq!(plan["privacy"]["protected_trait_inference"], false);
    assert_eq!(
        plan["admission"]["expected_output_sha256"],
        serde_json::Value::Null
    );
    assert_eq!(plan["admission"]["production_qualified"], false);
    assert_eq!(plan["admission"]["redistribution_qualified"], false);
    assert_eq!(plan["admission"]["commercial_use_qualified"], false);
}

#[test]
fn unmeasured_identity_v2_pack_cannot_claim_identity_authority() {
    let normalized = parse_and_normalize_model_pack_manifest(MANIFEST_JSON.as_bytes())
        .expect("strict identity v2 manifest must normalize");
    let vision = normalized
        .document
        .extensions
        .vision
        .as_ref()
        .expect("vision extension");

    assert_eq!(vision.identity_recognition, Some(false));
    assert!(vision
        .output_signals
        .as_ref()
        .is_some_and(|signals| signals.contains("face_embedding_128_f32_l2_normalized_untrusted")));
    assert_eq!(vision.maximum_signal_rate_hz, None);
}
