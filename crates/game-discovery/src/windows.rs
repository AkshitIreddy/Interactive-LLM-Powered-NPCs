use std::io;
use std::path::PathBuf;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

use crate::{
    DiscoveryService, EpicFilesystemScanner, GogFilesystemScanner, SteamFilesystemScanner,
};

/// Read-only launcher-root discovery. Parsing store manifests remains in the
/// platform-independent modules so it is fixture-testable on every CI host.
#[derive(Debug, Default)]
pub struct WindowsStoreLocator;

impl WindowsStoreLocator {
    pub fn steam_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ) {
            for name in ["SteamPath", "InstallPath"] {
                if let Ok(value) = key.get_value::<String, _>(name) {
                    roots.push(PathBuf::from(value));
                }
            }
        }
        roots.sort();
        roots.dedup();
        roots
    }

    pub fn gog_registry_installations(&self) -> io::Result<Vec<(String, PathBuf)>> {
        let mut games = Vec::new();
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let root = RegKey::predef(hive);
            for base in [
                r"SOFTWARE\GOG.com\Games",
                r"SOFTWARE\WOW6432Node\GOG.com\Games",
            ] {
                let Ok(key) = root.open_subkey_with_flags(base, KEY_READ) else {
                    continue;
                };
                for product_id in key.enum_keys().flatten() {
                    let Ok(game) = key.open_subkey_with_flags(&product_id, KEY_READ) else {
                        continue;
                    };
                    if let Ok(path) = game.get_value::<String, _>("path") {
                        games.push((product_id, PathBuf::from(path)));
                    }
                }
            }
        }
        games.sort();
        games.dedup();
        Ok(games)
    }

    pub fn discovery_service(&self) -> DiscoveryService {
        let mut steam_roots = self.steam_roots();
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            steam_roots.push(PathBuf::from(program_files_x86).join("Steam"));
        }
        steam_roots.sort();
        steam_roots.dedup();
        let program_data = std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let gog = self.gog_registry_installations().unwrap_or_default();
        DiscoveryService::new()
            .with_scanner(SteamFilesystemScanner { roots: steam_roots })
            .with_scanner(EpicFilesystemScanner { program_data })
            .with_scanner(GogFilesystemScanner {
                registry_installations: gog,
            })
    }
}
