use std::collections::BTreeMap;

use interactive_npcs_diagnostics::{
    DiagnosticFact, DiagnosticReport, DiagnosticReportBuilder, DiagnosticStatus,
};
#[cfg(not(windows))]
use interactive_npcs_game_discovery::DiscoveryService;
#[cfg(windows)]
use interactive_npcs_game_discovery::WindowsStoreLocator;
use npc_memory::VectorSearchBackend;
use serde::Serialize;

use crate::{
    CatalogTrustState, HostState, APPLICATION_VERSION, REQUIRED_AUTHORED_GAME_PROFILE_COUNT,
    REQUIRED_PROFILE_COUNT, REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT, SYNTHETIC_REVIEW_PROFILE_ID,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ready,
    Degraded,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub schema_version: String,
    pub status: DoctorStatus,
    pub profile_count: usize,
    pub authored_game_profile_count: usize,
    pub synthetic_review_profile_count: usize,
    pub synthetic_review_profile_ids: Vec<String>,
    pub provider_count: usize,
    pub model_count: usize,
    pub catalog_trust: CatalogTrustState,
    pub discovered_installation_count: usize,
    pub discovery_error_count: usize,
    pub vector_backend: String,
    pub hosted_provider_contracts: Vec<String>,
    pub model_manifest_example: String,
    pub diagnostics: DiagnosticReport,
    pub performance_measurements_captured: bool,
    pub power_profile_changed: bool,
}

impl HostState {
    pub async fn doctor(&self) -> DoctorReport {
        let mut builder = DiagnosticReportBuilder::new(generated_at(), APPLICATION_VERSION);
        let profile_count = self.profiles.profiles().len();
        let synthetic_review_profile_ids = self
            .profiles
            .profiles()
            .iter()
            .filter(|loaded| loaded.profile.id == SYNTHETIC_REVIEW_PROFILE_ID)
            .map(|loaded| loaded.profile.id.clone())
            .collect::<Vec<_>>();
        let synthetic_review_profile_count = synthetic_review_profile_ids.len();
        let authored_game_profile_count =
            profile_count.saturating_sub(synthetic_review_profile_count);
        let profile_corpus_complete = profile_count == REQUIRED_PROFILE_COUNT
            && authored_game_profile_count == REQUIRED_AUTHORED_GAME_PROFILE_COUNT
            && synthetic_review_profile_count == REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT;
        push_fact(
            &mut builder,
            "profiles.corpus",
            "Game profile corpus",
            if profile_corpus_complete {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Failed
            },
            format!(
                "{authored_game_profile_count} authored game profiles plus {synthetic_review_profile_count} explicit synthetic review profile loaded"
            ),
            None,
        );
        let (catalog_trust_status, catalog_trust_summary, catalog_trust_fix) =
            match self.catalog_trust {
                CatalogTrustState::DevelopmentUnsignedAllowed => (
                    DiagnosticStatus::Degraded,
                    "Unsigned provider catalog accepted only because this is a debug/development build"
                        .to_owned(),
                    Some(
                        "Release builds require a detached signature verified by an immutable compiled-in trusted key.",
                    ),
                ),
                CatalogTrustState::ReleaseSignatureVerified => (
                    DiagnosticStatus::Ok,
                    "Provider catalog signature verified against the immutable release trust root"
                        .to_owned(),
                    None,
                ),
            };
        push_fact(
            &mut builder,
            "providers.catalog_trust",
            "Provider catalog trust",
            catalog_trust_status,
            catalog_trust_summary,
            catalog_trust_fix,
        );
        let provider_count = self.catalog.content.providers.len();
        let model_count = self.catalog.content.models.len();
        push_fact(
            &mut builder,
            "providers.catalog",
            "Provider catalog",
            DiagnosticStatus::Ok,
            format!("{provider_count} providers and {model_count} model routes validated"),
            None,
        );

        let vector_backend = self
            .memory
            .vector_backend()
            .await
            .unwrap_or(VectorSearchBackend::ExactCosine);
        push_fact(
            &mut builder,
            "memory.sqlite",
            "Authoritative memory database",
            DiagnosticStatus::Ok,
            format!("SQLite initialized with {vector_backend:?} vector fallback"),
            None,
        );

        #[cfg(windows)]
        let discovery = WindowsStoreLocator.discovery_service();
        #[cfg(not(windows))]
        let discovery = DiscoveryService::new();
        let (installations, discovery_errors) = discovery.discover();
        let matched_installations = self.profiles.match_installations(&installations);
        push_fact(
            &mut builder,
            "games.discovery",
            "Game discovery boundary",
            if discovery_errors.is_empty() {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Degraded
            },
            format!(
                "{} supported matches from {} store candidates; {} scanner errors",
                matched_installations.len(),
                installations.len(),
                discovery_errors.len()
            ),
            Some("Store scanners are platform adapters; manual executable selection remains available."),
        );

        let manifest_status = validate_example_manifest(self);
        push_fact(
            &mut builder,
            "models.manifest",
            "Model pack manifest contract",
            if manifest_status == "valid" {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Degraded
            },
            format!("Example manifest status: {manifest_status}"),
            Some("Model installation remains disabled until a trusted catalog and valid manifest are present."),
        );

        push_fact(
            &mut builder,
            "credentials.vault",
            "Credential vault boundary",
            DiagnosticStatus::Ok,
            if cfg!(windows) {
                "Windows Credential Manager backend initialized".to_owned()
            } else {
                "Ephemeral CI vault initialized; live provider calls remain unavailable".to_owned()
            },
            None,
        );
        push_fact(
            &mut builder,
            "performance.environment",
            "Performance evidence",
            DiagnosticStatus::Skipped,
            "No performance measurements captured; current power mode and competing workloads were left unchanged".to_owned(),
            None,
        );
        let (diagnostics, _) = builder.build();
        let status = if diagnostics
            .facts
            .iter()
            .any(|fact| fact.status == DiagnosticStatus::Failed)
        {
            DoctorStatus::Failed
        } else if diagnostics
            .facts
            .iter()
            .any(|fact| fact.status == DiagnosticStatus::Degraded)
        {
            DoctorStatus::Degraded
        } else {
            DoctorStatus::Ready
        };
        DoctorReport {
            schema_version: "1.0.0".to_owned(),
            status,
            profile_count,
            authored_game_profile_count,
            synthetic_review_profile_count,
            synthetic_review_profile_ids,
            provider_count,
            model_count,
            catalog_trust: self.catalog_trust,
            discovered_installation_count: matched_installations.len(),
            discovery_error_count: discovery_errors.len(),
            vector_backend: format!("{vector_backend:?}"),
            hosted_provider_contracts: hosted_contracts(),
            model_manifest_example: manifest_status,
            diagnostics,
            performance_measurements_captured: false,
            power_profile_changed: false,
        }
    }
}

fn validate_example_manifest(state: &HostState) -> String {
    let path = state
        .config
        .repo_root
        .join("packaging/model-packs/model-pack-manifest.example.json");
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return "not_present".to_owned();
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return "unsafe".to_owned();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return "unreadable".to_owned();
    };
    let Ok(manifest) = model_manager::parse_and_normalize_model_pack_manifest(&bytes) else {
        return "incompatible".to_owned();
    };
    if manifest.core_manifest.validate().is_ok() {
        "valid".to_owned()
    } else {
        "invalid".to_owned()
    }
}

fn hosted_contracts() -> Vec<String> {
    // Referencing these public contract types at the host boundary keeps their
    // integration compile-checked without constructing transports or reading credentials.
    let _llm = std::mem::size_of::<npc_providers_llm::AdapterConfig>();
    let _retrieval = std::mem::size_of::<npc_providers_retrieval::EmbeddingRequestV1>();
    let _stt = std::mem::size_of::<npc_providers_stt::RecognitionConfig>();
    let _tts = std::mem::size_of::<npc_providers_tts::TtsSessionRequest>();
    vec![
        "llm:openai,anthropic,gemini,groq,cohere,nvidia-nim,openai-compatible".to_owned(),
        "retrieval:nvidia-nim-embeddings-adapter,nvidia-nim-reranking-contract".to_owned(),
        "stt:deepgram,assemblyai,elevenlabs,nvidia-nim-asr,openai".to_owned(),
        // Only providers with a production credential resolver, hosted
        // transport, stock-voice policy, and broker-compatible PCM builder may
        // be advertised at the runtime boundary. The generic command adapters
        // for Cartesia, Deepgram, and Inworld remain quarantined.
        "tts:elevenlabs".to_owned(),
        "tts-private-evaluation:nvidia-nim-magpie".to_owned(),
    ]
}

fn push_fact(
    builder: &mut DiagnosticReportBuilder,
    id: &str,
    label: &str,
    status: DiagnosticStatus,
    summary: String,
    fix: Option<&str>,
) {
    let _ = builder.push(DiagnosticFact {
        check_id: id.to_owned(),
        label: label.to_owned(),
        status,
        summary,
        fix: fix.map(str::to_owned),
        metadata: BTreeMap::new(),
    });
}

fn generated_at() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

#[cfg(test)]
mod tests {
    use super::hosted_contracts;
    use model_manager::{parse_and_normalize_model_pack_manifest, ModelPackNormalizationOriginV2};
    use std::path::PathBuf;

    #[test]
    fn hosted_llm_contracts_include_every_supported_adapter() {
        let llm = hosted_contracts()
            .into_iter()
            .find(|contract| contract.starts_with("llm:"))
            .expect("LLM provider contract must be reported");

        for provider in [
            "openai",
            "anthropic",
            "gemini",
            "groq",
            "cohere",
            "nvidia-nim",
            "openai-compatible",
        ] {
            assert!(
                llm.split_once(':')
                    .expect("typed provider contract")
                    .1
                    .split(',')
                    .any(|candidate| candidate == provider),
                "missing hosted LLM provider contract: {provider}"
            );
        }
    }

    #[test]
    fn hosted_retrieval_contracts_are_reported_without_network_probe() {
        assert!(hosted_contracts().iter().any(|contract| {
            contract == "retrieval:nvidia-nim-embeddings-adapter,nvidia-nim-reranking-contract"
        }));
    }

    #[test]
    fn hosted_tts_contracts_report_only_constructible_runtime_routes() {
        let tts = hosted_contracts()
            .into_iter()
            .find(|contract| contract.starts_with("tts:"))
            .expect("hosted TTS contract");
        assert_eq!(tts, "tts:elevenlabs");
        assert!(hosted_contracts()
            .iter()
            .any(|contract| contract == "tts-private-evaluation:nvidia-nim-magpie"));
        for quarantined in ["cartesia", "deepgram", "inworld"] {
            assert!(!tts
                .split_once(':')
                .expect("typed contract")
                .1
                .split(',')
                .any(|candidate| candidate == quarantined));
        }
    }

    #[test]
    fn packaged_example_manifest_matches_the_runtime_contract() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/model-packs/model-pack-manifest.example.json");
        let bytes = std::fs::read(&path).expect("packaged example manifest must be readable");
        let manifest = parse_and_normalize_model_pack_manifest(&bytes)
            .expect("packaged example manifest must normalize into the runtime contract");

        manifest
            .core_manifest
            .validate()
            .expect("packaged example manifest must remain contract-valid");
        assert_eq!(
            manifest.origin,
            ModelPackNormalizationOriginV2::CanonicalV2,
            "the stable example must use the one canonical manifest schema"
        );
    }
}
