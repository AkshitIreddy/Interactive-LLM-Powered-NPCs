use serde::Deserialize;
use std::path::PathBuf;
use thiserror::Error;

use crate::path_security::normalize_store_relative;
use crate::StoreRelativePathError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GogInstall {
    pub product_id: String,
    pub name: String,
    pub install_dir: PathBuf,
    pub play_tasks: Vec<PathBuf>,
    pub build_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum GogParseError {
    #[error("invalid GOG metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("GOG metadata is missing {0}")]
    Missing(&'static str),
    #[error("GOG metadata has an unsafe play task: {0}")]
    UnsafePlayTask(#[from] StoreRelativePathError),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GogInfo {
    game_id: String,
    name: String,
    root_game_id: Option<String>,
    build_id: Option<String>,
    play_tasks: Option<Vec<GogTask>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GogTask {
    #[serde(rename = "type")]
    task_type: String,
    path: Option<String>,
    is_primary: Option<bool>,
}

pub fn parse_gog_info(input: &str, install_dir: PathBuf) -> Result<GogInstall, GogParseError> {
    let info: GogInfo = serde_json::from_str(input)?;
    let product_id = info.root_game_id.unwrap_or(info.game_id);
    let mut play_tasks: Vec<_> = info
        .play_tasks
        .unwrap_or_default()
        .into_iter()
        .filter(|task| task.task_type.eq_ignore_ascii_case("FileTask"))
        .filter_map(|task| {
            task.path
                .map(|path| (task.is_primary.unwrap_or(false), path))
        })
        .collect();
    play_tasks.sort_by_key(|(primary, _)| !*primary);
    let play_tasks = play_tasks
        .into_iter()
        .map(|(_, path)| normalize_store_relative(&path))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(GogInstall {
        product_id,
        name: info.name,
        install_dir,
        play_tasks,
        build_id: info.build_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_root_game_id_and_primary_executable_first() {
        let install = parse_gog_info(
            r#"{"gameId":"dlc","rootGameId":"1207658924","name":"The Witcher 3","buildId":"42","playTasks":[{"type":"FileTask","path":"bin\\x64\\witcher3.exe","isPrimary":true}]}"#,
            PathBuf::from(r"D:\GOG\Witcher 3"),
        )
        .unwrap();
        assert_eq!(install.product_id, "1207658924");
        assert!(install.play_tasks[0].ends_with("witcher3.exe"));
    }

    #[test]
    fn rejects_hostile_play_task_fields() {
        for executable in [
            r"C:\outside.exe",
            r"\\server\share\outside.exe",
            r"\\?\C:\outside.exe",
            "/usr/bin/outside",
            r"..\outside.exe",
            r"safe/..\outside.exe",
            "%2e%2e/outside.exe",
            r"bin\game.exe:payload",
        ] {
            let info = serde_json::json!({
                "gameId": "42",
                "name": "Hostile",
                "playTasks": [{
                    "type": "FileTask",
                    "path": executable,
                    "isPrimary": true
                }]
            });
            assert!(
                matches!(
                    parse_gog_info(&info.to_string(), PathBuf::from(r"D:\Games\Hostile")),
                    Err(GogParseError::UnsafePlayTask(_))
                ),
                "accepted hostile play task: {executable}"
            );
        }
    }

    #[test]
    fn keeps_valid_nested_play_task_relative() {
        let install = parse_gog_info(
            r#"{"gameId":"42","name":"Game","playTasks":[{"type":"FileTask","path":"bin\\x64/game.exe","isPrimary":true}]}"#,
            PathBuf::from(r"D:\Games\Game"),
        )
        .unwrap();
        assert_eq!(
            install.play_tasks,
            vec![PathBuf::from("bin").join("x64").join("game.exe")]
        );
    }
}
