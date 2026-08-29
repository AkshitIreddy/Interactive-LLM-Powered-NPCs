# Startup and game detection

## App does not start

**Check:** run `./dev.ps1 environment` for source builds; record the first failed component and Windows Event/Application error without sensitive dumps.

**Try:** install/repair WebView2 Evergreen; verify supported x64 Windows; rebuild through the canonical command; remove only generated caches identified by the tool. Do not run legacy notebooks as a workaround.

## Game is not detected

**Check:** confirm the exact profile, store, executable/build, and single-player process. Use manual executable selection if offered.

**Likely causes:** unsupported store manifest, custom install path, changed executable/build, game not running, launcher rather than game selected, or insufficient read access.

**Try:** start the game normally, select its actual executable manually, refresh detection, and inspect profile diagnostics. Do not rename executables, inject a DLL, or weaken protection to force detection.

## Session is policy-blocked

This is expected when the runtime finds anti-cheat, protected/online mode, an ambiguous shared executable, or an unapproved build. Exit to a documented offline/story mode and re-detect. If ambiguity remains, use only an explicitly allowed audio/subtitle path or stop. The project does not provide bypasses.

## Wrong character/game state

Select/name the intended character manually. Low-confidence identity must not guess. Confirm the profile/build capability is live-certified rather than merely authored/replay-verified.

## Report fields

Build/commit, Windows build, store, profile, executable version/hash if diagnostics provides it, detected processes/modules (sanitized), session mode, protection-block reason, and reproduction.
