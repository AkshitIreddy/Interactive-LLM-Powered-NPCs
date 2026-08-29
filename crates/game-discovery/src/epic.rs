use serde::Deserialize;
use std::path::PathBuf;
use thiserror::Error;

use crate::path_security::normalize_store_relative;
use crate::StoreRelativePathError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicInstall {
    pub app_name: String,
    pub display_name: String,
    pub install_location: PathBuf,
    pub launch_executable: Option<PathBuf>,
    pub catalog_namespace: Option<String>,
    pub catalog_item_id: Option<String>,
    pub artifact_id: Option<String>,
    pub main_game_app_name: Option<String>,
    pub app_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum EpicParseError {
    #[error("invalid Epic manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Epic manifest has no application records")]
    Empty,
    #[error("Epic manifest record is missing {0}")]
    Missing(&'static str),
    #[error("Epic manifest has an unsafe launch executable: {0}")]
    UnsafeLaunchExecutable(#[from] StoreRelativePathError),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LauncherInstalled {
    installation_list: Vec<LauncherRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LauncherRecord {
    app_name: String,
    install_location: String,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    app_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ItemRecord {
    app_name: String,
    display_name: String,
    install_location: String,
    #[serde(default)]
    launch_executable: Option<String>,
    #[serde(default)]
    catalog_namespace: Option<String>,
    #[serde(default)]
    catalog_item_id: Option<String>,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    main_game_app_name: Option<String>,
    #[serde(default)]
    app_version_string: Option<String>,
    #[serde(default, rename = "bIsApplication")]
    b_is_application: Option<bool>,
}

pub fn parse_launcher_installed(input: &str) -> Result<Vec<EpicInstall>, EpicParseError> {
    let parsed: LauncherInstalled = serde_json::from_str(input)?;
    if parsed.installation_list.is_empty() {
        return Err(EpicParseError::Empty);
    }
    Ok(parsed
        .installation_list
        .into_iter()
        .map(|record| EpicInstall {
            display_name: record.app_name.clone(),
            app_name: record.app_name,
            install_location: PathBuf::from(record.install_location),
            launch_executable: None,
            catalog_namespace: None,
            catalog_item_id: None,
            artifact_id: record.artifact_id,
            main_game_app_name: None,
            app_version: record.app_version,
        })
        .collect())
}

pub fn parse_epic_item(input: &str) -> Result<Option<EpicInstall>, EpicParseError> {
    let record: ItemRecord = serde_json::from_str(input)?;
    if record.b_is_application == Some(false) && record.main_game_app_name.is_some() {
        return Ok(None);
    }
    let install_location = PathBuf::from(&record.install_location);
    let launch_executable = record
        .launch_executable
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_store_relative(&value))
        .transpose()?;
    Ok(Some(EpicInstall {
        app_name: record.app_name,
        display_name: record.display_name,
        install_location,
        launch_executable,
        catalog_namespace: record.catalog_namespace,
        catalog_item_id: record.catalog_item_id,
        artifact_id: record.artifact_id,
        main_game_app_name: record.main_game_app_name,
        app_version: record.app_version_string,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_launcher_installations() {
        let installs = parse_launcher_installed(
            r#"{"InstallationList":[{"InstallLocation":"D:\\Games\\KCD2","AppName":"KingdomComeDeliverance2","ArtifactId":"kcd2","AppVersion":"1.0"}]}"#,
        )
        .unwrap();
        assert_eq!(installs[0].artifact_id.as_deref(), Some("kcd2"));
    }

    #[test]
    fn filters_non_application_components() {
        let item = parse_epic_item(
            r#"{"AppName":"DLC","DisplayName":"DLC","InstallLocation":"D:\\Game","MainGameAppName":"Game","bIsApplication":false}"#,
        )
        .unwrap();
        assert!(item.is_none());
    }

    #[test]
    fn rejects_hostile_launch_executable_fields() {
        for executable in [
            r"C:\outside.exe",
            r"\\server\share\outside.exe",
            r"\\?\C:\outside.exe",
            r"\\.\GLOBALROOT\outside.exe",
            "/usr/bin/outside",
            r"..\outside.exe",
            r"safe/..\outside.exe",
            "%2e%2e/outside.exe",
            r"bin\game.exe:payload",
        ] {
            let item = serde_json::json!({
                "AppName": "Hostile",
                "DisplayName": "Hostile",
                "InstallLocation": "D:\\Games\\Hostile",
                "LaunchExecutable": executable,
                "bIsApplication": true
            });
            assert!(
                matches!(
                    parse_epic_item(&item.to_string()),
                    Err(EpicParseError::UnsafeLaunchExecutable(_))
                ),
                "accepted hostile LaunchExecutable: {executable}"
            );
        }
    }

    #[test]
    fn keeps_valid_nested_launch_executable_relative() {
        let item = parse_epic_item(
            r#"{"AppName":"Game","DisplayName":"Game","InstallLocation":"D:\\Game","LaunchExecutable":"bin\\x64/game.exe","bIsApplication":true}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            item.launch_executable,
            Some(PathBuf::from("bin").join("x64").join("game.exe"))
        );
    }
}
