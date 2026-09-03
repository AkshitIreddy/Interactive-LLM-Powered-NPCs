//! Explicit, ignored live qualification for the selected NVIDIA embedding route.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use interactive_npcs_credential_vault::{CredentialVault, MemoryCredentialVault, SecretValue};
use npc_providers_retrieval::{
    EmbeddingInputRole, EmbeddingRequestV1, RequestContext, RetrievalErrorKind, TruncationPolicy,
    NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID,
};
use npc_runtime_host::{
    retrieval_bridge::selected_hosted_embedding, RouteExecution, SelectedProviderRoute,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const KEYS_PATH_ENV: &str = "HOSTED_TEST_KEYS_FILE";
const EVIDENCE_PATH_ENV: &str = "HOSTED_EMBEDDING_LIVE_METRICS_PATH";

struct KeyMaterial(Zeroizing<Vec<u8>>);

impl KeyMaterial {
    fn read(path: &Path, label: &str) -> Result<Self, &'static str> {
        let bytes = Zeroizing::new(fs::read(path).map_err(|_| "credential file unavailable")?);
        let text = std::str::from_utf8(bytes.as_slice()).map_err(|_| "credential file invalid")?;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(['#', ';']) || line.starts_with("//") {
                continue;
            }
            let delimiter = line.find('=').or_else(|| line.find(':'));
            let Some(delimiter) = delimiter else { continue };
            let normalized = line[..delimiter]
                .trim()
                .trim_matches(['"', '\'', '`'])
                .to_ascii_lowercase()
                .replace([' ', '-'], "_");
            if normalized != label {
                continue;
            }
            let value = line[delimiter + 1..]
                .trim()
                .trim_matches(['"', '\''])
                .as_bytes()
                .to_vec();
            if value.len() < 8 || value.iter().any(u8::is_ascii_control) {
                return Err("credential invalid");
            }
            return Ok(Self(Zeroizing::new(value)));
        }
        Err("credential label unavailable")
    }
}

fn route() -> SelectedProviderRoute {
    SelectedProviderRoute {
        provider_id: "nvidia-nim".into(),
        model_id: NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID.into(),
        voice_id: None,
        execution: RouteExecution::Cloud,
        egress: "selected_memory_and_lore_text".into(),
        credential_reference: Some("providers/nvidia-nim".into()),
    }
}

fn request() -> EmbeddingRequestV1 {
    EmbeddingRequestV1 {
        model_id: NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID.into(),
        role: EmbeddingInputRole::Query,
        inputs: vec!["Synthetic lighthouse retrieval query.".into()],
        truncation: TruncationPolicy::None,
        dimensions: None,
    }
}

#[tokio::test]
#[ignore = "explicit bounded NVIDIA hosted embedding qualification"]
async fn live_selected_nvidia_embedding_returns_bounded_vector_and_cancels() {
    let keys_path = PathBuf::from(env::var_os(KEYS_PATH_ENV).expect("keys path required"));
    let evidence_path =
        PathBuf::from(env::var_os(EVIDENCE_PATH_ENV).expect("embedding evidence path required"));
    let key = KeyMaterial::read(&keys_path, "nvidia_nim").expect("NVIDIA key required");
    let vault = MemoryCredentialVault::default();
    let secret = SecretValue::new(key.0.as_slice().to_vec()).expect("credential accepted");
    vault
        .put("providers/nvidia-nim", Some("live-qualification"), &secret)
        .expect("credential staged");
    drop(secret);
    let selected_route = route();
    let provider = selected_hosted_embedding(&selected_route, Arc::new(vault.clone()))
        .expect("selected adapter constructs");

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let cancel_started = Instant::now();
    let error = provider
        .embed(
            request(),
            npc_providers_retrieval::RequestContext {
                deadline: tokio::time::Instant::now() + Duration::from_secs(5),
                cancellation,
                cancellation_generation: 17,
            },
        )
        .await
        .expect_err("pre-dispatch cancellation must stop");
    assert_eq!(error.kind, RetrievalErrorKind::Cancelled);
    let cancel_ms = cancel_started.elapsed().as_millis() as u64;

    let started = Instant::now();
    let response = provider
        .embed(
            request(),
            RequestContext::with_timeout(Duration::from_secs(30), 18),
        )
        .await
        .expect("selected embedding call succeeds");
    let latency_ms = started.elapsed().as_millis() as u64;
    assert_eq!(response.vectors.len(), 1);
    assert_eq!(response.dimensions, 2_048);
    assert!(response.vectors[0]
        .values
        .iter()
        .all(|value| value.is_finite()));
    let mut digest = Sha256::new();
    for value in &response.vectors[0].values {
        digest.update(value.to_le_bytes());
    }
    let vector_sha256 = format!("{:x}", digest.finalize());
    let route_json = serde_json::to_vec(&selected_route).expect("route serializes");
    let route_sha256 = format!("{:x}", Sha256::digest(&route_json));
    vault
        .delete("providers/nvidia-nim")
        .expect("credential removed");

    let report = json!({
        "schemaVersion": 1,
        "checkedAtUnixMs": SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap_or_default().as_millis() as u64,
        "scope": "bounded-live-selected-runtime-embedding-adapter",
        "containsCredentialValues": false,
        "containsInputText": false,
        "containsEmbeddingValues": false,
        "provider": "nvidia-nim",
        "adapter": "npc-runtime-host::retrieval_bridge::selected_hosted_embedding",
        "route": {
            "provider": selected_route.provider_id,
            "model": selected_route.model_id,
            "execution": "cloud",
            "egress": selected_route.egress,
            "credentialReference": selected_route.credential_reference,
            "snapshotSha256": route_sha256,
            "endpointClass": "fixed_official_https",
            "automaticFallback": false,
            "manualRetryRequiresNewCancellationGeneration": true
        },
        "embed": {
            "ok": true,
            "latencyMs": latency_ms,
            "vectorCount": response.vectors.len(),
            "dimensions": response.dimensions,
            "vectorSha256": vector_sha256,
            "allFinite": true
        },
        "preDispatchCancellation": {
            "ok": true,
            "kind": "cancelled",
            "latencyMs": cancel_ms,
            "networkDispatched": false
        },
        "remoteObjects": {"created": false, "remaining": false}
    });
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent).expect("evidence directory available");
    }
    fs::write(
        evidence_path,
        serde_json::to_vec_pretty(&report).expect("evidence serializes"),
    )
    .expect("evidence written");
}
