use std::collections::BTreeSet;

use crate::{HostedTtsProviderId, VoiceBinding, VoiceBindings};

#[derive(Clone, Debug, Default)]
pub struct FallbackAuthorization {
    /// Every cloud provider that the user authorized for this turn.
    pub authorized_providers: BTreeSet<HostedTtsProviderId>,
    pub allow_provider_change: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FallbackDecision<'a> {
    Primary(&'a VoiceBinding),
    Fallback {
        failed_provider: HostedTtsProviderId,
        binding: &'a VoiceBinding,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FallbackError {
    #[error("the primary provider is not authorized")]
    PrimaryNotAuthorized,
    #[error("no voice binding exists for the primary provider")]
    MissingPrimaryVoice,
    #[error("provider fallback is not authorized")]
    ProviderChangeNotAuthorized,
    #[error("no authorized provider has an explicit voice mapping")]
    NoAuthorizedMappedVoice,
    #[error("provider cannot change after an utterance has started")]
    UtteranceAlreadyStarted,
}

/// Selects an explicit voice mapping before a TTS session starts. This function
/// never performs nearest-neighbour guessing and never authorizes egress.
pub fn select_voice_route<'a>(
    intent_id: &str,
    primary: HostedTtsProviderId,
    ordered_fallbacks: &[HostedTtsProviderId],
    bindings: &'a VoiceBindings,
    authorization: &FallbackAuthorization,
) -> Result<FallbackDecision<'a>, FallbackError> {
    if !authorization.authorized_providers.contains(&primary) {
        return Err(FallbackError::PrimaryNotAuthorized);
    }
    let primary_binding = bindings
        .resolve(intent_id, primary)
        .ok_or(FallbackError::MissingPrimaryVoice)?;
    if ordered_fallbacks.is_empty() {
        return Ok(FallbackDecision::Primary(primary_binding));
    }
    if !authorization.allow_provider_change {
        return Err(FallbackError::ProviderChangeNotAuthorized);
    }
    ordered_fallbacks
        .iter()
        .copied()
        .filter(|provider| authorization.authorized_providers.contains(provider))
        .find_map(|provider| {
            bindings
                .resolve(intent_id, provider)
                .map(|binding| FallbackDecision::Fallback {
                    failed_provider: primary,
                    binding,
                })
        })
        .ok_or(FallbackError::NoAuthorizedMappedVoice)
}

/// Monotonic pin carried by a supervisor across connection attempts.
#[derive(Clone, Debug)]
pub struct UtteranceProviderPin {
    provider: HostedTtsProviderId,
    utterance_started: bool,
}

impl UtteranceProviderPin {
    #[must_use]
    pub fn new(provider: HostedTtsProviderId) -> Self {
        Self {
            provider,
            utterance_started: false,
        }
    }

    #[must_use]
    pub fn provider(&self) -> HostedTtsProviderId {
        self.provider
    }

    pub fn mark_started(&mut self) {
        self.utterance_started = true;
    }

    pub fn switch_before_start(
        &mut self,
        provider: HostedTtsProviderId,
        authorization: &FallbackAuthorization,
    ) -> Result<(), FallbackError> {
        if self.utterance_started {
            return Err(FallbackError::UtteranceAlreadyStarted);
        }
        if !authorization.allow_provider_change
            || !authorization.authorized_providers.contains(&provider)
        {
            return Err(FallbackError::ProviderChangeNotAuthorized);
        }
        self.provider = provider;
        Ok(())
    }
}
