use npc_game_profile::{load_profile, GameProfileV2};

pub fn cyberpunk_profile() -> GameProfileV2 {
    load_profile(include_bytes!(
        "../../../../profiles/games/cyberpunk-2077/profile.json"
    ))
    .expect("bundled Cyberpunk profile must be valid")
}
