//! Declarative provider and model catalog.
//!
//! The catalog intentionally contains no credentials and performs no network
//! calls. Discovery results are supplied by the caller and overlaid in memory.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

pub const FORMAT: &str = "npc-provider-catalog";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CatalogDocument {
    pub format: String,
    pub schema_version: u32,
    pub catalog_revision: u64,
    pub published_at: String,
    pub content: Catalog,
    #[serde(default)]
    pub signatures: Vec<DetachedSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub providers: Vec<Provider>,
    pub models: Vec<Model>,
    pub pack_templates: Vec<ModelPackTemplate>,
    pub voice_intents: Vec<VoiceIntent>,
    pub fallback_policy: FallbackPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DetachedSignature {
    pub key_id: String,
    pub algorithm: SignatureAlgorithm,
    pub signature_base64: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignatureAlgorithm {
    Ed25519,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    pub execution: ExecutionLocation,
    pub lifecycle: Lifecycle,
    pub egress: EgressClass,
    pub modalities: BTreeSet<Modality>,
    pub endpoint: EndpointPolicy,
    pub credential: CredentialPolicy,
    pub capabilities: ProviderCapabilities,
    #[serde(default)]
    pub privacy: PrivacyNotice,
    #[serde(default)]
    pub usage_tiers: Vec<UsageTier>,
    #[serde(default)]
    pub trial_terms: Option<TrialTerms>,
    #[serde(default)]
    pub recommendation: Option<ProviderRecommendation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLocation {
    Hosted,
    Local,
    ExternalLocal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Llm,
    Stt,
    Tts,
    Embedding,
    Rerank,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Stable,
    Experimental,
    QualificationRequired,
    Deprecated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EgressClass {
    Offline,
    UserConfiguredEndpoint,
    ProviderCloud,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EndpointPolicy {
    pub base_url: Option<String>,
    pub user_configurable: bool,
    pub tls_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialPolicy {
    pub required: bool,
    pub credential_reference_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapabilities {
    pub streaming_input: bool,
    pub streaming_output: bool,
    pub cancellation: bool,
    pub model_discovery: bool,
    pub structured_output: bool,
    pub word_timestamps: bool,
    pub visemes_or_alignment: bool,
    pub voice_discovery: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct PrivacyNotice {
    #[serde(default)]
    pub transmitted_data: BTreeSet<DataCategory>,
    pub policy_url: Option<String>,
    #[serde(default)]
    pub explicit_consent_required: bool,
}

/// Provider account/key classes that materially change whether a route is fit
/// for evaluation or production. This contains descriptive policy metadata,
/// never a credential value or account state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsageTier {
    pub id: UsageTierId,
    pub lifecycle: Lifecycle,
    pub intended_use: IntendedUse,
    pub notes: String,
    pub documentation_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum UsageTierId {
    Trial,
    Production,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntendedUse {
    Evaluation,
    Production,
}

/// Terms surfaced before enabling a hosted evaluation route. This is policy
/// metadata only and is not a substitute for the current linked agreement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrialTerms {
    /// Stable app-facing revision for explicit acknowledgement. Changing this
    /// value invalidates any acknowledgement of an older terms document.
    pub terms_revision: String,
    pub access_scope: String,
    pub account_scope: String,
    pub rate_limit_policy: TrialRateLimitPolicy,
    pub rate_limit_note: String,
    pub production_use: TrialUsePermission,
    pub commercial_use: TrialUsePermission,
    pub prohibited_data: BTreeSet<RestrictedData>,
    pub session_retention: SessionRetention,
    pub security_abuse_logging: bool,
    pub product_improvement_collection_disclosed: bool,
    pub service_specific_disclosures_apply: bool,
    pub exact_model_terms_apply: bool,
    pub terms_url: String,
    pub access_url: String,
    pub rate_limits_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrialRateLimitPolicy {
    DynamicModelSpecificUnpublished,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrialUsePermission {
    EvaluationOnly,
    Prohibited,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RestrictedData {
    Confidential,
    ControlledOrSensitive,
    Personal,
    GameSecrets,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionRetention {
    NotStoredAfterSessionUnlessServiceDiscloses,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderRecommendation {
    pub kind: RecommendationKind,
    pub reason: String,
    pub requires_explicit_model_selection: bool,
    pub never_default: bool,
    pub never_automatic_fallback: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationKind {
    ExperimentationConvenience,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DataCategory {
    Text,
    Audio,
    DerivedGameContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub provider_id: String,
    pub upstream_id: String,
    pub display_name: String,
    pub modality: Modality,
    pub lifecycle: Lifecycle,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
    pub availability: RouteAvailability,
    pub availability_note: String,
    #[serde(default)]
    pub live_qualification: LiveQualification,
    #[serde(default)]
    pub adapter_lifecycle: Option<AdapterLifecycleDetail>,
    #[serde(default)]
    pub qualification_requirements: BTreeSet<QualificationRequirement>,
    pub qualification_evidence: Option<QualificationEvidence>,
    pub eligibility: RouteEligibility,
    #[serde(default)]
    pub source: ModelSource,
}

/// Whether a declared route can actually be selected by the current product.
/// Catalog presence and provider capability claims are not runtime readiness.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteAvailability {
    ImplementedAdapter,
    CatalogOnly,
    PackCandidateUnqualified,
    InstalledQualified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiveQualification {
    #[default]
    NotRequired,
    PassedSynthetic,
    PassedLive,
    Pending,
    EndpointUnavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdapterLifecycleDetail {
    ExperimentalAwaitingLiveAudioQualification,
    ExperimentalHttpSmokeQualifiedAwaitingGrpcStreamingQualification,
    ExperimentalGrpcStreamingQualified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRequirement {
    SignedPack,
    RuntimeSelfTest,
    LicenseReview,
    Benchmark,
}

/// Evidence attached by the model manager after installing a pack. Checked-in
/// candidate records have no evidence and cannot claim `installed_qualified`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QualificationEvidence {
    pub signed_manifest_sha256: String,
    pub signature_key_id: String,
    pub self_test_report_id: String,
    pub license_approval_id: String,
    pub benchmark_report_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub structured_output: bool,
    pub low_latency: bool,
    pub multilingual: bool,
    pub word_timestamps: bool,
    pub visemes_or_alignment: bool,
    #[serde(default)]
    pub embedding_dimensions: Option<u32>,
    #[serde(default)]
    pub discovered_voice_count: Option<u32>,
    #[serde(default)]
    pub eligible_stock_voice_count: Option<u32>,
    #[serde(default)]
    pub voice_cloning_supported: bool,
    #[serde(default)]
    pub stock_voice_only: bool,
    #[serde(default)]
    pub audio_sample_rates_hz: Vec<u32>,
    #[serde(default)]
    pub audio_channels: Option<u8>,
    #[serde(default)]
    pub audio_encoding: Option<AudioEncoding>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioEncoding {
    LinearPcm,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RouteEligibility {
    pub default_candidate: bool,
    pub automatic_fallback_candidate: bool,
    pub qualification_gate: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    #[default]
    Curated,
    DiscoveryOverlay,
}

/// A template is not an installable pack. It declares the manifest fields the
/// model manager must fill and verify when a user imports or installs a pack.
/// This avoids publishing invented artifact hashes for unqualified models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelPackTemplate {
    pub id: String,
    pub display_name: String,
    pub modality: Modality,
    pub engine: String,
    pub lifecycle: Lifecycle,
    pub source_repository: String,
    pub distribution: PackDistribution,
    pub runtime_abi: String,
    pub license_review: String,
    pub required_manifest_fields: BTreeSet<ManifestField>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PackDistribution {
    UserImport,
    UpstreamDownloadAfterQualification,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ManifestField {
    ImmutableSourceRevision,
    ArtifactUrl,
    Sha256,
    SizeBytes,
    RuntimeAbi,
    Platform,
    Architecture,
    ResourceEnvelope,
    LicenseTerms,
    Attribution,
    SelfTest,
    CatalogSignature,
    LicenseApproval,
    BenchmarkReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VoiceIntent {
    pub id: String,
    pub display_name: String,
    pub tags: BTreeSet<String>,
    pub pace: f32,
    pub pitch_semitones: f32,
    pub warmth: f32,
    pub energy: f32,
    pub roughness: f32,
    pub expressiveness: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FallbackPolicy {
    pub automatic_fallback_enabled: bool,
    pub never_silent_egress_change: bool,
    pub never_silent_provider_change: bool,
    pub execution_modes: Vec<ExecutionModePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionModePolicy {
    pub mode: ExecutionMode,
    pub allowed_egress: BTreeSet<EgressClass>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Cloud,
    Hybrid,
    FullyLocal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredModel {
    pub provider_id: String,
    pub upstream_id: String,
    pub display_name: String,
    pub modality: Modality,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallbackAuthorization {
    pub execution_mode: ExecutionMode,
    pub allow_provider_change: bool,
    pub allow_egress_change: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustPolicy<'a> {
    DevelopmentAllowUnsigned,
    RequireSigned { trusted_key_ids: &'a [&'a str] },
}

pub trait SignatureVerifier {
    fn verify(
        &self,
        algorithm: SignatureAlgorithm,
        key_id: &str,
        message: &[u8],
        signature_base64: &str,
    ) -> bool;
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("failed to read catalog: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid catalog JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("catalog validation failed:\n{0}")]
    Validation(String),
    #[error("a trusted catalog signature is required")]
    SignatureRequired,
    #[error("no catalog signature was valid for a trusted key")]
    SignatureInvalid,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FallbackError {
    #[error("automatic fallback is disabled; route changes require an explicit user action")]
    AutomaticFallbackDisabled,
    #[error("target provider or model is unknown")]
    UnknownTarget,
    #[error("target egress class is forbidden by the selected execution mode")]
    EgressForbidden,
    #[error("changing egress class requires explicit user authorization")]
    EgressConsentRequired,
    #[error("changing providers requires explicit user authorization")]
    ProviderConsentRequired,
    #[error("target is not eligible for automatic fallback")]
    TargetNotEligible,
    #[error("fallback cannot change modality")]
    ModalityMismatch,
    #[error("target route is declared but not selectable")]
    TargetNotSelectable,
}

impl CatalogDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self, CatalogError> {
        let document: Self = serde_json::from_slice(bytes)?;
        document.validate()?;
        Ok(document)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, CatalogError> {
        Self::parse(&fs::read(path)?)
    }

    pub fn load_with_trust(
        bytes: &[u8],
        policy: TrustPolicy<'_>,
        verifier: &dyn SignatureVerifier,
    ) -> Result<Self, CatalogError> {
        let document = Self::parse(bytes)?;
        if let TrustPolicy::RequireSigned { trusted_key_ids } = policy {
            if document.signatures.is_empty() {
                return Err(CatalogError::SignatureRequired);
            }
            let message = document.signing_bytes()?;
            let valid = document.signatures.iter().any(|signature| {
                trusted_key_ids.contains(&signature.key_id.as_str())
                    && verifier.verify(
                        signature.algorithm,
                        &signature.key_id,
                        &message,
                        &signature.signature_base64,
                    )
            });
            if !valid {
                return Err(CatalogError::SignatureInvalid);
            }
        }
        Ok(document)
    }

    /// Deterministic bytes covered by detached signatures. Signatures are
    /// deliberately excluded. Producers must preserve list order.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CatalogError> {
        #[derive(Serialize)]
        struct SigningPayload<'a> {
            format: &'a str,
            schema_version: u32,
            catalog_revision: u64,
            published_at: &'a str,
            content: &'a Catalog,
        }
        Ok(serde_json::to_vec(&SigningPayload {
            format: &self.format,
            schema_version: self.schema_version,
            catalog_revision: self.catalog_revision,
            published_at: &self.published_at,
            content: &self.content,
        })?)
    }

    pub fn signing_sha256(&self) -> Result<String, CatalogError> {
        Ok(hex_lower(&Sha256::digest(self.signing_bytes()?)))
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        let mut errors = Vec::new();
        if self.format != FORMAT {
            errors.push(format!("format must be {FORMAT}"));
        }
        if self.schema_version != SCHEMA_VERSION {
            errors.push(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        if self.catalog_revision == 0 {
            errors.push("catalog_revision must be positive".into());
        }
        self.content.validate_into(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CatalogError::Validation(errors.join("\n")))
        }
    }
}

impl Catalog {
    fn validate_into(&self, errors: &mut Vec<String>) {
        let mut provider_ids = BTreeSet::new();
        for provider in &self.providers {
            validate_id("provider", &provider.id, errors);
            if !provider_ids.insert(provider.id.as_str()) {
                errors.push(format!("duplicate provider id: {}", provider.id));
            }
            match provider.execution {
                ExecutionLocation::Local | ExecutionLocation::ExternalLocal
                    if provider.egress != EgressClass::Offline =>
                {
                    errors.push(format!("local provider {} must be offline", provider.id));
                }
                ExecutionLocation::Hosted if provider.egress == EgressClass::Offline => {
                    errors.push(format!("hosted provider {} cannot be offline", provider.id));
                }
                _ => {}
            }
            if provider.endpoint.tls_required {
                if let Some(url) = &provider.endpoint.base_url {
                    if !url.starts_with("https://") {
                        errors.push(format!(
                            "provider {} requires an HTTPS base URL",
                            provider.id
                        ));
                    }
                }
            }
            if provider.credential.required
                && provider.credential.credential_reference_kind.as_deref()
                    != Some("windows_credential_manager")
            {
                errors.push(format!(
                    "provider {} credentials must be references to Windows Credential Manager",
                    provider.id
                ));
            }
            if provider.egress != EgressClass::Offline
                && !provider.privacy.explicit_consent_required
            {
                errors.push(format!(
                    "provider {} must require egress consent",
                    provider.id
                ));
            }
            let mut tier_ids = BTreeSet::new();
            for tier in &provider.usage_tiers {
                if !tier_ids.insert(tier.id) {
                    errors.push(format!("provider {} repeats a usage tier", provider.id));
                }
                if tier.notes.trim().is_empty() {
                    errors.push(format!(
                        "provider {} has an undocumented usage tier",
                        provider.id
                    ));
                }
                if !tier.documentation_url.starts_with("https://") {
                    errors.push(format!(
                        "provider {} usage-tier documentation must use HTTPS",
                        provider.id
                    ));
                }
                if tier.id == UsageTierId::Trial
                    && (tier.lifecycle != Lifecycle::Experimental
                        || tier.intended_use != IntendedUse::Evaluation)
                {
                    errors.push(format!(
                        "provider {} trial tier must be evaluation-only and experimental",
                        provider.id
                    ));
                }
                if tier.id == UsageTierId::Production
                    && tier.intended_use != IntendedUse::Production
                {
                    errors.push(format!(
                        "provider {} production tier must declare production use",
                        provider.id
                    ));
                }
            }
            if let Some(terms) = &provider.trial_terms {
                let required_restrictions: BTreeSet<_> = [
                    RestrictedData::Confidential,
                    RestrictedData::ControlledOrSensitive,
                    RestrictedData::Personal,
                    RestrictedData::GameSecrets,
                ]
                .into_iter()
                .collect();
                if terms.access_scope.trim().is_empty()
                    || terms.account_scope.trim().is_empty()
                    || terms.rate_limit_note.trim().is_empty()
                    || terms.terms_revision.trim().is_empty()
                {
                    errors.push(format!(
                        "provider {} trial access/account/rate-limit scope must be explicit",
                        provider.id
                    ));
                }
                if terms.production_use != TrialUsePermission::Prohibited
                    || terms.commercial_use != TrialUsePermission::Prohibited
                {
                    errors.push(format!(
                        "provider {} trial metadata must explicitly prohibit production and commercial use",
                        provider.id
                    ));
                }
                if !required_restrictions.is_subset(&terms.prohibited_data) {
                    errors.push(format!(
                        "provider {} trial metadata omits restricted-data classes",
                        provider.id
                    ));
                }
                if !terms.security_abuse_logging
                    || !terms.product_improvement_collection_disclosed
                    || !terms.service_specific_disclosures_apply
                    || !terms.exact_model_terms_apply
                {
                    errors.push(format!(
                        "provider {} trial metadata omits logging, product-improvement, disclosure, or model-terms caveats",
                        provider.id
                    ));
                }
                for url in [&terms.terms_url, &terms.access_url, &terms.rate_limits_url] {
                    if !url.starts_with("https://") {
                        errors.push(format!(
                            "provider {} trial documentation must use HTTPS",
                            provider.id
                        ));
                    }
                }
            }
            if let Some(recommendation) = &provider.recommendation {
                if recommendation.reason.trim().is_empty()
                    || !recommendation.requires_explicit_model_selection
                    || !recommendation.never_default
                    || !recommendation.never_automatic_fallback
                {
                    errors.push(format!(
                        "provider {} recommendation must preserve explicit selection and prohibit default/fallback routing",
                        provider.id
                    ));
                }
            }
        }

        let providers: BTreeMap<_, _> = self.providers.iter().map(|p| (p.id.as_str(), p)).collect();
        let mut model_ids = BTreeSet::new();
        let mut upstream_keys = BTreeSet::new();
        for model in &self.models {
            validate_id("model", &model.id, errors);
            if !model_ids.insert(model.id.as_str()) {
                errors.push(format!("duplicate model id: {}", model.id));
            }
            if !upstream_keys.insert((&model.provider_id, &model.upstream_id, model.modality)) {
                errors.push(format!(
                    "duplicate upstream model: {}/{} ({:?})",
                    model.provider_id, model.upstream_id, model.modality
                ));
            }
            match providers.get(model.provider_id.as_str()) {
                None => errors.push(format!("model {} references unknown provider", model.id)),
                Some(provider) if !provider.modalities.contains(&model.modality) => {
                    errors.push(format!(
                        "model {} modality is unsupported by provider {}",
                        model.id, provider.id
                    ))
                }
                _ => {}
            }
            if model.lifecycle != Lifecycle::Stable
                && (model.eligibility.default_candidate
                    || model.eligibility.automatic_fallback_candidate)
            {
                errors.push(format!(
                    "non-stable model {} cannot be a default or fallback",
                    model.id
                ));
            }
            if model.eligibility.automatic_fallback_candidate {
                errors.push(format!(
                    "model {} cannot be an automatic fallback candidate while automatic fallback is disabled",
                    model.id
                ));
            }
            if model.lifecycle == Lifecycle::QualificationRequired
                && model.eligibility.qualification_gate.is_none()
            {
                errors.push(format!("model {} requires a qualification gate", model.id));
            }
            if model.source == ModelSource::DiscoveryOverlay
                && (model.eligibility.default_candidate
                    || model.eligibility.automatic_fallback_candidate)
            {
                errors.push(format!(
                    "discovered model {} cannot route automatically",
                    model.id
                ));
            }
            if model.availability_note.trim().is_empty() {
                errors.push(format!("model {} omits an availability note", model.id));
            }
            if model
                .capabilities
                .eligible_stock_voice_count
                .zip(model.capabilities.discovered_voice_count)
                .is_some_and(|(eligible, discovered)| eligible > discovered)
            {
                errors.push(format!(
                    "model {} has more eligible voices than discovered voices",
                    model.id
                ));
            }
            let sample_rates: BTreeSet<_> = model
                .capabilities
                .audio_sample_rates_hz
                .iter()
                .copied()
                .collect();
            if model
                .capabilities
                .audio_channels
                .is_some_and(|channels| channels == 0)
                || model.capabilities.audio_sample_rates_hz.contains(&0)
                || sample_rates.len() != model.capabilities.audio_sample_rates_hz.len()
            {
                errors.push(format!("model {} has an invalid audio format", model.id));
            }
            if matches!(
                model.live_qualification,
                LiveQualification::Pending | LiveQualification::EndpointUnavailable
            ) && (model.eligibility.default_candidate
                || model.eligibility.automatic_fallback_candidate)
            {
                errors.push(format!(
                    "model {} with incomplete live qualification cannot route automatically",
                    model.id
                ));
            }
            let required_qualification: BTreeSet<_> = [
                QualificationRequirement::SignedPack,
                QualificationRequirement::RuntimeSelfTest,
                QualificationRequirement::LicenseReview,
                QualificationRequirement::Benchmark,
            ]
            .into_iter()
            .collect();
            match model.availability {
                RouteAvailability::ImplementedAdapter => {
                    if !model.qualification_requirements.is_empty()
                        || model.qualification_evidence.is_some()
                    {
                        errors.push(format!(
                            "implemented hosted route {} cannot carry pack qualification state",
                            model.id
                        ));
                    }
                }
                RouteAvailability::CatalogOnly => {
                    if model.lifecycle == Lifecycle::Stable
                        || model.eligibility.default_candidate
                        || model.eligibility.automatic_fallback_candidate
                    {
                        errors.push(format!(
                            "catalog-only route {} cannot be stable, default, or fallback",
                            model.id
                        ));
                    }
                    if model.qualification_evidence.is_some() {
                        errors.push(format!(
                            "catalog-only route {} cannot carry install evidence",
                            model.id
                        ));
                    }
                }
                RouteAvailability::PackCandidateUnqualified => {
                    let is_local = providers
                        .get(model.provider_id.as_str())
                        .map(|provider| provider.egress == EgressClass::Offline)
                        .unwrap_or(false);
                    if !is_local
                        || model.lifecycle != Lifecycle::QualificationRequired
                        || model.qualification_requirements != required_qualification
                        || model.qualification_evidence.is_some()
                        || model.eligibility.default_candidate
                        || model.eligibility.automatic_fallback_candidate
                    {
                        errors.push(format!(
                            "unqualified pack route {} must be local, gated by signed pack/self-test/license/benchmark, and unroutable",
                            model.id
                        ));
                    }
                }
                RouteAvailability::InstalledQualified => {
                    let is_local = providers
                        .get(model.provider_id.as_str())
                        .map(|provider| provider.egress == EgressClass::Offline)
                        .unwrap_or(false);
                    if !is_local
                        || model.lifecycle != Lifecycle::Stable
                        || model.qualification_requirements != required_qualification
                        || model
                            .qualification_evidence
                            .as_ref()
                            .is_none_or(|evidence| !evidence.is_complete())
                    {
                        errors.push(format!(
                            "installed route {} requires complete signed pack/self-test/license/benchmark evidence",
                            model.id
                        ));
                    }
                }
            }
        }

        let required_pack_fields: BTreeSet<_> = [
            ManifestField::ImmutableSourceRevision,
            ManifestField::Sha256,
            ManifestField::SizeBytes,
            ManifestField::RuntimeAbi,
            ManifestField::Platform,
            ManifestField::Architecture,
            ManifestField::ResourceEnvelope,
            ManifestField::LicenseTerms,
            ManifestField::Attribution,
            ManifestField::SelfTest,
            ManifestField::CatalogSignature,
            ManifestField::LicenseApproval,
            ManifestField::BenchmarkReport,
        ]
        .into_iter()
        .collect();
        let mut pack_ids = BTreeSet::new();
        for pack in &self.pack_templates {
            validate_id("pack template", &pack.id, errors);
            if !pack_ids.insert(pack.id.as_str()) {
                errors.push(format!("duplicate pack template id: {}", pack.id));
            }
            if !required_pack_fields.is_subset(&pack.required_manifest_fields) {
                errors.push(format!(
                    "pack template {} omits required integrity fields",
                    pack.id
                ));
            }
            if pack.license_review.trim().is_empty() {
                errors.push(format!(
                    "pack template {} omits license review status",
                    pack.id
                ));
            }
        }

        let mut intent_ids = BTreeSet::new();
        for intent in &self.voice_intents {
            validate_id("voice intent", &intent.id, errors);
            if !intent_ids.insert(intent.id.as_str()) {
                errors.push(format!("duplicate voice intent id: {}", intent.id));
            }
            for (field, value, min, max) in [
                ("pace", intent.pace, 0.5, 1.5),
                ("pitch_semitones", intent.pitch_semitones, -6.0, 6.0),
                ("warmth", intent.warmth, 0.0, 1.0),
                ("energy", intent.energy, 0.0, 1.0),
                ("roughness", intent.roughness, 0.0, 1.0),
                ("expressiveness", intent.expressiveness, 0.0, 1.0),
            ] {
                if !value.is_finite() || value < min || value > max {
                    errors.push(format!("voice intent {} has invalid {field}", intent.id));
                }
            }
        }

        let mode_count = self.fallback_policy.execution_modes.len();
        let unique_modes: BTreeSet<_> = self
            .fallback_policy
            .execution_modes
            .iter()
            .map(|p| p.mode as u8)
            .collect();
        if mode_count != 3 || unique_modes.len() != 3 {
            errors.push("fallback policy must define cloud, hybrid, and fully_local once".into());
        }
        let local = self
            .fallback_policy
            .execution_modes
            .iter()
            .find(|p| p.mode == ExecutionMode::FullyLocal);
        if local.map(|p| p.allowed_egress.clone())
            != Some([EgressClass::Offline].into_iter().collect())
        {
            errors.push("fully_local mode must allow only offline egress".into());
        }
        if !self.fallback_policy.never_silent_egress_change
            || !self.fallback_policy.never_silent_provider_change
        {
            errors.push("silent egress and provider changes must remain disabled".into());
        }
        if self.fallback_policy.automatic_fallback_enabled {
            errors.push(
                "automatic fallback must remain disabled until an explicit user-authorized mechanism exists"
                    .into(),
            );
        }
    }

    /// Overlay provider discovery without mutating curated metadata. Unknown
    /// discovered models are experimental and never automatic route targets.
    pub fn with_discovery(&self, discovered: &[DiscoveredModel]) -> Result<Self, CatalogError> {
        let mut result = self.clone();
        let providers: BTreeMap<_, _> = result
            .providers
            .iter()
            .map(|provider| (provider.id.clone(), provider.clone()))
            .collect();
        for item in discovered {
            let provider = providers.get(&item.provider_id).ok_or_else(|| {
                CatalogError::Validation(format!(
                    "discovery references unknown provider {}",
                    item.provider_id
                ))
            })?;
            if !provider.capabilities.model_discovery {
                return Err(CatalogError::Validation(format!(
                    "provider {} does not support model discovery",
                    item.provider_id
                )));
            }
            if !provider.modalities.contains(&item.modality) {
                return Err(CatalogError::Validation(format!(
                    "provider {} cannot discover {:?} models",
                    item.provider_id, item.modality
                )));
            }
            if result.models.iter().any(|model| {
                model.provider_id == item.provider_id
                    && model.upstream_id == item.upstream_id
                    && model.modality == item.modality
            }) {
                continue;
            }
            let digest = Sha256::digest(format!(
                "{}\0{}\0{:?}",
                item.provider_id, item.upstream_id, item.modality
            ));
            result.models.push(Model {
                id: format!(
                    "discovered.{}.{}",
                    item.provider_id,
                    &hex_lower(&digest)[..16]
                ),
                provider_id: item.provider_id.clone(),
                upstream_id: item.upstream_id.clone(),
                display_name: item.display_name.clone(),
                modality: item.modality,
                lifecycle: Lifecycle::Experimental,
                languages: item.languages.clone(),
                capabilities: item.capabilities.clone(),
                availability: if result.models.iter().any(|model| {
                    model.provider_id == item.provider_id
                        && model.modality == item.modality
                        && model.availability == RouteAvailability::ImplementedAdapter
                }) {
                    RouteAvailability::ImplementedAdapter
                } else {
                    RouteAvailability::CatalogOnly
                },
                availability_note: if result.models.iter().any(|model| {
                    model.provider_id == item.provider_id
                        && model.modality == item.modality
                        && model.availability == RouteAvailability::ImplementedAdapter
                }) {
                    "Discovered model uses an implemented provider adapter; manual selection is required until catalog qualification.".into()
                } else {
                    "Discovery metadata only; no runtime adapter exists for this provider modality.".into()
                },
                live_qualification: LiveQualification::NotRequired,
                adapter_lifecycle: None,
                qualification_requirements: BTreeSet::new(),
                qualification_evidence: None,
                eligibility: RouteEligibility {
                    default_candidate: false,
                    automatic_fallback_candidate: false,
                    qualification_gate: Some("user_selection_or_catalog_qualification".into()),
                },
                source: ModelSource::DiscoveryOverlay,
            });
        }
        let mut errors = Vec::new();
        result.validate_into(&mut errors);
        if errors.is_empty() {
            Ok(result)
        } else {
            Err(CatalogError::Validation(errors.join("\n")))
        }
    }

    pub fn check_fallback(
        &self,
        from_model_id: &str,
        to_model_id: &str,
        authorization: FallbackAuthorization,
    ) -> Result<(), FallbackError> {
        if !self.fallback_policy.automatic_fallback_enabled {
            return Err(FallbackError::AutomaticFallbackDisabled);
        }
        let from = self
            .models
            .iter()
            .find(|m| m.id == from_model_id)
            .ok_or(FallbackError::UnknownTarget)?;
        let to = self
            .models
            .iter()
            .find(|m| m.id == to_model_id)
            .ok_or(FallbackError::UnknownTarget)?;
        if from.modality != to.modality {
            return Err(FallbackError::ModalityMismatch);
        }
        if !to.is_selectable() {
            return Err(FallbackError::TargetNotSelectable);
        }
        if !to.eligibility.automatic_fallback_candidate {
            return Err(FallbackError::TargetNotEligible);
        }
        let providers: BTreeMap<_, _> = self.providers.iter().map(|p| (p.id.as_str(), p)).collect();
        let from_provider = providers
            .get(from.provider_id.as_str())
            .ok_or(FallbackError::UnknownTarget)?;
        let to_provider = providers
            .get(to.provider_id.as_str())
            .ok_or(FallbackError::UnknownTarget)?;
        let mode = self
            .fallback_policy
            .execution_modes
            .iter()
            .find(|p| p.mode == authorization.execution_mode)
            .ok_or(FallbackError::EgressForbidden)?;
        if !mode.allowed_egress.contains(&to_provider.egress) {
            return Err(FallbackError::EgressForbidden);
        }
        if from_provider.egress != to_provider.egress && !authorization.allow_egress_change {
            return Err(FallbackError::EgressConsentRequired);
        }
        if from_provider.id != to_provider.id && !authorization.allow_provider_change {
            return Err(FallbackError::ProviderConsentRequired);
        }
        Ok(())
    }

    /// Promotes a checked-in local candidate only after the model manager has
    /// verified all four qualification gates. Promotion never makes the model
    /// a default or automatic fallback by itself.
    pub fn with_qualified_install(
        &self,
        model_id: &str,
        evidence: QualificationEvidence,
    ) -> Result<Self, CatalogError> {
        if !evidence.is_complete() {
            return Err(CatalogError::Validation(
                "installed qualification evidence is incomplete".into(),
            ));
        }
        let mut result = self.clone();
        let model = result
            .models
            .iter_mut()
            .find(|model| model.id == model_id)
            .ok_or_else(|| CatalogError::Validation(format!("unknown model {model_id}")))?;
        if model.availability != RouteAvailability::PackCandidateUnqualified {
            return Err(CatalogError::Validation(format!(
                "model {model_id} is not an unqualified pack candidate"
            )));
        }
        model.availability = RouteAvailability::InstalledQualified;
        model.lifecycle = Lifecycle::Stable;
        model.qualification_evidence = Some(evidence);
        model.availability_note =
            "Installed pack passed signature, runtime self-test, license, and benchmark qualification."
                .into();
        let mut errors = Vec::new();
        result.validate_into(&mut errors);
        if errors.is_empty() {
            Ok(result)
        } else {
            Err(CatalogError::Validation(errors.join("\n")))
        }
    }

    /// Deterministically maps a character/encounter seed to a semantic voice
    /// intent. Provider voice IDs are deliberately not part of this contract.
    pub fn select_voice_intent<'a>(
        &'a self,
        seed: &str,
        required_tags: &[&str],
    ) -> Option<&'a VoiceIntent> {
        let mut candidates: Vec<_> = self
            .voice_intents
            .iter()
            .filter(|intent| required_tags.iter().all(|tag| intent.tags.contains(*tag)))
            .collect();
        candidates.sort_by(|a, b| a.id.cmp(&b.id));
        if candidates.is_empty() {
            return None;
        }
        let digest = Sha256::digest(seed.as_bytes());
        let index = u64::from_be_bytes(digest[..8].try_into().expect("digest slice"))
            % candidates.len() as u64;
        Some(candidates[index as usize])
    }
}

impl Model {
    pub fn is_selectable(&self) -> bool {
        matches!(
            self.availability,
            RouteAvailability::ImplementedAdapter | RouteAvailability::InstalledQualified
        ) && !matches!(
            self.live_qualification,
            LiveQualification::Pending | LiveQualification::EndpointUnavailable
        )
    }
}

impl QualificationEvidence {
    fn is_complete(&self) -> bool {
        [
            self.signed_manifest_sha256.as_str(),
            self.signature_key_id.as_str(),
            self.self_test_report_id.as_str(),
            self.license_approval_id.as_str(),
            self.benchmark_report_id.as_str(),
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
            && self.signed_manifest_sha256.len() == 64
            && self
                .signed_manifest_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
    }
}

fn validate_id(kind: &str, id: &str, errors: &mut Vec<String>) {
    let valid = !id.is_empty()
        && id.len() <= 96
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        });
    if !valid {
        errors.push(format!("invalid {kind} id: {id}"));
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled() -> CatalogDocument {
        CatalogDocument::parse(include_bytes!("../../../catalog/v1/catalog.json")).unwrap()
    }

    struct NeverValid;
    impl SignatureVerifier for NeverValid {
        fn verify(&self, _: SignatureAlgorithm, _: &str, _: &[u8], _: &str) -> bool {
            false
        }
    }

    #[test]
    fn bundled_catalog_is_valid_and_has_every_planned_integration() {
        let document = bundled();
        let ids: BTreeSet<_> = document
            .content
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        for expected in [
            "openai",
            "gemini",
            "anthropic",
            "groq",
            "mistral",
            "openrouter",
            "cohere",
            "nvidia-nim",
            "openai-compatible",
            "deepgram",
            "assemblyai",
            "elevenlabs",
            "cartesia",
            "inworld",
            "moonshine-local",
            "nemotron-local",
            "whispercpp-local",
            "kokoro-local",
            "chatterbox-local",
            "qwen-tts-local",
            "llamacpp-local",
            "onnx-embedding-local",
        ] {
            assert!(ids.contains(expected), "missing provider {expected}");
        }
        assert_eq!(document.content.providers.len(), 22);
        assert_eq!(document.content.models.len(), 34);
        assert_eq!(document.content.pack_templates.len(), 8);
        assert_eq!(document.content.voice_intents.len(), 8);
        for id in [
            "gemini.llm.gemini-3.1-flash-lite",
            "groq.llm.gpt-oss-20b",
            "groq.llm.qwen3.6-27b",
            "mistral.llm.ministral-8b-2512",
            "mistral.llm.ministral-3b-2512",
            "openrouter.llm.liquid-lfm-2.5-2.6b-free",
            "cohere.llm.command-a-plus-05-2026",
        ] {
            assert!(
                document
                    .content
                    .models
                    .iter()
                    .find(|model| model.id == id)
                    .expect("qualified hosted LLM model")
                    .is_selectable(),
                "qualified hosted LLM must be selectable: {id}"
            );
        }
        assert!(!document
            .content
            .models
            .iter()
            .find(|model| model.id == "openai-compatible.llm.user-selected")
            .expect("generic compatible catalog route")
            .is_selectable());
        assert!(!document.content.fallback_policy.automatic_fallback_enabled);
        assert!(document
            .content
            .models
            .iter()
            .all(|model| !model.eligibility.automatic_fallback_candidate));
        for id in [
            "deepgram.tts.aura-2-arcas-en",
            "cartesia.tts.sonic-3.6",
            "inworld.tts.inworld-tts-2-flash",
        ] {
            let route = document
                .content
                .models
                .iter()
                .find(|model| model.id == id)
                .expect("exact live-qualified hosted TTS route");
            assert_eq!(route.availability, RouteAvailability::ImplementedAdapter);
            assert_eq!(route.lifecycle, Lifecycle::Stable);
            assert!(route.is_selectable());
            assert!(route.capabilities.stock_voice_only);
            assert_eq!(route.capabilities.audio_sample_rates_hz, vec![24_000]);
            assert_eq!(route.capabilities.audio_channels, Some(1));
            assert_eq!(
                route.eligibility.qualification_gate.as_deref(),
                Some("exact_native_credential_model_stock_voice_and_egress_consent")
            );
        }

        let cohere = document
            .content
            .providers
            .iter()
            .find(|provider| provider.id == "cohere")
            .unwrap();
        assert_eq!(
            cohere.modalities,
            [Modality::Llm, Modality::Embedding, Modality::Rerank]
                .into_iter()
                .collect()
        );
        assert_eq!(cohere.usage_tiers.len(), 2);
        assert!(cohere.capabilities.model_discovery);
        assert!(cohere.capabilities.streaming_output);
        assert!(cohere.capabilities.structured_output);
        let cohere_chat = document
            .content
            .models
            .iter()
            .find(|model| model.id == "cohere.llm.command-a-plus-05-2026")
            .unwrap();
        assert_eq!(
            cohere_chat.availability,
            RouteAvailability::ImplementedAdapter
        );
        assert!(cohere_chat.is_selectable());
        for id in [
            "cohere.embedding.discovered-default",
            "cohere.rerank.discovered-default",
        ] {
            let route = document
                .content
                .models
                .iter()
                .find(|model| model.id == id)
                .unwrap();
            assert_eq!(route.availability, RouteAvailability::CatalogOnly);
            assert_eq!(route.lifecycle, Lifecycle::Experimental);
            assert!(!route.is_selectable());
        }

        let providers: BTreeMap<_, _> = document
            .content
            .providers
            .iter()
            .map(|provider| (provider.id.as_str(), provider))
            .collect();
        for model in &document.content.models {
            if providers[model.provider_id.as_str()].egress == EgressClass::Offline {
                assert_eq!(
                    model.availability,
                    RouteAvailability::PackCandidateUnqualified
                );
                assert_eq!(model.lifecycle, Lifecycle::QualificationRequired);
                assert!(!model.is_selectable());
                assert!(!model.eligibility.default_candidate);
                assert!(!model.eligibility.automatic_fallback_candidate);
                assert!(model.qualification_evidence.is_none());
            }
        }
        for pack in &document.content.pack_templates {
            assert_eq!(pack.lifecycle, Lifecycle::QualificationRequired);
            assert!(pack
                .required_manifest_fields
                .contains(&ManifestField::CatalogSignature));
            assert!(pack
                .required_manifest_fields
                .contains(&ManifestField::LicenseApproval));
            assert!(pack
                .required_manifest_fields
                .contains(&ManifestField::BenchmarkReport));
        }

        let nvidia = document
            .content
            .providers
            .iter()
            .find(|provider| provider.id == "nvidia-nim")
            .unwrap();
        assert_eq!(
            nvidia.modalities,
            [
                Modality::Llm,
                Modality::Embedding,
                Modality::Rerank,
                Modality::Stt,
                Modality::Tts,
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(nvidia.usage_tiers.len(), 2);
        let terms = nvidia.trial_terms.as_ref().unwrap();
        assert_eq!(
            terms.terms_revision,
            "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1"
        );
        assert_eq!(terms.production_use, TrialUsePermission::Prohibited);
        assert_eq!(terms.commercial_use, TrialUsePermission::Prohibited);
        assert_eq!(
            terms.rate_limit_policy,
            TrialRateLimitPolicy::DynamicModelSpecificUnpublished
        );
        assert!(terms.security_abuse_logging);
        assert!(terms.product_improvement_collection_disclosed);
        assert!(terms.service_specific_disclosures_apply);
        assert!(terms.exact_model_terms_apply);
        let recommendation = nvidia.recommendation.as_ref().unwrap();
        assert_eq!(
            recommendation.kind,
            RecommendationKind::ExperimentationConvenience
        );
        assert!(recommendation.requires_explicit_model_selection);
        assert!(recommendation.never_default);
        assert!(recommendation.never_automatic_fallback);
        let nvidia_chat = document
            .content
            .models
            .iter()
            .find(|model| model.id == "nvidia-nim.llm.discovered-default")
            .unwrap();
        assert_eq!(
            nvidia_chat.availability,
            RouteAvailability::ImplementedAdapter
        );
        assert_eq!(
            nvidia_chat.live_qualification,
            LiveQualification::PassedSynthetic
        );
        assert!(nvidia_chat.is_selectable());
        assert!(!nvidia_chat.eligibility.default_candidate);
        assert!(!nvidia_chat.eligibility.automatic_fallback_candidate);
        assert_eq!(
            nvidia_chat.eligibility.qualification_gate.as_deref(),
            Some(
                "native_provider_wide_private_evaluation_acknowledgement_explicit_user_model_selection_and_endpoint_probe"
            )
        );
        let nvidia_embedding = document
            .content
            .models
            .iter()
            .find(|model| model.id == "nvidia-nim.embedding.nemotron-3-embed-1b")
            .unwrap();
        assert_eq!(
            nvidia_embedding.availability,
            RouteAvailability::ImplementedAdapter
        );
        assert!(nvidia_embedding.is_selectable());
        assert_eq!(
            nvidia_embedding.capabilities.embedding_dimensions,
            Some(2048)
        );
        assert_eq!(
            nvidia_embedding.live_qualification,
            LiveQualification::PassedLive
        );
        assert!(!nvidia_embedding.eligibility.default_candidate);
        assert!(!nvidia_embedding.eligibility.automatic_fallback_candidate);
        assert_eq!(
            nvidia_embedding.eligibility.qualification_gate.as_deref(),
            Some(
                "native_provider_wide_private_evaluation_acknowledgement_explicit_user_model_selection_and_egress_consent"
            )
        );
        let rerank = document
            .content
            .models
            .iter()
            .find(|model| model.id == "nvidia-nim.rerank.discovered-default")
            .unwrap();
        assert_eq!(rerank.availability, RouteAvailability::CatalogOnly);
        assert_eq!(
            rerank.live_qualification,
            LiveQualification::EndpointUnavailable
        );
        assert!(!rerank.is_selectable());

        let asr = document
            .content
            .models
            .iter()
            .find(|model| model.id == "nvidia-nim.stt.nemotron-asr-streaming")
            .unwrap();
        assert_eq!(asr.availability, RouteAvailability::ImplementedAdapter);
        assert_eq!(asr.live_qualification, LiveQualification::Pending);
        assert_eq!(
            asr.adapter_lifecycle,
            Some(AdapterLifecycleDetail::ExperimentalAwaitingLiveAudioQualification)
        );
        assert!(!asr.is_selectable());
        assert!(!asr.eligibility.default_candidate);
        assert!(!asr.eligibility.automatic_fallback_candidate);

        let magpie = document
            .content
            .models
            .iter()
            .find(|model| model.id == "nvidia-nim-magpie")
            .unwrap();
        assert_eq!(magpie.availability, RouteAvailability::ImplementedAdapter);
        assert_eq!(magpie.live_qualification, LiveQualification::PassedLive);
        assert_eq!(
            magpie.adapter_lifecycle,
            Some(AdapterLifecycleDetail::ExperimentalGrpcStreamingQualified)
        );
        assert!(magpie.is_selectable());
        assert_eq!(magpie.capabilities.discovered_voice_count, Some(86));
        assert_eq!(magpie.capabilities.eligible_stock_voice_count, Some(30));
        assert!(magpie.capabilities.stock_voice_only);
        assert!(!magpie.capabilities.voice_cloning_supported);
        assert_eq!(
            magpie.capabilities.audio_sample_rates_hz,
            vec![22_050, 44_100]
        );
        assert_eq!(magpie.capabilities.audio_channels, Some(1));
        assert_eq!(
            magpie.capabilities.audio_encoding,
            Some(AudioEncoding::LinearPcm)
        );
        assert!(!magpie.eligibility.default_candidate);
        assert!(!magpie.eligibility.automatic_fallback_candidate);
        assert_eq!(
            magpie.eligibility.qualification_gate.as_deref(),
            Some(
                "native_provider_wide_private_evaluation_acknowledgement_user_key_stock_voice_selection_promotion_and_publication_false"
            )
        );

        let assemblyai = document
            .content
            .models
            .iter()
            .find(|model| model.id == "assemblyai.stt.default")
            .unwrap();
        assert_eq!(assemblyai.upstream_id, "u3-rt-pro");
        assert_eq!(assemblyai.live_qualification, LiveQualification::PassedLive);
        assert_eq!(
            assemblyai.availability,
            RouteAvailability::ImplementedAdapter
        );
        assert!(!assemblyai.eligibility.default_candidate);
        assert!(!assemblyai.eligibility.automatic_fallback_candidate);

        assert_eq!(
            document.signing_sha256().unwrap(),
            "a2c5ea813ed701b7894a15e06c9dfe3ad75128cef6ab67269a1b05c2258335ee"
        );
    }

    #[test]
    fn production_trust_rejects_unsigned_catalog() {
        let bytes = include_bytes!("../../../catalog/v1/catalog.json");
        let result = CatalogDocument::load_with_trust(
            bytes,
            TrustPolicy::RequireSigned {
                trusted_key_ids: &["release-1"],
            },
            &NeverValid,
        );
        assert!(matches!(result, Err(CatalogError::SignatureRequired)));
    }

    #[test]
    fn discovery_preserves_curated_entries_and_quarantines_unknown_models() {
        let catalog = bundled().content;
        let curated_len = catalog.models.len();
        let overlay = catalog
            .with_discovery(&[
                DiscoveredModel {
                    provider_id: "openai".into(),
                    upstream_id: "discovered-example".into(),
                    display_name: "Discovered Example".into(),
                    modality: Modality::Llm,
                    languages: vec!["und".into()],
                    capabilities: ModelCapabilities::default(),
                },
                DiscoveredModel {
                    provider_id: "openai".into(),
                    upstream_id: "discovered-example".into(),
                    display_name: "Duplicate Discovery".into(),
                    modality: Modality::Llm,
                    languages: vec![],
                    capabilities: ModelCapabilities::default(),
                },
            ])
            .unwrap();
        assert_eq!(overlay.models.len(), curated_len + 1);
        let added = overlay.models.last().unwrap();
        assert_eq!(added.lifecycle, Lifecycle::Experimental);
        assert_eq!(added.source, ModelSource::DiscoveryOverlay);
        assert!(!added.eligibility.default_candidate);
        assert!(!added.eligibility.automatic_fallback_candidate);
        assert_eq!(added.availability, RouteAvailability::ImplementedAdapter);
        assert!(added.is_selectable());
    }

    #[test]
    fn discovery_is_rejected_for_a_non_discovering_provider() {
        let result = bundled().content.with_discovery(&[DiscoveredModel {
            provider_id: "moonshine-local".into(),
            upstream_id: "surprise".into(),
            display_name: "Surprise".into(),
            modality: Modality::Stt,
            languages: vec![],
            capabilities: ModelCapabilities::default(),
        }]);
        assert!(matches!(result, Err(CatalogError::Validation(_))));
    }

    #[test]
    fn discovery_does_not_invent_a_cohere_embedding_adapter() {
        let catalog = bundled().content;
        let overlay = catalog
            .with_discovery(&[DiscoveredModel {
                provider_id: "cohere".into(),
                upstream_id: "embed-fixture".into(),
                display_name: "Embed Fixture".into(),
                modality: Modality::Embedding,
                languages: vec!["en".into()],
                capabilities: ModelCapabilities::default(),
            }])
            .unwrap();
        let added = overlay.models.last().unwrap();
        assert_eq!(added.availability, RouteAvailability::CatalogOnly);
        assert!(!added.is_selectable());
    }

    #[test]
    fn automatic_fallback_is_disabled_even_with_full_consent() {
        let catalog = bundled().content;
        let error = catalog
            .check_fallback(
                "local.llm.custom-gguf",
                "openai.llm.discovered-default",
                FallbackAuthorization {
                    execution_mode: ExecutionMode::FullyLocal,
                    allow_provider_change: true,
                    allow_egress_change: true,
                },
            )
            .unwrap_err();
        assert_eq!(error, FallbackError::AutomaticFallbackDisabled);
    }

    #[test]
    fn automatic_fallback_is_disabled_before_consent_evaluation() {
        let catalog = bundled().content;
        let error = catalog
            .check_fallback(
                "local.llm.custom-gguf",
                "openai.llm.discovered-default",
                FallbackAuthorization {
                    execution_mode: ExecutionMode::Hybrid,
                    allow_provider_change: true,
                    allow_egress_change: false,
                },
            )
            .unwrap_err();
        assert_eq!(error, FallbackError::AutomaticFallbackDisabled);
    }

    #[test]
    fn automatic_fallback_cannot_reroute_across_modalities() {
        let catalog = bundled().content;
        let error = catalog
            .check_fallback(
                "openai.llm.discovered-default",
                "openai.stt.discovered-default",
                FallbackAuthorization {
                    execution_mode: ExecutionMode::Cloud,
                    allow_provider_change: false,
                    allow_egress_change: false,
                },
            )
            .unwrap_err();
        assert_eq!(error, FallbackError::AutomaticFallbackDisabled);
    }

    #[test]
    fn automatic_fallback_cannot_target_catalog_only_routes() {
        let catalog = bundled().content;
        let error = catalog
            .check_fallback(
                "local.embedding.custom-onnx-int8",
                "cohere.embedding.discovered-default",
                FallbackAuthorization {
                    execution_mode: ExecutionMode::Hybrid,
                    allow_provider_change: true,
                    allow_egress_change: true,
                },
            )
            .unwrap_err();
        assert_eq!(error, FallbackError::AutomaticFallbackDisabled);
    }

    fn complete_evidence() -> QualificationEvidence {
        QualificationEvidence {
            signed_manifest_sha256:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            signature_key_id: "release-model-pack-1".into(),
            self_test_report_id: "self-test:fixture-pass".into(),
            license_approval_id: "license-review:approved".into(),
            benchmark_report_id: "benchmark:windows-matrix-pass".into(),
        }
    }

    #[test]
    fn complete_pack_evidence_promotes_but_does_not_default_a_local_route() {
        let catalog = bundled().content;
        let promoted = catalog
            .with_qualified_install("local.llm.custom-gguf", complete_evidence())
            .unwrap();
        let model = promoted
            .models
            .iter()
            .find(|model| model.id == "local.llm.custom-gguf")
            .unwrap();
        assert_eq!(model.availability, RouteAvailability::InstalledQualified);
        assert_eq!(model.lifecycle, Lifecycle::Stable);
        assert!(model.is_selectable());
        assert!(!model.eligibility.default_candidate);
        assert!(!model.eligibility.automatic_fallback_candidate);
    }

    #[test]
    fn incomplete_pack_evidence_cannot_promote_a_local_route() {
        let catalog = bundled().content;
        let mut evidence = complete_evidence();
        evidence.benchmark_report_id.clear();
        let error = catalog
            .with_qualified_install("local.llm.custom-gguf", evidence)
            .unwrap_err()
            .to_string();
        assert!(error.contains("evidence is incomplete"));
    }

    #[test]
    fn voice_assignment_is_deterministic_and_tag_filtered() {
        let catalog = bundled().content;
        let first = catalog
            .select_voice_intent("skyrim:guard:encounter-42", &["grounded"])
            .unwrap();
        let second = catalog
            .select_voice_intent("skyrim:guard:encounter-42", &["grounded"])
            .unwrap();
        assert_eq!(first.id, second.id);
        assert!(first.tags.contains("grounded"));
        assert!(catalog
            .select_voice_intent("x", &["nonexistent-tag"])
            .is_none());
    }

    #[test]
    fn validation_rejects_nonstable_default_routes() {
        let mut document = bundled();
        document.content.models[0].lifecycle = Lifecycle::Experimental;
        document.content.models[0].eligibility.default_candidate = true;
        let error = document.validate().unwrap_err().to_string();
        assert!(error.contains("non-stable model"));
    }

    #[test]
    fn validation_rejects_any_automatic_fallback_configuration() {
        let mut document = bundled();
        document.content.fallback_policy.automatic_fallback_enabled = true;
        document.content.models[0]
            .eligibility
            .automatic_fallback_candidate = true;
        let error = document.validate().unwrap_err().to_string();
        assert!(error.contains("automatic fallback candidate"));
        assert!(error.contains("automatic fallback must remain disabled"));
    }

    #[test]
    fn validation_rejects_a_production_labeled_trial_tier() {
        let mut document = bundled();
        let cohere = document
            .content
            .providers
            .iter_mut()
            .find(|provider| provider.id == "cohere")
            .unwrap();
        cohere.usage_tiers[0].lifecycle = Lifecycle::Stable;
        let error = document.validate().unwrap_err().to_string();
        assert!(error.contains("trial tier must be evaluation-only and experimental"));
    }

    #[test]
    fn validation_rejects_incomplete_or_production_enabled_trial_terms() {
        let mut document = bundled();
        let nvidia = document
            .content
            .providers
            .iter_mut()
            .find(|provider| provider.id == "nvidia-nim")
            .unwrap();
        let terms = nvidia.trial_terms.as_mut().unwrap();
        terms.production_use = TrialUsePermission::EvaluationOnly;
        terms.prohibited_data.remove(&RestrictedData::GameSecrets);
        let error = document.validate().unwrap_err().to_string();
        assert!(error.contains("prohibit production and commercial use"));
        assert!(error.contains("omits restricted-data classes"));
    }
}
