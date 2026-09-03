//! Deterministic, fail-closed selection of an already-running game window.
//!
//! Store discovery proves where a game is installed. This module deliberately
//! keeps the separate runtime question narrow: which exact process instance
//! and top-level HWND may be captured? A binding includes the process creation
//! identity and full executable path so PID or HWND reuse cannot silently move
//! a session to a launcher, another edition, or an unrelated process.

use regex::Regex;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessWindowRule {
    pub executable: String,
    pub required: bool,
    pub window_title_regex: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningGameWindow {
    pub pid: u32,
    /// Stable process creation identity from the operating system. It must not
    /// be a scan timestamp: PID reuse is rejected only when this value changes.
    pub process_started_at_ticks: u64,
    pub executable_path: PathBuf,
    /// Raw HWND value widened to u64 for platform-neutral fixture tests.
    pub hwnd: u64,
    pub title: String,
    pub visible: bool,
    pub top_level: bool,
    pub cloaked: bool,
    pub minimized: bool,
    pub client_width: u32,
    pub client_height: u32,
    pub foreground: bool,
    pub observed_at_monotonic_ms: u64,
    /// Last frame sequence delivered by capture for this exact binding.
    pub frame_sequence: Option<u64>,
    pub frame_observed_at_monotonic_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundGameTarget {
    pub pid: u32,
    pub process_started_at_ticks: u64,
    pub executable_path: PathBuf,
    pub hwnd: u64,
    pub executable_leaf: String,
    pub selected_at_monotonic_ms: u64,
}

#[derive(Debug)]
struct CompiledRule {
    executable: String,
    title: Option<Regex>,
}

#[derive(Debug)]
pub struct TargetPolicy {
    primary: Vec<CompiledRule>,
    excluded_titles: Vec<Regex>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TargetPolicyError {
    #[error("at least one primary process rule is required")]
    MissingPrimary,
    #[error("process executable must be a leaf .exe name: {0}")]
    InvalidExecutable(String),
    #[error("invalid window-title regex {pattern}: {message}")]
    InvalidRegex { pattern: String, message: String },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TargetSelectionError {
    #[error("no eligible primary game window was found")]
    NotFound,
    #[error("{count} eligible game windows are ambiguous")]
    Ambiguous { count: usize },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TargetRevalidationError {
    #[error("the bound HWND no longer exists")]
    StaleWindow,
    #[error("the bound HWND now belongs to another PID")]
    WrongPid,
    #[error("the PID was reused by a newer process instance")]
    StaleProcess,
    #[error("the process executable no longer matches the bound executable")]
    WrongExecutable,
    #[error("the observation predates the target binding")]
    StaleObservation,
    #[error("the window no longer satisfies the primary game-window policy")]
    IneligibleWindow,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CaptureFreshnessError {
    #[error(transparent)]
    Target(#[from] TargetRevalidationError),
    #[error("capture has not delivered a frame for the bound target")]
    NoFrame,
    #[error("capture frame sequence did not advance")]
    FrameDidNotAdvance,
    #[error("capture frame timestamp is in the future")]
    FrameTimestampInFuture,
    #[error("capture frame is stale by {age_ms} ms (limit {maximum_age_ms} ms)")]
    StaleFrame { age_ms: u64, maximum_age_ms: u64 },
}

impl TargetPolicy {
    pub fn new(
        rules: impl IntoIterator<Item = ProcessWindowRule>,
        excluded_window_title_regexes: impl IntoIterator<Item = String>,
    ) -> Result<Self, TargetPolicyError> {
        let mut primary = Vec::new();
        for rule in rules {
            validate_executable_leaf(&rule.executable)?;
            if !rule.required {
                continue;
            }
            primary.push(CompiledRule {
                executable: rule.executable,
                title: rule
                    .window_title_regex
                    .as_deref()
                    .map(compile_regex)
                    .transpose()?,
            });
        }
        if primary.is_empty() {
            return Err(TargetPolicyError::MissingPrimary);
        }
        primary.sort_by(|left, right| {
            left.executable
                .to_ascii_lowercase()
                .cmp(&right.executable.to_ascii_lowercase())
        });
        primary.dedup_by(|left, right| {
            left.executable.eq_ignore_ascii_case(&right.executable)
                && left.title.as_ref().map(Regex::as_str) == right.title.as_ref().map(Regex::as_str)
        });

        let mut excluded_titles = excluded_window_title_regexes
            .into_iter()
            .map(|pattern| compile_regex(&pattern))
            .collect::<Result<Vec<_>, _>>()?;
        excluded_titles.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        excluded_titles.dedup_by(|left, right| left.as_str() == right.as_str());
        Ok(Self {
            primary,
            excluded_titles,
        })
    }

    /// Select one exact process instance and HWND. Multiple matching windows
    /// are never resolved by enumeration order: a unique foreground window is
    /// accepted, otherwise the user must choose explicitly.
    pub fn select(
        &self,
        observations: &[RunningGameWindow],
    ) -> Result<BoundGameTarget, TargetSelectionError> {
        let eligible: Vec<_> = observations
            .iter()
            .filter(|observation| self.is_eligible(observation))
            .collect();
        let selected = match eligible.as_slice() {
            [] => return Err(TargetSelectionError::NotFound),
            [only] => *only,
            many => {
                let foreground: Vec<_> = many
                    .iter()
                    .copied()
                    .filter(|observation| observation.foreground)
                    .collect();
                match foreground.as_slice() {
                    [only] => *only,
                    _ => {
                        return Err(TargetSelectionError::Ambiguous {
                            count: eligible.len(),
                        })
                    }
                }
            }
        };
        Ok(bind(selected))
    }

    /// Revalidate an immutable binding against a fresh OS snapshot. The lookup
    /// starts with HWND so a stale handle reused by another process is reported
    /// distinctly and cannot fall through to another matching game window.
    pub fn revalidate<'a>(
        &self,
        binding: &BoundGameTarget,
        observations: &'a [RunningGameWindow],
    ) -> Result<&'a RunningGameWindow, TargetRevalidationError> {
        let current = observations
            .iter()
            .find(|observation| observation.hwnd == binding.hwnd)
            .ok_or(TargetRevalidationError::StaleWindow)?;
        if current.pid != binding.pid {
            return Err(TargetRevalidationError::WrongPid);
        }
        if current.process_started_at_ticks != binding.process_started_at_ticks {
            return Err(TargetRevalidationError::StaleProcess);
        }
        if !paths_equal_windows(&current.executable_path, &binding.executable_path)
            || executable_leaf(&current.executable_path)
                .is_none_or(|leaf| !leaf.eq_ignore_ascii_case(&binding.executable_leaf))
        {
            return Err(TargetRevalidationError::WrongExecutable);
        }
        if current.observed_at_monotonic_ms < binding.selected_at_monotonic_ms {
            return Err(TargetRevalidationError::StaleObservation);
        }
        if !self.is_eligible(current) {
            return Err(TargetRevalidationError::IneligibleWindow);
        }
        Ok(current)
    }

    pub fn require_advancing_capture<'a>(
        &self,
        binding: &BoundGameTarget,
        observations: &'a [RunningGameWindow],
        previous_frame_sequence: u64,
        now_monotonic_ms: u64,
        maximum_frame_age_ms: u64,
    ) -> Result<&'a RunningGameWindow, CaptureFreshnessError> {
        let current = self.revalidate(binding, observations)?;
        let sequence = current
            .frame_sequence
            .ok_or(CaptureFreshnessError::NoFrame)?;
        let frame_at = current
            .frame_observed_at_monotonic_ms
            .ok_or(CaptureFreshnessError::NoFrame)?;
        if sequence <= previous_frame_sequence {
            return Err(CaptureFreshnessError::FrameDidNotAdvance);
        }
        let Some(age_ms) = now_monotonic_ms.checked_sub(frame_at) else {
            return Err(CaptureFreshnessError::FrameTimestampInFuture);
        };
        if age_ms > maximum_frame_age_ms {
            return Err(CaptureFreshnessError::StaleFrame {
                age_ms,
                maximum_age_ms: maximum_frame_age_ms,
            });
        }
        Ok(current)
    }

    fn is_eligible(&self, observation: &RunningGameWindow) -> bool {
        if observation.pid == 0
            || observation.process_started_at_ticks == 0
            || observation.hwnd == 0
            || !observation.executable_path.is_absolute()
            || !observation.visible
            || !observation.top_level
            || observation.cloaked
            || observation.minimized
            || observation.client_width == 0
            || observation.client_height == 0
            || self
                .excluded_titles
                .iter()
                .any(|pattern| pattern.is_match(&observation.title))
        {
            return false;
        }
        let Some(leaf) = executable_leaf(&observation.executable_path) else {
            return false;
        };
        self.primary.iter().any(|rule| {
            leaf.eq_ignore_ascii_case(&rule.executable)
                && rule
                    .title
                    .as_ref()
                    .is_none_or(|title| title.is_match(&observation.title))
        })
    }
}

fn compile_regex(pattern: &str) -> Result<Regex, TargetPolicyError> {
    Regex::new(pattern).map_err(|error| TargetPolicyError::InvalidRegex {
        pattern: pattern.to_owned(),
        message: error.to_string(),
    })
}

fn validate_executable_leaf(executable: &str) -> Result<(), TargetPolicyError> {
    let path = Path::new(executable);
    let one_normal_component = {
        let mut components = path.components();
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
    };
    let is_exe = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
    if executable.trim() != executable
        || executable.is_empty()
        || executable.contains(['/', '\\', ':'])
        || !one_normal_component
        || !is_exe
    {
        return Err(TargetPolicyError::InvalidExecutable(executable.to_owned()));
    }
    Ok(())
}

fn executable_leaf(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

fn normalized_windows_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    Some(text.replace('/', "\\").to_ascii_lowercase())
}

fn paths_equal_windows(left: &Path, right: &Path) -> bool {
    normalized_windows_path(left) == normalized_windows_path(right)
}

fn bind(observation: &RunningGameWindow) -> BoundGameTarget {
    BoundGameTarget {
        pid: observation.pid,
        process_started_at_ticks: observation.process_started_at_ticks,
        executable_path: observation.executable_path.clone(),
        hwnd: observation.hwnd,
        executable_leaf: executable_leaf(&observation.executable_path)
            .unwrap_or_default()
            .to_owned(),
        selected_at_monotonic_ms: observation.observed_at_monotonic_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> TargetPolicy {
        TargetPolicy::new(
            [
                ProcessWindowRule {
                    executable: "interactive-npcs-synthetic-target.exe".into(),
                    required: true,
                    window_title_regex: Some("^Interactive NPCs Synthetic Target$".into()),
                },
                ProcessWindowRule {
                    executable: "synthetic-launcher.exe".into(),
                    required: false,
                    window_title_regex: None,
                },
            ],
            ["(?i)launcher".into(), "(?i)crash reporter".into()],
        )
        .unwrap()
    }

    fn target() -> RunningGameWindow {
        RunningGameWindow {
            pid: 4242,
            process_started_at_ticks: 9001,
            executable_path: absolute_executable("interactive-npcs-synthetic-target.exe"),
            hwnd: 0x1234,
            title: "Interactive NPCs Synthetic Target".into(),
            visible: true,
            top_level: true,
            cloaked: false,
            minimized: false,
            client_width: 1280,
            client_height: 720,
            foreground: true,
            observed_at_monotonic_ms: 10_000,
            frame_sequence: Some(10),
            frame_observed_at_monotonic_ms: Some(9_990),
        }
    }

    fn absolute_executable(leaf: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Games\Interactive NPCs").join(leaf)
        } else {
            PathBuf::from("/games/interactive-npcs").join(leaf)
        }
    }

    #[test]
    fn selects_the_exact_synthetic_target_and_ignores_auxiliary_launcher() {
        let mut launcher = target();
        launcher.pid = 4000;
        launcher.process_started_at_ticks = 8000;
        launcher.hwnd = 0x1000;
        launcher.executable_path = absolute_executable("synthetic-launcher.exe");
        launcher.title = "Synthetic Launcher".into();
        let selected = policy().select(&[launcher, target()]).unwrap();
        assert_eq!(selected.pid, 4242);
        assert_eq!(selected.hwnd, 0x1234);
        assert_eq!(
            selected.executable_leaf,
            "interactive-npcs-synthetic-target.exe"
        );
    }

    #[test]
    fn unique_foreground_window_resolves_two_valid_instances() {
        let first = target();
        let mut second = target();
        second.pid = 4343;
        second.process_started_at_ticks = 9002;
        second.hwnd = 0x1235;
        second.foreground = false;
        assert_eq!(policy().select(&[first, second]).unwrap().pid, 4242);
    }

    #[test]
    fn two_nonforeground_game_windows_are_ambiguous() {
        let mut first = target();
        first.foreground = false;
        let mut second = target();
        second.pid = 4343;
        second.process_started_at_ticks = 9002;
        second.hwnd = 0x1235;
        second.foreground = false;
        assert_eq!(
            policy().select(&[first, second]),
            Err(TargetSelectionError::Ambiguous { count: 2 })
        );
    }

    #[test]
    fn rejects_stale_or_reused_pid_hwnd_and_wrong_executable() {
        let original = target();
        let binding = policy().select(std::slice::from_ref(&original)).unwrap();

        assert_eq!(
            policy().revalidate(&binding, &[]),
            Err(TargetRevalidationError::StaleWindow)
        );

        let mut wrong_pid = original.clone();
        wrong_pid.pid += 1;
        assert_eq!(
            policy().revalidate(&binding, &[wrong_pid]),
            Err(TargetRevalidationError::WrongPid)
        );

        let mut reused_pid = original.clone();
        reused_pid.process_started_at_ticks += 1;
        assert_eq!(
            policy().revalidate(&binding, &[reused_pid]),
            Err(TargetRevalidationError::StaleProcess)
        );

        let mut wrong_executable = original;
        wrong_executable.executable_path = if cfg!(windows) {
            PathBuf::from(r"C:\Other\Interactive NPCs\interactive-npcs-synthetic-target.exe")
        } else {
            PathBuf::from("/other/interactive-npcs/interactive-npcs-synthetic-target.exe")
        };
        assert_eq!(
            policy().revalidate(&binding, &[wrong_executable]),
            Err(TargetRevalidationError::WrongExecutable)
        );
    }

    #[test]
    fn rejects_title_geometry_visibility_and_old_observations_after_binding() {
        for mutate in [
            |item: &mut RunningGameWindow| item.title = "Crash Reporter".into(),
            |item: &mut RunningGameWindow| item.client_width = 0,
            |item: &mut RunningGameWindow| item.visible = false,
            |item: &mut RunningGameWindow| item.cloaked = true,
            |item: &mut RunningGameWindow| item.minimized = true,
        ] {
            let original = target();
            let binding = policy().select(std::slice::from_ref(&original)).unwrap();
            let mut changed = original;
            mutate(&mut changed);
            assert_eq!(
                policy().revalidate(&binding, &[changed]),
                Err(TargetRevalidationError::IneligibleWindow)
            );
        }

        let original = target();
        let binding = policy().select(std::slice::from_ref(&original)).unwrap();
        let mut old = original;
        old.observed_at_monotonic_ms -= 1;
        assert_eq!(
            policy().revalidate(&binding, &[old]),
            Err(TargetRevalidationError::StaleObservation)
        );
    }

    #[test]
    fn advancing_capture_requires_a_new_recent_frame_from_the_same_binding() {
        let current = target();
        let binding = policy().select(std::slice::from_ref(&current)).unwrap();
        assert!(policy()
            .require_advancing_capture(&binding, std::slice::from_ref(&current), 9, 10_000, 100)
            .is_ok());
        assert_eq!(
            policy().require_advancing_capture(
                &binding,
                std::slice::from_ref(&current),
                10,
                10_000,
                100,
            ),
            Err(CaptureFreshnessError::FrameDidNotAdvance)
        );

        let mut stale = current;
        stale.frame_sequence = Some(11);
        stale.frame_observed_at_monotonic_ms = Some(9_000);
        assert_eq!(
            policy().require_advancing_capture(&binding, &[stale], 10, 10_000, 100),
            Err(CaptureFreshnessError::StaleFrame {
                age_ms: 1_000,
                maximum_age_ms: 100,
            })
        );
    }

    #[test]
    fn rejects_invalid_policy_executables_and_regexes() {
        for executable in [r"..\game.exe", r"C:\game.exe", "game.com", " game.exe"] {
            assert!(matches!(
                TargetPolicy::new(
                    [ProcessWindowRule {
                        executable: executable.into(),
                        required: true,
                        window_title_regex: None,
                    }],
                    [],
                ),
                Err(TargetPolicyError::InvalidExecutable(_))
            ));
        }
        assert!(matches!(
            TargetPolicy::new(
                [ProcessWindowRule {
                    executable: "game.exe".into(),
                    required: true,
                    window_title_regex: Some("(".into()),
                }],
                [],
            ),
            Err(TargetPolicyError::InvalidRegex { .. })
        ));
    }
}
