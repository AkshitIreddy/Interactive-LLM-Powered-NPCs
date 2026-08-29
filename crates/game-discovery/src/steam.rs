use crate::path_security::normalize_store_relative;
use crate::vdf::{self, VdfError, VdfValue};
use crate::StoreRelativePathError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamLibrary {
    pub path: PathBuf,
    pub app_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamApp {
    pub app_id: String,
    pub name: String,
    pub install_dir_name: String,
    pub state_flags: Option<u64>,
    pub build_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum SteamParseError {
    #[error(transparent)]
    Vdf(#[from] VdfError),
    #[error("missing object {0}")]
    MissingObject(&'static str),
    #[error("missing string field {0}")]
    MissingField(&'static str),
    #[error("unsafe Steam install directory: {0}")]
    UnsafeInstallDirectory(#[from] StoreRelativePathError),
}

pub fn parse_library_folders(input: &str) -> Result<Vec<SteamLibrary>, SteamParseError> {
    let root = vdf::parse(input)?;
    let folders = root
        .get("libraryfolders")
        .or_else(|| root.get("LibraryFolders"))
        .and_then(VdfValue::as_object)
        .ok_or(SteamParseError::MissingObject("libraryfolders"))?;

    let mut libraries = Vec::new();
    for (key, value) in folders {
        if !key.chars().all(|character| character.is_ascii_digit()) {
            continue;
        }
        match value {
            VdfValue::Text(path) => libraries.push(SteamLibrary {
                path: PathBuf::from(path),
                app_ids: Vec::new(),
            }),
            VdfValue::Object(fields) => {
                let path = fields
                    .get("path")
                    .and_then(VdfValue::as_text)
                    .ok_or(SteamParseError::MissingField("path"))?;
                let app_ids = fields
                    .get("apps")
                    .and_then(VdfValue::as_object)
                    .map(|apps| apps.keys().cloned().collect())
                    .unwrap_or_default();
                libraries.push(SteamLibrary {
                    path: PathBuf::from(path),
                    app_ids,
                });
            }
        }
    }
    libraries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(libraries)
}

pub fn parse_app_manifest(input: &str) -> Result<SteamApp, SteamParseError> {
    let root = vdf::parse(input)?;
    let state = root
        .get("AppState")
        .or_else(|| root.get("appstate"))
        .and_then(VdfValue::as_object)
        .ok_or(SteamParseError::MissingObject("AppState"))?;

    let text = |name: &'static str| {
        state
            .get(name)
            .and_then(VdfValue::as_text)
            .ok_or(SteamParseError::MissingField(name))
    };
    let app_id = text("appid")?.to_string();
    let name = text("name")?.to_string();
    let install_dir_name = normalize_store_relative(text("installdir")?)?
        .to_string_lossy()
        .into_owned();
    let state_flags = state
        .get("StateFlags")
        .or_else(|| state.get("stateflags"))
        .and_then(VdfValue::as_text)
        .and_then(|value| value.parse().ok());
    let build_id = state
        .get("buildid")
        .and_then(VdfValue::as_text)
        .map(ToOwned::to_owned);

    Ok(SteamApp {
        app_id,
        name,
        install_dir_name,
        state_flags,
        build_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_libraryfolders_layout() {
        let libraries = parse_library_folders(
            r#"
            "libraryfolders" {
              "0" { "path" "C:\\Program Files (x86)\\Steam" "apps" { "489830" "1" } }
              "1" { "path" "D:\\SteamLibrary" "apps" { "1091500" "1" } }
              "contentstatsid" "123"
            }
            "#,
        )
        .unwrap();
        assert_eq!(libraries.len(), 2);
        assert!(libraries[1].app_ids.contains(&"1091500".to_string()));
    }

    #[test]
    fn parses_app_manifest_without_guessing_folder_name() {
        let app = parse_app_manifest(
            r#"
            "AppState" {
              "appid" "1091500"
              "name" "Cyberpunk 2077"
              "StateFlags" "4"
              "installdir" "Cyberpunk 2077"
              "buildid" "19990001"
            }
            "#,
        )
        .unwrap();
        assert_eq!(app.app_id, "1091500");
        assert_eq!(app.state_flags, Some(4));
        assert_eq!(app.build_id.as_deref(), Some("19990001"));
    }

    #[test]
    fn rejects_hostile_install_directory_fields() {
        for install_dir in [
            r"C:\outside",
            r"\\server\share",
            r"\\?\C:\outside",
            "/outside",
            r"..\outside",
            r"safe/..\outside",
            "%2e%2e/outside",
            "safe:stream",
        ] {
            let vdf_install_dir = install_dir.replace('\\', r"\\");
            let manifest = format!(
                r#""AppState" {{ "appid" "42" "name" "Hostile" "installdir" "{vdf_install_dir}" }}"#
            );
            assert!(
                matches!(
                    parse_app_manifest(&manifest),
                    Err(SteamParseError::UnsafeInstallDirectory(_))
                ),
                "accepted hostile installdir: {install_dir}"
            );
        }
    }
}
