use crate::{
    ArtifactV1, ModelPackKindV1, ModelPackManifestV1, ModelPackScopeV1, PackRevision, Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const API_FIRST_SELECTION_POLICY_V1: &str = "npc.model-selection/api-first-v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackSelectionOriginV1 {
    ExplicitUser,
    AutomaticRecommendation,
    DefaultSelection,
    DependencyResolution,
    Migration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackSelectionActionV1 {
    InstallOnly,
    InstallAndActivate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PackSelectionRequestV1 {
    pub selection_id: String,
    pub origin: PackSelectionOriginV1,
    pub action: PackSelectionActionV1,
    pub selected_unix_seconds: u64,
}

/// Manifest-bound capability token produced only by a selection policy decision.
/// Private fields prevent arbitrary callers from constructing an authorization directly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackSelectionAuthorizationV1 {
    policy: String,
    identity: PackRevision,
    manifest_sha256: Sha256Digest,
    kind: ModelPackKindV1,
    scope: ModelPackScopeV1,
    selection_id: String,
    selected_unix_seconds: u64,
    activation_allowed: bool,
    artifacts: BTreeMap<String, (u64, Sha256Digest)>,
}

impl PackSelectionAuthorizationV1 {
    pub fn selection_id(&self) -> &str {
        &self.selection_id
    }

    pub fn identity(&self) -> &PackRevision {
        &self.identity
    }

    pub fn kind(&self) -> &ModelPackKindV1 {
        &self.kind
    }

    pub fn activation_allowed(&self) -> bool {
        self.activation_allowed
    }

    pub fn validate_for_manifest(
        &self,
        manifest: &ModelPackManifestV1,
    ) -> Result<(), PackSelectionError> {
        if self.policy != API_FIRST_SELECTION_POLICY_V1
            || self.identity != manifest.identity()
            || self.manifest_sha256
                != manifest
                    .digest()
                    .map_err(|error| PackSelectionError::Manifest(error.to_string()))?
            || self.kind != manifest.capability.kind
            || self.scope != manifest.capability.scope
        {
            return Err(PackSelectionError::AuthorizationMismatch);
        }
        if self.kind != ModelPackKindV1::LipSync || self.scope != ModelPackScopeV1::Generic {
            return Err(PackSelectionError::AuthorizationOutsideApiFirstPolicy);
        }
        validate_selection_id(&self.selection_id)?;
        Ok(())
    }

    pub(crate) fn validate_download_artifact(
        &self,
        identity: &PackRevision,
        artifact: &ArtifactV1,
    ) -> Result<(), PackSelectionError> {
        if self.policy != API_FIRST_SELECTION_POLICY_V1
            || &self.identity != identity
            || self.kind != ModelPackKindV1::LipSync
            || self.scope != ModelPackScopeV1::Generic
        {
            return Err(PackSelectionError::AuthorizationMismatch);
        }
        if self.artifacts.get(&artifact.id) != Some(&(artifact.size_bytes, artifact.sha256.clone()))
        {
            return Err(PackSelectionError::ArtifactAuthorizationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ApiFirstPackSelectionPolicyV1;

impl ApiFirstPackSelectionPolicyV1 {
    pub fn authorize(
        &self,
        manifest: &ModelPackManifestV1,
        request: PackSelectionRequestV1,
    ) -> Result<PackSelectionAuthorizationV1, PackSelectionError> {
        manifest
            .validate()
            .map_err(|error| PackSelectionError::Manifest(error.to_string()))?;
        validate_selection_id(&request.selection_id)?;
        if request.origin != PackSelectionOriginV1::ExplicitUser {
            return Err(PackSelectionError::ExplicitUserSelectionRequired(
                request.origin,
            ));
        }
        if manifest.capability.kind != ModelPackKindV1::LipSync {
            return Err(PackSelectionError::LocalInferenceKindBlocked(
                manifest.capability.kind.clone(),
            ));
        }
        if manifest.capability.scope != ModelPackScopeV1::Generic {
            return Err(PackSelectionError::GenericLipSyncRequired);
        }
        Ok(PackSelectionAuthorizationV1 {
            policy: API_FIRST_SELECTION_POLICY_V1.to_owned(),
            identity: manifest.identity(),
            manifest_sha256: manifest
                .digest()
                .map_err(|error| PackSelectionError::Manifest(error.to_string()))?,
            kind: manifest.capability.kind.clone(),
            scope: manifest.capability.scope.clone(),
            selection_id: request.selection_id,
            selected_unix_seconds: request.selected_unix_seconds,
            activation_allowed: request.action == PackSelectionActionV1::InstallAndActivate,
            artifacts: manifest
                .artifacts
                .iter()
                .map(|artifact| {
                    (
                        artifact.id.clone(),
                        (artifact.size_bytes, artifact.sha256.clone()),
                    )
                })
                .collect(),
        })
    }
}

fn validate_selection_id(value: &str) -> Result<(), PackSelectionError> {
    if !(8..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(PackSelectionError::InvalidSelectionId);
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PackSelectionError {
    #[error("model pack manifest is invalid: {0}")]
    Manifest(String),
    #[error("an explicit user selection is required; received {0:?}")]
    ExplicitUserSelectionRequired(PackSelectionOriginV1),
    #[error("API-first mode blocks local inference pack kind {0:?}")]
    LocalInferenceKindBlocked(ModelPackKindV1),
    #[error("API-first mode permits only a generic lip-sync pack")]
    GenericLipSyncRequired,
    #[error("selection ID is invalid")]
    InvalidSelectionId,
    #[error("selection authorization does not match this manifest or pack")]
    AuthorizationMismatch,
    #[error("artifact is not covered by this explicit pack selection")]
    ArtifactAuthorizationMismatch,
    #[error("selection authorization is outside the API-first baseline policy")]
    AuthorizationOutsideApiFirstPolicy,
    #[error("selection did not explicitly authorize activation")]
    ActivationNotAuthorized,
}
