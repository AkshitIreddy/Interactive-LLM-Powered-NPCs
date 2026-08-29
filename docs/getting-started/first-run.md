# First run and onboarding

The 2.0 onboarding contract is:

`Welcome → hardware/privacy scan → execution mode → game → providers/models → microphone/PTT rehearsal → optional presence → performance target → deterministic simulation → Ready`

## What each step establishes

1. **Welcome:** shows pre-release/build identity and links to privacy/support.
2. **Hardware and privacy:** records OS, CPU/GPU, available memory, display/capture capabilities, audio devices, and network preference. It does not scan personal files or upload results by default.
3. **Conversation routing:** selects explicit hosted providers or Offline. This is independent of performance quality.
4. **Game:** chooses a detected profile or Generic Game; ambiguous/protected configurations are refused or reduced safely.
5. **Providers/models:** configures an explicit hosted provider for LLM, STT, TTS, and optional retrieval. Credentials go to Windows Credential Manager. Named loadouts may group route/model choices with global/game/character inheritance, but switching remains explicit and never authorizes silent fallback. Optional generic lip-sync pack selection is separate and never automatic.
6. **Microphone/PTT:** lets the user choose a device/key and rehearse start, endpoint, playback, and interruption. Push-to-talk is the safe default.
7. **Presence:** explains optional webcam/perception behavior. It is local, ephemeral, visibly active, and off unless enabled.
8. **Performance:** chooses Competitive, Fast, Balanced, Immersive, Maximum Quality, or Custom using current resource estimates.
9. **Simulation:** runs a deterministic fictional conversation without game capture, microphone upload, or persistent character memory.
10. **Ready:** reports exact enabled capabilities and fallbacks before starting a session.

## Starting a session

Launch the game in its supported offline mode, prefer windowed/borderless display, then select **Start** in Home. Verify the detected executable/build and capability summary. If the NPC is obscured or offscreen, select/name the intended character; conversation continues through audio/subtitles.

The application must never claim visual integration merely because a profile exists. Profiles do not require mods or native adapters.

## Stopping

Stop from the compact session panel or configured hotkey. The runtime stops capture/playback, commits delivered dialogue atomically, releases workers, and leaves incomplete model output uncommitted.
