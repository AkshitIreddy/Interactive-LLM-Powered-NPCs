# System requirements

## Release status

These are 2.0 target requirements. There is no public installer or certified RC yet.

## Operating system

- Windows 10 x64, version 22H2, or Windows 11 x64
- Current vendor graphics driver
- WebView2 Evergreen Runtime
- A standard interactive user session; the runtime is not a Windows service

Native capture, audio, credential, and packaging behavior must be validated on Windows. WSL cannot certify them.

## Inputs and game mode

- Microphone; headphones are strongly recommended
- Windowed or borderless game mode for external capture/overlay
- Single-player/offline session
- Manual character selection remains available when identity evidence is absent

True exclusive fullscreen, minimized/protected windows, anti-cheat, and some swapchains may fall back to audio/subtitles. The product does not use an injected or per-game adapter fallback.

## Cloud mode

Cloud mode needs an account/key for every selected provider, a network connection, and compliance with provider billing, quota, region, age, and content terms. Keys are stored by the native runtime through Windows Credential Manager, never in profile files.

## Optional local lip-sync resources

The base application does not bundle AI models, Python, CUDA, or FFmpeg. LLM, STT, TTS, and retrieval are API-first. The only optional user-downloadable AI packs contemplated are generic local screen-space lip-sync candidates, and none is advertised as available until qualified.

Every eligible lip-sync pack must disclose its exact model/revision, download and installed size, peak RAM/VRAM, backend/driver constraints, license/use terms, measured visual quality, latency/game impact, and experimental caveats before the user explicitly downloads it. There are no automatic model downloads. A failed or absent pack leaves conversation audio/subtitles intact.

No static GPU model guarantees lip-sync success: the game and optional visual worker share live memory. The runtime must inspect available budget, reserve game headroom, and disable animation before audio/subtitles. API-only conversation requires no local model-capable GPU.

Published minimums will come from clean-machine and game-load benchmarks, not estimates.

## Developer requirements

Source development adds PowerShell 5.1+ or PowerShell 7, Git, pinned Rust 1.96.1, pinned Node.js 20.20.2 and pnpm 10.28.2, Visual Studio Build Tools with the Windows SDK and C++ workload, CMake 3.24+, plus optional pinned Python only for isolated lip-sync research packs. See [developer setup](../development/setup.md).
