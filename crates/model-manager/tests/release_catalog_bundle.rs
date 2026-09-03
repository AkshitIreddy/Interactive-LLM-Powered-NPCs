#![allow(clippy::unwrap_used)]

use ed25519_dalek::SigningKey;
use model_manager::*;
use std::fs;
use std::path::PathBuf;

fn pack_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/model-packs")
        .join(name)
}

fn source(name: &str) -> ReleaseManifestSourceV1 {
    let bytes = fs::read(pack_path(name)).unwrap();
    ReleaseManifestSourceV1 {
        file_name: name.to_owned(),
        raw_document_sha256: Sha256Digest::of_bytes(&bytes),
        normalized: parse_and_normalize_model_pack_manifest(&bytes).unwrap(),
        review_evidence: vec![],
        qualified_envelopes: vec![],
    }
}

#[test]
fn checked_local_review_bootstrap_bundle_verifies_from_distributed_files() {
    let directory = pack_path("");
    let (root, bundle) = load_release_catalog_bundle_v1(&directory).unwrap();
    assert_eq!(
        root.trust_scope,
        ReleaseCatalogTrustScopeV1::AutomatedLocalReviewBootstrap
    );
    assert!(!root.production_trust);
    let verified = verify_release_catalog_bundle_v1(
        &root,
        &bundle,
        &CatalogTrustState::default(),
        bundle.signed.generated_unix_seconds + 1,
    )
    .unwrap();
    verify_release_manifest_files_v1(&directory, &verified).unwrap();
    verify_release_envelope_files_v1(
        &directory,
        &verified,
        bundle.signed.generated_unix_seconds + 1,
    )
    .unwrap();
    let snapshots = verified.trusted_pack_snapshots().unwrap();
    assert_eq!(snapshots.len(), 6);
    assert!(snapshots.iter().all(|pack| {
        pack.admission_state == ModelPackAdmissionStateV2::BlockedPendingMeasurement
            && pack.measurement_status == TrustedPackMeasurementStatusV1::Unavailable
            && pack.qualified_measurement.is_none()
    }));
    assert_eq!(
        snapshots
            .iter()
            .map(|pack| pack.qualified_envelope_count)
            .sum::<usize>(),
        2
    );
    let bge = snapshots
        .iter()
        .find(|pack| pack.identity.pack_id.as_str() == "bge-small-en-v1.5-onnx-fp32")
        .unwrap();
    assert_eq!(bge.qualified_envelope_count, 1);
    let visual = snapshots
        .iter()
        .find(|pack| pack.identity.pack_id.as_str() == "openseeface-mnv3-lm1-mouth-signal")
        .unwrap();
    assert_eq!(visual.qualified_envelope_count, 1);
    assert_eq!(
        snapshots
            .iter()
            .map(|pack| pack.non_qualifying_review_evidence_count)
            .sum::<usize>(),
        3
    );
}

fn fixture() -> (ReleaseCatalogRootV1, SignedReleaseCatalogBundleV1) {
    let signers = vec![
        ReleaseCatalogSignerV1 {
            key_id: "fixture-release-key-a".to_owned(),
            signing_key: SigningKey::from_bytes(&[11; 32]),
        },
        ReleaseCatalogSignerV1 {
            key_id: "fixture-release-key-b".to_owned(),
            signing_key: SigningKey::from_bytes(&[22; 32]),
        },
    ];
    build_release_catalog_bundle_v1(
        vec![
            source("qwen3-4b-instruct-2507-q4-k-m.json"),
            source("openseeface-mnv3-lm1-mouth-signal.json"),
        ],
        7,
        1_000,
        2_000,
        &signers,
        2,
    )
    .unwrap()
}

#[test]
fn bundle_binds_v2_source_core_license_admission_and_empty_real_envelopes() {
    let (root, bundle) = fixture();
    assert_eq!(
        root.trust_scope,
        ReleaseCatalogTrustScopeV1::AutomatedLocalReviewBootstrap
    );
    assert!(!root.production_trust);
    assert!(root.rotation_required_before_release);
    assert!(!root.promotion_supported);
    assert!(!root.publication_supported);
    let verified =
        verify_release_catalog_bundle_v1(&root, &bundle, &CatalogTrustState::default(), 1_500)
            .unwrap();
    assert_eq!(verified.source_inventory.manifests.len(), 2);
    for binding in &verified.source_inventory.manifests {
        assert_eq!(binding.source_schema, MODEL_PACK_MANIFEST_SCHEMA_V2);
        assert!(binding.qualified_envelopes.is_empty());
        assert!(!binding.license.license_name.is_empty());
        assert!(matches!(
            binding.admission_state,
            ModelPackAdmissionStateV2::BlockedPendingMeasurement
        ));
        assert_eq!(
            verified
                .catalog
                .entry(&binding.identity)
                .unwrap()
                .manifest_sha256,
            binding.normalized_manifest_sha256
        );
    }
}

#[test]
fn tamper_rollback_and_missing_files_fail_closed() {
    let (root, bundle) = fixture();
    let mut tampered = bundle.clone();
    tampered.source_inventory.manifests[0]
        .admission_reason
        .push_str(" tampered");
    assert!(matches!(
        verify_release_catalog_bundle_v1(&root, &tampered, &CatalogTrustState::default(), 1_500),
        Err(ReleaseCatalogBundleError::InventorySignatureThreshold { .. })
    ));

    let accepted =
        verify_release_catalog_bundle_v1(&root, &bundle, &CatalogTrustState::default(), 1_500)
            .unwrap();
    let mut rollback = bundle.clone();
    rollback.signed.version = 6;
    assert!(matches!(
        verify_release_catalog_bundle_v1(&root, &rollback, &accepted.trust_state, 1_500),
        Err(ReleaseCatalogBundleError::Catalog(
            CatalogError::Rollback { .. }
        ))
    ));

    let directory = tempfile::tempdir().unwrap();
    assert!(matches!(
        load_release_catalog_bundle_v1(directory.path()),
        Err(ReleaseCatalogBundleError::Read(_, _))
    ));
}

#[test]
fn distributed_source_files_are_required_and_bound_to_signed_inventory() {
    let (root, bundle) = fixture();
    let verified =
        verify_release_catalog_bundle_v1(&root, &bundle, &CatalogTrustState::default(), 1_500)
            .unwrap();
    let directory = tempfile::tempdir().unwrap();
    for binding in &verified.source_inventory.manifests {
        fs::copy(
            pack_path(&binding.file_name),
            directory.path().join(&binding.file_name),
        )
        .unwrap();
    }
    verify_release_manifest_files_v1(directory.path(), &verified).unwrap();

    let first = &verified.source_inventory.manifests[0];
    fs::write(directory.path().join(&first.file_name), b"{}").unwrap();
    assert!(matches!(
        verify_release_manifest_files_v1(directory.path(), &verified),
        Err(ReleaseCatalogBundleError::SourceManifestDigestMismatch(_))
    ));
    fs::remove_file(directory.path().join(&first.file_name)).unwrap();
    assert!(matches!(
        verify_release_manifest_files_v1(directory.path(), &verified),
        Err(ReleaseCatalogBundleError::Read(_, _))
    ));
}

#[test]
fn distributed_qualified_envelope_is_required_and_bound_to_signed_inventory() {
    let source_directory = pack_path("");
    let (root, bundle) = load_release_catalog_bundle_v1(&source_directory).unwrap();
    let now = bundle.signed.generated_unix_seconds + 1;
    let verified =
        verify_release_catalog_bundle_v1(&root, &bundle, &CatalogTrustState::default(), now)
            .unwrap();
    verify_release_envelope_files_v1(&source_directory, &verified, now).unwrap();

    let directory = tempfile::tempdir().unwrap();
    let mut copied = Vec::new();
    for source in &verified.source_inventory.manifests {
        for binding in &source.qualified_envelopes {
            let relative =
                PathBuf::from("qual").join(format!("{}.json", binding.envelope_sha256.as_str()));
            let destination = directory.path().join(&relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(source_directory.join(&relative), &destination).unwrap();
            copied.push(destination);
        }
    }
    assert_eq!(copied.len(), 2);
    verify_release_envelope_files_v1(directory.path(), &verified, now).unwrap();

    let destination = &copied[0];
    fs::write(destination, b"{}\n").unwrap();
    assert!(matches!(
        verify_release_envelope_files_v1(directory.path(), &verified, now),
        Err(ReleaseCatalogBundleError::QualifiedEnvelopeDigestMismatch(
            _
        ))
    ));
    fs::remove_file(destination).unwrap();
    assert!(matches!(
        verify_release_envelope_files_v1(directory.path(), &verified, now),
        Err(ReleaseCatalogBundleError::Read(_, _))
    ));
}
