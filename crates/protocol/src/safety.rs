use serde::{Deserialize, Serialize};

/// Authenticated per-turn safety evidence admitted by the native control
/// plane. `ConsoleIsolated` is a narrow no-game-interaction degradation; it is
/// never equivalent to `VerifiedSafe`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnSafetyEvidenceStateV1 {
    VerifiedSafe,
    ConsoleIsolated,
    #[default]
    Unknown,
    Blocked,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnProfileSafetyPolicyV1 {
    SyntheticFixture,
    SinglePlayerOnly,
    OfflineOnly,
    ConsoleIsolatedNoGameInteraction,
    #[default]
    Unknown,
}

/// Exact authenticated Control-to-runtime safety context. All fields are
/// required on this internal wire. A containing request may default the whole
/// context to `Unknown`, but omission can never manufacture an admitted pair.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnSafetyContextV1 {
    pub evidence_state: TurnSafetyEvidenceStateV1,
    pub profile_policy: TurnProfileSafetyPolicyV1,
    pub visuals_allowed: bool,
    pub protected_online_detected: bool,
    pub anti_cheat_detected: bool,
}

impl TurnSafetyContextV1 {
    pub const fn verified_safe() -> Self {
        Self {
            evidence_state: TurnSafetyEvidenceStateV1::VerifiedSafe,
            profile_policy: TurnProfileSafetyPolicyV1::SinglePlayerOnly,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: false,
        }
    }

    pub const fn verified_synthetic_fixture() -> Self {
        Self {
            evidence_state: TurnSafetyEvidenceStateV1::VerifiedSafe,
            profile_policy: TurnProfileSafetyPolicyV1::SyntheticFixture,
            visuals_allowed: true,
            protected_online_detected: false,
            anti_cheat_detected: false,
        }
    }

    pub const fn verified_console_isolated() -> Self {
        Self {
            evidence_state: TurnSafetyEvidenceStateV1::ConsoleIsolated,
            profile_policy: TurnProfileSafetyPolicyV1::ConsoleIsolatedNoGameInteraction,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: false,
        }
    }

    pub fn validate_admitted(self) -> Result<(), TurnSafetyContextErrorV1> {
        let pair_admitted = matches!(
            (self.evidence_state, self.profile_policy),
            (
                TurnSafetyEvidenceStateV1::VerifiedSafe,
                TurnProfileSafetyPolicyV1::SyntheticFixture
                    | TurnProfileSafetyPolicyV1::SinglePlayerOnly
                    | TurnProfileSafetyPolicyV1::OfflineOnly
            ) | (
                TurnSafetyEvidenceStateV1::ConsoleIsolated,
                TurnProfileSafetyPolicyV1::ConsoleIsolatedNoGameInteraction
            )
        );
        if !pair_admitted {
            return Err(TurnSafetyContextErrorV1::UnverifiedOrMismatched);
        }
        if self.evidence_state == TurnSafetyEvidenceStateV1::ConsoleIsolated && self.visuals_allowed
        {
            return Err(TurnSafetyContextErrorV1::ConsoleVisualsForbidden);
        }
        if self.protected_online_detected {
            return Err(TurnSafetyContextErrorV1::ProtectedOnlineDetected);
        }
        if self.anti_cheat_detected {
            return Err(TurnSafetyContextErrorV1::AntiCheatDetected);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum TurnSafetyContextErrorV1 {
    #[error("trusted target safety evidence is absent, unknown, blocked, or policy-mismatched")]
    UnverifiedOrMismatched,
    #[error("console-isolated turns cannot authorize game visuals")]
    ConsoleVisualsForbidden,
    #[error("trusted game-state evidence detected protected online play")]
    ProtectedOnlineDetected,
    #[error("trusted game-state evidence detected anti-cheat")]
    AntiCheatDetected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_pairs_are_admitted_and_unknown_or_crossed_pairs_fail_closed() {
        for admitted in [
            TurnSafetyContextV1::verified_safe(),
            TurnSafetyContextV1::verified_synthetic_fixture(),
            TurnSafetyContextV1 {
                profile_policy: TurnProfileSafetyPolicyV1::OfflineOnly,
                ..TurnSafetyContextV1::verified_safe()
            },
            TurnSafetyContextV1::verified_console_isolated(),
        ] {
            admitted.validate_admitted().expect("exact admitted pair");
        }

        for rejected in [
            TurnSafetyContextV1::default(),
            TurnSafetyContextV1 {
                evidence_state: TurnSafetyEvidenceStateV1::VerifiedSafe,
                profile_policy: TurnProfileSafetyPolicyV1::ConsoleIsolatedNoGameInteraction,
                ..TurnSafetyContextV1::default()
            },
            TurnSafetyContextV1 {
                evidence_state: TurnSafetyEvidenceStateV1::ConsoleIsolated,
                profile_policy: TurnProfileSafetyPolicyV1::SinglePlayerOnly,
                ..TurnSafetyContextV1::default()
            },
            TurnSafetyContextV1 {
                visuals_allowed: true,
                ..TurnSafetyContextV1::verified_console_isolated()
            },
        ] {
            assert!(rejected.validate_admitted().is_err());
        }
    }

    #[test]
    fn camel_case_wire_is_exact_and_unknown_values_are_rejected() {
        let value = serde_json::to_value(TurnSafetyContextV1::verified_console_isolated())
            .expect("serialize safety context");
        assert_eq!(value["evidenceState"], "consoleIsolated");
        assert_eq!(value["profilePolicy"], "consoleIsolatedNoGameInteraction");
        assert_eq!(value["visualsAllowed"], false);

        let mut unknown = value.clone();
        unknown["evidenceState"] = serde_json::json!("verifiedConsoleIsolated");
        assert!(serde_json::from_value::<TurnSafetyContextV1>(unknown).is_err());

        let mut extra = value;
        extra["providerDeclaredSafe"] = serde_json::json!(true);
        assert!(serde_json::from_value::<TurnSafetyContextV1>(extra).is_err());
    }
}
