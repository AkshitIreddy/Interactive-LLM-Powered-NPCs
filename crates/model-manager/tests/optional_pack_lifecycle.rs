#![allow(clippy::unwrap_used)]

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use model_manager::*;
use std::collections::BTreeSet;
use tokio_util::sync::CancellationToken;

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::of_bytes(bytes)
}

fn manifest(payload: &[u8], source: String) -> ModelPackManifestV1 {
    ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: PackId::parse("optional.synthetic.embedding").unwrap(),
        revision: Revision::parse("fixture-r1").unwrap(),
        display_name: "Synthetic optional pack".to_owned(),
        description: "Tiny non-model lifecycle fixture.".to_owned(),
        source_project: "https://example.invalid/synthetic".to_owned(),
        source_revision: "fixture-source-r1".to_owned(),
        capability: ModelPackCapabilityV1 {
            kind: ModelPackKindV1::Embedding,
            scope: ModelPackScopeV1::Generic,
        },
        artifacts: vec![ArtifactV1 {
            id: "weights".to_owned(),
            kind: ArtifactKind::File,
            archive_format: None,
            source_urls: vec![source],
            size_bytes: payload.len() as u64,
            sha256: digest(payload),
            destination: "models/fixture.bin".to_owned(),
            strip_prefix: None,
            required_paths: vec![],
        }],
        runtime: RuntimeCompatibilityV1 {
            runtime: "synthetic-runtime".to_owned(),
            abi: "npc-synthetic-v1".to_owned(),
            minimum_runtime_revision: Some("fixture-r1".to_owned()),
            supported_platforms: BTreeSet::from([Platform::Windows10, Platform::Windows11]),
            supported_architectures: BTreeSet::from([Architecture::X86_64]),
        },
        hardware: HardwareRequirementsV1 {
            minimum_ram_bytes: 1,
            recommended_ram_bytes: 1,
            minimum_vram_bytes: None,
            recommended_vram_bytes: None,
            minimum_cpu_threads: 1,
            accelerators: BTreeSet::from([Accelerator::Cpu]),
            required_cpu_features: BTreeSet::new(),
        },
        resources: ResourceEnvelopeV1 {
            storage_bytes: payload.len() as u64,
            peak_install_bytes: payload.len() as u64 * 2,
            measured_ram_bytes: None,
            measured_vram_bytes: None,
            measured_load_millis: None,
            benchmark_hardware: None,
            quality_tier: QualityTier::Fast,
            languages: BTreeSet::from(["en".to_owned()]),
        },
        license: LicenseMetadataV1 {
            spdx_expression: Some("MIT".to_owned()),
            license_name: "MIT".to_owned(),
            license_url: "https://opensource.org/license/mit".to_owned(),
            attribution: "Synthetic fixture".to_owned(),
            redistributable: true,
            commercial_use: LicensePermission::Allowed,
            derivative_use: LicensePermission::Allowed,
            acceptance_required: true,
            notices: vec![],
        },
        self_test: SelfTestV1 {
            kind: "synthetic-no-inference".to_owned(),
            suite_revision: "fixture-r1".to_owned(),
            allowed_runtime_backends: BTreeSet::from(["synthetic-runtime".to_owned()]),
            input_fixture: "tests/synthetic.json".to_owned(),
            expected_output_sha256: None,
            timeout_millis: 1_000,
        },
    }
}

fn trusted_catalog(manifest: ModelPackManifestV1) -> TrustedCatalog {
    let payload = CatalogPayloadV1 {
        schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
        version: 1,
        generated_unix_seconds: 100,
        expires_unix_seconds: 1_000,
        entries: vec![CatalogEntryV1 {
            manifest_sha256: manifest.digest().unwrap(),
            manifest,
            channels: BTreeSet::from(["optional-local".to_owned()]),
            published_unix_seconds: 100,
            revoked: false,
            revocation_reason: None,
        }],
    };
    let keys = [
        ("fixture-a", SigningKey::from_bytes(&[31; 32])),
        ("fixture-b", SigningKey::from_bytes(&[47; 32])),
    ];
    let bytes = canonical_catalog_payload_bytes(&payload).unwrap();
    let signed = SignedCatalogV1 {
        signed: payload,
        signatures: keys
            .iter()
            .map(|(key_id, key)| CatalogSignatureV1 {
                key_id: (*key_id).to_owned(),
                algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
                signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(key.sign(&bytes).to_bytes()),
            })
            .collect(),
    };
    let verifier = Ed25519CatalogVerifier::new(
        keys.iter()
            .map(|(key_id, key)| ((*key_id).to_owned(), key.verifying_key().to_bytes())),
    )
    .unwrap();
    verify_catalog(
        &signed,
        &verifier,
        &CatalogTrustPolicy::default(),
        &CatalogTrustState::default(),
        200,
    )
    .unwrap()
    .0
}

#[tokio::test]
async fn catalog_consent_resume_stage_repair_and_remove_are_one_fail_closed_path() {
    let payload = b"tiny synthetic artifact; never model weights".to_vec();
    let manifest = manifest(
        &payload,
        "https://models.example.invalid/synthetic-fixture".to_owned(),
    );
    let identity = manifest.identity();
    let root = tempfile::tempdir().unwrap();
    let mut lifecycle = TrustedOptionalPackLifecycleV1::new(
        root.path().to_path_buf(),
        trusted_catalog(manifest),
        CatalogTrustDomainV1::LocalReviewDevOnly,
        DownloadPolicy {
            maximum_artifact_bytes: 1_024,
            checkpoint_bytes: 4,
            allow_http_loopback: true,
            ..DownloadPolicy::default()
        },
    )
    .unwrap();

    assert!(matches!(
        lifecycle
            .install_or_resume(
                &identity,
                "explicit-fixture-selection".to_owned(),
                200,
                false,
                true,
                &CancellationToken::new(),
            )
            .await,
        Err(OptionalPackLifecycleError::ExplicitConfirmationRequired)
    ));
    let download = lifecycle
        .manager()
        .storage()
        .download_path(&identity, "weights")
        .unwrap();
    std::fs::write(&download, &payload).unwrap();
    let state = lifecycle
        .import_verified_downloads(
            &identity,
            "explicit-fixture-selection".to_owned(),
            200,
            true,
            true,
            vec![ArtifactEvidence {
                artifact_id: "weights".to_owned(),
                size_bytes: payload.len() as u64,
                sha256: digest(&payload),
            }],
        )
        .unwrap();
    assert_eq!(state, InstallState::AwaitingSelfTest);
    assert!(lifecycle
        .manager()
        .storage()
        .active_pointer(&identity.pack_id)
        .unwrap()
        .is_none());
    let installed = lifecycle
        .manager()
        .storage()
        .installed_inventory(&identity)
        .unwrap()
        .unwrap();
    assert_eq!(installed.identity, identity);
    assert_eq!(installed.files.len(), 1);
    assert_eq!(installed.files[0].relative_path, "models/fixture.bin");
    assert_eq!(
        std::fs::read(installed.root.join("models/fixture.bin")).unwrap(),
        payload
    );
    assert!(lifecycle.begin_repair(&identity).unwrap().healthy);

    std::fs::write(installed.root.join("models/fixture.bin"), b"tampered").unwrap();
    let assessment = lifecycle.begin_repair(&identity).unwrap();
    assert!(!assessment.healthy);
    assert_eq!(lifecycle.state(&identity), Some(&InstallState::Downloading));
    assert_eq!(lifecycle.remove(&identity, true).unwrap(), 1);
    assert!(lifecycle.state(&identity).is_none());
}
