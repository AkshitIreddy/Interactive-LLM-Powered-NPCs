use std::collections::HashSet;

use crate::types::{
    ExecutionMode, NetworkPolicy, ProviderDescriptor, ProviderLocation, TurnRequest,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivacyDecision {
    Allowed,
    BlockedOffline,
    BlockedFullyLocal,
    CloudProviderNotAuthorized,
    RetentionNotAuthorized,
}

#[derive(Clone, Debug)]
pub struct PrivacyPolicy {
    execution_mode: ExecutionMode,
    network_policy: NetworkPolicy,
    authorized_cloud_providers: HashSet<String>,
    allow_retaining_providers: bool,
}

impl PrivacyPolicy {
    pub fn from_request(request: &TurnRequest) -> Self {
        Self {
            execution_mode: request.execution_mode,
            network_policy: request.network_policy,
            authorized_cloud_providers: request
                .authorized_cloud_providers
                .iter()
                .cloned()
                .collect(),
            allow_retaining_providers: request.allow_retaining_providers,
        }
    }

    pub fn evaluate(&self, descriptor: &ProviderDescriptor) -> PrivacyDecision {
        if descriptor.location.is_networked() && self.network_policy == NetworkPolicy::Offline {
            return PrivacyDecision::BlockedOffline;
        }
        if descriptor.location.is_networked() && self.execution_mode == ExecutionMode::FullyLocal {
            return PrivacyDecision::BlockedFullyLocal;
        }
        if descriptor.location.is_networked()
            && !self.authorized_cloud_providers.contains(&descriptor.id)
        {
            return PrivacyDecision::CloudProviderNotAuthorized;
        }
        if descriptor.may_retain_data && !self.allow_retaining_providers {
            return PrivacyDecision::RetentionNotAuthorized;
        }
        PrivacyDecision::Allowed
    }
}

#[derive(Clone, Debug)]
pub struct FallbackPolicy {
    pub allow_provider_fallback: bool,
    pub allow_local_to_cloud_fallback: bool,
}

impl FallbackPolicy {
    pub fn from_request(request: &TurnRequest) -> Self {
        Self {
            allow_provider_fallback: request.allow_provider_fallback,
            allow_local_to_cloud_fallback: request.allow_local_to_cloud_fallback,
        }
    }

    pub fn allows_transition(&self, from: &ProviderDescriptor, to: &ProviderDescriptor) -> bool {
        if from.id == to.id {
            return true;
        }
        if !self.allow_provider_fallback {
            return false;
        }
        if from.location.is_local()
            && matches!(to.location, ProviderLocation::Cloud { .. })
            && !self.allow_local_to_cloud_fallback
        {
            return false;
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct RoutingPlan {
    pub privacy: PrivacyPolicy,
    pub fallback: FallbackPolicy,
}

impl RoutingPlan {
    pub fn from_request(request: &TurnRequest) -> Self {
        Self {
            privacy: PrivacyPolicy::from_request(request),
            fallback: FallbackPolicy::from_request(request),
        }
    }

    /// Returns allowed providers in declared preference order. A fallback candidate
    /// must be both privacy-safe and explicitly allowed as a transition.
    pub fn candidates<'a, T, F>(&self, providers: &'a [T], descriptor: F) -> Vec<&'a T>
    where
        F: Fn(&T) -> &ProviderDescriptor,
    {
        let primary = providers.iter().find(|provider| {
            self.privacy.evaluate(descriptor(provider)) == PrivacyDecision::Allowed
        });
        let Some(primary) = primary else {
            return Vec::new();
        };
        providers
            .iter()
            .filter(|candidate| {
                self.privacy.evaluate(descriptor(candidate)) == PrivacyDecision::Allowed
                    && self
                        .fallback
                        .allows_transition(descriptor(primary), descriptor(candidate))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DataClass, ProviderModality};
    use std::collections::BTreeMap;

    fn descriptor(id: &str, location: ProviderLocation) -> ProviderDescriptor {
        ProviderDescriptor {
            id: id.into(),
            display_name: id.into(),
            modality: ProviderModality::LanguageModel,
            location,
            may_retain_data: false,
            transmitted_data: vec![DataClass::Transcript],
            capabilities: BTreeMap::new(),
        }
    }

    fn request() -> TurnRequest {
        TurnRequest {
            session_id: "s".into(),
            turn_id: "t".into(),
            transcript: "hello".into(),
            character_hint: None,
            game_id: "generic".into(),
            locale: "en-US".into(),
            execution_mode: ExecutionMode::Hybrid,
            network_policy: NetworkPolicy::Online,
            authorized_cloud_providers: vec!["cloud".into()],
            allow_provider_fallback: true,
            allow_local_to_cloud_fallback: false,
            allow_retaining_providers: false,
            metadata: Default::default(),
        }
    }

    #[test]
    fn local_to_cloud_is_not_silent() {
        let plan = RoutingPlan::from_request(&request());
        let local = descriptor("local", ProviderLocation::Local);
        let cloud = descriptor(
            "cloud",
            ProviderLocation::Cloud {
                service: "test".into(),
            },
        );
        assert!(!plan.fallback.allows_transition(&local, &cloud));
    }

    #[test]
    fn offline_blocks_cloud_even_when_authorized() {
        let mut request = request();
        request.network_policy = NetworkPolicy::Offline;
        let plan = RoutingPlan::from_request(&request);
        let cloud = descriptor(
            "cloud",
            ProviderLocation::Cloud {
                service: "test".into(),
            },
        );
        assert_eq!(
            plan.privacy.evaluate(&cloud),
            PrivacyDecision::BlockedOffline
        );
    }
}
