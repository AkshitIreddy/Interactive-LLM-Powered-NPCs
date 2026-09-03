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
    #[error("Epic manifest has an invalid install location")]
    InvalidInstallLocation,
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
    parsed
        .installation_list
        .into_iter()
        .map(|record| {
            require_text(&record.app_name, "AppName")?;
            require_text(&record.install_location, "InstallLocation")?;
            validate_install_location(&record.install_location)?;
            Ok(EpicInstall {
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
        })
        .collect()
}

pub fn parse_epic_item(input: &str) -> Result<Option<EpicInstall>, EpicParseError> {
    let record: ItemRecord = serde_json::from_str(input)?;
    if record.b_is_application == Some(false) {
        return Ok(None);
    }
    require_text(&record.app_name, "AppName")?;
    require_text(&record.display_name, "DisplayName")?;
    require_text(&record.install_location, "InstallLocation")?;
    validate_install_location(&record.install_location)?;
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

fn require_text(value: &str, field: &'static str) -> Result<(), EpicParseError> {
    if value.trim().is_empty() {
        Err(EpicParseError::Missing(field))
    } else {
        Ok(())
    }
}

fn validate_install_location(value: &str) -> Result<(), EpicParseError> {
    let bytes = value.as_bytes();
    let windows_drive_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let windows_device =
        value.starts_with(r"\\?\") || value.starts_with(r"\\.\") || value.starts_with(r"\??\");
    let unc_absolute = value.starts_with(r"\\") && !windows_device;
    let platform_absolute = std::path::Path::new(value).is_absolute();
    if windows_device
        || value.chars().any(char::is_control)
        || !(windows_drive_absolute || unc_absolute || platform_absolute)
    {
        Err(EpicParseError::InvalidInstallLocation)
    } else {
        Ok(())
    }
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
        for item in [
            r#"{"AppName":"DLC","DisplayName":"DLC","InstallLocation":"D:\\Game","MainGameAppName":"Game","bIsApplication":false}"#,
            r#"{"AppName":"Prerequisite","DisplayName":"Prerequisite","InstallLocation":"D:\\Game","bIsApplication":false}"#,
        ] {
            assert!(parse_epic_item(item).unwrap().is_none());
        }
    }

    #[test]
    fn rejects_blank_launcher_and_item_identity_fields() {
        assert!(matches!(
            parse_launcher_installed(
                r#"{"InstallationList":[{"InstallLocation":"D:\\Game","AppName":" "}]}"#
            ),
            Err(EpicParseError::Missing("AppName"))
        ));
        assert!(matches!(
            parse_epic_item(
                r#"{"AppName":"Game","DisplayName":" ","InstallLocation":"D:\\Game","bIsApplication":true}"#
            ),
            Err(EpicParseError::Missing("DisplayName"))
        ));
    }

    #[test]
    fn rejects_relative_device_and_control_character_install_locations() {
        for install_location in ["Games/Game", r"\\?\C:\Games\Game", "D:\\Games\\Game\n"] {
            let item = serde_json::json!({
                "AppName": "Game",
                "DisplayName": "Game",
                "InstallLocation": install_location,
                "bIsApplication": true
            });
            assert!(matches!(
                parse_epic_item(&item.to_string()),
                Err(EpicParseError::InvalidInstallLocation)
            ));
        }
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
