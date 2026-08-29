# Support

Interactive LLM Powered NPCs 2.0 is a development preview. There is no public installer, supported release candidate, paid plan, or guaranteed response time.

## Before filing a report

1. Confirm that the behavior belongs to 2.0 rather than the unsupported prototype.
2. Read [Getting started](docs/getting-started/installation.md) and [Troubleshooting](docs/troubleshooting/README.md).
3. Run Diagnostics when available, review the bundle, and remove personal or game-sensitive content.
4. Search existing repository issues.

A useful report includes the exact commit/local RC; Windows edition/build; CPU, GPU, VRAM, RAM, driver, power mode, display/DPI/HDR state; game profile/store/build/session mode; execution/performance modes; provider/model identifiers (never keys); reproduction; expected/actual result; deterministic-simulation result; and safe diagnostics.

For latency/FPS issues, include the raw result directory and environment manifest. Do not submit hand-timed estimates as benchmarks.

## Known boundaries

- Target platforms are Windows 10 22H2 and Windows 11 x64.
- True exclusive fullscreen, protected capture, minimized windows, anti-cheat, and some swapchains may require audio/subtitles or an approved native adapter.
- Risk-gated profiles are offline/story-mode only.
- Generic mode does not promise identity, world state, actions, or lip-sync.
- Provider availability, billing, policy, quotas, and data handling belong to that provider.
- Local performance depends on live resources remaining after the game starts.

Report vulnerabilities privately via [SECURITY.md](SECURITY.md). Never upload credentials, raw conversations, recordings, webcam frames, proprietary captures, memory databases, or unsanitized dumps publicly.

Requests to bypass protections, enable protected online modes, imitate performers without rights, redistribute restricted weights, or copy game/wiki content are not supported.
