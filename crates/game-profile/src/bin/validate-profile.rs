#![allow(clippy::print_stderr, clippy::print_stdout)]

use npc_game_profile::{load_profile, ProfileLoadError};
use serde::Serialize;
use std::{env, fs, path::PathBuf, process::ExitCode};

#[derive(Serialize)]
struct FileResult {
    path: String,
    valid: bool,
    profile_id: Option<String>,
    errors: Vec<npc_game_profile::ValidationIssue>,
    message: Option<String>,
}

fn main() -> ExitCode {
    let paths: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: validate-profile <profile.json> [profile.json ...]");
        return ExitCode::from(2);
    }

    let mut any_invalid = false;
    let results: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let result = match fs::read(&path) {
                Ok(bytes) => match load_profile(&bytes) {
                    Ok(profile) => FileResult {
                        path: path.display().to_string(),
                        valid: true,
                        profile_id: Some(profile.id),
                        errors: vec![],
                        message: None,
                    },
                    Err(ProfileLoadError::Validation(report)) => FileResult {
                        path: path.display().to_string(),
                        valid: false,
                        profile_id: None,
                        errors: report.errors,
                        message: None,
                    },
                    Err(error) => FileResult {
                        path: path.display().to_string(),
                        valid: false,
                        profile_id: None,
                        errors: vec![],
                        message: Some(error.to_string()),
                    },
                },
                Err(error) => FileResult {
                    path: path.display().to_string(),
                    valid: false,
                    profile_id: None,
                    errors: vec![],
                    message: Some(error.to_string()),
                },
            };
            any_invalid |= !result.valid;
            result
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&results).expect("results serialize")
    );
    if any_invalid {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
