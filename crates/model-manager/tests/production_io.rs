#![allow(clippy::unwrap_used)]

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use model_manager::*;
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::of_bytes(bytes)
}

fn catalog_binding(manifest: &ModelPackManifestV1) -> CatalogInstallBindingV1 {
    CatalogInstallBindingV1 {
        catalog_payload_sha256: digest(
            format!("filesystem-catalog:{}", manifest.digest().unwrap()).as_bytes(),
        ),
        catalog_version: 1,
        trust_domain: CatalogTrustDomainV1::ReleaseThreshold,
    }
}

fn selection_authorization(manifest: &ModelPackManifestV1) -> PackSelectionAuthorizationV1 {
    ApiFirstPackSelectionPolicyV1
        .authorize(
            manifest,
            PackSelectionRequestV1 {
                selection_id: format!("test-selection-{}-{}", manifest.pack_id, manifest.revision),
                origin: PackSelectionOriginV1::ExplicitUser,
                action: PackSelectionActionV1::InstallAndActivate,
                selected_unix_seconds: 1,
            },
        )
        .unwrap()
}

struct FixtureAttestationVerifier;

impl SelfTestAttestationVerifier for FixtureAttestationVerifier {
    fn verify(
        &self,
        claimed_runner_id: &str,
        proof: &AttestationProofV1,
        _canonical_payload: &[u8],
    ) -> Result<VerifiedRunnerIdentity, AttestationVerificationError> {
        if claimed_runner_id != "filesystem-fixture-runner"
            || proof.algorithm != "fixture-proof"
            || proof.value != "valid"
        {
            return Err(AttestationVerificationError::InvalidProof);
        }
        Ok(VerifiedRunnerIdentity {
            runner_id: claimed_runner_id.to_owned(),
            trust_revision: "fixture-trust-v1".to_owned(),
        })
    }
}

fn attest(challenge: SelfTestChallengeV1) -> SelfTestAttestationV1 {
    SelfTestAttestationV1 {
        signed: SelfTestAttestationPayloadV1 {
            schema: SELF_TEST_ATTESTATION_SCHEMA_V1.to_owned(),
            challenge,
            runner_id: "filesystem-fixture-runner".to_owned(),
            outcome: AttestedSelfTestOutcomeV1 {
                passed: true,
                duration_millis: 1,
                output_sha256: None,
                diagnostic_code: None,
            },
        },
        proof: AttestationProofV1 {
            algorithm: "fixture-proof".to_owned(),
            value: "valid".to_owned(),
        },
    }
}

fn install_filesystem_pack(
    manager: &mut ModelPackManager<FilesystemPackStorage>,
    manifest: ModelPackManifestV1,
    bytes: &[u8],
) -> PackRevision {
    let identity = manifest.identity();
    let download = manager
        .storage()
        .download_path(&identity, "weights")
        .unwrap();
    std::fs::create_dir_all(download.parent().unwrap()).unwrap();
    std::fs::write(download, bytes).unwrap();
    let identity = if manager.active_revision(&manifest.pack_id).is_some() {
        manager
            .begin_update(
                manifest.clone(),
                catalog_binding(&manifest),
                selection_authorization(&manifest),
                true,
            )
            .unwrap()
    } else {
        manager
            .begin_install(
                manifest.clone(),
                catalog_binding(&manifest),
                selection_authorization(&manifest),
                true,
            )
            .unwrap()
    };
    let journal = manager.download_journal_mut(&identity, "weights").unwrap();
    journal
        .accept_response(ResumeResponse {
            status: 200,
            range_start: None,
            total_size: bytes.len() as u64,
            validator: Some("\"filesystem-fixture\"".to_owned()),
        })
        .unwrap();
    journal.record_bytes(bytes.len() as u64).unwrap();
    manager
        .accept_verified_artifact(
            &identity,
            ArtifactEvidence {
                artifact_id: "weights".to_owned(),
                size_bytes: bytes.len() as u64,
                sha256: digest(bytes),
            },
        )
        .unwrap();
    manager.stage(&identity).unwrap();
    let challenge = manager
        .issue_self_test_challenge(&identity, "mock-runtime", 100, 60)
        .unwrap();
    manager
        .record_self_test_attestation(
            &identity,
            attest(challenge),
            &FixtureAttestationVerifier,
            120,
        )
        .unwrap();
    manager.activate(&identity).unwrap();
    identity
}

fn artifact(bytes: &[u8], source: String) -> ArtifactV1 {
    ArtifactV1 {
        id: "weights".to_owned(),
        kind: ArtifactKind::File,
        archive_format: None,
        source_urls: vec![source],
        size_bytes: bytes.len() as u64,
        sha256: digest(bytes),
        destination: "models/weights.bin".to_owned(),
        strip_prefix: None,
        required_paths: vec![],
    }
}

fn manifest(pack: &str, revision: &str, bytes: &[u8]) -> ModelPackManifestV1 {
    ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: PackId::parse(pack).unwrap(),
        revision: Revision::parse(revision).unwrap(),
        display_name: "Production fixture".to_owned(),
        description: "Filesystem and cryptography integration fixture.".to_owned(),
        source_project: "https://models.example.test/project".to_owned(),
        source_revision: format!("commit-{}", &digest(bytes).as_str()[..12]),
        capability: ModelPackCapabilityV1 {
            kind: ModelPackKindV1::LipSync,
            scope: ModelPackScopeV1::Generic,
        },
        artifacts: vec![artifact(
            bytes,
            "https://models.example.test/weights".to_owned(),
        )],
        runtime: RuntimeCompatibilityV1 {
            runtime: "onnxruntime".to_owned(),
            abi: "npc-onnx-v1".to_owned(),
            minimum_runtime_revision: Some("1.20".to_owned()),
            supported_platforms: BTreeSet::from([Platform::Windows10, Platform::Windows11]),
            supported_architectures: BTreeSet::from([Architecture::X86_64]),
        },
        hardware: HardwareRequirementsV1 {
            minimum_ram_bytes: 1,
            recommended_ram_bytes: 2,
            minimum_vram_bytes: None,
            recommended_vram_bytes: None,
            minimum_cpu_threads: 1,
            accelerators: BTreeSet::from([Accelerator::Cpu]),
            required_cpu_features: BTreeSet::new(),
        },
        resources: ResourceEnvelopeV1 {
            storage_bytes: bytes.len() as u64,
            peak_install_bytes: bytes.len() as u64 * 2,
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
            attribution: "Fixture Authors".to_owned(),
            redistributable: true,
            commercial_use: LicensePermission::Allowed,
            derivative_use: LicensePermission::Allowed,
            acceptance_required: false,
            notices: vec![],
        },
        self_test: SelfTestV1 {
            kind: "fixture".to_owned(),
            suite_revision: "fixture-v1".to_owned(),
            allowed_runtime_backends: BTreeSet::from(["mock-runtime".to_owned()]),
            input_fixture: "tests/input.json".to_owned(),
            expected_output_sha256: None,
            timeout_millis: 1_000,
        },
    }
}

fn download_selection(identity: &PackRevision, bytes: &[u8]) -> PackSelectionAuthorizationV1 {
    selection_authorization(&manifest(
        identity.pack_id.as_str(),
        identity.revision.as_str(),
        bytes,
    ))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn downloader_recovers_from_interrupted_body_with_exact_range_and_etag() {
    let payload = b"0123456789abcdef".to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_server = Arc::clone(&observed);
    let server_payload = payload.clone();
    let server = tokio::spawn(async move {
        for request_index in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            observed_server.lock().unwrap().push(request);
            if request_index == 0 {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"fixture-v1\"\r\nConnection: close\r\n\r\n",
                    server_payload.len()
                );
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(&server_payload[..5]).await.unwrap();
                stream.shutdown().await.unwrap();
            } else {
                let header = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 5-{}/{}\r\nETag: \"fixture-v1\"\r\nConnection: close\r\n\r\n",
                    server_payload.len() - 5,
                    server_payload.len() - 1,
                    server_payload.len()
                );
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(&server_payload[5..]).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        }
    });

    let temporary = TempDir::new().unwrap();
    let journals = FileDownloadJournalStore::new(temporary.path().join("journals")).unwrap();
    let identity = PackRevision {
        pack_id: PackId::parse("download.fixture").unwrap(),
        revision: Revision::parse("r1").unwrap(),
    };
    let artifact = artifact(&payload, format!("http://{address}/model"));
    let selection = download_selection(&identity, &payload);
    let downloader = HttpsArtifactDownloader::new(DownloadPolicy {
        checkpoint_bytes: 1,
        allow_http_loopback: true,
        ..DownloadPolicy::default()
    })
    .unwrap();
    let destination = temporary.path().join("weights.part");
    let first = downloader
        .download(
            &identity,
            &artifact,
            &selection,
            &destination,
            &journals,
            &CancellationToken::new(),
        )
        .await;
    assert!(first.is_err());
    let saved = journals.load(&identity, "weights").await.unwrap().unwrap();
    assert_eq!(saved.received_bytes, 5);
    assert_eq!(saved.validator.as_deref(), Some("\"fixture-v1\""));

    let evidence = downloader
        .download(
            &identity,
            &artifact,
            &selection,
            &destination,
            &journals,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(evidence.sha256, digest(&payload));
    server.await.unwrap();
    let requests = observed.lock().unwrap();
    assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("if-range: \"fixture-v1\""));
}

#[tokio::test]
async fn downloader_honors_cancellation_while_server_stalls() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nETag: \"v1\"\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let temporary = TempDir::new().unwrap();
    let journals = FileDownloadJournalStore::new(temporary.path().join("journals")).unwrap();
    let identity = PackRevision {
        pack_id: PackId::parse("cancel.fixture").unwrap(),
        revision: Revision::parse("r1").unwrap(),
    };
    let artifact = artifact(b"payload", format!("http://{address}/model"));
    let selection = download_selection(&identity, b"payload");
    let downloader = HttpsArtifactDownloader::new(DownloadPolicy {
        allow_http_loopback: true,
        idle_timeout: Duration::from_secs(10),
        ..DownloadPolicy::default()
    })
    .unwrap();
    let cancellation = CancellationToken::new();
    let cancel_clone = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel_clone.cancel();
    });
    let result = downloader
        .download(
            &identity,
            &artifact,
            &selection,
            temporary.path().join("cancel.part"),
            &journals,
            &cancellation,
        )
        .await;
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    server.abort();
}

#[tokio::test]
async fn downloader_rejects_a_complete_body_with_the_wrong_hash() {
    let payload = b"malicious bytes".to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_payload = payload.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_request(&mut stream).await;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"wrong\"\r\nConnection: close\r\n\r\n",
            server_payload.len()
        );
        stream.write_all(header.as_bytes()).await.unwrap();
        stream.write_all(&server_payload).await.unwrap();
    });
    let temporary = TempDir::new().unwrap();
    let journals = FileDownloadJournalStore::new(temporary.path().join("journals")).unwrap();
    let identity = PackRevision {
        pack_id: PackId::parse("hash.fixture").unwrap(),
        revision: Revision::parse("r1").unwrap(),
    };
    let mut declared = artifact(&payload, format!("http://{address}/model"));
    declared.sha256 = digest(b"different bytes");
    let mut selection_manifest = manifest(
        identity.pack_id.as_str(),
        identity.revision.as_str(),
        &payload,
    );
    selection_manifest.artifacts[0].sha256 = declared.sha256.clone();
    let selection = selection_authorization(&selection_manifest);
    let downloader = HttpsArtifactDownloader::new(DownloadPolicy {
        checkpoint_bytes: 1,
        allow_http_loopback: true,
        ..DownloadPolicy::default()
    })
    .unwrap();
    let result = downloader
        .download(
            &identity,
            &declared,
            &selection,
            temporary.path().join("wrong.part"),
            &journals,
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(
        result,
        Err(DownloadError::Verification(
            VerificationError::DigestMismatch { .. }
        ))
    ));
    let reset = journals.load(&identity, "weights").await.unwrap().unwrap();
    assert_eq!(reset.received_bytes, 0);
    assert!(!reset.complete);
    server.await.unwrap();
}

#[tokio::test]
async fn downloader_rejects_pack_or_artifact_not_covered_by_user_selection_before_network() {
    let temporary = TempDir::new().unwrap();
    let journals = FileDownloadJournalStore::new(temporary.path().join("journals")).unwrap();
    let selected_identity = PackRevision {
        pack_id: PackId::parse("selected.lipsync").unwrap(),
        revision: Revision::parse("r1").unwrap(),
    };
    let requested_identity = PackRevision {
        pack_id: PackId::parse("unselected.lipsync").unwrap(),
        revision: Revision::parse("r1").unwrap(),
    };
    let payload = b"never-downloaded";
    let selection = download_selection(&selected_identity, payload);
    let requested = artifact(payload, "https://127.0.0.1:1/must-not-connect".to_owned());
    let downloader = HttpsArtifactDownloader::new(DownloadPolicy::default()).unwrap();
    let result = downloader
        .download(
            &requested_identity,
            &requested,
            &selection,
            temporary.path().join("blocked.part"),
            &journals,
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(
        result,
        Err(DownloadError::Selection(
            PackSelectionError::AuthorizationMismatch
        ))
    ));
    assert!(!temporary.path().join("blocked.part").exists());
}

#[test]
fn filesystem_stage_survives_restart_then_activates_audits_and_rolls_back() {
    let temporary = TempDir::new().unwrap();
    let policy = ArchivePolicy::default();
    let bytes_v1 = b"weights-v1";
    let manifest_v1 = manifest("storage.fixture", "r1", bytes_v1);
    let storage = FilesystemPackStorage::new(temporary.path(), policy.clone()).unwrap();
    let mut manager = ModelPackManager::new(storage);
    let identity_v1 = install_filesystem_pack(&mut manager, manifest_v1, bytes_v1);
    assert!(
        manager
            .storage()
            .inspect(&identity_v1)
            .unwrap()
            .content_healthy
    );
    manager
        .add_reference(&identity_v1.pack_id, "game:one")
        .unwrap();
    let storage = manager.into_storage();
    drop(storage);
    let mut storage = FilesystemPackStorage::new(temporary.path(), policy.clone()).unwrap();
    assert_eq!(storage.reference_count(&identity_v1.pack_id).unwrap(), 1);
    storage
        .remove_reference(&identity_v1.pack_id, "game:one")
        .unwrap();

    let bytes_v2 = b"weights-v2";
    let manifest_v2 = manifest("storage.fixture", "r2", bytes_v2);
    let mut manager = ModelPackManager::new(storage);
    // Rehydrate the active revision for this focused filesystem test through a fresh install
    // manager. Persisted rollback itself remains owned by the filesystem backend.
    let identity_v2 = install_filesystem_pack(&mut manager, manifest_v2, bytes_v2);
    let mut storage = manager.into_storage();
    assert_eq!(
        storage
            .active_pointer(&identity_v2.pack_id)
            .unwrap()
            .unwrap()
            .active_revision,
        identity_v2.revision
    );
    assert_eq!(
        storage.rollback_persisted(&identity_v1.pack_id).unwrap(),
        identity_v1
    );
}

#[test]
fn filesystem_audit_detects_post_activation_tampering() {
    let temporary = TempDir::new().unwrap();
    let bytes = b"authentic";
    let manifest = manifest("audit.fixture", "r1", bytes);
    let storage = FilesystemPackStorage::new(temporary.path(), ArchivePolicy::default()).unwrap();
    let mut manager = ModelPackManager::new(storage);
    let identity = install_filesystem_pack(&mut manager, manifest, bytes);
    let installed = temporary
        .path()
        .join("packs/audit.fixture/versions/r1/models/weights.bin");
    std::fs::write(installed, b"tampered").unwrap();
    assert!(
        !manager
            .storage()
            .inspect(&identity)
            .unwrap()
            .content_healthy
    );
}

#[test]
fn attested_activation_rejects_post_test_staging_tamper() {
    let temporary = TempDir::new().unwrap();
    let bytes = b"attested-authentic";
    let manifest = manifest("attested-tamper.fixture", "r1", bytes);
    let identity = manifest.identity();
    let storage = FilesystemPackStorage::new(temporary.path(), ArchivePolicy::default()).unwrap();
    let download = storage.download_path(&identity, "weights").unwrap();
    std::fs::create_dir_all(download.parent().unwrap()).unwrap();
    std::fs::write(download, bytes).unwrap();
    let mut manager = ModelPackManager::new(storage);
    manager
        .begin_install(
            manifest.clone(),
            catalog_binding(&manifest),
            selection_authorization(&manifest),
            true,
        )
        .unwrap();
    let journal = manager.download_journal_mut(&identity, "weights").unwrap();
    journal
        .accept_response(ResumeResponse {
            status: 200,
            range_start: None,
            total_size: bytes.len() as u64,
            validator: Some("\"tamper-fixture\"".to_owned()),
        })
        .unwrap();
    journal.record_bytes(bytes.len() as u64).unwrap();
    manager
        .accept_verified_artifact(
            &identity,
            ArtifactEvidence {
                artifact_id: "weights".to_owned(),
                size_bytes: bytes.len() as u64,
                sha256: digest(bytes),
            },
        )
        .unwrap();
    manager.stage(&identity).unwrap();
    let challenge = manager
        .issue_self_test_challenge(&identity, "mock-runtime", 100, 60)
        .unwrap();
    manager
        .record_self_test_attestation(
            &identity,
            attest(challenge),
            &FixtureAttestationVerifier,
            120,
        )
        .unwrap();
    std::fs::write(
        temporary
            .path()
            .join("packs")
            .join(identity.pack_id.as_str())
            .join("versions")
            .join(identity.revision.as_str())
            .join("models/weights.bin"),
        b"tampered-after-test",
    )
    .unwrap();
    assert!(matches!(
        manager.activate(&identity),
        Err(ManagerError::Storage(_)) | Err(ManagerError::StagedContentChangedAfterTest)
    ));
    assert!(matches!(
        manager.record(&identity).unwrap().state,
        InstallState::Quarantined {
            phase: InstallPhase::Activation,
            ..
        }
    ));
    assert!(manager.active_revision(&identity.pack_id).is_none());
}

#[test]
fn zip_and_tar_link_attacks_fail_before_writing_members() {
    let temporary = TempDir::new().unwrap();
    let staging = temporary.path().join("staging");
    std::fs::create_dir(&staging).unwrap();

    let zip_path = temporary.path().join("attack.zip");
    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("../escape.bin", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"escape").unwrap();
        zip.finish().unwrap();
    }
    let zip_bytes = std::fs::read(&zip_path).unwrap();
    let mut zip_artifact = artifact(&zip_bytes, "https://example.test/attack.zip".to_owned());
    zip_artifact.kind = ArtifactKind::Archive;
    zip_artifact.archive_format = Some(ArchiveFormatV1::Zip);
    zip_artifact.destination = "models".to_owned();
    assert!(extract_artifact(
        &zip_artifact,
        &zip_path,
        &staging,
        &ArchivePolicy::default()
    )
    .is_err());
    assert!(!temporary.path().join("escape.bin").exists());

    let tar_path = temporary.path().join("attack.tar");
    {
        let file = std::fs::File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        builder
            .append_link(&mut header, "models/link", "../../escape")
            .unwrap();
        builder.finish().unwrap();
    }
    let tar_bytes = std::fs::read(&tar_path).unwrap();
    let mut tar_artifact = artifact(&tar_bytes, "https://example.test/attack.tar".to_owned());
    tar_artifact.kind = ArtifactKind::Archive;
    tar_artifact.archive_format = Some(ArchiveFormatV1::Tar);
    tar_artifact.destination = "models".to_owned();
    assert!(extract_artifact(
        &tar_artifact,
        &tar_path,
        &staging,
        &ArchivePolicy::default()
    )
    .is_err());
}

#[test]
fn zip_tar_and_tar_bz2_archives_extract_only_inside_declared_destination() {
    let temporary = TempDir::new().unwrap();
    let zip_path = temporary.path().join("safe.zip");
    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("nested/model.bin", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"zip-model").unwrap();
        zip.finish().unwrap();
    }
    let zip_bytes = std::fs::read(&zip_path).unwrap();
    let mut zip_artifact = artifact(&zip_bytes, "https://example.test/safe.zip".to_owned());
    zip_artifact.kind = ArtifactKind::Archive;
    zip_artifact.archive_format = Some(ArchiveFormatV1::Zip);
    zip_artifact.destination = "models/zip".to_owned();
    let zip_staging = temporary.path().join("zip-staging");
    std::fs::create_dir(&zip_staging).unwrap();
    extract_artifact(
        &zip_artifact,
        &zip_path,
        &zip_staging,
        &ArchivePolicy::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(zip_staging.join("models/zip/nested/model.bin")).unwrap(),
        b"zip-model"
    );

    let tar_path = temporary.path().join("safe.tar");
    {
        let file = std::fs::File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);
        let bytes = b"tar-model";
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "nested/model.bin", bytes.as_slice())
            .unwrap();
        builder.finish().unwrap();
    }
    let tar_bytes = std::fs::read(&tar_path).unwrap();
    let mut tar_artifact = artifact(&tar_bytes, "https://example.test/safe.tar".to_owned());
    tar_artifact.kind = ArtifactKind::Archive;
    tar_artifact.archive_format = Some(ArchiveFormatV1::Tar);
    tar_artifact.destination = "models/tar".to_owned();
    let tar_staging = temporary.path().join("tar-staging");
    std::fs::create_dir(&tar_staging).unwrap();
    extract_artifact(
        &tar_artifact,
        &tar_path,
        &tar_staging,
        &ArchivePolicy::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(tar_staging.join("models/tar/nested/model.bin")).unwrap(),
        b"tar-model"
    );

    let tar_bz2_path = temporary.path().join("safe.tar.bz2");
    {
        let file = std::fs::File::create(&tar_bz2_path).unwrap();
        let encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::best());
        let mut builder = tar::Builder::new(encoder);
        let bytes = b"tar-bz2-model";
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "nested/model.bin", bytes.as_slice())
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();
    }
    let tar_bz2_bytes = std::fs::read(&tar_bz2_path).unwrap();
    let mut tar_bz2_artifact = artifact(
        &tar_bz2_bytes,
        "https://example.test/safe.tar.bz2".to_owned(),
    );
    tar_bz2_artifact.kind = ArtifactKind::Archive;
    tar_bz2_artifact.archive_format = Some(ArchiveFormatV1::TarBz2);
    tar_bz2_artifact.destination = "models/tar-bz2".to_owned();
    let tar_bz2_staging = temporary.path().join("tar-bz2-staging");
    std::fs::create_dir(&tar_bz2_staging).unwrap();
    extract_artifact(
        &tar_bz2_artifact,
        &tar_bz2_path,
        &tar_bz2_staging,
        &ArchivePolicy::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(tar_bz2_staging.join("models/tar-bz2/nested/model.bin")).unwrap(),
        b"tar-bz2-model"
    );
}

#[test]
fn zip_archive_strip_prefix_and_required_paths_form_a_closed_world_install() {
    let temporary = TempDir::new().unwrap();
    let archive_path = temporary.path().join("runtime.zip");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (path, bytes) in [
            ("runtime-root/GIT_COMMIT_ID", b"commit".as_slice()),
            ("runtime-root/LICENSE", b"license".as_slice()),
            (
                "runtime-root/ThirdPartyNotices.txt",
                b"third-party notices".as_slice(),
            ),
            ("runtime-root/VERSION_NUMBER", b"1.0".as_slice()),
            ("runtime-root/lib/runtime.dll", b"runtime".as_slice()),
            ("runtime-root/lib/shared.dll", b"shared".as_slice()),
            (
                "runtime-root/include/ignored.h",
                b"not installed".as_slice(),
            ),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    let bytes = std::fs::read(&archive_path).unwrap();
    let mut archive = artifact(&bytes, "https://example.test/runtime.zip".to_owned());
    archive.kind = ArtifactKind::Archive;
    archive.archive_format = Some(ArchiveFormatV1::Zip);
    archive.destination = "runtime/1.0".to_owned();
    archive.strip_prefix = Some("runtime-root".to_owned());
    archive.required_paths = vec![
        "GIT_COMMIT_ID".to_owned(),
        "LICENSE".to_owned(),
        "ThirdPartyNotices.txt".to_owned(),
        "VERSION_NUMBER".to_owned(),
        "lib/runtime.dll".to_owned(),
        "lib/shared.dll".to_owned(),
    ];

    let staging = temporary.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let extracted =
        extract_artifact(&archive, &archive_path, &staging, &ArchivePolicy::default()).unwrap();

    assert_eq!(extracted.files.len(), 6);
    assert_eq!(
        std::fs::read(staging.join("runtime/1.0/ThirdPartyNotices.txt")).unwrap(),
        b"third-party notices"
    );
    assert_eq!(
        std::fs::read(staging.join("runtime/1.0/lib/runtime.dll")).unwrap(),
        b"runtime"
    );
    assert!(!staging.join("runtime/1.0/include/ignored.h").exists());
}

#[test]
fn zip_archive_missing_a_required_member_is_rejected() {
    let temporary = TempDir::new().unwrap();
    let archive_path = temporary.path().join("incomplete-runtime.zip");
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "runtime-root/lib/runtime.dll",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"runtime").unwrap();
        zip.finish().unwrap();
    }
    let bytes = std::fs::read(&archive_path).unwrap();
    let mut archive = artifact(
        &bytes,
        "https://example.test/incomplete-runtime.zip".to_owned(),
    );
    archive.kind = ArtifactKind::Archive;
    archive.archive_format = Some(ArchiveFormatV1::Zip);
    archive.destination = "runtime/1.0".to_owned();
    archive.strip_prefix = Some("runtime-root".to_owned());
    archive.required_paths = vec![
        "lib/runtime.dll".to_owned(),
        "ThirdPartyNotices.txt".to_owned(),
    ];

    let staging = temporary.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let error =
        extract_artifact(&archive, &archive_path, &staging, &ArchivePolicy::default()).unwrap_err();
    assert!(matches!(
        error,
        ExtractionError::MissingRequiredMember(path) if path == "ThirdPartyNotices.txt"
    ));
}

#[test]
fn real_ed25519_adapter_accepts_valid_catalog_and_rejects_tampering() {
    let signing = SigningKey::from_bytes(&[7_u8; 32]);
    let verifier = Ed25519CatalogVerifier::new([(
        "catalog-root-a".to_owned(),
        signing.verifying_key().to_bytes(),
    )])
    .unwrap();
    let manifest = manifest("crypto.fixture", "r1", b"weights");
    let payload = CatalogPayloadV1 {
        schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
        version: 1,
        generated_unix_seconds: 100,
        expires_unix_seconds: 500,
        entries: vec![CatalogEntryV1 {
            manifest_sha256: manifest.digest().unwrap(),
            manifest,
            channels: BTreeSet::from(["stable".to_owned()]),
            published_unix_seconds: 100,
            revoked: false,
            revocation_reason: None,
        }],
    };
    let message = canonical_catalog_payload_bytes(&payload).unwrap();
    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing.sign(&message).to_bytes());
    let catalog = SignedCatalogV1 {
        signed: payload.clone(),
        signatures: vec![CatalogSignatureV1 {
            key_id: "catalog-root-a".to_owned(),
            algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
            signature,
        }],
    };
    let policy = CatalogTrustPolicy {
        signature_threshold: 1,
        maximum_lifetime_seconds: 1_000,
        maximum_clock_skew_seconds: 10,
    };
    assert!(verify_catalog(
        &catalog,
        &verifier,
        &policy,
        &CatalogTrustState::default(),
        200
    )
    .is_ok());

    let mut tampered = catalog;
    tampered.signed.expires_unix_seconds = 600;
    assert!(matches!(
        verify_catalog(
            &tampered,
            &verifier,
            &policy,
            &CatalogTrustState::default(),
            200
        ),
        Err(CatalogError::SignatureThreshold { .. })
    ));
}
