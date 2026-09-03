# Generated project sidecars

The Tauri bundle expects these ignored, target-suffixed build products:

- `npc-runtime-x86_64-pc-windows-msvc.exe`
- `npc-media-broker-x86_64-pc-windows-msvc.exe`
- `npc-mouth-worker-x86_64-pc-windows-msvc.exe`
- `npc-subtitle-presenter-x86_64-pc-windows-msvc.exe`

The base Tauri config names only the runtime and broker so ordinary nested
source checks do not depend on absent product staging. The Debug-review and
Release packaging overlays require all four names, and
`scripts/prepare-sidecars.ps1` builds, audits, and copies them before Tauri is
invoked. C++ children are always Release builds, even for a Debug review app;
every staged child must be x64 Windows GUI subsystem and import no Debug CRT.
`sidecar-manifest.v1.json` records the exact SHA-256 and import inventory.

Only project-owned binaries are copied.
Local AI runtimes and model weights are never placed here; the Model Manager
installs separately verified runtime/model packs after onboarding.
