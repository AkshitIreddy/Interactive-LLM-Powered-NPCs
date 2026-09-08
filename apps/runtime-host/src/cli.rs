use std::{io::Read, path::PathBuf, str::FromStr, sync::Arc};

use clap::{Parser, Subcommand};
use npc_protocol::LaunchNonce;
use serde::Serialize;
use thiserror::Error;

use crate::{
    profile_replays::ProfileReplayCorpus,
    profiles::{GenericGameSelection, GENERIC_GAME_ID},
    simulation::{SimulationProfilePolicy, SimulationSafetyContext, SimulationSafetyEvidenceState},
    HostConfig, HostState, ServeOptions, SimulationRequest,
};

#[derive(Debug, Parser)]
#[command(
    name = "npc-runtime",
    version,
    about = "Interactive NPCs trusted runtime host"
)]
pub struct Cli {
    /// Repository root containing profiles/ and catalog/.
    #[arg(long, global = true)]
    pub repo_root: Option<PathBuf>,
    /// Writable per-user application data root. Tests and portable runs should inject this.
    #[arg(long, global = true)]
    pub app_data: Option<PathBuf>,
    /// Exact native application identifier used to isolate persisted provider credentials.
    #[arg(
        long,
        global = true,
        default_value = interactive_npcs_credential_vault::PRODUCTION_APPLICATION_NAMESPACE
    )]
    pub application_namespace: String,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run privacy-safe readiness checks without network calls or performance sampling.
    Doctor,
    /// Load and validate the exact 20-profile corpus.
    ValidateProfiles,
    /// Verify the 20-profile integrity ledger and execute every deterministic replay.
    ValidateProfileReplays,
    /// Run one deterministic, offline fixture turn through runtime-core and SQLite.
    SimulateTurn {
        #[arg(long, default_value = "skyrim-special-edition")]
        game: String,
        #[arg(long)]
        character: Option<String>,
        /// Manual game label for `--game generic-game`.
        #[arg(long)]
        generic_game_name: Option<String>,
        /// Manual executable leaf (for example Game.exe) for `--game generic-game`.
        #[arg(long)]
        generic_executable: Option<String>,
        /// Manual character name for `--game generic-game`.
        #[arg(long)]
        generic_character_name: Option<String>,
        /// Refuse Generic Game because protected online play was detected.
        #[arg(long, default_value_t = false)]
        protected_online_detected: bool,
        /// Refuse Generic Game because anti-cheat was detected.
        #[arg(long, default_value_t = false)]
        anti_cheat_detected: bool,
        /// Fixture transcript. Omit to read at most 64 KiB from standard input.
        #[arg(long)]
        transcript: Option<String>,
        #[arg(long, default_value = "en-US")]
        locale: String,
        #[arg(long, default_value = "cli-session")]
        session_id: String,
        #[arg(long, default_value = "cli-turn-1")]
        turn_id: String,
    },
    /// Serve the authenticated local control endpoint until Ctrl+C or a shutdown request.
    Serve {
        /// Per-launch UUID passed by the desktop shell. Generated if omitted.
        #[arg(long)]
        nonce: Option<String>,
        /// Portable CI Unix-socket path. Ignored by the Windows named-pipe backend.
        #[arg(long)]
        endpoint: Option<PathBuf>,
    },
}

pub async fn run(cli: Cli) -> Result<(), CliError> {
    let repo_root = resolve_repo_root(cli.repo_root)?;
    let app_data = resolve_app_data(cli.app_data);
    let state = HostState::initialize_for_application(
        HostConfig {
            repo_root,
            app_data,
        },
        &cli.application_namespace,
    )
    .await?;
    match cli.command {
        Command::Doctor => write_json(&state.doctor().await)?,
        Command::ValidateProfiles => write_json(&ValidationOutput {
            schema_version: "1.0.0",
            valid: true,
            profile_count: state.profiles.profiles().len(),
            profiles: state.profiles.summaries(),
            generic_mode: state.profiles.generic_contract(),
        })?,
        Command::ValidateProfileReplays => {
            let corpus = ProfileReplayCorpus::load(&state.config.repo_root, &state)?;
            write_json(&corpus.validate_runtime(&state).await?)?;
        }
        Command::SimulateTurn {
            game,
            character,
            generic_game_name,
            generic_executable,
            generic_character_name,
            protected_online_detected,
            anti_cheat_detected,
            transcript,
            locale,
            session_id,
            turn_id,
        } => {
            let transcript = match transcript {
                Some(value) => value,
                None => read_bounded_stdin()?,
            };
            let result = state
                .simulate_turn(SimulationRequest {
                    session_id,
                    turn_id,
                    game_id: game.clone(),
                    character_id: character,
                    effective_game_profile: None,
                    native_identity_decision: None,
                    enabled_spoiler_tiers: Vec::new(),
                    generic_selection: match (
                        generic_game_name,
                        generic_executable,
                        generic_character_name,
                    ) {
                        (Some(game_name), Some(executable_name), Some(character_name)) => {
                            Some(GenericGameSelection {
                                game_name,
                                executable_name,
                                character_name,
                                protected_online_detected,
                                anti_cheat_detected,
                            })
                        }
                        (None, None, None) => None,
                        _ => return Err(CliError::IncompleteGenericSelection),
                    },
                    safety_context: if game == GENERIC_GAME_ID {
                        SimulationSafetyContext::default()
                    } else {
                        SimulationSafetyContext {
                            evidence_state: if protected_online_detected || anti_cheat_detected {
                                SimulationSafetyEvidenceState::Blocked
                            } else {
                                SimulationSafetyEvidenceState::Unknown
                            },
                            profile_policy: SimulationProfilePolicy::Unknown,
                            visuals_allowed: false,
                            protected_online_detected,
                            anti_cheat_detected,
                        }
                    },
                    application_namespace: None,
                    transcript,
                    locale,
                    execution_mode: None,
                    dev_live_tts: None,
                    route_snapshot: None,
                    input: Default::default(),
                    delivery: Default::default(),
                    audio_playback_leases: Vec::new(),
                    private_evaluation_acknowledgements: Vec::new(),
                    subtitle_presentation_context: None,
                })
                .await?;
            write_json(&result)?;
        }
        Command::Serve { nonce, endpoint } => {
            let nonce = match nonce {
                Some(value) => LaunchNonce::from_str(&value).map_err(|_| CliError::InvalidNonce)?,
                None => LaunchNonce::new(),
            };
            let mut options = ServeOptions::new(nonce.clone());
            options.endpoint_override = endpoint.clone();
            let shutdown = options.shutdown.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    shutdown.cancel();
                }
            });
            write_json(&ServeDescriptor {
                schema_version: "1.0.0",
                transport: if cfg!(windows) {
                    "windows_named_pipe"
                } else {
                    "portable_unix_socket"
                },
                endpoint: endpoint.map(|path| path.display().to_string()),
                launch_nonce: nonce.to_string(),
                protocol_version: "1.0.0",
                max_message_bytes: crate::MAX_CONTROL_MESSAGE_BYTES,
            })?;
            Arc::new(state).serve(options).await?;
        }
    }
    Ok(())
}

fn resolve_repo_root(explicit: Option<PathBuf>) -> Result<PathBuf, CliError> {
    if let Some(path) = explicit {
        return canonical_directory(path).ok_or(CliError::RepoRoot);
    }
    let mut candidate = std::env::current_dir().map_err(|_| CliError::RepoRoot)?;
    loop {
        if candidate.join("profiles/games").is_dir()
            && candidate.join("catalog/v1/catalog.json").is_file()
        {
            return canonical_directory(candidate).ok_or(CliError::RepoRoot);
        }
        if !candidate.pop() {
            break;
        }
    }
    Err(CliError::RepoRoot)
}

fn canonical_directory(path: PathBuf) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    canonical.is_dir().then_some(canonical)
}

fn resolve_app_data(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("Interactive NPCs");
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("interactive-npcs");
    }
    std::env::temp_dir().join(format!("interactive-npcs-v2-{}", std::process::id()))
}

fn read_bounded_stdin() -> Result<String, CliError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CliError::Input)?;
    if bytes.len() > 64 * 1024 {
        return Err(CliError::Input);
    }
    String::from_utf8(bytes).map_err(|_| CliError::Input)
}

fn write_json(value: &impl Serialize) -> Result<(), CliError> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, value).map_err(|_| CliError::Output)?;
    use std::io::Write;
    writeln!(&mut lock).map_err(|_| CliError::Output)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationOutput {
    schema_version: &'static str,
    valid: bool,
    profile_count: usize,
    profiles: Vec<crate::profiles::ProfileSummary>,
    generic_mode: crate::profiles::GenericGameContract,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServeDescriptor {
    schema_version: &'static str,
    transport: &'static str,
    endpoint: Option<String>,
    launch_nonce: String,
    protocol_version: &'static str,
    max_message_bytes: usize,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("repository root could not be resolved")]
    RepoRoot,
    #[error("serve nonce must be a non-nil UUID")]
    InvalidNonce,
    #[error("standard input is invalid or exceeds 64 KiB")]
    Input,
    #[error("generic mode requires game name, executable, and character name together")]
    IncompleteGenericSelection,
    #[error("command output failed")]
    Output,
    #[error(transparent)]
    Bootstrap(#[from] crate::bootstrap::BootstrapError),
    #[error(transparent)]
    Simulation(#[from] crate::simulation::SimulationError),
    #[error(transparent)]
    Control(#[from] crate::control::ControlError),
    #[error(transparent)]
    ProfileReplay(#[from] crate::profile_replays::ProfileReplayError),
}
