use npc_game_profile::{
    load_profile, validate_json_schema, CharacterDataReadiness, IntegrationMode,
    LocalActivationPolicy, ProviderStrategy, ScreenSpaceLipSyncRecommendation,
};
use serde_json::Value;

fn profile_value() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../profiles/games/cyberpunk-2077/profile.json"
    ))
    .expect("checked-in Cyberpunk profile must deserialize as JSON")
}

#[test]
fn optional_policy_fields_preserve_existing_profile_compatibility() {
    let profile = load_profile(include_bytes!(
        "../../../profiles/games/cyberpunk-2077/profile.json"
    ))
    .expect("checked-in Cyberpunk profile must satisfy the typed profile contract");
    // Corpus migration may populate these fields, but omission remains valid for
    // older GameProfileV2 documents.
    if let Some(policy) = profile.recommendations {
        assert_eq!(policy.integration_mode, IntegrationMode::ExternalOnly);
        assert_eq!(policy.provider_strategy, ProviderStrategy::ApiFirst);
        assert_eq!(
            policy.local_activation_policy,
            LocalActivationPolicy::MeasuredWholeLoadoutFitRequired
        );
        assert!(policy.game_resource_reserve_required);
        assert_eq!(
            policy.screen_space_lip_sync,
            ScreenSpaceLipSyncRecommendation::ExperimentalOptInAfterExactTargetAndAdvancingFrameQualification
        );
    }
}

#[test]
fn typed_recommendations_and_curated_readiness_validate() {
    let mut value = profile_value();
    value["content"]["character_data_readiness"] = serde_json::json!("curated");
    value["recommendations"] = serde_json::json!({
        "integration_mode": "external_only",
        "provider_strategy": "api_first",
        "local_activation_policy": "measured_whole_loadout_fit_required",
        "game_resource_reserve_required": true,
        "screen_space_lip_sync": "experimental_opt_in_after_exact_target_and_advancing_frame_qualification"
    });
    let encoded = serde_json::to_vec(&value).expect("test profile must serialize");
    let profile = load_profile(&encoded).expect("typed recommendation fixture must load");
    assert_eq!(
        profile.content.character_data_readiness,
        Some(CharacterDataReadiness::Curated)
    );
    assert!(profile.recommendations.is_some());
}

#[test]
fn non_curated_readiness_needs_notes_and_resource_reserve_cannot_be_disabled() {
    let mut value = profile_value();
    value["content"]["character_data_readiness"] = serde_json::json!("partial");
    value["content"]
        .as_object_mut()
        .expect("checked-in profile content must remain an object")
        .remove("character_data_readiness_notes");
    let report = validate_json_schema(&value);
    assert!(!report.is_valid());

    value["content"]["character_data_readiness_notes"] =
        serde_json::json!("Only two characters have reviewed authored data.");
    value["recommendations"] = serde_json::json!({
        "integration_mode": "external_only",
        "provider_strategy": "api_first",
        "local_activation_policy": "measured_whole_loadout_fit_required",
        "game_resource_reserve_required": false,
        "screen_space_lip_sync": "experimental_opt_in_after_exact_target_and_advancing_frame_qualification"
    });
    let report = validate_json_schema(&value);
    assert!(!report.is_valid());
}
